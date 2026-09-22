//! P14b on a real window: Reduce File Size says in words that the copy
//! keeps no history, writes a smaller new file, leaves the open document as
//! it was, and cannot be reached through Save.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::file_dialogs::{FileAction, REDUCE_TEXT};
use onionskin_core::images::{image_document, ImageColor, ImageData, ImagePage};

/// A 5 x 4 inch page drawing a 2000 x 1600 photo-like image: 400 ppi.
fn heavy() -> Vec<u8> {
    let (width, height) = (2000u32, 1600u32);
    let mut samples = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let texture = ((x * 7 + y * 13) % 17) as u8;
            samples.extend([
                (x * 255 / width) as u8 ^ texture,
                (y * 255 / height) as u8,
                90,
            ]);
        }
    }
    image_document(&ImagePage {
        width,
        height,
        dpi: (400.0, 400.0),
        color: ImageColor::Rgb,
        data: ImageData::Samples(samples),
        alpha: None,
        inverted_cmyk: false,
        icc: None,
    })
    .expect("builds")
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

#[gpui::test]
fn reduce_file_size_writes_a_smaller_copy_and_says_history_is_discarded(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("photo.pdf");
    let original = heavy();
    std::fs::write(&path, &original).expect("writes");
    let chosen = dir.path().join("photo small.pdf");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::ReduceFileSize, window, cx)
                .expect("opens the dialog");
            let tree = frame.accessible(window, cx);
            let question = tree.find(&"file-question".into()).expect("the dialog says");
            assert_eq!(question.label, REDUCE_TEXT);
            assert!(question.label.contains("keeps no editing history"));
            frame.run_activation(Activation::File(FileAction::ReduceFileSize), window, cx);
        })
        .unwrap();
    let answer = chosen.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();

    let written = std::fs::read(&chosen).expect("the copy was written");
    assert!(written.len() * 2 < original.len());
    assert_eq!(
        std::fs::read(&path).expect("reads"),
        original,
        "the open document's file is untouched"
    );
    window
        .update(cx, |frame, _window, cx| {
            let canvas = frame.tabs.active().unwrap().canvas.clone();
            let model = &canvas.read(cx).model;
            assert_eq!(
                model.path(),
                Some(path.clone()),
                "the tab stays on the original"
            );
            assert!(!model.history_facts().dirty);
            assert!(frame
                .notices
                .iter()
                .any(|notice| notice.contains("keeps no editing history")));
        })
        .unwrap();
}

/// The flattening path is not reachable from Save: an edit then Save
/// appends a section to the file, it never rewrites it.
#[gpui::test]
fn save_appends_and_never_rewrites(cx: &mut TestAppContext) {
    use crate::shell::chrome::global_bar::PageCommand;

    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("photo.pdf");
    std::fs::write(&path, heavy()).expect("writes");
    let window = window_on(&path, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Page(PageCommand::RotateClockwise), window, cx)
                .expect("rotates");
            frame
                .run_main_menu_command(MenuCommand::Save, window, cx)
                .expect("saves");
        })
        .unwrap();
    cx.run_until_parked();
    let saved = onionskin_cos::Document::open_path(&path).expect("reopens");
    assert_eq!(
        saved.sections().expect("sections").len(),
        2,
        "an appended section"
    );
}

/// Reduce File Size on an encrypted document is disabled with the
/// encrypted-source reason, as every command that reads a document out is.
#[gpui::test]
fn reduce_file_size_is_refused_on_an_encrypted_document(cx: &mut TestAppContext) {
    let bytes = std::fs::read(onionskin_corpus_testing::encrypted_fixture(
        "r4-aes-128.pdf",
    ))
    .expect("reads");
    let (window, _) = bound_window_from_bytes(vec![("locked.pdf", bytes)], cx);
    window
        .update(cx, |frame, _window, cx| {
            let entry = crate::shell::chrome::global_bar::main_menu_schema(frame.menu_state(cx))
                .into_iter()
                .flat_map(|section| section.entries)
                .find(|entry| entry.command == MenuCommand::ReduceFileSize)
                .expect("in the File menu");
            assert_eq!(
                entry.availability.reason(),
                Some(onionskin_core::protection::Refusal::EncryptedSource.reason())
            );
        })
        .unwrap();
}
