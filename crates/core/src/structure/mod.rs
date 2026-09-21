//! The tagged-PDF structure tree: reading it, keeping it valid through an edit,
//! and proving that with an invariant.
//!
//! What M3 owes here is narrow and worth stating plainly. It is not a PDF/UA
//! checker, not reading-order repair, not autotagging and not `/RoleMap`
//! resolution beyond what the invariant needs; those land at M5. It is: read
//! the tree, maintain it through the three edits M3 can make, and check the
//! result.
//!
//! The invariant is callable from every editing package's tests, which is the
//! point of it. A package that moves pages or elements asserts on
//! [`check`] rather than inventing its own idea of a valid tree.

mod invariant;
mod maintain;
mod read;

pub use invariant::{Report, Violation};
pub use maintain::Maintenance;
pub use read::{Element, Kid, ParentEntry, Structure, StructureTree};

use onionskin_cos::{Document as CosDocument, ObjRef};

use crate::edit::Transaction;
use crate::Result;

/// Read the structure tree, or report that this document has none.
pub fn read_structure(doc: &CosDocument) -> Result<Structure> {
    read::read(doc)
}

/// Check the tree against the document it describes. An untagged document
/// passes with an empty report.
pub fn check(doc: &CosDocument, structure: &Structure, page_count: usize) -> Result<Report> {
    invariant::check(doc, structure, page_count)
}

/// Remove a page's elements and their `/ParentTree` entries.
pub fn remove_page(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
) -> Result<Maintenance> {
    maintain::remove_page(tx, structure, page)
}

/// Remove several pages' elements in one rewrite. See `maintain::remove_pages`
/// for why a loop over [`remove_page`] is wrong.
pub fn remove_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    pages: &[ObjRef],
) -> Result<Maintenance> {
    maintain::remove_pages(tx, structure, pages)
}

/// Reorder the root's `/K` to match a new page order.
pub fn reorder_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    order: &[ObjRef],
) -> Result<Maintenance> {
    maintain::reorder_pages(tx, structure, order)
}

/// Attach an `/Annot` element for an annotation, returning the
/// `/StructParent` index the caller writes onto the annotation itself.
pub fn attach_annotation(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
    annotation: ObjRef,
) -> Result<(Maintenance, Option<i64>)> {
    maintain::attach_annotation(tx, structure, page, annotation)
}
