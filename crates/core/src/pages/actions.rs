//! Fix-up 7: `/OpenAction` and page-level `/AA`.
//!
//! The catalog's `/OpenAction` can name the page a document opens on. If that
//! page is gone the document opens by following a reference into a garbage
//! object, which is worse than opening at page one.
//!
//! **This module drops it rather than retargeting it.** Retargeting would have
//! to choose a page, and there is no honest choice: the page the user deleted
//! is not "the one after it", and silently landing somewhere else is a document
//! that lies about where it opens. Dropping the key makes a reader open at the
//! first page, which is the documented default and what the user gets from a
//! document that never had one.
//!
//! A **surviving** page's `/AA` is untouched. Its actions are that page's own,
//! the page is still there, and "unimplemented means untouched" applies: the
//! only page-level actions this looks at are ones naming a removed page.

use std::collections::BTreeSet;

use onionskin_cos::{Name, ObjRef, Object};

use super::destinations::names_removed_page;
use super::rewrite::dict_at;
use crate::edit::Transaction;
use crate::Result;

/// Drop `/OpenAction` when it names a removed page. Returns whether it did.
pub(crate) fn repair(
    tx: &mut Transaction<'_>,
    catalog_ref: ObjRef,
    removed: &BTreeSet<u32>,
) -> Result<bool> {
    if removed.is_empty() {
        return Ok(false);
    }
    let catalog = dict_at(tx, catalog_ref)?;
    let Some(action) = catalog.get(b"OpenAction").cloned() else {
        return Ok(false);
    };
    if !names_removed_page(tx, &action, removed)? {
        return Ok(false);
    }

    let mut catalog = dict_at(tx, catalog_ref)?;
    catalog.remove(b"OpenAction");
    tx.set_object(
        catalog_ref.number,
        catalog_ref.generation,
        Object::Dict(catalog),
    )?;
    let _ = Name::new("OpenAction");
    Ok(true)
}
