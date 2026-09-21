//! Move Pages within a document.

use onionskin_core::{pages, Document, PageIndex};
use onionskin_plugin_api::command_ids::{MOVE_PAGE_EARLIER, MOVE_PAGE_LATER};
use onionskin_plugin_api::{Command, CommandError};

use crate::{edit, editing};

/// Move `pages`, keeping their order, to sit before the page now at `before`
/// (`page_count` for the end), as one undo step.
pub fn move_pages(
    doc: &mut Document,
    pages: &[PageIndex],
    before: PageIndex,
) -> Result<(), CommandError> {
    edit(doc, "Move Pages", |tx, structure| {
        pages::move_pages(tx, structure, pages, before).map(|_| ())
    })
}

pub(crate) fn commands() -> Vec<Command> {
    vec![
        editing(MOVE_PAGE_EARLIER, "Move Page Earlier", |ctx| {
            // The first page has nowhere earlier to go: a move to where it
            // already is would be an undo entry that changes nothing.
            match ctx.page.checked_sub(1) {
                Some(before) => move_pages(ctx.doc, &[ctx.page], before),
                None => Ok(()),
            }
        }),
        editing(MOVE_PAGE_LATER, "Move Page Later", |ctx| {
            // "Before the page two along", because `before` counts in the
            // document as it was, where the moved page still occupies a slot.
            let before = ctx.page + 2;
            if before > ctx.doc.page_count() {
                return Ok(());
            }
            move_pages(ctx.doc, &[ctx.page], before)
        }),
    ]
}
