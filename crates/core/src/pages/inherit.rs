//! The raw page-tree walk, and materializing what a page inherits.
//!
//! **Why this does not use `cos::Document::page`.** That accessor resolves the
//! four inheritable attributes for a reader, and its resolved view is lossy for
//! exactly the two things materialization exists to preserve:
//!
//! - `/Resources` comes back as a `Dict` **value**, never the `Object::Ref` the
//!   file had. Materializing from it inlines a full copy of a shared resource
//!   dictionary into every rewritten page dict: a document whose thousand pages
//!   share one gets a thousand copies of it.
//! - `MediaBox` and `CropBox` come back through a policy filter that yields
//!   `None` for a degenerate or non-finite box. Materializing from that emits a
//!   page with **no** `/MediaBox`, under a flat `/Pages` node that has none
//!   either - turning a page with a bad box into a page with no box.
//!
//! Both are invisible to a before-and-after comparison that reads through the
//! same accessor. So this walks the dictionaries itself and carries every
//! inheritable entry **exactly as written**, reference or value, sane or not.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Name, ObjRef, Object};

use crate::{Error, Result};

/// How deep a page tree may be before this refuses to walk it. The same order
/// as `cos`'s own indirection limit: a tree deeper than this is a loop or an
/// attack, not a document.
const MAX_DEPTH: usize = 64;

/// How many nodes a walk may visit. A page tree that shares nodes legitimately
/// can visit more nodes than it has pages, so this is not a page count.
const MAX_NODES: usize = 500_000;

/// The four inheritable entries, each exactly as the nearest ancestor wrote it.
#[derive(Clone, Default, Debug, PartialEq)]
pub(crate) struct Inheritable {
    pub(crate) resources: Option<Object>,
    pub(crate) media_box: Option<Object>,
    pub(crate) crop_box: Option<Object>,
    pub(crate) rotate: Option<Object>,
}

impl Inheritable {
    /// The keys, paired with their values, in the order a page dict carries
    /// them. One list, so a key cannot be read in one place and written in
    /// another.
    pub(crate) fn entries(&self) -> [(&'static str, Option<&Object>); 4] {
        [
            ("Resources", self.resources.as_ref()),
            ("MediaBox", self.media_box.as_ref()),
            ("CropBox", self.crop_box.as_ref()),
            ("Rotate", self.rotate.as_ref()),
        ]
    }

    /// Take whatever this dictionary states, leaving the rest inherited.
    fn absorb(&mut self, dict: &Dict) {
        for (key, slot) in [
            (b"Resources".as_slice(), &mut self.resources),
            (b"MediaBox".as_slice(), &mut self.media_box),
            (b"CropBox".as_slice(), &mut self.crop_box),
            (b"Rotate".as_slice(), &mut self.rotate),
        ] {
            if let Some(value) = dict.get(key) {
                *slot = Some(value.clone());
            }
        }
    }
}

/// One leaf of the page tree, in document order.
#[derive(Clone, Debug)]
pub(crate) struct Leaf {
    pub(crate) objref: ObjRef,
    pub(crate) dict: Dict,
    /// What this page ends up with after inheritance, which is what a flat
    /// tree has to write onto the page itself.
    pub(crate) inherited: Inheritable,
}

impl Leaf {
    /// The page dict with its inheritance materialized: every inheritable entry
    /// the page did not state itself, written on as the ancestor wrote it.
    ///
    /// Materialized **before** `/Parent` is changed, which is the ordering a
    /// reviewer checks: materializing afterwards reads inheritance through the
    /// new flat node, finds nothing above the page, and silently drops every
    /// inherited attribute. That passes on a shallow tree, where the page
    /// already carried everything.
    pub(crate) fn materialized(&self, parent: ObjRef) -> Dict {
        let mut dict = self.dict.clone();
        for (key, value) in self.inherited.entries() {
            if let (None, Some(value)) = (dict.get(key.as_bytes()), value) {
                dict.set(Name::new(key), value.clone());
            }
        }
        dict.set(Name::new("Parent"), Object::Ref(parent));
        dict.set(Name::new("Type"), Object::name("Page"));
        dict
    }
}

/// Every leaf of the page tree rooted at `root`, in document order.
///
/// `resolve` reads an object by number from wherever the caller's current view
/// of the document is, so this walks the **edited** document rather than the
/// bytes on disk.
pub(crate) fn walk(
    root: ObjRef,
    resolve: &mut dyn FnMut(u32) -> Result<Option<Object>>,
) -> Result<Vec<Leaf>> {
    let mut leaves = Vec::new();
    let mut path = BTreeSet::new();
    let mut visits = 0usize;
    descend(
        root,
        &Inheritable::default(),
        resolve,
        &mut leaves,
        &mut path,
        &mut visits,
        0,
    )?;
    Ok(leaves)
}

fn descend(
    node: ObjRef,
    inherited: &Inheritable,
    resolve: &mut dyn FnMut(u32) -> Result<Option<Object>>,
    leaves: &mut Vec<Leaf>,
    path: &mut BTreeSet<u32>,
    visits: &mut usize,
    depth: usize,
) -> Result<()> {
    *visits += 1;
    if depth >= MAX_DEPTH || *visits > MAX_NODES {
        return Err(Error::PageTreeTooLarge {
            depth,
            visits: *visits,
        });
    }
    // A path set rather than a visited set: a node legitimately shared between
    // two branches still walks, while a cycle terminates.
    if !path.insert(node.number) {
        return Ok(());
    }
    let Some(Object::Dict(dict)) = resolve(node.number)? else {
        path.remove(&node.number);
        return Ok(());
    };

    let mut inherited = inherited.clone();
    inherited.absorb(&dict);

    // A node with `/Kids` is internal even when it also claims `/Type /Page`,
    // which some producers do; a node without them is a leaf whatever it
    // claims. `cos` reads the tree the same way, and the two have to agree or
    // a rewrite renumbers a different set of pages than the viewer shows.
    let kids = match resolve_entry(&dict, b"Kids", resolve)? {
        Some(Object::Array(kids)) => kids,
        _ => {
            leaves.push(Leaf {
                objref: node,
                dict,
                inherited,
            });
            path.remove(&node.number);
            return Ok(());
        }
    };
    for kid in kids {
        if let Some(kid) = kid.as_reference() {
            descend(kid, &inherited, resolve, leaves, path, visits, depth + 1)?;
        }
    }
    path.remove(&node.number);
    Ok(())
}

/// A dictionary entry with one level of indirection followed.
pub(crate) fn resolve_entry(
    dict: &Dict,
    key: &[u8],
    resolve: &mut dyn FnMut(u32) -> Result<Option<Object>>,
) -> Result<Option<Object>> {
    match dict.get(key) {
        Some(Object::Ref(objref)) => resolve(objref.number),
        other => Ok(other.cloned()),
    }
}
