//! The Combine and Split dialogs, driven through `run_activation` - the one
//! route a click and a screen reader share - on a real window.

use super::*;
use crate::shell::chrome::accessible::TextField;
use crate::shell::chrome::combine_dialog::{CombineAction, CombineEntry, CombineEntryPoint};
use crate::shell::chrome::split_dialog::{SplitAction, SplitMode};

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

fn entry(path: PathBuf) -> CombineEntry {
    CombineEntry {
        page_count: onionskin_commands_core::combine::page_count(&path).map_err(|e| e.to_string()),
        path,
        pages: None,
    }
}

/// A window whose one tab is `bytes` saved at `path`, so a split has a folder
/// to write into.
fn window_on_file(
    path: &Path,
    bytes: &[u8],
    cx: &mut TestAppContext,
) -> gpui::WindowHandle<ShellFrame> {
    std::fs::write(path, bytes).expect("the fixture is written");
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

fn described(
    frame: &mut ShellFrame,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) -> Vec<String> {
    let tree = frame.accessible(window, cx);
    let mut found = Vec::new();
    collect(&tree, &mut found);
    found
}

fn collect(element: &crate::shell::chrome::accessible::Element, into: &mut Vec<String>) {
    into.push(element.label.to_string());
    for child in &element.children {
        collect(child, into);
    }
}

#[gpui::test]
fn the_file_menu_opens_combine_and_says_which_entry_opened_it(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    for (command, entry_point) in [
        (MenuCommand::CombineFiles, CombineEntryPoint::Combine),
        (
            MenuCommand::CreateFromFiles,
            CombineEntryPoint::CreateFromFiles,
        ),
    ] {
        window
            .update(cx, |frame, window, cx| {
                frame.run_activation(Activation::MainMenu(command), window, cx);
                assert_eq!(frame.dialog, Some(ShellDialog::Combine(entry_point)));
                let labels = described(frame, window, cx);
                assert!(
                    labels.iter().any(|label| label == entry_point.title()),
                    "{labels:?}"
                );
                let says_so = labels
                    .iter()
                    .any(|label| label.contains("same as Combine Files"));
                assert_eq!(says_so, entry_point == CombineEntryPoint::CreateFromFiles);
                frame.close_dialog(window, cx);
            })
            .unwrap();
    }
}

/// Reorder, preview, remove and expand, each through the activation a click
/// sends, each visible in what the dialog describes.
#[gpui::test]
fn the_combine_list_reorders_previews_expands_and_removes(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::MainMenu(MenuCommand::CombineFiles), window, cx);
            let state = frame.organize.combine.as_mut().expect("the dialog is up");
            state
                .list
                .add([entry(seed("hello.pdf")), entry(seed("two-page.pdf"))]);
            frame.run_activation(Activation::Combine(CombineAction::MoveDown), window, cx);
        })
        .unwrap();
    window
        .update(cx, |frame, window, cx| {
            let labels = described(frame, window, cx);
            let rows: Vec<&String> = labels
                .iter()
                .filter(|label| label.contains(".pdf - "))
                .collect();
            assert_eq!(rows, ["two-page.pdf - 2 pages", "hello.pdf - 1 page"]);

            frame.run_activation(Activation::Combine(CombineAction::Select(0)), window, cx);
            frame
                .text_field(TextField::CombinePages)
                .expect("the page field is there")
                .update(cx, |input, cx| input.set_query("2", cx));
            frame.run_activation(Activation::Combine(CombineAction::ApplyPages), window, cx);
            let labels = described(frame, window, cx);
            assert!(
                labels
                    .iter()
                    .any(|label| label == "two-page.pdf - 1 page of 2: 2"),
                "{labels:?}"
            );

            frame.run_activation(Activation::Combine(CombineAction::Remove), window, cx);
            let state = frame.combine_dialog().expect("still up");
            assert_eq!(state.list.entries.len(), 1);
            assert_eq!(state.list.entries[0].path, seed("hello.pdf"));
        })
        .unwrap();
}

#[gpui::test]
fn a_bad_page_list_is_reported_in_the_dialog(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::MainMenu(MenuCommand::CombineFiles), window, cx);
            frame
                .organize
                .combine
                .as_mut()
                .expect("up")
                .list
                .add([entry(seed("hello.pdf"))]);
            frame
                .text_field(TextField::CombinePages)
                .expect("field")
                .update(cx, |input, cx| input.set_query("5", cx));
            frame.run_activation(Activation::Combine(CombineAction::ApplyPages), window, cx);
            let error = frame.combine_dialog().and_then(|state| state.error.clone());
            assert_eq!(error.as_deref(), Some("There is no page 5; the file has 1"));
            assert!(described(frame, window, cx)
                .contains(&"There is no page 5; the file has 1".to_owned()));
        })
        .unwrap();
}

/// Combine asks where, writes there, and opens what it wrote.
#[gpui::test]
fn combining_writes_the_file_chosen_and_opens_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let destination = dir.path().join("Both.pdf");
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::MainMenu(MenuCommand::CombineFiles), window, cx);
            frame
                .organize
                .combine
                .as_mut()
                .expect("up")
                .list
                .add([entry(seed("hello.pdf")), entry(seed("two-page.pdf"))]);
            frame.run_activation(Activation::Combine(CombineAction::Submit), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(destination.clone()));
    cx.run_until_parked();

    let combined = Document::open_path(&destination).expect("the combined file opens");
    assert_eq!(combined.page_count(), 3);
    window
        .update(cx, |frame, _, _| {
            assert!(
                frame.dialog.is_none(),
                "the dialog closes once the file is written"
            );
            assert_eq!(
                frame.tabs.tabs().len(),
                2,
                "and the combined file opens in a tab"
            );
        })
        .unwrap();
}

/// The split dialog cuts the open document beside itself.
#[gpui::test]
fn splitting_from_the_dialog_writes_the_parts_beside_the_document(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("Pair.pdf");
    let window = window_on_file(
        &path,
        &std::fs::read(seed("two-page.pdf")).expect("read"),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::MainMenu(MenuCommand::SplitDocument), window, cx);
            assert_eq!(frame.dialog, Some(ShellDialog::Split));
            frame.run_activation(
                Activation::Split(SplitAction::SetMode(SplitMode::PageCount)),
                window,
                cx,
            );
            frame
                .text_field(TextField::SplitValue)
                .expect("the value field is there")
                .update(cx, |input, cx| input.set_query("1", cx));
            frame.run_activation(Activation::Split(SplitAction::Submit), window, cx);
            assert!(
                frame.dialog.is_none(),
                "{:?}",
                frame.split_dialog().and_then(|s| s.error.clone())
            );
        })
        .unwrap();
    for part in 1..=2 {
        let written = dir.path().join(format!("Pair - Part {part}.pdf"));
        assert_eq!(
            Document::open_path(&written).expect("opens").page_count(),
            1
        );
    }
}

/// Bookmarks need no value, so the field leaves the dialog - and the tab
/// order - rather than lingering as a stop that does nothing.
#[gpui::test]
fn the_value_field_leaves_when_splitting_at_bookmarks(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["two-page.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::MainMenu(MenuCommand::SplitDocument), window, cx);
            assert!(frame.text_field(TextField::SplitValue).is_some());
            let named = |labels: &[String]| {
                labels
                    .iter()
                    .filter(|label| *label == "Number of pages")
                    .count()
            };
            assert_eq!(
                named(&described(frame, window, cx)),
                2,
                "the choice, and the field it names"
            );
            frame.run_activation(
                Activation::Split(SplitAction::SetMode(SplitMode::TopLevelBookmarks)),
                window,
                cx,
            );
            assert!(frame.text_field(TextField::SplitValue).is_none());
            assert_eq!(
                named(&described(frame, window, cx)),
                1,
                "only the choice now"
            );
        })
        .unwrap();
}

/// A bad value is reported in the dialog, which stays up.
#[gpui::test]
fn a_bad_split_value_is_reported_and_nothing_is_written(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("Pair.pdf");
    let window = window_on_file(
        &path,
        &std::fs::read(seed("two-page.pdf")).expect("read"),
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::MainMenu(MenuCommand::SplitDocument), window, cx);
            frame
                .text_field(TextField::SplitValue)
                .expect("field")
                .update(cx, |input, cx| input.set_query("zero", cx));
            frame.run_activation(Activation::Split(SplitAction::Submit), window, cx);
            assert_eq!(frame.dialog, Some(ShellDialog::Split));
            assert!(frame
                .split_dialog()
                .and_then(|state| state.error.clone())
                .is_some());
        })
        .unwrap();
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("list").count(),
        1,
        "only the document"
    );
}

/// The session-scoped refusal: an encrypted document's Split entry is disabled
/// with the document's reason, through the registry's read-out effect.
#[gpui::test]
fn split_is_disabled_on_an_encrypted_document(cx: &mut TestAppContext) {
    let encrypted = onionskin_corpus_testing::encrypted_fixture("r4-aes-128.pdf");
    let dir = tempfile::tempdir().expect("dir");
    let window = window_on_file(
        &dir.path().join("Locked.pdf"),
        &std::fs::read(encrypted).expect("read"),
        cx,
    );
    window
        .update(cx, |frame, _, cx| {
            let reason = frame.command_unavailable(MenuCommand::SplitDocument, cx);
            assert_eq!(
                reason,
                Some(onionskin_core::protection::Refusal::EncryptedSource.reason())
            );
            assert_eq!(
                frame.command_unavailable(MenuCommand::CombineFiles, cx),
                None
            );
        })
        .unwrap();
}
