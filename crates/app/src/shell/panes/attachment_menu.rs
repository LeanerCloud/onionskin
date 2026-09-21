//! The attachments pane's context menu (parity row 28): Add, and the row
//! commands for the attachment it was opened on.
//!
//! Every entry runs the same activation the row's own button or the pane's
//! Add button runs, so the menu is another way to reach them rather than a
//! second implementation.

use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point,
    Styled as _,
};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::attachments::{
    add_availability, command_activation, AttachmentAction, AttachmentCommand, ADD_LABEL,
};
use super::{menu_element, menu_row, NavigationPanesState, PaneAction};

/// Where the open menu is, and which row it acts on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) struct AttachmentsMenu {
    pub(in crate::shell) row: Option<usize>,
    pub(in crate::shell) at: Point<Pixels>,
}

/// Said by a row command when the menu was opened on no row.
const NO_ROW: &str = "Right-click an attachment to choose it";

/// The menu's entries: label, availability, and what choosing one runs.
fn entries(state: &NavigationPanesState) -> Vec<(&'static str, MenuAvailability, Activation)> {
    let row = state.attachments_menu.and_then(|menu| menu.row);
    let mut entries = vec![(
        ADD_LABEL,
        add_availability(state.edit_refusal),
        Activation::Pane(PaneAction::Attachment(AttachmentAction::Add)),
    )];
    for command in AttachmentCommand::ALL {
        let activation = row.and_then(|row| command_activation(row, command));
        let availability = match (row, command.availability(state.edit_refusal)) {
            (None, MenuAvailability::Enabled) => MenuAvailability::Disabled(NO_ROW),
            (_, availability) => availability,
        };
        entries.push((
            command.label(),
            availability,
            // A disabled entry runs nothing; closing the menu is the
            // harmless thing to name for it.
            activation.unwrap_or(Activation::Pane(PaneAction::DismissMenus)),
        ));
    }
    entries
}

pub(super) fn accessible_menu(state: &NavigationPanesState) -> Element {
    menu_element(
        "attachments-context-menu",
        "Attachments",
        "attachments-menu-entry",
        entries(state),
    )
}

pub(super) fn render_menu(
    state: &NavigationPanesState,
    at: Point<Pixels>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut menu = div()
        .id("attachments-context-menu")
        .absolute()
        .top(at.y)
        .left(px(4.0))
        .w(px(228.0))
        .p_1()
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text);
    for (index, (label, availability, activation)) in entries(state).into_iter().enumerate() {
        menu = menu.child(menu_row(
            "attachments-menu-entry",
            index,
            label,
            availability,
            activation,
            theme,
            cx,
        ));
    }
    menu
}
