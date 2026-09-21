//! Fix-up 1: `/PageLabels`.
//!
//! A number tree keyed on page **index**, whose entries each begin a numbering
//! range that runs until the next entry. Reordering or deleting pages moves
//! every key after the first change, so the tree is rebuilt rather than edited:
//! a label range whose key still points at the old index labels the wrong
//! pages, and the tree is still perfectly well-formed while it does.
//!
//! The rebuild is per page rather than per range. Ranges are recovered
//! afterwards by dropping any entry whose label would follow from the one
//! before it, which is what keeps a 1000-page document's tree small while
//! surviving a reorder that interleaves two ranges.

use std::collections::BTreeMap;

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::rewrite::{dict_at, resolve};
use super::tree::{self, Entry, Kind};
use crate::edit::Transaction;
use crate::Result;

/// Rebuild `/PageLabels` for the new order. Returns whether there was one.
pub(crate) fn rebuild(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    moved: &BTreeMap<usize, usize>,
    pages_before: usize,
) -> Result<bool> {
    let catalog = dict_at(tx, catalog_ref)?;
    let Some(labels) = catalog.get(b"PageLabels").cloned() else {
        return Ok(false);
    };
    let root = tree::root_ref(tx, Some(&labels))?;
    let entries = tree::read(tx, Some(&labels), Kind::Number)?;
    if entries.is_empty() {
        return Ok(false);
    }

    // The label dictionary in force at each old page index, by walking the
    // ranges. A range runs from its key until the next one.
    let mut in_force: Vec<(Dict, i64)> = Vec::with_capacity(pages_before);
    let mut current: Option<(Dict, i64)> = None;
    let mut next = 0usize;
    for entry in &entries {
        let Object::Integer(start) = entry.key else {
            continue;
        };
        let start = start.max(0) as usize;
        while next < start && next < pages_before {
            if let Some((dict, offset)) = &current {
                in_force.push((dict.clone(), *offset));
            } else {
                in_force.push((Dict::new(), 0));
            }
            bump(&mut current);
            next += 1;
        }
        let Some(Object::Dict(dict)) = resolve(tx, Some(&entry.value))? else {
            continue;
        };
        let first = match resolve(tx, dict.get(b"St"))? {
            Some(Object::Integer(value)) => value,
            _ => 1,
        };
        current = Some((dict, first));
        next = start;
    }
    while next < pages_before {
        if let Some((dict, offset)) = &current {
            in_force.push((dict.clone(), *offset));
        } else {
            in_force.push((Dict::new(), 0));
        }
        bump(&mut current);
        next += 1;
    }

    // One entry per surviving page, each its own one-page range, then the
    // runs collapsed back where consecutive pages continue a sequence.
    let mut per_page: BTreeMap<usize, (Dict, i64)> = BTreeMap::new();
    for (old, new) in moved {
        if let Some(label) = in_force.get(*old) {
            per_page.insert(*new, label.clone());
        }
    }

    let mut rebuilt: Vec<Entry> = Vec::new();
    let mut previous: Option<(Dict, i64)> = None;
    for (index, (dict, number)) in &per_page {
        let continues = previous
            .as_ref()
            .is_some_and(|(before, count)| before == dict && count + 1 == *number);
        if !continues {
            let mut range = dict.clone();
            range.set(Name::new("St"), Object::Integer(*number));
            rebuilt.push(Entry {
                key: Object::Integer(*index as i64),
                value: Object::Dict(range),
            });
        }
        previous = Some((dict.clone(), *number));
    }

    let written = tree::write(tx, root, Kind::Number, &rebuilt)?;
    let mut catalog = dict_at(tx, catalog_ref)?;
    match written {
        Some(objref) => catalog.set(Name::new("PageLabels"), Object::Ref(objref)),
        None => {
            catalog.remove(b"PageLabels");
        }
    }
    tx.set_object(
        catalog_ref.number,
        catalog_ref.generation,
        Object::Dict(catalog),
    )?;
    Ok(true)
}

fn bump(current: &mut Option<(Dict, i64)>) {
    if let Some((_, number)) = current {
        *number += 1;
    }
}
