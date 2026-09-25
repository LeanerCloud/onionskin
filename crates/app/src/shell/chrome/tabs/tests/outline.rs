//! Bookmark and attachment authoring from the panes, on a real window.

use gpui::point;
use onionskin_core::{AnnotationFilter, Document};

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
            assert_eq!(
                labels,
                [
                    "Add Attachment…",
                    "Open",
                    "Save",
                    "Edit Description…",
                    "Delete",
                    "Search Attachments…"
                ]
            );
            let delete = menu.children[4].activation.clone().expect("Delete runs");
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

/// Open a PDF attachment in a tab of its own; anything else is refused.
#[gpui::test]
fn a_pdf_attachment_opens_in_a_tab_and_another_file_does_not(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![
            ("annexed.pdf", crate::shell::fixtures::attached_pdf_pdf()),
            ("notes.pdf", crate::shell::fixtures::attachment_pdf()),
        ],
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ActivateTab(0), window, cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.tabs().len(), 3, "the annex opened");
            let opened = frame.tabs.active().expect("a tab");
            assert_eq!(opened.title(), "annex.pdf");
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let notes = frame
                .tabs
                .tabs()
                .iter()
                .position(|tab| tab.title() == "notes.pdf")
                .expect("the second document is open");
            frame.run_activation(Activation::ActivateTab(notes), window, cx);
            // The pane stays open across the switch; read this document.
            assert_eq!(frame.navigation.active(), Some(NavigationPane::Attachments));
            let canvas = frame.active_canvas().expect("a tab").clone();
            frame.navigation.reread(&canvas, cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.tabs().len(), 3, "a text file does not open");
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some(crate::shell::chrome::tabs::attachment_commands::NOT_A_PDF)
            );
        })
        .unwrap();
}

#[gpui::test]
fn malformed_pdf_attachment_is_refused_without_a_tab_or_traversal_name(cx: &mut TestAppContext) {
    let parent = crate::shell::fixtures::attached_pdf_with_payload(
        "../../etc/passwd",
        b"%PDF-not-a-valid-document",
    );
    let (window, _) = bound_window_from_bytes(vec![("parent.pdf", parent)], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            let attachment = frame.navigation.attachment(0).expect("attachment");
            assert_eq!(attachment.file_name(), "passwd");
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.tabs().len(), 1);
            assert!(frame
                .notices
                .last()
                .is_some_and(|notice| notice.contains("could not be opened")));
        })
        .unwrap();
}

#[gpui::test]
fn same_named_attachments_from_different_documents_open_independent_copies(
    cx: &mut TestAppContext,
) {
    let heron_parent = crate::shell::fixtures::attached_pdf_with_word("heron");
    let egret_parent = crate::shell::fixtures::attached_pdf_with_word("egret");
    let mut heron_document = Document::open_bytes(heron_parent.clone()).expect("opens heron");
    let mut egret_document = Document::open_bytes(egret_parent.clone()).expect("opens egret");
    let heron_attachment = heron_document.attachments().expect("lists heron");
    let egret_attachment = egret_document.attachments().expect("lists egret");
    assert_eq!(heron_attachment.len(), 1);
    assert_eq!(egret_attachment.len(), 1);
    assert_eq!(heron_attachment[0].name, "annex.pdf");
    assert_eq!(egret_attachment[0].name, "annex.pdf");
    assert_eq!(heron_attachment[0].stream, 7);
    assert_eq!(egret_attachment[0].stream, 7);
    let heron_bytes = heron_document
        .attachment_bytes(0)
        .expect("extracts heron attachment");
    let egret_bytes = egret_document
        .attachment_bytes(0)
        .expect("extracts egret attachment");
    assert_ne!(heron_bytes, egret_bytes);
    assert_eq!(
        Document::open_bytes(heron_bytes.clone())
            .expect("opens heron attachment")
            .page_text(0)
            .expect("reads heron text")
            .flatten()
            .text,
        "heron"
    );
    assert_eq!(
        Document::open_bytes(egret_bytes.clone())
            .expect("opens egret attachment")
            .page_text(0)
            .expect("reads egret text")
            .flatten()
            .text,
        "egret"
    );

    let (window, _) = bound_window_from_bytes(
        vec![
            ("parent-heron.pdf", heron_parent.clone()),
            ("parent-egret.pdf", egret_parent.clone()),
        ],
        cx,
    );
    let parent_canvases = window
        .update(cx, |frame, _window, _cx| {
            frame
                .tabs
                .tabs()
                .iter()
                .map(|tab| tab.canvas.clone())
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(parent_canvases.len(), 2);
    let recents_before = window
        .update(cx, |frame, _window, _cx| {
            frame.settings.recents.documents().len()
        })
        .unwrap();

    let first_child = window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ActivateTab(0), window, cx);
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.tabs().len(), 3);
            let tab = frame.tabs.active().expect("heron attachment tab");
            assert_eq!(tab.title(), "annex.pdf");
            let text = tab.canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .page_text(0)
                    .expect("reads heron tab text")
                    .flatten()
                    .text
            });
            assert_eq!(text, "heron");
            tab.canvas.clone()
        })
        .unwrap();

    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ActivateTab(1), window, cx);
            let parent = frame.active_canvas().expect("egret parent").clone();
            frame.navigation.reread(&parent, cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.tabs().len(), 4);
        })
        .unwrap();

    let repeated_child = window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ActivateTab(0), window, cx);
            let parent = frame.active_canvas().expect("heron parent").clone();
            frame.navigation.reread(&parent, cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.tabs().len(), 5);
            let canvas = frame.tabs.active().expect("repeated child").canvas.clone();
            canvas.update(cx, |canvas, cx| {
                let result = canvas
                    .model
                    .document_mut()
                    .edit_pages("Add page", |tx, structure| {
                        onionskin_core::pages::insert_blank_pages(
                            tx,
                            structure,
                            1,
                            1,
                            [0.0, 0.0, 300.0, 100.0],
                        )
                        .map(|_| ())
                    });
                if result.is_ok() {
                    canvas.handle_change(Ok(true), cx);
                }
            });
            let page_count = canvas.update(cx, |canvas, _| canvas.model.view_state().page_count);
            assert_eq!(page_count, 2);
            canvas.entity_id()
        })
        .unwrap();

    window
        .update(cx, |frame, _window, cx| {
            let children: Vec<_> = frame
                .tabs
                .tabs()
                .iter()
                .filter(|tab| tab.title() == "annex.pdf")
                .collect();
            assert_eq!(children.len(), 3);
            assert_ne!(
                children[0].canvas.entity_id(),
                children[1].canvas.entity_id()
            );
            assert_ne!(
                children[1].canvas.entity_id(),
                children[2].canvas.entity_id()
            );
            assert_eq!(children[0].canvas.entity_id(), first_child.entity_id());
            assert_eq!(children[2].canvas.entity_id(), repeated_child);
            for (tab, expected_text) in children.iter().zip(["heron", "egret", "heron"]) {
                assert_eq!(tab.title(), "annex.pdf");
                assert_eq!(tab.canvas.read(cx).model.path(), None);
                let text = tab.canvas.update(cx, |canvas, _| {
                    canvas
                        .model
                        .document_mut()
                        .page_text(0)
                        .expect("reads child text")
                        .flatten()
                        .text
                });
                assert_eq!(text, expected_text);
            }
            assert_eq!(children[0].canvas.read(cx).model.view_state().page_count, 1);
            assert_eq!(children[1].canvas.read(cx).model.view_state().page_count, 1);
            assert_eq!(children[2].canvas.read(cx).model.view_state().page_count, 2);
            for (canvas, original) in parent_canvases.iter().zip([&heron_parent, &egret_parent]) {
                let bytes = canvas.update(cx, |canvas, _| {
                    canvas
                        .model
                        .document_mut()
                        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                        .expect("reads parent bytes")
                        .as_ref()
                        .clone()
                });
                assert_eq!(&bytes, original);
            }
            for (canvas, original) in parent_canvases.iter().zip([&heron_bytes, &egret_bytes]) {
                let bytes = canvas.update(cx, |canvas, _| {
                    canvas
                        .model
                        .document_mut()
                        .attachment_bytes(0)
                        .expect("reads parent attachment")
                });
                assert_eq!(&bytes, original);
            }
            let tree = frame.accessible(_window, cx);
            for child in &children {
                let node = tree
                    .find(&tab_element_id(child.canvas.entity_id()).into())
                    .expect("same-name attachment tab node");
                assert_eq!(node.label.as_str(), "annex.pdf");
            }
            assert_eq!(frame.settings.recents.documents().len(), recents_before);
        })
        .unwrap();
    assert!(!std::env::temp_dir().join("onionskin-attachments").exists());
}

#[gpui::test]
fn a_detached_attachment_save_as_uses_its_name_and_becomes_path_backed(cx: &mut TestAppContext) {
    let parent_bytes = crate::shell::fixtures::attached_pdf_pdf();
    let (window, _) = bound_window_from_bytes(vec![("parent.pdf", parent_bytes.clone())], cx);
    let dir = tempfile::tempdir().expect("dir");
    let destination = dir.path().join("saved-annex.pdf");
    let expected = destination.clone();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            assert_eq!(frame.tabs.active().expect("child").title(), "annex.pdf");
            let child = frame.tabs.active().expect("child").canvas.clone();
            child.update(cx, |canvas, cx| {
                let result = canvas
                    .model
                    .document_mut()
                    .edit_pages("Add page", |tx, structure| {
                        onionskin_core::pages::insert_blank_pages(
                            tx,
                            structure,
                            1,
                            1,
                            [0.0, 0.0, 300.0, 100.0],
                        )
                        .map(|_| ())
                    });
                if result.is_ok() {
                    canvas.handle_change(Ok(true), cx);
                }
            });
            let child_id = child.entity_id();
            assert_eq!(
                frame
                    .accessible(window, cx)
                    .find(&tab_element_id(child_id).into())
                    .and_then(|tab| tab.description.as_deref()),
                Some("Unsaved changes")
            );
            frame.save_active(cx);
        })
        .unwrap();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            let tab = frame.tabs.active().expect("unsaved child");
            assert_eq!(tab.title(), "annex.pdf");
            assert_eq!(tab.canvas.read(cx).model.path(), None);
            assert_eq!(
                frame
                    .accessible(_window, cx)
                    .find(&tab_element_id(tab.canvas.entity_id()).into())
                    .and_then(|tab| tab.description.as_deref()),
                Some("Unsaved changes")
            );
            frame.context_menus.tab_context_menu =
                Some(crate::shell::chrome::tabs::context::TabContextMenu {
                    tab_index: 1,
                    origin: point(px(0.0), px(0.0)),
                });
            let tree = frame.accessible(_window, cx);
            let menu = tree.find(&"tab-context-menu".into()).expect("tab menu");
            assert!(menu.children[3].state.disabled);
            assert!(menu.children[4].state.disabled);
            frame.save_active_as(cx);
        })
        .unwrap();
    assert!(cx.did_prompt_for_new_path());
    let rejected = dir.path().join("missing").join("rejected.pdf");
    cx.simulate_new_path_selection(move |_| Some(rejected));
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            let tab = frame.tabs.active().expect("rejected child");
            assert_eq!(tab.title(), "annex.pdf");
            assert_eq!(tab.canvas.read(cx).model.path(), None);
            assert_eq!(
                frame
                    .accessible(_window, cx)
                    .find(&tab_element_id(tab.canvas.entity_id()).into())
                    .and_then(|tab| tab.description.as_deref()),
                Some("Unsaved changes")
            );
        })
        .unwrap();
    window
        .update(cx, |frame, _window, cx| frame.save_active_as(cx))
        .unwrap();
    window
        .update(cx, |frame, _window, cx| frame.activate(0, cx))
        .unwrap();
    cx.simulate_new_path_selection(move |_| Some(destination.clone()));
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            let child = frame
                .tabs
                .tabs()
                .iter()
                .find(|tab| tab.title() == "saved-annex.pdf")
                .expect("saved child");
            let tab = child;
            assert_eq!(tab.canvas.read(cx).model.path(), Some(expected.clone()));
            frame.activate(
                frame
                    .tabs
                    .tabs()
                    .iter()
                    .position(|candidate| candidate.title() == "saved-annex.pdf")
                    .unwrap(),
                cx,
            );
            frame.context_menus.tab_context_menu =
                Some(crate::shell::chrome::tabs::context::TabContextMenu {
                    tab_index: 1,
                    origin: point(px(0.0), px(0.0)),
                });
            let tree = frame.accessible(_window, cx);
            let menu = tree
                .find(&"tab-context-menu".into())
                .expect("saved tab menu");
            assert!(!menu.children[3].state.disabled);
            assert!(!menu.children[4].state.disabled);
            let child = frame.tabs.active().expect("saved child").canvas.clone();
            child.update(cx, |canvas, cx| {
                let result = canvas.model.document_mut().edit_pages(
                    "Add page after Save As",
                    |tx, structure| {
                        onionskin_core::pages::insert_blank_pages(
                            tx,
                            structure,
                            1,
                            1,
                            [0.0, 0.0, 300.0, 100.0],
                        )
                        .map(|_| ())
                    },
                );
                if result.is_ok() {
                    canvas.handle_change(Ok(true), cx);
                }
            });
            frame.save_active(cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        Document::open_path(&expected)
            .expect("reopens")
            .page_count(),
        3
    );
    let parent = window
        .update(cx, |frame, _window, cx| {
            frame.tabs.tabs()[0].canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("parent bytes")
                    .as_ref()
                    .clone()
            })
        })
        .unwrap();
    assert_eq!(parent, parent_bytes);
}

#[gpui::test]
fn a_detached_attachment_new_window_keeps_its_name_and_pathlessness(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("parent.pdf", crate::shell::fixtures::attached_pdf_pdf())],
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            frame
                .run_main_menu_command(MenuCommand::NewWindow, window, cx)
                .expect("new window");
        })
        .unwrap();
    cx.run_until_parked();
    let child_window = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<ShellFrame>())
        .find(|handle| *handle != window)
        .expect("detached child window");
    child_window
        .update(cx, |frame, _window, cx| {
            let tab = frame.tabs.active().expect("child tab");
            assert_eq!(tab.title(), "annex.pdf");
            assert_eq!(tab.canvas.read(cx).model.path(), None);
        })
        .unwrap();
}

#[gpui::test]
fn a_save_as_response_after_closing_its_originating_tab_is_ignored(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("parent.pdf", crate::shell::fixtures::attached_pdf_pdf())],
        cx,
    );
    let dir = tempfile::tempdir().expect("dir");
    let destination = dir.path().join("closed-origin.pdf");
    let expected = destination.clone();
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            frame.save_active_as(cx);
            frame.run_tab_command(TabCommand::Close, 1, cx).unwrap();
            assert_eq!(frame.tabs.tabs().len(), 1);
        })
        .unwrap();
    cx.simulate_new_path_selection(move |_| Some(destination.clone()));
    cx.run_until_parked();
    assert!(!expected.exists());
}

#[gpui::test]
fn edit_description_rewrites_what_the_pane_shows(cx: &mut TestAppContext) {
    use crate::shell::chrome::description_dialog::DescriptionAction;
    let (window, _) = bound_window_from_bytes(
        vec![("notes.pdf", crate::shell::fixtures::attachment_pdf())],
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::EditDescription(0))),
                window,
                cx,
            );
            assert_eq!(frame.dialog, Some(ShellDialog::AttachmentDescription));
            let input = frame
                .text_field(TextField::AttachmentDescription)
                .expect("the field")
                .clone();
            assert_eq!(input.read(cx).query(), "Reviewer notes");
            input.update(cx, |input, cx| {
                input.set_query("Final notes".to_owned(), cx)
            });
            frame.run_activation(
                Activation::Description(DescriptionAction::Submit),
                window,
                cx,
            );
            assert_eq!(frame.dialog, None);
            assert_eq!(
                frame
                    .navigation
                    .attachment(0)
                    .and_then(|a| a.description.clone()),
                Some("Final notes".to_owned())
            );
        })
        .unwrap();
}

#[gpui::test]
fn search_attachments_opens_advanced_search_over_them(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("notes.pdf", crate::shell::fixtures::attachment_pdf())],
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Search)),
                window,
                cx,
            );
            assert_eq!(frame.dialog, Some(ShellDialog::AdvancedSearch));
            assert!(
                frame
                    .advanced_search_dialog()
                    .expect("open")
                    .form
                    .include_attachments
            );
        })
        .unwrap();
}
