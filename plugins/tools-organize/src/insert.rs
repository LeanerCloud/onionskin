//! Insert Pages: blank, from a file, or from another open document, and the
//! move between open documents built on the last.

use std::path::Path;

use onionskin_core::{pages, Document, PageIndex};
use onionskin_plugin_api::command_ids::INSERT_BLANK_PAGE;
use onionskin_plugin_api::{Command, CommandError};

use crate::{edit, editing};

/// Insert `count` blank pages of `media_box` before position `at`, as one
/// undo step.
pub fn insert_blank_pages(
    doc: &mut Document,
    at: PageIndex,
    count: usize,
    media_box: [f64; 4],
) -> Result<(), CommandError> {
    edit(doc, "Insert Blank Pages", |tx, structure| {
        pages::insert_blank_pages(tx, structure, at, count, media_box).map(|_| ())
    })
}

/// Insert `pages` of the PDF at `path` before position `at`: every page when
/// `pages` is `None`. The file is opened the way any document is, repairing
/// what it has to, and refused if it is encrypted.
pub fn insert_pages_from(
    doc: &mut Document,
    path: &Path,
    pages: Option<&[PageIndex]>,
    at: PageIndex,
) -> Result<(), CommandError> {
    let mut source = Document::open_path(path).map_err(|source| CommandError::Edit {
        label: INSERT,
        source,
    })?;
    copy_pages_between(doc, &mut source, pages, at)
}

const INSERT: &str = "Insert Pages";

/// Copy `pages` of `source` - all of them for `None` - into `doc` before
/// position `at`, as one undo step in `doc`. `source` is read as its session
/// has it, unsaved edits included, which is what the user is looking at.
pub fn copy_pages_between(
    doc: &mut Document,
    source: &mut Document,
    pages: Option<&[PageIndex]>,
    at: PageIndex,
) -> Result<(), CommandError> {
    let all: Vec<PageIndex> = (0..source.page_count()).collect();
    let pages = pages.unwrap_or(&all);
    let source = source.structure().map_err(|source| CommandError::Edit {
        label: INSERT,
        source,
    })?;
    edit(doc, INSERT, |tx, structure| {
        pages::insert_pages_from(tx, structure, source, pages, at).map(|_| ())
    })
}

/// Move `pages` of `source` into `doc`: a copy into `doc`, then a delete in
/// `source`. Each is one undo step **in its own document**, so the source's
/// Undo puts its pages back whatever happens to `doc`.
///
/// The delete is checked before anything is copied - moving every page out of
/// a document would leave it empty - so a refused move changes neither.
pub fn move_pages_between(
    doc: &mut Document,
    source: &mut Document,
    pages: &[PageIndex],
    at: PageIndex,
) -> Result<(), CommandError> {
    let remaining = (0..source.page_count())
        .filter(|page| !pages.contains(page))
        .count();
    if remaining == 0 {
        return Err(CommandError::Edit {
            label: "Move Pages",
            source: onionskin_core::Error::WouldLeaveNoPages,
        });
    }
    copy_pages_between(doc, source, Some(pages), at)?;
    crate::delete_pages(source, pages)
}

pub(crate) fn commands() -> Vec<Command> {
    vec![editing(INSERT_BLANK_PAGE, "Insert Blank Page", |ctx| {
        // After the current page, at its size: what Acrobat's Insert Blank
        // Page does with no dialog.
        let media_box = ctx
            .doc
            .page_geometry(ctx.page)
            .map_err(|source| CommandError::Page {
                page: ctx.page,
                source,
            })?
            .media_box;
        insert_blank_pages(ctx.doc, ctx.page + 1, 1, media_box)
    })]
}
