//! The bookmarks pane: the document outline, and clicking one to go there.
//!
//! Authoring is the context menu's, in `bookmark_edit`. A bookmark the file
//! gave no destination is still listed, and says it has nowhere to go:
//! dropping it would hide part of the outline the document has.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{OutlineItem, PageIndex};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::bookmark_edit::BookmarkAction;
use super::{empty_message, error_message, list, PaneAction, ROW_HEIGHT};
use crate::a11y::State as A11yState;

/// Deeper than this and the indent would leave no room for the title. The
/// reader already caps its own descent; this only caps the drawing.
const MAX_INDENT: usize = 8;
const INDENT: f32 = 12.0;
/// Said where the list would be when the document has no outline.
const NO_BOOKMARKS: &str = "This document has no bookmarks.";

/// One drawn row: an item, flattened out of the tree with the depth it sat
/// at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct BookmarkRow {
    pub(in crate::shell) title: String,
    pub(in crate::shell) page: Option<PageIndex>,
    pub(in crate::shell) depth: usize,
    /// Its index among its siblings at each level, which is how the outline
    /// writer addresses it.
    pub(in crate::shell) path: Vec<usize>,
}

impl BookmarkRow {
    /// Whether clicking the row goes anywhere, and what it says when it does
    /// not.
    ///
    /// A query about this document, not a milestone: the outline reader
    /// resolved the destination or it did not, so the row goes live on its
    /// own for any file that names one.
    pub(in crate::shell) fn availability(&self) -> MenuAvailability {
        match self.page {
            Some(_) => MenuAvailability::Enabled,
            None => MenuAvailability::Disabled("This bookmark names no destination"),
        }
    }

    /// The row's text. A file may leave `/Title` out, and an empty row nobody
    /// can see is not an empty row, so it gets a mark.
    fn text(&self) -> String {
        if self.title.is_empty() {
            "(untitled)".to_owned()
        } else {
            self.title.clone()
        }
    }

    /// What is heard after the name.
    ///
    /// The nesting is drawn as left padding and nothing else, so a reader
    /// given the rows as drawn hears a flat list where the document has a
    /// tree. The level is said, and a row that goes nowhere still says why.
    fn announcement(&self) -> String {
        let level = format!("Level {}", self.depth + 1);
        match self.availability().reason() {
            Some(reason) => format!("{level}. {reason}"),
            None => level,
        }
    }
}

/// What the bookmarks pane tells a screen reader.
pub(super) fn accessible(items: Result<&[OutlineItem], &String>) -> Vec<Element> {
    let items = match items {
        Ok(items) => items,
        Err(message) => {
            return vec![Element::new(
                "bookmark-rows-error",
                Role::Alert,
                message.clone(),
            )]
        }
    };
    if items.is_empty() {
        return vec![Element::new(
            "bookmark-rows-empty",
            Role::Label,
            NO_BOOKMARKS,
        )];
    }

    vec![
        Element::new("bookmark-rows", Role::Tree, "Bookmarks").with_children(
            rows(items)
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    let described =
                        Element::new(("bookmark-row", index), Role::TreeItem, row.text())
                            .with_state(A11yState::enabled(row.availability().is_enabled()))
                            .with_description(row.announcement());
                    match row.page {
                        Some(page) => {
                            described.with_activation(Activation::Pane(PaneAction::GoToPage(page)))
                        }
                        None => described,
                    }
                })
                .collect(),
        ),
    ]
}

/// The outline as rows, parents before their children, which is the order
/// the pane draws and the order the document lists them in.
pub(super) fn rows(items: &[OutlineItem]) -> Vec<BookmarkRow> {
    let mut found = Vec::new();
    flatten(items, &mut Vec::new(), &mut found);
    found
}

fn flatten(items: &[OutlineItem], path: &mut Vec<usize>, found: &mut Vec<BookmarkRow>) {
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        found.push(BookmarkRow {
            title: item.title.clone(),
            page: item.page,
            depth: path.len() - 1,
            path: path.clone(),
        });
        flatten(&item.children, path, found);
        path.pop();
    }
}

pub(super) fn render(
    items: Result<&[OutlineItem], &String>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let items = match items {
        Ok(items) => items,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if items.is_empty() {
        return empty_message(NO_BOOKMARKS, theme).into_any_element();
    }

    let mut body = list("bookmark-rows").on_mouse_down(
        MouseButton::Right,
        cx.listener(|frame, event: &gpui::MouseDownEvent, _window, cx| {
            frame.run_pane_action(
                PaneAction::Bookmark(BookmarkAction::OpenMenu {
                    row: None,
                    at: event.position,
                }),
                cx,
            );
        }),
    );
    for (index, row) in rows(items).into_iter().enumerate() {
        let availability = row.availability();
        let enabled = availability.is_enabled();
        let page = row.page;
        let mut element = div()
            .id(("bookmark-row", index))
            .min_h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .py_1()
            .pr_2()
            .pl(px(8.0 + INDENT * row.depth.min(MAX_INDENT) as f32))
            .text_sm()
            .text_color(if enabled {
                theme.text
            } else {
                theme.disabled_text
            })
            // The row's own right-click wins over the list's: the menu then
            // acts on this bookmark.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |frame, event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    frame.run_pane_action(
                        PaneAction::Bookmark(BookmarkAction::OpenMenu {
                            row: Some(index),
                            at: event.position,
                        }),
                        cx,
                    );
                }),
            )
            .child(row.text());
        if let (true, Some(page)) = (enabled, page) {
            element = element
                .cursor_pointer()
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::Pane(PaneAction::GoToPage(page)), window, cx);
                }));
        } else {
            element = element.when_some(availability.reason(), |element, reason| {
                element.child(
                    div()
                        .pl_2()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(reason),
                )
            });
        }
        body = body.child(element);
    }
    body.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, page: Option<PageIndex>, children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem {
            title: title.to_owned(),
            page,
            children,
        }
    }

    /// Parents come before their children, and the depth is the indent the
    /// pane draws. A flatten that lost the nesting would still list every
    /// title, which is why the depths are asserted and not only the order.
    #[test]
    fn the_tree_flattens_parents_first_with_the_depth_it_was_nested_at() {
        let outline = vec![
            item(
                "one",
                Some(0),
                vec![item(
                    "one.one",
                    Some(1),
                    vec![item("one.one.one", None, vec![])],
                )],
            ),
            item("two", Some(4), vec![]),
        ];

        let rows = rows(&outline);

        assert_eq!(
            rows.iter()
                .map(|row| (row.title.as_str(), row.depth, row.page))
                .collect::<Vec<_>>(),
            [
                ("one", 0, Some(0)),
                ("one.one", 1, Some(1)),
                ("one.one.one", 2, None),
                ("two", 0, Some(4)),
            ]
        );
    }

    /// A bookmark with no destination is listed and says why it does not
    /// navigate. Hiding it would misreport the outline; letting it navigate
    /// would have to invent a page.
    #[test]
    fn a_bookmark_without_a_destination_is_listed_and_disabled_with_a_reason() {
        let rows = rows(&[
            item("nowhere", None, vec![]),
            item("somewhere", Some(2), vec![]),
        ]);

        assert_eq!(rows.len(), 2);
        assert!(!rows[0].availability().is_enabled());
        assert_eq!(
            rows[0].availability().reason(),
            Some("This bookmark names no destination")
        );
        assert!(rows[1].availability().is_enabled());
        assert_eq!(rows[1].availability().reason(), None);
    }

    #[test]
    fn an_empty_outline_produces_no_rows() {
        assert!(rows(&[]).is_empty());
    }

    /// The nesting is drawn as left padding and nothing else, so a reader
    /// given the rows as drawn would hear a flat list where the document has
    /// a tree. The level is said out loud instead.
    #[test]
    fn a_bookmarks_nesting_depth_is_announced_rather_than_left_to_the_indent() {
        let outline = vec![item(
            "one",
            Some(0),
            vec![item(
                "one.one",
                Some(1),
                vec![item("one.one.one", Some(2), vec![])],
            )],
        )];

        let described = accessible(Ok(&outline));
        let rows = &described[0].children;

        assert_eq!(rows[0].description.as_deref(), Some("Level 1"));
        assert_eq!(rows[1].description.as_deref(), Some("Level 2"));
        assert_eq!(rows[2].description.as_deref(), Some("Level 3"));
    }

    /// A bookmark the file gave no destination is announced, disabled, with
    /// the reason it goes nowhere, and offers no page to go to.
    #[test]
    fn a_bookmark_with_no_destination_is_announced_as_disabled_and_says_why() {
        let outline = vec![
            item("nowhere", None, vec![]),
            item("somewhere", Some(2), vec![]),
        ];

        let described = accessible(Ok(&outline));
        let rows = &described[0].children;

        assert!(rows[0].state.disabled);
        assert_eq!(
            rows[0].description.as_deref(),
            Some("Level 1. This bookmark names no destination")
        );
        assert_eq!(rows[0].activation, None);
        assert!(!rows[1].state.disabled);
        assert_eq!(
            rows[1].activation,
            Some(Activation::Pane(PaneAction::GoToPage(2)))
        );
    }

    /// One described row per drawn row, in the order they are drawn, with the
    /// title the row draws. A file that left `/Title` out gets the same mark
    /// in both places.
    #[test]
    fn the_described_rows_are_the_drawn_rows_in_order() {
        let outline = vec![
            item("one", Some(0), vec![item("", Some(1), vec![])]),
            item("two", Some(4), vec![]),
        ];

        let described = accessible(Ok(&outline));
        let rows = &described[0].children;

        assert_eq!(described.len(), 1);
        assert_eq!(described[0].role, Role::Tree);
        assert_eq!(rows.len(), self::rows(&outline).len());
        assert_eq!(
            rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>(),
            ["one", "(untitled)", "two"]
        );
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.role, Role::TreeItem);
            assert_eq!(row.key, gpui::ElementId::from(("bookmark-row", index)));
        }
    }

    /// A document with no outline says so, and a reader that failed says what
    /// went wrong rather than leaving an empty tree that reads as no outline.
    #[test]
    fn an_empty_outline_and_a_failed_read_are_announced_differently() {
        let empty = accessible(Ok(&[]));
        assert_eq!(empty[0].role, Role::Label);
        assert_eq!(empty[0].label, NO_BOOKMARKS);

        let failure = "the outline could not be decoded".to_owned();
        let broken = accessible(Err(&failure));
        assert_eq!(broken[0].role, Role::Alert);
        assert_eq!(broken[0].label, failure);
    }
}
