//! Delete Pages. A deleted page is dropped from the page tree and nothing
//! else happens to it (T5): its objects stay in the file, unreachable, and an
//! undo puts it back by restoring the tree.

use onionskin_core::{pages, Document, PageIndex};
use onionskin_plugin_api::command_ids::DELETE_PAGE;
use onionskin_plugin_api::{Command, CommandError};

use crate::{edit, editing};

/// Delete `pages`, as one undo step. Deleting every page is refused.
pub fn delete_pages(doc: &mut Document, pages: &[PageIndex]) -> Result<(), CommandError> {
    edit(doc, "Delete Pages", |tx, structure| {
        pages::delete_pages(tx, structure, pages).map(|_| ())
    })
}

pub(crate) fn commands() -> Vec<Command> {
    vec![editing(DELETE_PAGE, "Delete Page", |ctx| {
        delete_pages(ctx.doc, &[ctx.page])
    })]
}
