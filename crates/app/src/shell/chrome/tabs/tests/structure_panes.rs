//! The Tags and Content panes, on a real window.

use super::*;
use crate::shell::fixtures::tagged_pdf;
use crate::shell::panes::{ContentAction, NavigationPane, PaneAction, TagAction};

fn labels(
    frame: &ShellFrame,
    window: &mut gpui::Window,
    cx: &mut Context<ShellFrame>,
    id: &'static str,
) -> Vec<String> {
    let tree = frame.accessible(window, cx);
    tree.find(&id.into())
        .map(|rows| rows.children.iter().map(|row| row.label.clone()).collect())
        .unwrap_or_default()
}

fn highlights(frame: &ShellFrame, cx: &mut Context<ShellFrame>) -> usize {
    let canvas = frame.tabs.active().expect("a tab").canvas.clone();
    canvas.update(cx, |canvas, _| {
        canvas.model.update().expect("a frame runs");
        canvas.model.paint_list().expect("paints").highlights.len()
    })
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn the_tags_pane_opens_an_element_and_boxes_its_content_on_the_page(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(vec![("tagged.pdf", tagged_pdf())], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Tags), cx);
            assert_eq!(
                labels(frame, window, cx, "tag-rows"),
                ["Document"],
                "the tree starts folded to its top level"
            );

            // Document, H1, P, H2, P, Figure are objects 6 to 11. Choosing an
            // element boxes it; the disclosure is what opens it.
            frame.run_pane_action(PaneAction::Tag(TagAction::Select(6)), cx);
            assert_eq!(labels(frame, window, cx, "tag-rows").len(), 1);
            frame.run_pane_action(PaneAction::Tag(TagAction::Toggle(6)), cx);
            let opened = labels(frame, window, cx, "tag-rows");
            assert_eq!(opened.len(), 6, "{opened:?}");
            assert_eq!(opened[1], "H1  Title");
            assert_eq!(opened[5], "Figure  A cat");

            assert_eq!(
                highlights(frame, cx),
                1,
                "the whole document's content is boxed"
            );
            frame.run_pane_action(PaneAction::Tag(TagAction::Select(7)), cx);
            assert_eq!(highlights(frame, cx), 1);

            frame.run_pane_action(PaneAction::Select(NavigationPane::Tags), cx);
            assert_eq!(highlights(frame, cx), 0, "the box goes with the pane");
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn choosing_an_element_goes_to_the_page_its_content_is_on(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("forty.pdf", crate::shell::fixtures::tagged_pages_pdf(40))],
        cx,
    );
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Tags), cx);
            // Pages are objects 3 to 42, their streams 43 to 82, the root 83,
            // the Document 84, and the headings 85 to 124.
            frame.run_pane_action(PaneAction::Tag(TagAction::Select(84)), cx);
            frame.run_pane_action(PaneAction::Tag(TagAction::Select(124)), cx);
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            let page = canvas.update(cx, |canvas, _| canvas.model.view_state().current_page);
            assert_eq!(page, 39, "the last heading is on the last page");
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn the_content_pane_lists_the_page_the_view_is_on(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("forty.pdf", crate::shell::fixtures::tagged_pages_pdf(40))],
        cx,
    );
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            canvas.update(cx, |canvas, cx| {
                let moved = canvas.model.go_to_page(30);
                canvas.handle_change(moved, cx);
            });
            frame.run_pane_action(PaneAction::Select(NavigationPane::Content), cx);
            let tree = frame.accessible(window, cx);
            let rows = tree
                .find(&"content-rows".into())
                .expect("the pane lists it");
            assert_eq!(rows.label, "Content of page 31");
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn an_untagged_document_has_no_tags_to_list(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Tags), cx);
            let tree = frame.accessible(window, cx);
            let message = tree.find(&"tag-rows-empty".into()).expect("it says so");
            assert_eq!(message.label, "This document has no tags.");
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn the_content_pane_groups_the_pages_drawing_and_boxes_a_piece(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(vec![("tagged.pdf", tagged_pdf())], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Content), cx);
            assert_eq!(
                labels(frame, window, cx, "content-rows"),
                ["Text (4)", "Paths (1)"]
            );

            frame.run_pane_action(
                PaneAction::Content(ContentAction::Toggle(
                    crate::shell::panes::ContentGroup::Text,
                )),
                cx,
            );
            let opened = labels(frame, window, cx, "content-rows");
            assert_eq!(opened[1], "Title  (H1, id 0)");
            assert_eq!(opened.len(), 2 + 4);

            assert_eq!(highlights(frame, cx), 0);
            frame.run_pane_action(PaneAction::Content(ContentAction::Select(0)), cx);
            assert_eq!(highlights(frame, cx), 1, "the chosen piece is boxed");
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn new_bookmarks_from_structure_makes_the_headings_nested_in_one_undo_step(
    cx: &mut TestAppContext,
) {
    use crate::shell::panes::{BookmarkAction, BookmarksCommand};

    let (window, _) = bound_window_from_bytes(vec![("tagged.pdf", tagged_pdf())], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            let tree = frame.accessible(window, cx);
            let button = tree
                .find(&"bookmark-from-structure".into())
                .expect("a button, because a right-click is not the keyboard's");
            assert_eq!(
                button.activation,
                Some(crate::shell::chrome::accessible::Activation::Pane(
                    PaneAction::Bookmark(BookmarkAction::Run(BookmarksCommand::FromStructure))
                ))
            );
            assert!(!button.state.disabled);
            frame.run_pane_action(
                PaneAction::Bookmark(BookmarkAction::Run(BookmarksCommand::FromStructure)),
                cx,
            );
            let rows = labels(frame, window, cx, "bookmark-rows");
            assert_eq!(rows, ["Title", "Details"]);
            let tree = frame.accessible(window, cx);
            let feedback = tree
                .find(&"navigation-pane-feedback".into())
                .expect("the pane says what it did");
            assert_eq!(feedback.label, "Added 2 bookmarks from the headings.");
            let described = tree.find(&"bookmark-rows".into()).expect("the list");
            assert_eq!(
                described.children[1].description.as_deref(),
                Some("Level 2"),
                "the H2 hangs from the H1"
            );

            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().undo().expect("undo");
            });
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"bookmark-rows".into()).is_none(),
                "one undo takes both away"
            );
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn an_untagged_document_says_it_has_no_tags_to_make_bookmarks_from(cx: &mut TestAppContext) {
    use crate::shell::panes::{BookmarkAction, BookmarksCommand};

    let (window, _) = bound_window(&["hello.pdf"], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            frame.run_pane_action(
                PaneAction::Bookmark(BookmarkAction::Run(BookmarksCommand::FromStructure)),
                cx,
            );
            let tree = frame.accessible(window, cx);
            let feedback = tree
                .find(&"navigation-pane-feedback".into())
                .expect("the pane says why");
            assert_eq!(
                feedback.label,
                "This document has no tags to make bookmarks from."
            );
            assert!(tree.find(&"bookmark-rows".into()).is_none());
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn what_was_boxed_and_chosen_does_not_outlive_a_reread_of_the_document(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(vec![("tagged.pdf", tagged_pdf())], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Tags), cx);
            frame.run_pane_action(PaneAction::Tag(TagAction::Toggle(6)), cx);
            frame.run_pane_action(PaneAction::Tag(TagAction::Select(7)), cx);
            assert_eq!(highlights(frame, cx), 1);

            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            frame.navigation_mut().reread(&canvas, cx);

            assert_eq!(highlights(frame, cx), 0, "the box named the old document");
            let tree = frame.accessible(window, cx);
            let row = tree.find(&("tag-row", 7usize).into()).expect("the row");
            assert!(
                !row.state.selected.unwrap_or(false),
                "nor is the row chosen"
            );
        })
        .unwrap();
}

#[cfg(feature = "shell-test-support")]
#[gpui::test]
fn leaving_a_tab_takes_the_box_off_it_and_the_pane_forgets_what_was_chosen(
    cx: &mut TestAppContext,
) {
    let (window, _) =
        bound_window_from_bytes(vec![("a.pdf", tagged_pdf()), ("b.pdf", tagged_pdf())], cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            frame.activate(0, cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Tags), cx);
            frame.run_pane_action(PaneAction::Tag(TagAction::Select(7)), cx);
            assert_eq!(highlights(frame, cx), 1);

            frame.activate(1, cx);
            frame.activate(0, cx);
            assert_eq!(
                highlights(frame, cx),
                0,
                "the box stayed behind on the first tab"
            );
        })
        .unwrap();
}
