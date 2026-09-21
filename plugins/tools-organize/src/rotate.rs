//! Rotate Pages: writes `/Rotate` on the page, where View > Rotate turns only
//! the view. Composes with whatever the page already had, inherited or not.

use onionskin_core::{pages, Document, PageIndex};
use onionskin_plugin_api::command_ids::{ROTATE_PAGE_CLOCKWISE, ROTATE_PAGE_COUNTERCLOCKWISE};
use onionskin_plugin_api::{Command, CommandError};

use crate::{edit, editing};

/// Which way to turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Clockwise,
    Counterclockwise,
    HalfTurn,
}

impl Turn {
    fn quarter_turns(self) -> i32 {
        match self {
            Turn::Clockwise => 1,
            Turn::Counterclockwise => -1,
            Turn::HalfTurn => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Turn::Clockwise => "Rotate Pages Clockwise",
            Turn::Counterclockwise => "Rotate Pages Counterclockwise",
            Turn::HalfTurn => "Rotate Pages 180°",
        }
    }
}

/// Turn `pages` by `turn`, as one undo step.
pub fn rotate_pages(
    doc: &mut Document,
    pages: &[PageIndex],
    turn: Turn,
) -> Result<(), CommandError> {
    edit(doc, turn.label(), |tx, _| {
        pages::rotate_pages(tx, pages, turn.quarter_turns())
    })
}

pub(crate) fn commands() -> Vec<Command> {
    vec![
        editing(ROTATE_PAGE_CLOCKWISE, "Rotate Page Clockwise", |ctx| {
            rotate_pages(ctx.doc, &[ctx.page], Turn::Clockwise)
        }),
        editing(
            ROTATE_PAGE_COUNTERCLOCKWISE,
            "Rotate Page Counterclockwise",
            |ctx| rotate_pages(ctx.doc, &[ctx.page], Turn::Counterclockwise),
        ),
    ]
}
