//! Annotations as appended objects.
//!
//! One place authors them, so the comment tools own geometry and defaults and
//! nothing else. Each authored annotation writes its dictionary, its `/AP`
//! `/N` appearance stream, the page's rewritten `/Annots`, and on a tagged
//! document an `/Annot` structure element with a fresh `/StructParent`: one
//! transaction, one undo entry.

mod appearance;
pub(crate) mod author;
mod filter;
mod model;
pub mod properties;
mod read;
mod rebuild;
pub mod review;

pub use filter::AnnotationFilter;
pub use model::{
    Annotation, BaseFont, BorderEffect, Color, Flags, Intent, LineEnding, Quad, Rect, StampArt,
    Subtype, TextStyle,
};
pub use read::ReadAnnotation;

pub(crate) use author::text_string;

use std::collections::BTreeMap;

use onionskin_cos::{Document as CosDocument, ObjRef, PendingEdit};

use crate::edit::Transaction;
use crate::structure::Structure;
use crate::Result;

/// Write an annotation onto a page, returning its reference.
///
/// `now` is seconds since the Unix epoch, passed in rather than read here so a
/// test can assert on the dates and so nothing in `core` reaches for a clock
/// in the middle of an edit.
pub fn add_annotation(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
    annotation: &Annotation,
    now: i64,
) -> Result<ObjRef> {
    author::add(tx, structure, page, annotation, now)
}

/// Give an ink annotation new strokes, as an eraser does: new `/InkList`,
/// `/Rect`, appearance and `/M`. See `author::set_ink`.
pub fn set_ink_strokes(
    tx: &mut Transaction<'_>,
    annotation: ObjRef,
    strokes: Vec<Vec<(f64, f64)>>,
    now: i64,
) -> Result<()> {
    author::set_ink(tx, annotation, strokes, now)
}

/// Stop a page naming an annotation. Returns whether it named one.
pub fn remove_annotation(
    tx: &mut Transaction<'_>,
    page: ObjRef,
    annotation: ObjRef,
) -> Result<bool> {
    author::remove(tx, page, annotation)
}

/// Every annotation the document carries, as the session sees it: pending
/// objects win over the base, so a comment authored a moment ago is in the
/// list.
pub fn read_annotations(
    doc: &CosDocument,
    page_count: usize,
    pending: &BTreeMap<u32, PendingEdit>,
) -> Result<Vec<ReadAnnotation>> {
    read::read(doc, page_count, pending)
}

/// The PDF date string this crate writes, exposed so a caller can compare one
/// it read against one it would have written.
pub fn pdf_date(seconds_since_epoch: i64) -> String {
    author::pdf_date(seconds_since_epoch)
}
