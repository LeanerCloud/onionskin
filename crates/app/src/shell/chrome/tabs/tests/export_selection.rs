//! P22 on a real window: Export Selection As from the text context menu
//! asks where, and writes the selection as RTF with its faces, or as plain
//! text for any other name.

use super::*;
use crate::shell::context_menu::CanvasContextCommand;

/// A window over `hello.pdf` with the whole of page one selected.
fn selected(cx: &mut TestAppContext) -> (gpui::WindowHandle<ShellFrame>, String) {
    let (window, _bindings) =
        bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::default(), cx);
    let text = window
        .update(cx, |frame, _window, cx| {
            let canvas = frame.active_canvas().unwrap().clone();
            canvas.update(cx, |canvas, _| {
                let mut document = canvas.model.document_mut();
                let page = document.page_text(0).expect("reads").clone();
                let (text, spans) = onionskin_core::textselect::styled_text(&page);
                document
                    .selection_mut()
                    .set_text(onionskin_core::TextSelection {
                        page: 0,
                        quads: Vec::new(),
                        text: text.clone(),
                        spans,
                    });
                text
            })
        })
        .unwrap();
    assert!(!text.is_empty(), "hello.pdf has text");
    (window, text)
}

fn export_to(
    window: gpui::WindowHandle<ShellFrame>,
    output: &Path,
    cx: &mut TestAppContext,
) -> Vec<String> {
    window
        .update(cx, |frame, window, cx| {
            frame.run_canvas_context_command(CanvasContextCommand::ExportSelectionAs, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    let chosen = output.to_path_buf();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, _cx| frame.notices.clone())
        .unwrap()
}

#[gpui::test]
fn a_txt_name_gets_the_selected_text(cx: &mut TestAppContext) {
    let (window, text) = selected(cx);
    let dir = tempfile::tempdir().expect("dir");
    let output = dir.path().join("words.txt");
    let notices = export_to(window, &output, cx);
    assert_eq!(std::fs::read_to_string(&output).expect("written"), text);
    assert!(notices
        .iter()
        .any(|notice| notice.starts_with("Exported the selection to")));
}

#[cfg(feature = "codecs-common")]
#[gpui::test]
fn an_rtf_name_gets_rich_text_in_the_selections_face(cx: &mut TestAppContext) {
    let (window, text) = selected(cx);
    let dir = tempfile::tempdir().expect("dir");
    let output = dir.path().join("words.rtf");
    export_to(window, &output, cx);
    let rtf = std::fs::read_to_string(&output).expect("written");
    assert!(rtf.starts_with("{\\rtf1"), "{rtf}");
    let first_word = text.split_whitespace().next().expect("a word");
    assert!(rtf.contains(first_word), "{rtf}");
    assert!(rtf.contains("\\fs"), "a size is carried: {rtf}");
}

/// Writing the selection to a file reads the document out, so an encrypted
/// document refuses it before asking where, with the document's reason.
#[gpui::test]
fn an_encrypted_document_refuses_before_asking_where(cx: &mut TestAppContext) {
    let bytes = std::fs::read(onionskin_corpus_testing::encrypted_fixture(
        "r4-aes-128.pdf",
    ))
    .expect("read");
    let (window, _bindings) = bound_window_from_bytes(vec![("Locked.pdf", bytes)], cx);
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().unwrap().clone();
            canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().selection_mut().set_text(
                    onionskin_core::TextSelection {
                        page: 0,
                        quads: Vec::new(),
                        text: "secret".into(),
                        spans: Vec::new(),
                    },
                );
            });
            frame.run_canvas_context_command(CanvasContextCommand::ExportSelectionAs, window, cx);
            assert!(
                frame
                    .notices
                    .iter()
                    .any(|notice| notice.starts_with("The selection cannot be exported")),
                "{:?}",
                frame.notices
            );
        })
        .unwrap();
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path());
}
