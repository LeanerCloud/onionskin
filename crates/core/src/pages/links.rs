//! Fix-up 4: `/Link` annotations on surviving pages.
//!
//! A different object from the outline entries: a link lives in a surviving
//! page's `/Annots`, and its `/Dest` or its `/A` `/GoTo` action can name a page
//! that is gone. Left alone it is a link that resolves into a garbage page
//! object - still parseable, no longer in the tree - so clicking it takes the
//! reader somewhere that does not exist.
//!
//! A link naming a **surviving** page is left exactly as written, including
//! when that page moved: a destination names the page object, not its index,
//! so a reorder does not touch it. That is worth asserting rather than
//! assuming, because a fix-up that rewrote destinations by index would break
//! every link in a reordered document while passing a delete-only test.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Name, ObjRef, Object};

use super::destinations::names_removed_page;
use super::inherit::Leaf;
use super::rewrite::{dict_at, resolve, PageSource};
use crate::edit::Transaction;
use crate::Result;

/// Drop every link on a surviving page that names a removed one. Returns how
/// many went.
pub(crate) fn repair(
    tx: &mut Transaction<'_>,
    survivors: &BTreeMap<u32, usize>,
    leaves: &[Leaf],
    order: &[PageSource],
    removed: &BTreeSet<u32>,
) -> Result<usize> {
    if removed.is_empty() {
        return Ok(0);
    }
    let mut pages: Vec<ObjRef> = Vec::new();
    for source in order {
        match source {
            PageSource::Existing(index) => pages.push(leaves[*index].objref),
            PageSource::Imported(objref) => pages.push(*objref),
        }
    }
    let _ = survivors;

    let mut dropped = 0;
    for page in pages {
        let dict = dict_at(tx, page)?;
        let annots_entry = dict.get(b"Annots").cloned();
        let Some(Object::Array(annots)) = resolve(tx, annots_entry.as_ref())? else {
            continue;
        };

        let mut kept = Vec::with_capacity(annots.len());
        let mut changed = false;
        for annot in &annots {
            if is_doomed_link(tx, annot, removed)? {
                changed = true;
                dropped += 1;
            } else {
                kept.push(annot.clone());
            }
        }
        if !changed {
            continue;
        }

        match annots_entry.as_ref().and_then(Object::as_reference) {
            Some(objref) if !kept.is_empty() => {
                tx.put_object(objref.number, objref.generation, Object::Array(kept))?;
            }
            _ => {
                let mut dict = dict_at(tx, page)?;
                if kept.is_empty() {
                    // An empty `/Annots` is legal and says nothing; removing
                    // the key is what the annotation writer does too, so a
                    // page ends up in one shape rather than two.
                    dict.remove(b"Annots");
                } else {
                    dict.set(Name::new("Annots"), Object::Array(kept));
                }
                tx.put_object(page.number, page.generation, Object::Dict(dict))?;
            }
        }
    }
    Ok(dropped)
}

fn is_doomed_link(tx: &Transaction<'_>, annot: &Object, removed: &BTreeSet<u32>) -> Result<bool> {
    let Some(Object::Dict(dict)) = resolve(tx, Some(annot))? else {
        return Ok(false);
    };
    if dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map(|name| name.as_bytes().to_vec())
        != Some(b"Link".to_vec())
    {
        return Ok(false);
    }
    if let Some(dest) = dict.get(b"Dest") {
        if names_removed_page(tx, dest, removed)? {
            return Ok(true);
        }
    }
    if let Some(action) = dict.get(b"A") {
        if names_removed_page(tx, action, removed)? {
            return Ok(true);
        }
    }
    Ok(false)
}
