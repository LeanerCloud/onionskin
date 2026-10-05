//! Bookmark authoring from the pane: the context menu, and what each entry
//! does to the outline through `core`'s outline writer.
//!
//! A bookmark is addressed by the path the outline reader gave its row, so
//! the menu acts on the row it was opened on and nothing else. New and
//! Rename need a title typed, which is a dialog, and dialogs are the
//! frame's: those two entries are run by the frame, and the rest here.

use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Point, Styled as _,
};
use onionskin_core::{
    add_bookmark_tree, delete_bookmark, move_bookmark, plan_bookmarks_from_structure,
    set_bookmark_destination, OutlineItem,
};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::bookmarks::{rows, BookmarkRow};
use super::{document_edit as edit, menu_element, menu_row, NavigationPanesState, PaneAction};

/// Parity row 27's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum BookmarksCommand {
    New,
    /// A bookmark for each heading of a tagged document.
    FromStructure,
    Rename,
    SetDestination,
    Indent,
    Outdent,
    Delete,
    /// The style a reader honours: `/F` and `/C`.
    Properties,
}

impl BookmarksCommand {
    pub(in crate::shell) const ALL: [Self; 8] = [
        Self::New,
        Self::FromStructure,
        Self::Rename,
        Self::SetDestination,
        Self::Indent,
        Self::Outdent,
        Self::Delete,
        Self::Properties,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::New => "New Bookmark",
            Self::FromStructure => "New Bookmarks From Structure",
            Self::Rename => "Rename Bookmark…",
            Self::SetDestination => "Set Destination To Current Page",
            Self::Indent => "Nest Under Bookmark Above",
            Self::Outdent => "Move Out One Level",
            Self::Delete => "Delete Bookmark",
            Self::Properties => "Properties…",
        }
    }

    /// Live for the row the menu was opened on, and for this document. A
    /// document that may not be edited disables every entry with its reason.
    pub(in crate::shell) fn availability(
        self,
        target: Option<&BookmarkRow>,
        refusal: Option<&'static str>,
    ) -> MenuAvailability {
        use MenuAvailability::{Disabled, Enabled};

        if let Some(reason) = refusal {
            return Disabled(reason);
        }
        match (self, target) {
            (Self::New | Self::FromStructure, _) => Enabled,
            (_, None) => Disabled("Right-click a bookmark to choose it"),
            (Self::Indent, Some(row)) if row.path.last() == Some(&0) => {
                Disabled("No bookmark above it to nest under")
            }
            (Self::Outdent, Some(row)) if row.depth == 0 => Disabled("Already at the top level"),
            (_, Some(_)) => Enabled,
        }
    }
}

/// What the bookmarks menu does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum BookmarkAction {
    /// Open the menu at `at`, on the row at `row`, or on none.
    OpenMenu {
        row: Option<usize>,
        at: Point<Pixels>,
    },
    Run(BookmarksCommand),
}

/// Where the open menu is, and which row it acts on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) struct BookmarksMenu {
    pub(in crate::shell) row: Option<usize>,
    pub(in crate::shell) at: Point<Pixels>,
}

fn run_command(command: BookmarksCommand) -> Activation {
    Activation::Pane(PaneAction::Bookmark(BookmarkAction::Run(command)))
}

/// The row the open menu acts on, from the outline the pane holds.
pub(in crate::shell) fn target(state: &NavigationPanesState) -> Option<BookmarkRow> {
    let row = state.bookmarks_menu?.row?;
    rows(state.bookmarks()?).into_iter().nth(row)
}

/// How many bookmarks hang directly under `path`.
fn children_at(items: &[OutlineItem], path: &[usize]) -> usize {
    let mut level = items;
    for &step in path {
        match level.get(step) {
            Some(item) => level = &item.children,
            None => return 0,
        }
    }
    level.len()
}

pub(super) fn run(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    action: BookmarkAction,
    cx: &mut Context<ShellFrame>,
) {
    match action {
        BookmarkAction::OpenMenu { row, at } => {
            state.bookmarks_menu = Some(BookmarksMenu { row, at });
        }
        BookmarkAction::Run(BookmarksCommand::FromStructure) => {
            state.bookmarks_menu = None;
            if let Some(canvas) = canvas {
                from_structure(state, canvas, cx);
            }
        }
        BookmarkAction::Run(command) => {
            let target = target(state);
            let items = state.bookmarks().unwrap_or_default().to_vec();
            state.bookmarks_menu = None;
            let (Some(canvas), Some(row)) = (canvas, target) else {
                return;
            };
            let path = row.path;
            match command {
                BookmarksCommand::SetDestination => {
                    edit(state, canvas, cx, "Set Destination", |page, tx| {
                        set_bookmark_destination(tx, &path, Some(page))
                    });
                }
                BookmarksCommand::Indent => {
                    let Some((&last, parent)) = path.split_last() else {
                        return;
                    };
                    let above: Vec<usize> = [parent, &[last.saturating_sub(1)]].concat();
                    let index = children_at(&items, &above);
                    edit(state, canvas, cx, "Nest Bookmark", |_, tx| {
                        move_bookmark(tx, &path, &above, index).map(|_| ())
                    });
                }
                BookmarksCommand::Outdent => {
                    let Some((&parent_index, grandparent)) = path[..path.len() - 1].split_last()
                    else {
                        return;
                    };
                    let grandparent = grandparent.to_vec();
                    edit(state, canvas, cx, "Move Bookmark Out", |_, tx| {
                        move_bookmark(tx, &path, &grandparent, parent_index + 1).map(|_| ())
                    });
                }
                BookmarksCommand::Delete => {
                    edit(state, canvas, cx, "Delete Bookmark", |_, tx| {
                        delete_bookmark(tx, &path)
                    });
                }
                // Run by the frame: it opens the dialog, or asks for the
                // title first, or toggles the pane's own wrapping.
                // (`FromStructure` is handled above and never reaches here.)
                BookmarksCommand::New
                | BookmarksCommand::FromStructure
                | BookmarksCommand::Rename
                | BookmarksCommand::Properties => {}
            }
        }
    }
}

/// A bookmark for each heading, as one undo step after the bookmarks already
/// there. A document that cannot be read as tagged, or has no heading with
/// words and content, says so in the pane instead of making an empty edit.
fn from_structure(
    state: &mut NavigationPanesState,
    canvas: &Entity<Canvas>,
    cx: &mut Context<ShellFrame>,
) {
    let blocks = canvas.update(cx, |canvas, _| canvas.model.structure_blocks());
    let blocks = match blocks {
        Ok(Some(blocks)) => blocks,
        Ok(None) => {
            state.feedback = Some("This document has no tags to make bookmarks from.".to_owned());
            return;
        }
        Err(error) => {
            state.feedback = Some(error.to_string());
            return;
        }
    };
    let plan = plan_bookmarks_from_structure(&blocks);
    if plan.is_empty() {
        state.feedback = Some("No headings with words were found.".to_owned());
        return;
    }
    if let Some(added) = edit(
        state,
        canvas,
        cx,
        "New Bookmarks From Structure",
        |_, tx| add_bookmark_tree(tx, &plan),
    ) {
        state.feedback = Some(added_feedback(added));
    }
}

fn added_feedback(added: usize) -> String {
    let noun = if added == 1 { "bookmark" } else { "bookmarks" };
    format!("Added {added} {noun} from the headings.")
}

/// The pane's own New Bookmark button, above the list and there when the
/// list is empty: the discoverable way to make the first bookmark, and the
/// keyboard's, since a right-click is neither.
pub(super) fn new_button_element(refusal: Option<&'static str>) -> Element {
    command_button_element("bookmark-new", BookmarksCommand::New, refusal)
}

/// The pane's button for New Bookmarks From Structure, beside New: a
/// right-click is no more the keyboard's here than it is for New.
pub(super) fn structure_button_element(refusal: Option<&'static str>) -> Element {
    command_button_element(
        "bookmark-from-structure",
        BookmarksCommand::FromStructure,
        refusal,
    )
}

fn command_button_element(
    id: &'static str,
    command: BookmarksCommand,
    refusal: Option<&'static str>,
) -> Element {
    let availability = command.availability(None, refusal);
    let button = Element::new(id, accesskit::Role::Button, command.label())
        .with_state(crate::a11y::State::enabled(availability.is_enabled()))
        .with_activation(run_command(command));
    match availability.reason() {
        Some(reason) => button.with_description(reason),
        None => button,
    }
}

pub(super) fn render_new_button(
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .child(render_command_button(
            "bookmark-new",
            BookmarksCommand::New,
            refusal,
            theme,
            cx,
        ))
        .child(render_command_button(
            "bookmark-from-structure",
            BookmarksCommand::FromStructure,
            refusal,
            theme,
            cx,
        ))
}

fn render_command_button(
    id: &'static str,
    command: BookmarksCommand,
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    use gpui::StatefulInteractiveElement as _;

    let availability = command.availability(None, refusal);
    let enabled = availability.is_enabled();
    let button = div()
        .id(id)
        .mx_2()
        .my_1()
        .px_2()
        .py(px(2.0))
        .rounded_sm()
        .text_xs()
        .bg(theme.surface)
        .text_color(if enabled {
            theme.text
        } else {
            theme.disabled_text
        })
        .child(command.label());
    if enabled {
        button
            .cursor_pointer()
            .hover(move |button| button.bg(theme.hover))
            .on_click(cx.listener(move |frame, _event, window, cx| {
                frame.run_activation(run_command(command), window, cx);
            }))
    } else {
        button
    }
}

/// The described menu, built from the entries the drawn one is.
pub(super) fn accessible_menu(state: &NavigationPanesState) -> Element {
    let target = target(state);
    menu_element(
        "bookmarks-context-menu",
        "Bookmarks",
        "bookmarks-menu-entry",
        BookmarksCommand::ALL.map(|command| {
            (
                command.label(),
                command.availability(target.as_ref(), state.edit_refusal),
                run_command(command),
            )
        }),
    )
}

pub(super) fn render_menu(
    state: &NavigationPanesState,
    at: Point<Pixels>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let target = target(state);
    let mut menu = div()
        .id("bookmarks-context-menu")
        .absolute()
        .top(at.y)
        .left(px(4.0))
        .w(px(228.0))
        .p_1()
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text);
    for (index, command) in BookmarksCommand::ALL.into_iter().enumerate() {
        menu = menu.child(menu_row(
            "bookmarks-menu-entry",
            index,
            command.label(),
            command.availability(target.as_ref(), state.edit_refusal),
            run_command(command),
            theme,
            cx,
        ));
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feedback_counts_in_the_singular_and_the_plural() {
        assert_eq!(added_feedback(1), "Added 1 bookmark from the headings.");
        assert_eq!(added_feedback(2), "Added 2 bookmarks from the headings.");
    }

    fn row(path: &[usize]) -> BookmarkRow {
        BookmarkRow {
            title: "x".into(),
            page: Some(0),
            depth: path.len() - 1,
            path: path.to_vec(),
        }
    }

    #[test]
    fn nesting_needs_a_bookmark_above_and_moving_out_needs_a_parent() {
        let first = row(&[0]);
        let second_child = row(&[0, 1]);
        assert!(!BookmarksCommand::Indent
            .availability(Some(&first), None)
            .is_enabled());
        assert!(!BookmarksCommand::Outdent
            .availability(Some(&first), None)
            .is_enabled());
        assert!(BookmarksCommand::Indent
            .availability(Some(&second_child), None)
            .is_enabled());
        assert!(BookmarksCommand::Outdent
            .availability(Some(&second_child), None)
            .is_enabled());
    }

    #[test]
    fn with_no_row_only_the_commands_that_make_bookmarks_are_live_and_a_refusal_disables_everything(
    ) {
        for command in BookmarksCommand::ALL {
            assert_eq!(
                command.availability(None, None).is_enabled(),
                matches!(
                    command,
                    BookmarksCommand::New | BookmarksCommand::FromStructure
                ),
                "{}",
                command.label()
            );
            let refused = command.availability(Some(&row(&[1])), Some("Encrypted"));
            assert_eq!(refused.reason(), Some("Encrypted"));
        }
    }

    #[test]
    fn children_are_counted_at_a_path() {
        let leaf = |title: &str| OutlineItem {
            title: title.into(),
            page: None,
            children: Vec::new(),
        };
        let items = vec![OutlineItem {
            title: "a".into(),
            page: None,
            children: vec![leaf("a1"), leaf("a2")],
        }];
        assert_eq!(children_at(&items, &[]), 1);
        assert_eq!(children_at(&items, &[0]), 2);
        assert_eq!(children_at(&items, &[0, 0]), 0);
        assert_eq!(children_at(&items, &[3]), 0);
    }
}
