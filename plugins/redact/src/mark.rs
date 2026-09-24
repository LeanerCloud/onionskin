//! Marking for redaction: text, a region, whole pages, or what a search
//! found. Each is one undo step, and nothing is removed until the marks are
//! applied.

use onionskin_core::redactions::{
    add_redaction, remove_redaction, set_redaction, RedactionLook, RedactionMark,
};
use onionskin_core::{Document, ObjRef, PageIndex, PageQuad};
use onionskin_plugin_api::CommandError;

use crate::find::Found;

fn edit_error(label: &'static str) -> impl Fn(onionskin_core::Error) -> CommandError {
    move |source| CommandError::Edit { label, source }
}

const MARK: &str = "Mark for Redaction";

/// Mark the text under `quads` on `page`.
pub fn mark_text(
    doc: &mut Document,
    page: PageIndex,
    quads: &[PageQuad],
    look: &RedactionLook,
) -> Result<ObjRef, CommandError> {
    doc.edit_annotations(MARK, |tx, _| add_redaction(tx, page, [0.0; 4], quads, look))
        .map_err(edit_error(MARK))
}

/// Mark the region `rect` on `page`.
pub fn mark_region(
    doc: &mut Document,
    page: PageIndex,
    rect: [f64; 4],
    look: &RedactionLook,
) -> Result<ObjRef, CommandError> {
    doc.edit_annotations(MARK, |tx, _| add_redaction(tx, page, rect, &[], look))
        .map_err(edit_error(MARK))
}

/// Mark the whole of each page in `pages`: its media box, so nothing drawn
/// anywhere on it survives. How many were marked.
pub fn mark_pages(
    doc: &mut Document,
    pages: &[PageIndex],
    look: &RedactionLook,
) -> Result<usize, CommandError> {
    let label = "Mark Pages for Redaction";
    let boxes = media_boxes(doc, pages, label)?;
    doc.edit_annotations(label, |tx, _| {
        for (page, rect) in &boxes {
            add_redaction(tx, *page, *rect, &[], look)?;
        }
        Ok(boxes.len())
    })
    .map_err(edit_error(label))
}

/// Mark everything `found` lists, as one step. How many were marked.
pub fn mark_found(
    doc: &mut Document,
    found: &[Found],
    look: &RedactionLook,
) -> Result<usize, CommandError> {
    let label = "Mark Search Results for Redaction";
    doc.edit_annotations(label, |tx, _| {
        for hit in found {
            add_redaction(tx, hit.page, [0.0; 4], &hit.quads, look)?;
        }
        Ok(found.len())
    })
    .map_err(edit_error(label))
}

/// Give `mark` a new look: Redaction Properties.
pub fn set_look(
    doc: &mut Document,
    mark: ObjRef,
    look: &RedactionLook,
) -> Result<(), CommandError> {
    let label = "Redaction Properties";
    doc.edit_annotations(label, |tx, _| set_redaction(tx, mark, look))
        .map_err(edit_error(label))
}

/// Take `mark` off `page` without applying it.
pub fn unmark(doc: &mut Document, page: PageIndex, mark: ObjRef) -> Result<(), CommandError> {
    let label = "Remove Redaction Mark";
    doc.edit_annotations(label, |tx, _| remove_redaction(tx, page, mark).map(|_| ()))
        .map_err(edit_error(label))
}

/// Every mark the document has now.
pub fn marks(doc: &mut Document) -> Result<Vec<RedactionMark>, CommandError> {
    doc.redactions().map_err(|source| CommandError::Failed {
        label: "Redact",
        reason: source.to_string(),
    })
}

fn media_boxes(
    doc: &mut Document,
    pages: &[PageIndex],
    label: &'static str,
) -> Result<Vec<(PageIndex, [f64; 4])>, CommandError> {
    let structure = doc.structure().map_err(edit_error(label))?;
    pages
        .iter()
        .map(|&page| {
            let node = structure.page(page).map_err(|source| CommandError::Page {
                page,
                source: source.into(),
            })?;
            Ok((page, node.media_box.unwrap_or([0.0, 0.0, 612.0, 792.0])))
        })
        .collect()
}
