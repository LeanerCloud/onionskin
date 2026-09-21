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
        "r4-aes-128.pdf",
    ))
    .expect("reads");
    let (window, _) = bound_window_from_bytes(vec![("locked.pdf", bytes)], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Bookmarks), cx);
            open_menu(frame, None, cx);
            let tree = frame.accessible(window, cx);
            let menu = tree.find(&"bookmarks-context-menu".into()).expect("menu");
            let reason = onionskin_core::protection::Refusal::EncryptedSource.reason();
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
