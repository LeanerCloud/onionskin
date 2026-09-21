//! Fix-up 2: named destinations, in both places a document keeps them.
//!
//! `/Dests` in the catalog is a flat dictionary; `/Names /Dests` is a **name
//! tree**. A fix-up that walks only the flat one repairs the older form and
//! leaves every modern document broken, which is the shape of mistake this
//! module's name is meant to make hard to repeat.
//!
//! An entry naming a removed page is dropped, and the count is returned so a
//! user interface can say what it dropped rather than estimate.
//!
//! The name tree is **rebuilt**, not filtered: see [`super::tree`] for why
//! filtering passes every structural check and breaks lookup.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::rewrite::{dict_at, resolve};
use super::tree::{self, Entry, Kind};
use crate::edit::Transaction;
use crate::Result;

/// Drop every destination naming a removed page. Returns how many went.
pub(crate) fn repair(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    removed: &BTreeSet<u32>,
) -> Result<usize> {
    let mut dropped = flat(tx, catalog_ref, removed)?;
    dropped += name_tree(tx, catalog_ref, removed)?;
    Ok(dropped)
}

/// `/Dests`, the flat dictionary.
fn flat(tx: &mut Transaction<'_>, catalog_ref: ObjRef, removed: &BTreeSet<u32>) -> Result<usize> {
    let catalog = dict_at(tx, catalog_ref)?;
    let Some(entry) = catalog.get(b"Dests").cloned() else {
        return Ok(0);
    };
    let Some(Object::Dict(dests)) = resolve(tx, Some(&entry))? else {
        return Ok(0);
    };

    let mut kept = Dict::new();
    let mut dropped = 0;
    for (key, value) in dests.iter() {
        if names_removed_page(tx, value, removed)? {
            dropped += 1;
        } else {
            kept.set(key.clone(), value.clone());
        }
    }
    if dropped == 0 {
        return Ok(0);
    }

    match entry.as_reference() {
        Some(objref) => {
            tx.set_object(objref.number, objref.generation, Object::Dict(kept))?;
        }
        None => {
            let mut catalog = dict_at(tx, catalog_ref)?;
            catalog.set(Name::new("Dests"), Object::Dict(kept));
            tx.set_object(
                catalog_ref.number,
                catalog_ref.generation,
                Object::Dict(catalog),
            )?;
        }
    }
    Ok(dropped)
}

/// `/Names /Dests`, the name tree.
fn name_tree(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    removed: &BTreeSet<u32>,
) -> Result<usize> {
    let catalog = dict_at(tx, catalog_ref)?;
    let names_entry = catalog.get(b"Names").cloned();
    let Some(Object::Dict(names)) = resolve(tx, names_entry.as_ref())? else {
        return Ok(0);
    };
    let Some(dests) = names.get(b"Dests").cloned() else {
        return Ok(0);
    };

    let entries = tree::read(tx, Some(&dests), Kind::NameString)?;
    let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
    let mut dropped = 0;
    for entry in entries {
        if names_removed_page(tx, &entry.value, removed)? {
            dropped += 1;
        } else {
            kept.push(entry);
        }
    }
    if dropped == 0 {
        return Ok(0);
    }

    let root = tree::root_ref(tx, Some(&dests))?;
    let written = tree::write(tx, root, Kind::NameString, &kept)?;
    let mut names = names;
    match written {
        Some(objref) => names.set(Name::new("Dests"), Object::Ref(objref)),
        None => {
            names.remove(b"Dests");
        }
    }
    write_names(tx, catalog_ref, names_entry.as_ref(), names)?;
    Ok(dropped)
}

fn write_names(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    entry: Option<&Object>,
    names: Dict,
) -> Result<()> {
    match entry.and_then(Object::as_reference) {
        Some(objref) => tx.set_object(objref.number, objref.generation, Object::Dict(names))?,
        None => {
            let mut catalog = dict_at(tx, catalog_ref)?;
            catalog.set(Name::new("Names"), Object::Dict(names));
            tx.set_object(
                catalog_ref.number,
                catalog_ref.generation,
                Object::Dict(catalog),
            )?;
        }
    }
    Ok(())
}

/// Whether a destination - in any of the three shapes one can take - lands on
/// a page that is gone.
///
/// The shapes: an array whose first element is the page reference; a dictionary
/// with `/D` holding that array; and an action dictionary with `/S /GoTo` and
/// its own `/D`. A fix-up that knows only the first misses every destination a
/// bookmark actually uses.
pub(crate) fn names_removed_page(
    tx: &Transaction<'_>,
    destination: &Object,
    removed: &BTreeSet<u32>,
) -> Result<bool> {
    Ok(page_of(tx, destination, 0)?.is_some_and(|number| removed.contains(&number)))
}

/// The page object a destination names, if it names one directly.
pub(crate) fn page_of(
    tx: &Transaction<'_>,
    destination: &Object,
    depth: usize,
) -> Result<Option<u32>> {
    if depth > 8 {
        return Ok(None);
    }
    let resolved = resolve(tx, Some(destination))?;
    match resolved {
        Some(Object::Array(items)) => Ok(items
            .first()
            .and_then(Object::as_reference)
            .map(|objref| objref.number)),
        Some(Object::Dict(dict)) => {
            // Both a destination dictionary and a `/GoTo` action keep the
            // array under `/D`.
            if let Some(inner) = dict.get(b"D") {
                return page_of(tx, inner, depth + 1);
            }
            if let Some(action) = dict.get(b"A") {
                return page_of(tx, action, depth + 1);
            }
            Ok(None)
        }
        // A named destination: a string naming an entry in one of the two
        // tables above. Those tables are repaired in the same pass, so a name
        // still present resolves and one that was dropped resolves to nothing
        // - which is a broken link, not a dangling reference.
        _ => Ok(None),
    }
}
