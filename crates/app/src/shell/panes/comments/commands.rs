//! What can be done to a comment, as one list the inline buttons, the
//! context menu and the accessibility tree all draw from.

use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point,
    Styled as _,
};

use super::super::super::chrome::accessible::{Activation, Element};
use super::super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::{menu_element, menu_row, PaneAction};
use super::model::Thread;
use super::CommentAction;
use onionskin_core::review::REVIEW_STATES;

/// A control the pane shows: a label, whether it can be used now, and what
/// it runs.
pub(super) type Command = (&'static str, MenuAvailability, Activation);

pub(super) fn activation(action: CommentAction) -> Activation {
    Activation::Pane(PaneAction::Comment(action))
}

fn status_label(status: &'static str) -> &'static str {
    if status == "None" {
        "Clear Status"
    } else {
        status
    }
}

/// What can be done to `thread`'s comment. A document that may not be
/// edited disables every command that writes, with its reason; marking read
/// or unread writes nothing, so it stays.
pub(super) fn commands(thread: &Thread, read: bool, refusal: Option<&'static str>) -> Vec<Command> {
    let writes = refusal.map_or(MenuAvailability::Enabled, MenuAvailability::Disabled);
    let mut listed = vec![
        ("Reply", writes, CommentAction::Reply),
        ("Edit Text", writes, CommentAction::Edit),
    ];
    listed.extend(REVIEW_STATES.into_iter().map(|status| {
        (
            status_label(status),
            writes,
            CommentAction::SetStatus(status),
        )
    }));
    listed.push((
        if thread.checked { "Uncheck" } else { "Check" },
        writes,
        CommentAction::ToggleMark,
    ));
    listed.push((
        if read {
            "Mark as Unread"
        } else {
            "Mark as Read"
        },
        MenuAvailability::Enabled,
        CommentAction::ToggleRead,
    ));
    listed.push(("Delete", writes, CommentAction::Delete));
    listed
        .into_iter()
        .map(|(label, availability, action)| (label, availability, activation(action)))
        .collect()
}

pub(super) fn draft_commands() -> Vec<Command> {
    vec![
        (
            "Save",
            MenuAvailability::Enabled,
            activation(CommentAction::SaveDraft),
        ),
        (
            "Cancel",
            MenuAvailability::Enabled,
            activation(CommentAction::CancelDraft),
        ),
    ]
}

/// The context menu, described.
pub(super) fn accessible_menu(commands: Vec<Command>) -> Element {
    menu_element(
        "comments-context-menu",
        "Comment",
        "comments-menu-entry",
        commands,
    )
}

/// The context menu, drawn where it was opened.
pub(super) fn render_menu(
    commands: Vec<Command>,
    at: Point<Pixels>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut menu = div()
        .id("comments-context-menu")
        .absolute()
        .top(at.y)
        .left(px(4.0))
        .w(px(228.0))
        .p_1()
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text);
    for (index, (label, availability, activation)) in commands.into_iter().enumerate() {
        menu = menu.child(menu_row(
            "comments-menu-entry",
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

#[cfg(test)]
pub(super) mod tests {
    use onionskin_core::{Flags, ObjRef, ReadAnnotation, Rect, Subtype};

    use super::*;

    pub(in crate::shell::panes::comments) fn thread(checked: bool) -> Thread {
        Thread {
            comment: ReadAnnotation {
                objref: ObjRef::new(5, 0),
                page: 2,
                subtype: None::<Subtype>,
                raw_subtype: "Highlight".into(),
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                quads: Vec::new(),
                contents: None,
                author: Some("Ana".into()),
                modified: Some("D:20260921143000".into()),
                color: None,
                flags: Flags(4),
                in_reply_to: None,
                has_appearance: true,
                ink: Vec::new(),
                border_width: 1.0,
                subject: None,
                state: None,
                opacity: None,
            },
            replies: Vec::new(),
            status: None,
            checked,
        }
    }

    fn labels(commands: &[Command]) -> Vec<&'static str> {
        commands.iter().map(|(label, _, _)| *label).collect()
    }

    #[test]
    fn every_command_is_offered_and_live_on_an_editable_document() {
        let offered = commands(&thread(false), false, None);
        assert_eq!(
            labels(&offered),
            [
                "Reply",
                "Edit Text",
                "Accepted",
                "Rejected",
                "Cancelled",
                "Completed",
                "Clear Status",
                "Check",
                "Mark as Read",
                "Delete"
            ]
        );
        assert!(offered
            .iter()
            .all(|(_, availability, _)| availability.is_enabled()));
        let flipped = commands(&thread(true), true, None);
        assert!(labels(&flipped).contains(&"Uncheck"));
        assert!(labels(&flipped).contains(&"Mark as Unread"));
    }

    /// Marking read is the reader's own state and writes nothing, so a
    /// document that may not be edited still allows it.
    #[test]
    fn a_refused_document_disables_every_command_that_writes_with_its_reason() {
        let reason = "The document is encrypted";
        for (label, availability, _) in commands(&thread(false), false, Some(reason)) {
            if label == "Mark as Read" {
                assert!(availability.is_enabled());
            } else {
                assert_eq!(availability.reason(), Some(reason), "{label}");
            }
        }
    }
}
