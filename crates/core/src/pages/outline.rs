//! Fix-up 3: the outline chain.
//!
//! **This is the half that fails silently.** The destinations fix-up handles an
//! outline item's own `/A` and `/D`; this handles the *chain* those items hang
//! in, and under the free-nothing rule a sibling that still names a removed
//! item resolves into garbage rather than into nothing. A reader following
//! `/Next` walks into a dictionary that is still there, still parses, and is no
//! longer part of the tree.
//!
//! So four things: re-link `/Prev` and `/Next` past a dropped item; fix the
//! parent's `/First` and `/Last` when the dropped item was one; recompute every
//! ancestor's `/Count` **preserving its sign**, because the sign is what says
//! whether a node is open and a recount that returns a magnitude closes every
//! expanded bookmark in the document; and unlink a node whose descendants are
//! all gone.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::destinations::names_removed_page;
use super::rewrite::dict_at;
use crate::edit::Transaction;
use crate::Result;

/// Drop every outline item naming a removed page, and repair the chain around
/// it. Returns how many items went.
pub(crate) fn repair(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    removed: &BTreeSet<u32>,
) -> Result<usize> {
    let catalog = dict_at(tx, catalog_ref)?;
    let Some(Object::Ref(root)) = catalog.get(b"Outlines").cloned() else {
        return Ok(0);
    };
    if dict_at(tx, root).is_err() {
        return Ok(0);
    }

    // The whole tree, read once: chains are re-linked in memory and written
    // back at the end, because a re-link that writes as it walks reads objects
    // it has already changed.
    let mut items: BTreeMap<u32, Dict> = BTreeMap::new();
    let mut order: Vec<u32> = Vec::new();
    read_tree(tx, root, &mut items, &mut order, 0)?;

    let mut doomed: BTreeSet<u32> = BTreeSet::new();
    for number in &order {
        let dict = &items[number];
        let names_gone = match dict.get(b"Dest") {
            Some(dest) => names_removed_page(tx, dest, removed)?,
            None => match dict.get(b"A") {
                Some(action) => names_removed_page(tx, action, removed)?,
                None => false,
            },
        };
        if names_gone {
            doomed.insert(*number);
        }
    }
    if doomed.is_empty() {
        return Ok(0);
    }

    // An item whose children all go and which names nothing itself stays: it
    // is a heading, and removing a heading the user still has pages under is
    // not this fix-up's business. An item that is doomed takes its subtree
    // with it, because a child of a dropped bookmark has no parent to hang on.
    let mut dropped: BTreeSet<u32> = BTreeSet::new();
    for number in &order {
        if doomed.contains(number) || parent_dropped(&items, *number, &doomed) {
            dropped.insert(*number);
        }
    }

    for number in &dropped {
        unlink(&mut items, *number);
    }
    for number in &dropped {
        items.remove(number);
    }
    recount(tx, &mut items, root)?;

    // Only the survivors are written back. The dropped dictionaries stay in
    // the file, unreferenced, per the free-nothing rule.
    for (number, dict) in &items {
        tx.put_object(*number, 0, Object::Dict(dict.clone()))?;
    }
    Ok(dropped.len())
}

fn parent_dropped(items: &BTreeMap<u32, Dict>, number: u32, doomed: &BTreeSet<u32>) -> bool {
    let mut at = number;
    for _ in 0..64 {
        let Some(dict) = items.get(&at) else {
            return false;
        };
        let Some(parent) = dict.get(b"Parent").and_then(Object::as_reference) else {
            return false;
        };
        if doomed.contains(&parent.number) {
            return true;
        }
        at = parent.number;
    }
    false
}

fn read_tree(
    tx: &Transaction<'_>,
    node: ObjRef,
    items: &mut BTreeMap<u32, Dict>,
    order: &mut Vec<u32>,
    depth: usize,
) -> Result<()> {
    if depth > 64 || items.contains_key(&node.number) {
        return Ok(());
    }
    let Ok(dict) = dict_at(tx, node) else {
        return Ok(());
    };
    items.insert(node.number, dict.clone());
    order.push(node.number);

    let mut child = dict.get(b"First").and_then(Object::as_reference);
    let mut guard = 0;
    while let Some(objref) = child {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        read_tree(tx, objref, items, order, depth + 1)?;
        child = items
            .get(&objref.number)
            .and_then(|kid| kid.get(b"Next"))
            .and_then(Object::as_reference);
        if child.is_some_and(|next| items.contains_key(&next.number)) {
            break;
        }
    }
    Ok(())
}

/// Take one item out of its sibling chain and out of its parent's ends.
fn unlink(items: &mut BTreeMap<u32, Dict>, number: u32) {
    let Some(dict) = items.get(&number).cloned() else {
        return;
    };
    let previous = dict.get(b"Prev").and_then(Object::as_reference);
    let next = dict.get(b"Next").and_then(Object::as_reference);
    let parent = dict.get(b"Parent").and_then(Object::as_reference);

    if let Some(previous) = previous.and_then(|objref| items.get_mut(&objref.number)) {
        match next {
            Some(next) => previous.set(Name::new("Next"), Object::Ref(next)),
            None => {
                previous.remove(b"Next");
            }
        }
    }
    if let Some(next) = next.and_then(|objref| items.get_mut(&objref.number)) {
        match previous {
            Some(previous) => next.set(Name::new("Prev"), Object::Ref(previous)),
            None => {
                next.remove(b"Prev");
            }
        }
    }
    if let Some(parent) = parent.and_then(|objref| items.get_mut(&objref.number)) {
        for (key, replacement) in [("First", next), ("Last", previous)] {
            if parent
                .get(key.as_bytes())
                .and_then(Object::as_reference)
                .map(|objref| objref.number)
                == Some(number)
            {
                match replacement {
                    Some(objref) => parent.set(Name::new(key), Object::Ref(objref)),
                    None => {
                        parent.remove(key.as_bytes());
                    }
                }
            }
        }
    }
}

/// Recompute `/Count` on every node that has one, **keeping its sign**.
///
/// ISO 32000-1 12.3.3: a positive count is an open node showing that many
/// descendants, a negative one is a closed node hiding that many. A recount
/// that writes the magnitude opens every collapsed bookmark in the document,
/// and every structural check still passes.
fn recount(tx: &mut Transaction<'_>, items: &mut BTreeMap<u32, Dict>, root: ObjRef) -> Result<()> {
    let _ = tx;
    let numbers: Vec<u32> = items.keys().copied().collect();
    for number in numbers {
        let Some(dict) = items.get(&number) else {
            continue;
        };
        let had = dict.get(b"Count").and_then(Object::as_integer);
        if had.is_none() && number != root.number {
            // A leaf with no children never had a `/Count` and does not gain
            // one: adding one is a change to a part of the file this fix-up
            // was not asked to touch.
            if dict.get(b"First").is_none() {
                continue;
            }
        }
        let open = had.is_none_or(|count| count >= 0);
        let total = descendants(items, number, 0);
        if total == 0 && number != root.number {
            items.entry(number).and_modify(|dict| {
                dict.remove(b"Count");
            });
            continue;
        }
        let value = if open { total } else { -total };
        items.entry(number).and_modify(|dict| {
            dict.set(Name::new("Count"), Object::Integer(value));
        });
    }
    Ok(())
}

/// Every descendant of a node, which is what `/Count`'s magnitude means for an
/// open node: children plus the descendants of open children.
fn descendants(items: &BTreeMap<u32, Dict>, number: u32, depth: usize) -> i64 {
    if depth > 64 {
        return 0;
    }
    let Some(dict) = items.get(&number) else {
        return 0;
    };
    let mut total = 0;
    let mut child = dict.get(b"First").and_then(Object::as_reference);
    let mut guard = 0;
    while let Some(objref) = child {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        total += 1;
        let open = items
            .get(&objref.number)
            .and_then(|kid| kid.get(b"Count"))
            .and_then(Object::as_integer)
            .is_some_and(|count| count > 0);
        if open {
            total += descendants(items, objref.number, depth + 1);
        }
        child = items
            .get(&objref.number)
            .and_then(|kid| kid.get(b"Next"))
            .and_then(Object::as_reference);
    }
    total
}
