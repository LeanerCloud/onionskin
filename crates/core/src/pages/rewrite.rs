//! The transformation itself: a new page order becomes a flat `/Pages` node.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::inherit::{walk, Leaf};
use super::{actions, destinations, fields, labels, links, outline, threads};
use crate::edit::Transaction;
use crate::structure::{remove_page, reorder_pages, Structure};
use crate::{Error, Result};

/// Where a page in the new order comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSource {
    /// A page of this document, by its current index.
    Existing(usize),
    /// A page already imported into this document as an object of its own,
    /// with its references renumbered. The importer produces these; this
    /// module only places them.
    Imported(ObjRef),
}

/// What a rewrite changed, in the terms a user interface reports.
///
/// The counts are **real**, not estimates: each fix-up returns what it actually
/// dropped, so "3 bookmarks removed" means three entries left the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rewrite {
    pub pages_before: usize,
    pub pages_after: usize,
    /// Named destinations, in `/Dests` and the `/Names /Dests` tree together.
    pub destinations_dropped: usize,
    pub bookmarks_dropped: usize,
    pub links_dropped: usize,
    pub form_fields_dropped: usize,
    pub article_beads_dropped: usize,
    pub threads_dropped: usize,
    /// Whether the catalog's `/OpenAction` named a page that is gone.
    pub open_action_dropped: bool,
    pub page_labels_rebuilt: bool,
}

/// Rewrite the page tree to `order`, repairing everything a page-set change
/// breaks.
///
/// The whole thing is one transaction: a page tree rewritten without its
/// destinations repaired is not a state anyone should be able to undo *into*.
///
/// `structure` is the document's structure tree as read before the edit, the
/// same convention `add_annotation` follows: the tree is read once by the
/// caller, and P4's maintenance hooks keep it valid through the rewrite.
pub fn rewrite_page_tree(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    order: &[PageSource],
) -> Result<Rewrite> {
    let catalog_ref = catalog_ref(tx)?;
    let catalog = dict_at(tx, catalog_ref)?;
    let root = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .ok_or(Error::NoPageTree)?;

    let leaves = {
        let mut resolve = resolver(tx);
        walk(root, &mut resolve)?
    };

    // Refused rather than aliased: see `Error::RepeatedPage`.
    let mut seen = BTreeSet::new();
    for source in order {
        if let PageSource::Existing(index) = source {
            if *index >= leaves.len() {
                return Err(Error::NoSuchPage {
                    page: *index,
                    count: leaves.len(),
                });
            }
            if !seen.insert(*index) {
                return Err(Error::RepeatedPage { index: *index });
            }
        }
    }

    // Which old index each surviving page ends up at, and which are gone.
    // Both are what every fix-up below asks about, so they are computed once.
    let mut moved: BTreeMap<usize, usize> = BTreeMap::new();
    for (position, source) in order.iter().enumerate() {
        if let PageSource::Existing(index) = source {
            moved.insert(*index, position);
        }
    }
    let removed: BTreeSet<u32> = leaves
        .iter()
        .enumerate()
        .filter(|(index, _)| !moved.contains_key(index))
        .map(|(_, leaf)| leaf.objref.number)
        .collect();
    let survivors: BTreeMap<u32, usize> = moved
        .iter()
        .map(|(old, new)| (leaves[*old].objref.number, *new))
        .collect();

    let mut kids = Vec::with_capacity(order.len());
    for source in order {
        match source {
            PageSource::Existing(index) => {
                let leaf: &Leaf = &leaves[*index];
                // Materialized before the parent pointer changes; see
                // `Leaf::materialized` for why the order is the thing.
                let dict = leaf.materialized(root);
                tx.put_object(
                    leaf.objref.number,
                    leaf.objref.generation,
                    Object::Dict(dict),
                )?;
                kids.push(Object::Ref(leaf.objref));
            }
            PageSource::Imported(objref) => {
                let mut dict = dict_at(tx, *objref)?;
                dict.set(Name::new("Parent"), Object::Ref(root));
                dict.set(Name::new("Type"), Object::name("Page"));
                tx.put_object(objref.number, objref.generation, Object::Dict(dict))?;
                kids.push(Object::Ref(*objref));
            }
        }
    }

    // The flat node, on the original root's object number. Reusing it means
    // the catalog's `/Pages` still resolves and every incremental section that
    // ever named it still names the page tree.
    let mut flat = Dict::new();
    flat.set(Name::new("Type"), Object::name("Pages"));
    flat.set(Name::new("Count"), Object::Integer(kids.len() as i64));
    flat.set(Name::new("Kids"), Object::Array(kids));
    tx.put_object(root.number, root.generation, Object::Dict(flat))?;

    let mut report = Rewrite {
        pages_before: leaves.len(),
        pages_after: order.len(),
        ..Rewrite::default()
    };

    // Seven fix-ups, seven calls. Each walks a different part of the document
    // and none of them finds another's case.
    report.page_labels_rebuilt = labels::rebuild(tx, catalog_ref, &moved, leaves.len())?;
    report.destinations_dropped = destinations::repair(tx, catalog_ref, &removed)?;
    let outline = outline::repair(tx, catalog_ref, &removed)?;
    report.bookmarks_dropped = outline;
    report.links_dropped = links::repair(tx, &survivors, &leaves, order, &removed)?;
    report.form_fields_dropped = fields::repair(tx, catalog_ref, &removed)?;
    let threads = threads::repair(tx, catalog_ref, &removed)?;
    report.article_beads_dropped = threads.beads;
    report.threads_dropped = threads.threads;
    report.open_action_dropped = actions::repair(tx, catalog_ref, &removed)?;

    // The structure tree, through P4's hooks rather than a second idea of what
    // a valid tree is. Removed pages first, so the reorder sees only survivors.
    // `/StructParents` needs no renumbering: `/ParentTree` is keyed on those
    // values, not on page indices, so a surviving page's key still resolves
    // wherever the page ends up.
    for number in &removed {
        let objref = leaves
            .iter()
            .find(|leaf| leaf.objref.number == *number)
            .map(|leaf| leaf.objref)
            .expect("a removed page came from the walk");
        remove_page(tx, structure, objref)?;
    }
    let new_order: Vec<ObjRef> = order
        .iter()
        .map(|source| match source {
            PageSource::Existing(index) => leaves[*index].objref,
            PageSource::Imported(objref) => *objref,
        })
        .collect();
    reorder_pages(tx, structure, &new_order)?;

    Ok(report)
}

/// A reader that sees the transaction's own pending edits, so a fix-up reads
/// what the rewrite has already written rather than what is on disk.
pub(crate) fn resolver<'t, 'b>(
    tx: &'t Transaction<'b>,
) -> impl FnMut(u32) -> Result<Option<Object>> + 't {
    move |number| Ok(tx.object(number)?.map(|state| state.object))
}

pub(crate) fn object_at(tx: &Transaction<'_>, objref: ObjRef) -> Result<Option<Object>> {
    Ok(tx.object(objref.number)?.map(|state| state.object))
}

pub(crate) fn dict_at(tx: &Transaction<'_>, objref: ObjRef) -> Result<Dict> {
    match object_at(tx, objref)? {
        Some(Object::Dict(dict)) => Ok(dict),
        Some(Object::Stream(stream)) => Ok(stream.dict),
        _ => Err(Error::NotADictionary {
            number: objref.number,
        }),
    }
}

/// One level of indirection followed, against the transaction's view.
pub(crate) fn resolve(tx: &Transaction<'_>, object: Option<&Object>) -> Result<Option<Object>> {
    match object {
        Some(Object::Ref(objref)) => object_at(tx, *objref),
        other => Ok(other.cloned()),
    }
}

pub(crate) fn catalog_ref(tx: &Transaction<'_>) -> Result<ObjRef> {
    tx.trailer_value(b"Root")
        .and_then(|object| object.as_reference())
        .ok_or(Error::NoPageTree)
}
