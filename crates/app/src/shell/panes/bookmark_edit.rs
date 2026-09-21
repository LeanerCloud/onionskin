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
use onionskin_core::{delete_bookmark, move_bookmark, set_bookmark_destination, OutlineItem};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::bookmarks::{rows, BookmarkRow};
use super::{document_edit as edit, menu_element, menu_row, NavigationPanesState, PaneAction};

/// Parity row 27's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum BookmarksCommand {
    New,
    Rename,
    SetDestination,
    Indent,
    Outdent,
    Delete,
}

impl BookmarksCommand {
    pub(in crate::shell) const ALL: [Self; 6] = [
        Self::New,
        Self::Rename,
        Self::SetDestination,
        Self::Indent,
        Self::Outdent,
        Self::Delete,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::New => "New Bookmark",
            Self::Rename => "Rename Bookmark…",
            Self::SetDestination => "Set Destination To Current Page",
            Self::Indent => "Nest Under Bookmark Above",
            Self::Outdent => "Move Out One Level",
            Self::Delete => "Delete Bookmark",
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
            (Self::New, _) => Enabled,
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
                // Run by the frame, which asks for the title first.
                BookmarksCommand::New | BookmarksCommand::Rename => {}
            }
        }
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
    fn with_no_row_only_new_is_live_and_a_refusal_disables_everything() {
        for command in BookmarksCommand::ALL {
            assert_eq!(
                command.availability(None, None).is_enabled(),
                command == BookmarksCommand::New,
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
