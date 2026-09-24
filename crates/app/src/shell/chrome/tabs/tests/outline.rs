//! Bookmark and attachment authoring from the panes, on a real window.

use gpui::point;

use super::*;
use crate::shell::chrome::accessible::TextField;
use crate::shell::chrome::bookmark_dialog::BookmarkTitleAction;
use crate::shell::panes::{
    AttachmentAction, BookmarkAction, BookmarksCommand, NavigationPane, PaneAction,
};

fn seed_copy(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(name);
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name),
        &path,
    )
    .expect("copies");
    (dir, path)
}

fn window_on(path: &Path, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let model = CanvasModel::new(
        Document::open_path(path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    bound_window_with_models(
        vec![(path.to_path_buf(), model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0
}

fn open_menu(frame: &mut ShellFrame, row: Option<usize>, cx: &mut Context<ShellFrame>) {
    frame.run_pane_action(
        PaneAction::Bookmark(BookmarkAction::OpenMenu {
            row,
            at: point(px(10.0), px(10.0)),
        }),
        cx,
    );
}

fn run(
    frame: &mut ShellFrame,
    command: BookmarksCommand,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) {
    frame.run_activation(
        Activation::Pane(PaneAction::Bookmark(BookmarkAction::Run(command))),
        window,
        cx,
    );
}

/// New Bookmark on the row, with a title typed in the dialog.
fn new_bookmark(
    frame: &mut ShellFrame,
    after: Option<usize>,
    title: &str,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) {
    open_menu(frame, after, cx);
    run(frame, BookmarksCommand::New, window, cx);
    assert_eq!(frame.dialog, Some(ShellDialog::BookmarkTitle));
    frame
        .text_field(TextField::BookmarkTitle)
        .expect("the title field")
        .clone()
        .update(cx, |input, cx| input.set_query(title, cx));
    frame.run_activation(
        Activation::BookmarkTitle(BookmarkTitleAction::Submit),
        window,
        cx,
    );
    assert_eq!(frame.dialog, None, "a titled bookmark closes the dialog");
}

fn outline(frame: &ShellFrame, cx: &mut Context<ShellFrame>) -> Vec<(String, usize)> {
    let canvas = frame.tabs.active().expect("a tab").canvas.clone();
    let items = canvas.update(cx, |canvas, _| {
        canvas.model.outline().expect("reads").to_vec()
    });
    let mut rows = Vec::new();
    fn walk(items: &[onionskin_core::OutlineItem], depth: usize, rows: &mut Vec<(String, usize)>) {
        for item in items {
            rows.push((item.title.clone(), depth));
            walk(&item.children, depth + 1, rows);
        }
    }
    walk(&items, 0, &mut rows);
    rows
}

#[gpui::test]
fn bookmarks_are_made_nested_renamed_and_deleted_from_the_pane(cx: &mut TestAppContext) {
    let (_dir, path) = seed_copy("two-page.pdf");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            new_bookmark(frame, None, "Intro", window, cx);
            new_bookmark(frame, Some(0), "Details", window, cx);
            assert_eq!(
                outline(frame, cx),
                [("Intro".to_owned(), 0), ("Details".to_owned(), 0)]
            );

            // The menu is in the tree while it is open, with its entries.
            open_menu(frame, Some(1), cx);
            let tree = frame.accessible(window, cx);
            let menu = tree
                .find(&"bookmarks-context-menu".into())
                .expect("the open menu is described");
            assert_eq!(menu.role, Role::Menu);
            assert_eq!(menu.children.len(), BookmarksCommand::ALL.len());
            run(frame, BookmarksCommand::Indent, window, cx);
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"bookmarks-context-menu".into()).is_none(),
                "running an entry closes the menu"
            );
            assert_eq!(
                outline(frame, cx),
                [("Intro".to_owned(), 0), ("Details".to_owned(), 1)],
                "nested under the bookmark above"
            );
            let described = tree.find(&("bookmark-row", 1usize).into()).expect("row");
            assert_eq!(described.description.as_deref(), Some("Level 2"));

            open_menu(frame, Some(1), cx);
            run(frame, BookmarksCommand::Rename, window, cx);
            frame
                .text_field(TextField::BookmarkTitle)
                .expect("the title field")
                .clone()
                .update(cx, |input, cx| input.set_query("Specifics", cx));
            frame.run_activation(
                Activation::BookmarkTitle(BookmarkTitleAction::Submit),
                window,
                cx,
            );
            assert_eq!(
                outline(frame, cx),
                [("Intro".to_owned(), 0), ("Specifics".to_owned(), 1)]
            );

            open_menu(frame, Some(0), cx);
            run(frame, BookmarksCommand::Delete, window, cx);
            assert_eq!(
                outline(frame, cx),
                Vec::<(String, usize)>::new(),
                "with its child"
            );
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"bookmark-rows-empty".into()).is_some(),
                "the pane re-read"
            );
        })
        .unwrap();
}

#[gpui::test]
fn a_blank_title_is_refused_in_the_dialog(cx: &mut TestAppContext) {
    let (_dir, path) = seed_copy("two-page.pdf");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            open_menu(frame, None, cx);
            run(frame, BookmarksCommand::New, window, cx);
            frame
                .text_field(TextField::BookmarkTitle)
                .expect("the title field")
                .clone()
                .update(cx, |input, cx| input.set_query("   ", cx));
            frame.run_activation(
                Activation::BookmarkTitle(BookmarkTitleAction::Submit),
                window,
                cx,
            );
            assert_eq!(frame.dialog, Some(ShellDialog::BookmarkTitle));
            assert_eq!(
                frame
                    .bookmark_title_dialog()
                    .and_then(|state| state.error.clone())
                    .as_deref(),
                Some("A bookmark needs a title")
            );
        })
        .unwrap();
}

#[gpui::test]
fn an_encrypted_documents_bookmark_menu_is_disabled_with_its_reason(cx: &mut TestAppContext) {
    let bytes = std::fs::read(onionskin_corpus_testing::encrypted_fixture(
        "r6-aes-256-print-only.pdf",
    ))
    .expect("reads");
    let (window, _) = bound_window_from_bytes(vec![("locked.pdf", bytes)], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            open_menu(frame, None, cx);
            let tree = frame.accessible(window, cx);
            let menu = tree.find(&"bookmarks-context-menu".into()).expect("menu");
            let reason = onionskin_core::protection::Refusal::Restricted(
                onionskin_core::protection::EditKind::Content,
            )
            .reason();
            for entry in &menu.children {
                assert!(entry.state.disabled, "{}", entry.label);
                assert_eq!(entry.description.as_deref(), Some(reason));
            }
        })
        .unwrap();
}

#[gpui::test]
fn an_attachment_is_added_listed_and_deleted_from_the_pane(cx: &mut TestAppContext) {
    let (dir, path) = seed_copy("hello.pdf");
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, b"remember the milk").expect("writes");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            let tree = frame.accessible(window, cx);
            let add = tree.find(&"attachment-add".into()).expect("Add is offered");
            assert!(!add.state.disabled);
            frame.attach_file(&file, cx);

            let tree = frame.accessible(window, cx);
            let row = tree
                .find(&("attachment-row", 0usize).into())
                .expect("the attachment is listed");
            assert_eq!(row.label, "notes.txt");
            assert_eq!(row.description.as_deref(), Some("text/plain · 17 bytes"));

            frame.run_pane_action(
                PaneAction::Attachment(AttachmentAction::OpenMenu {
                    row: Some(0),
                    at: point(px(10.0), px(10.0)),
                }),
                cx,
            );
            let tree = frame.accessible(window, cx);
            let menu = tree
                .find(&"attachments-context-menu".into())
                .expect("the open menu is described");
            let labels: Vec<&str> = menu
                .children
                .iter()
                .map(|entry| entry.label.as_str())
                .collect();
            assert_eq!(labels, ["Add Attachment…", "Open", "Save", "Delete"]);
            let delete = menu.children[3].activation.clone().expect("Delete runs");
            frame.run_activation(delete, window, cx);

            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"attachments-context-menu".into()).is_none());
            assert!(
                tree.find(&"attachment-rows-empty".into()).is_some(),
                "the pane re-read"
            );
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            let listed = canvas.update(cx, |canvas, _| {
                canvas.model.attachments().expect("lists").len()
            });
            assert_eq!(listed, 0);
        })
        .unwrap();
}

#[gpui::test]
fn a_file_that_cannot_be_read_is_reported_in_the_pane(cx: &mut TestAppContext) {
    let (dir, path) = seed_copy("hello.pdf");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.attach_file(&dir.path().join("missing.txt"), cx);
            let tree = frame.accessible(window, cx);
            let feedback = tree
                .find(&"navigation-pane-feedback".into())
                .expect("the pane says why");
            assert!(feedback.label.contains("missing.txt"), "{}", feedback.label);
        })
        .unwrap();
}

/// Found by hand: with no bookmarks the pane drew only its empty message,
/// which nothing could right-click, so the first bookmark could not be made.
/// The pane's own button makes it, from the accessibility tree as a
/// keyboard or screen reader user would.
#[gpui::test]
fn the_first_bookmark_is_made_from_the_panes_own_button(cx: &mut TestAppContext) {
    let (_dir, path) = seed_copy("two-page.pdf");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            let tree = frame.accessible(window, cx);
            let button = tree
                .find(&"bookmark-new".into())
                .expect("the empty pane offers New Bookmark");
            assert!(!button.state.disabled);
            let activation = button.activation.clone().expect("it runs");
            frame.run_activation(activation, window, cx);
            assert_eq!(frame.dialog, Some(ShellDialog::BookmarkTitle));
            frame.run_activation(
                Activation::BookmarkTitle(BookmarkTitleAction::Submit),
                window,
                cx,
            );
            assert_eq!(outline(frame, cx), [("Untitled".to_owned(), 0)]);
        })
        .unwrap();
}

/// Acrobat types a comment where it is: "Click where you want to place the
/// note. Type text in the pop-up note." Placing a sticky note or a text box
/// opens a focused field on it, and what is typed becomes the comment's
/// text, drawn into a text box's appearance.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn placing_a_text_comment_opens_a_field_and_the_typed_text_is_its_text(cx: &mut TestAppContext) {
    for tool_id in ["sticky-note", "text-box"] {
        let (_dir, path) = seed_copy("hello.pdf");
        let window = window_on(&path, cx);
        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.tabs.active().expect("a tab").canvas.clone();
                canvas.update(cx, |canvas, _| {
                    let index = canvas
                        .model
                        .registry()
                        .tools()
                        .position(|tool| tool.id() == tool_id)
                        .expect("installed");
                    canvas.model.activate_tool(index).expect("activates");
                    let page = canvas.model.viewport().visible_pages().unwrap()[0].rect;
                    let at = gpui::point(
                        px(page.origin.x + page.size.width / 3.0),
                        px(page.origin.y + page.size.height / 3.0),
                    );
                    canvas
                        .model
                        .pointer_down(at, 1.0, gpui::Modifiers::default())
                        .unwrap();
                    canvas
                        .model
                        .pointer_up(at, 1.0, gpui::Modifiers::default())
                        .unwrap();
                    assert!(
                        canvas.model.text_target().is_some(),
                        "{tool_id}: the placed comment waits for its text"
                    );
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                let canvas = frame.tabs.active().expect("a tab").canvas.clone();
                canvas.update(cx, |canvas, cx| {
                    canvas.sync_inline_text(window, cx);
                    let input = canvas
                        .inline
                        .as_ref()
                        .expect("the field is open")
                        .input
                        .clone();
                    assert!(
                        input.read(cx).focus_handle(cx).is_focused(window),
                        "{tool_id}: typing goes straight into it"
                    );
                    input.update(cx, |input, cx| input.set_query("Check this figure", cx));
                    canvas.finish_inline_text(cx);
                    assert!(canvas.inline.is_none(), "Enter closed it");
                    let annotations = canvas.model.document_mut().annotations().expect("reads");
                    let placed = annotations
                        .iter()
                        .find(|annotation| annotation.in_reply_to.is_none())
                        .expect("the comment");
                    assert_eq!(
                        placed.contents.as_deref(),
                        Some("Check this figure"),
                        "{tool_id}"
                    );
                });
            })
            .unwrap();
    }
}
