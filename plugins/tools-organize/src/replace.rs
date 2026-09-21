//! Replace Pages: one transaction, not delete-then-insert. Undoing half a
//! replacement leaves the old pages gone and the new ones not arrived, which
//! is not a state anyone chose.

use onionskin_core::{pages, Document, PageIndex};
use onionskin_plugin_api::CommandError;

use crate::edit;

/// Replace `targets` of `doc` with `pages` of `source`, one for one. Refused
/// for an encrypted `source`, by the importer, before anything is written.
pub fn replace_pages_from(
    doc: &mut Document,
    source: &mut Document,
    pages: &[PageIndex],
    targets: &[PageIndex],
) -> Result<(), CommandError> {
    const LABEL: &str = "Replace Pages";
    let source = source.structure().map_err(|source| CommandError::Edit {
        label: LABEL,
        source,
    })?;
    edit(doc, LABEL, |tx, structure| {
        pages::replace_pages_from(tx, structure, source, pages, targets).map(|_| ())
    })
}
