//! Renumber Pages: the document's `/PageLabels`.

use onionskin_core::pages::{self, LabelRange};
use onionskin_core::Document;
use onionskin_plugin_api::command_ids::RESET_PAGE_NUMBERING;
use onionskin_plugin_api::{Command, CommandError};

use crate::{edit, editing};

/// Replace the document's page labels with `ranges`, as one undo step. An
/// empty list goes back to plain page numbers.
pub fn renumber_pages(doc: &mut Document, ranges: &[LabelRange]) -> Result<(), CommandError> {
    edit(doc, "Renumber Pages", |tx, _| {
        pages::set_page_labels(tx, ranges)
    })
}

pub(crate) fn commands() -> Vec<Command> {
    vec![editing(
        RESET_PAGE_NUMBERING,
        "Number Pages From 1",
        |ctx| renumber_pages(ctx.doc, &[]),
    )]
}
