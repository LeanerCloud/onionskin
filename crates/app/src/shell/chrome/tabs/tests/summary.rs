//! Summarize Comments on a real window: the Edit menu entry, the dialog, the
//! save prompt, and the summary opened in a tab.

use onionskin_core::{add_annotation, Annotation, Rect, Subtype};

use super::*;
use crate::shell::chrome::summary_dialog::{SummaryAction, SummaryChoice};

/// A window whose one tab is `bytes` saved at `path`, with a comment on its
/// first page when `commented`.
fn window_on(
    path: &Path,
    bytes: &[u8],
    commented: bool,
    cx: &mut TestAppContext,
) -> gpui::WindowHandle<ShellFrame> {
    std::fs::write(path, bytes).expect("the fixture is written");
    let mut document = Document::open_path(path).expect("opens");
    if commented {
        let page = document
            .structure()
            .expect("the document")
            .page(0)
            .expect("page one")
            .objref;
        let mut note = Annotation::new(Subtype::Text, Rect::new(10.0, 10.0, 30.0, 30.0));
        note.contents = Some("Recheck the total".into());
        note.author = Some("Ana".into());
        document
            .edit_annotations("Comment", |tx, structure| {
                add_annotation(tx, structure, page, &note, 0).map(|_| ())
            })
            .expect("comments");
    }
    let model = CanvasModel::new(
        document,
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

fn seed_bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name),
    )
    .expect("the seed reads")
}

#[gpui::test]
fn the_summary_is_written_where_the_user_says_and_opened(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let window = window_on(
        &dir.path().join("Report.pdf"),
        &seed_bytes("two-page.pdf"),
        true,
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::SummarizeComments, window, cx)
                .expect("live");
            assert_eq!(frame.dialog, Some(ShellDialog::Summary));
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"summary-comments-only".into()).is_some());
            frame.run_activation(
                Activation::Summary(SummaryAction::Choose(SummaryChoice::DocumentAndComments)),
                window,
                cx,
            );
            frame.run_activation(Activation::Summary(SummaryAction::Submit), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    let output = dir.path().join("Report - Comments.pdf");
    let chosen = output.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();

    window
        .update(cx, |frame, _, cx| {
            assert_eq!(frame.dialog, None);
            let tab = frame.tabs.active().expect("the summary's tab");
            assert_eq!(tab.source, output);
            assert_eq!(
                tab.canvas.read(cx).model.view_state().page_count,
                3,
                "page one, its comments, page two"
            );
        })
        .unwrap();
    let mut summary = Document::open_path(&output).expect("a PDF");
    let text = summary.page_text(1).expect("extracts").flatten().text;
    assert!(text.contains("Recheck the total"), "{text}");
}

#[gpui::test]
fn a_document_without_comments_says_so_in_the_dialog(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let window = window_on(
        &dir.path().join("Clean.pdf"),
        &seed_bytes("hello.pdf"),
        false,
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::SummarizeComments, window, cx)
                .expect("live");
            frame.summarize_to(&dir.path().join("out.pdf"), cx);
            let error = frame
                .summary_dialog()
                .and_then(|state| state.error.clone())
                .expect("an error in the dialog");
            assert_eq!(error, "The document has no comments to summarize");
        })
        .unwrap();
    assert!(!dir.path().join("out.pdf").exists());
}

/// The session-scoped refusal: an encrypted document's entry is disabled with
/// the document's reason, through the command's read-out effect.
#[gpui::test]
fn summarize_is_disabled_on_an_encrypted_document(cx: &mut TestAppContext) {
    let encrypted = onionskin_corpus_testing::encrypted_fixture("r6-aes-256-print-only.pdf");
    let dir = tempfile::tempdir().expect("dir");
    let window = window_on(
        &dir.path().join("Locked.pdf"),
        &std::fs::read(encrypted).expect("read"),
        false,
        cx,
    );
    window
        .update(cx, |frame, _, cx| {
            assert_eq!(
                frame.command_unavailable(MenuCommand::SummarizeComments, cx),
                Some(onionskin_core::protection::Refusal::EncryptedSource.reason())
            );
        })
        .unwrap();
}
