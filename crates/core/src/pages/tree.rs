//! The number-tree and name-tree rebuild that `/PageLabels`, `/Names /Dests`
//! and P4's `/ParentTree` all need.
//!
//! **A tree is rebuilt, never filtered.** Dropping entries out of the leaves
//! and leaving the ancestors alone passes every structural check - the tree is
//! still well-formed, `/Kids` still resolve, entries are still sorted - and
//! breaks lookup, because a reader descends by comparing against each node's
//! `/Limits` and a stale bracket sends it down the wrong branch or refuses a
//! key that is there. So the entries are collected, sorted, and a fresh tree is
//! written with `/Limits` recomputed on every node that has them.
//!
//! One helper for both kinds. They differ only in which key holds the leaves
//! (`/Nums` against `/Names`) and in how a key compares: a number tree's keys
//! are integers, a name tree's are byte strings ordered by ISO 32000-1
//! 7.9.6's rule, which is a plain byte comparison.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::rewrite::{object_at, resolve};
use crate::edit::Transaction;
use crate::Result;

/// How many entries go in one leaf before the tree branches. Small enough that
/// a large tree is a tree rather than one enormous array, large enough that a
/// document's worth of destinations is two levels.
const LEAF_CAPACITY: usize = 64;

/// A tree's flavour: which key holds its entries, and how its keys sort.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `/Nums`: integer keys, as `/PageLabels` uses.
    Number,
    /// `/Names`: string keys, as `/Names /Dests` uses.
    NameString,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Number => "Nums",
            Kind::NameString => "Names",
        }
    }
}

/// One entry: its key as the file writes it, and its value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) key: Object,
    pub(crate) value: Object,
}

impl Entry {
    /// The sort key. Integers sort as integers and strings as bytes, which is
    /// the ordering a reader's binary search assumes; anything else sorts
    /// last, so a malformed entry cannot displace a well-formed one.
    fn order(&self) -> (u8, i64, Vec<u8>) {
        match &self.key {
            Object::Integer(value) => (0, *value, Vec::new()),
            Object::String(bytes) => (1, 0, bytes.clone()),
            _ => (2, 0, Vec::new()),
        }
    }
}

/// Every entry of a tree, in order, flattened out of whatever shape it had.
pub(crate) fn read(tx: &Transaction<'_>, root: Option<&Object>, kind: Kind) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut depth = 0;
    collect(tx, root, kind, &mut entries, &mut depth)?;
    entries.sort_by_key(Entry::order);
    Ok(entries)
}

fn collect(
    tx: &Transaction<'_>,
    node: Option<&Object>,
    kind: Kind,
    out: &mut Vec<Entry>,
    depth: &mut usize,
) -> Result<()> {
    *depth += 1;
    if *depth > 64 {
        return Ok(());
    }
    let resolved = resolve(tx, node)?;
    let Some(Object::Dict(dict)) = resolved else {
        *depth -= 1;
        return Ok(());
    };
    if let Some(Object::Array(items)) = resolve(tx, dict.get(kind.key().as_bytes()))? {
        for pair in items.chunks(2) {
            if let [key, value] = pair {
                out.push(Entry {
                    key: key.clone(),
                    value: value.clone(),
                });
            }
        }
    }
    if let Some(Object::Array(kids)) = resolve(tx, dict.get(b"Kids"))? {
        for kid in kids {
            collect(tx, Some(&kid), kind, out, depth)?;
        }
    }
    *depth -= 1;
    Ok(())
}

/// Write a fresh tree holding `entries`, returning the root's reference, or
/// `None` when there is nothing left to hold.
///
/// `root` is reused when there is one, so every reference to the tree still
/// resolves; its children are new objects, and the old ones become garbage
/// nothing reaches, per the free-nothing rule.
pub(crate) fn write(
    tx: &mut Transaction<'_>,
    root: Option<ObjRef>,
    kind: Kind,
    entries: &[Entry],
) -> Result<Option<ObjRef>> {
    if entries.is_empty() {
        return Ok(None);
    }
    let root = match root {
        Some(objref) => objref,
        None => ObjRef::new(tx.reserve(), 0),
    };

    if entries.len() <= LEAF_CAPACITY {
        // One node, which is both root and leaf. A root leaf carries no
        // `/Limits`: ISO 32000-1 7.9.6 gives them to every node *except* the
        // root, and writing one there makes some readers refuse the tree.
        let mut dict = Dict::new();
        dict.set(Name::new(kind.key()), flatten(entries));
        tx.put_object(root.number, root.generation, Object::Dict(dict))?;
        return Ok(Some(root));
    }

    let mut kids = Vec::new();
    for chunk in entries.chunks(LEAF_CAPACITY) {
        let mut leaf = Dict::new();
        leaf.set(Name::new(kind.key()), flatten(chunk));
        // Recomputed from the chunk's own first and last key, which is the
        // whole point: a `/Limits` copied from the old tree brackets entries
        // this leaf no longer has.
        leaf.set(Name::new("Limits"), limits(chunk));
        let objref = ObjRef::new(tx.reserve(), 0);
        tx.put_object(objref.number, 0, Object::Dict(leaf))?;
        kids.push(Object::Ref(objref));
    }

    let mut dict = Dict::new();
    dict.set(Name::new("Kids"), Object::Array(kids));
    tx.put_object(root.number, root.generation, Object::Dict(dict))?;
    Ok(Some(root))
}

fn flatten(entries: &[Entry]) -> Object {
    let mut items = Vec::with_capacity(entries.len() * 2);
    for entry in entries {
        items.push(entry.key.clone());
        items.push(entry.value.clone());
    }
    Object::Array(items)
}

fn limits(entries: &[Entry]) -> Object {
    let first = entries.first().map(|entry| entry.key.clone());
    let last = entries.last().map(|entry| entry.key.clone());
    Object::Array(vec![
        first.unwrap_or(Object::Null),
        last.unwrap_or(Object::Null),
    ])
}

/// The reference a tree-holding entry names, if it is indirect.
pub(crate) fn root_ref(tx: &Transaction<'_>, object: Option<&Object>) -> Result<Option<ObjRef>> {
    match object {
        Some(Object::Ref(objref)) => {
            // Only reuse it if it is really there; a dangling reference is
            // better replaced than written through.
            Ok(object_at(tx, *objref)?.map(|_| *objref))
        }
        _ => Ok(None),
    }
}
