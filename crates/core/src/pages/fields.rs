//! Fix-up 5: `/AcroForm /Fields`.
//!
//! M3 authors no form fields. It deletes pages, and deleting a page out of a
//! form document is an ordinary thing to do - so the field tree and the page
//! tree have to be made to agree, or the form names widgets on a page nobody
//! can reach and a reader's field enumeration walks into garbage.
//!
//! Two things go: a field whose widget sat on a removed page, and a `/Parent`
//! field node left with no children by that drop. The second is the one a
//! naive filter misses, and it leaves a form with an empty group in it.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::rewrite::{dict_at, resolve};
use crate::edit::Transaction;
use crate::Result;

/// Drop every field whose widget was on a removed page. Returns how many went,
/// counting emptied parent nodes.
pub(crate) fn repair(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    removed: &BTreeSet<u32>,
) -> Result<usize> {
    if removed.is_empty() {
        return Ok(0);
    }
    let catalog = dict_at(tx, catalog_ref)?;
    let acroform_entry = catalog.get(b"AcroForm").cloned();
    let Some(Object::Dict(acroform)) = resolve(tx, acroform_entry.as_ref())? else {
        return Ok(0);
    };
    let fields_entry = acroform.get(b"Fields").cloned();
    let Some(Object::Array(fields)) = resolve(tx, fields_entry.as_ref())? else {
        return Ok(0);
    };

    let mut dropped = 0;
    let mut kept = Vec::with_capacity(fields.len());
    for field in &fields {
        if survives(tx, field, removed, &mut dropped, 0)? {
            kept.push(field.clone());
        }
    }
    if dropped == 0 {
        return Ok(0);
    }

    match fields_entry.as_ref().and_then(Object::as_reference) {
        Some(objref) => tx.set_object(objref.number, objref.generation, Object::Array(kept))?,
        None => {
            let mut acroform = acroform.clone();
            acroform.set(Name::new("Fields"), Object::Array(kept));
            match acroform_entry.as_ref().and_then(Object::as_reference) {
                Some(objref) => {
                    tx.set_object(objref.number, objref.generation, Object::Dict(acroform))?
                }
                None => {
                    let mut catalog = dict_at(tx, catalog_ref)?;
                    catalog.set(Name::new("AcroForm"), Object::Dict(acroform));
                    tx.set_object(
                        catalog_ref.number,
                        catalog_ref.generation,
                        Object::Dict(catalog),
                    )?;
                }
            }
        }
    }
    Ok(dropped)
}

/// Whether a field survives, pruning its children in place as it goes.
///
/// A terminal field is a widget: it survives when its page does. A non-terminal
/// one survives only while it still has a child, which is what removes a group
/// emptied by the drop rather than leaving it as an empty node.
fn survives(
    tx: &mut Transaction<'_>,
    field: &Object,
    removed: &BTreeSet<u32>,
    dropped: &mut usize,
    depth: usize,
) -> Result<bool> {
    if depth > 32 {
        return Ok(true);
    }
    let Some(Object::Dict(dict)) = resolve(tx, Some(field))? else {
        return Ok(true);
    };

    let kids_entry = dict.get(b"Kids").cloned();
    if let Some(Object::Array(kids)) = resolve(tx, kids_entry.as_ref())? {
        let mut kept = Vec::with_capacity(kids.len());
        for kid in &kids {
            if survives(tx, kid, removed, dropped, depth + 1)? {
                kept.push(kid.clone());
            }
        }
        if kept.is_empty() {
            *dropped += 1;
            return Ok(false);
        }
        if kept.len() != kids.len() {
            write_kids(tx, field, &dict, kids_entry.as_ref(), kept)?;
        }
        return Ok(true);
    }

    // A terminal field. A widget merged into the field dictionary names its
    // page with `/P`; one that does not is not on any page this can check, and
    // is left alone.
    match dict.get(b"P").and_then(Object::as_reference) {
        Some(page) if removed.contains(&page.number) => {
            *dropped += 1;
            Ok(false)
        }
        _ => Ok(true),
    }
}

fn write_kids(
    tx: &mut Transaction<'_>,
    field: &Object,
    dict: &Dict,
    kids_entry: Option<&Object>,
    kept: Vec<Object>,
) -> Result<()> {
    match kids_entry.and_then(Object::as_reference) {
        Some(objref) => tx.set_object(objref.number, objref.generation, Object::Array(kept))?,
        None => {
            let Some(objref) = field.as_reference() else {
                return Ok(());
            };
            let mut dict = dict.clone();
            dict.set(Name::new("Kids"), Object::Array(kept));
            tx.set_object(objref.number, objref.generation, Object::Dict(dict))?;
        }
    }
    Ok(())
}
