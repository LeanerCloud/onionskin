//! P13c on a real window: Edit > Cut / Copy / Paste / Delete answered by the
//! active tool, Copy File to Clipboard, and Attach to Email's availability.

use super::*;

fn window_over_copy(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    gpui::WindowHandle<ShellFrame>,
    Vec<crate::keymap::Binding>,
) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("hello.pdf");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"),
        &path,
    )
    .expect("copies");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let (window, bindings) = bound_window_with_models(
        vec![(path.clone(), model)],
        crate::config::ConfigPaths::default(),
        cx,
    );
    (dir, path, window, bindings)
}

/// The Edit menu's four verbs, as the menu schema describes them.
#[cfg(feature = "tools-basic")]
fn edit_entries(frame: &ShellFrame, cx: &App) -> Vec<(String, Option<&'static str>)> {
    let schema = crate::shell::chrome::global_bar::main_menu_schema(frame.menu_state(cx));
    schema
        .iter()
        .flat_map(|section| section.entries.iter())
        .filter(|entry| matches!(entry.command, MenuCommand::Edit(_)))
        .map(|entry| (entry.label.to_owned(), entry.availability.reason()))
        .collect()
}

/// With the text tool active and text selected, Copy (pressed as its
/// keystroke) puts the selection on the clipboard; Cut, Paste and Delete are
/// disabled with the reason the tool gives, because text here is not
/// editable.
#[cfg(feature = "tools-basic")]
#[gpui::test]
fn the_edit_verbs_are_what_the_active_tool_answers(cx: &mut TestAppContext) {
    use onionskin_core::TextSelection;

    let (_dir, _path, window, bindings) = window_over_copy(cx);
    window
        .update(cx, |frame, _window, cx| {
            let canvas = frame.tabs.active().unwrap().canvas.clone();
            canvas.update(cx, |canvas, _| {
                let index = canvas
                    .model
                    .registry()
                    .tools()
                    .position(|tool| tool.id() == "select-text")
                    .expect("installed");
                canvas.model.activate_tool(index).expect("activates");
                canvas
                    .model
                    .document_mut()
                    .selection_mut()
                    .set_text(TextSelection {
                        page: 0,
                        quads: Vec::new(),
                        text: "Hello, world".into(),
                    });
            });
            let entries = edit_entries(frame, cx);
            assert_eq!(
                entries,
                [
                    ("Cut".to_owned(), Some("The active tool has nothing to cut")),
                    ("Copy".to_owned(), None),
                    ("Paste".to_owned(), Some("The active tool does not paste")),
                    (
                        "Delete".to_owned(),
                        Some("The active tool has nothing to delete")
                    ),
                ]
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.copy"));
    cx.run_until_parked();
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Hello, world")
    );
    // A verb no tool claims runs nothing, even pressed.
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("kept".into()));
    cx.simulate_keystrokes(window.into(), &keystroke_for(&bindings, "edit.cut"));
    cx.run_until_parked();
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("kept")
    );
}

/// Copy File to Clipboard puts the saved file's URI on the clipboard, which
/// resolves back to the file.
#[gpui::test]
fn copy_file_to_clipboard_names_the_saved_file(cx: &mut TestAppContext) {
    let (_dir, path, window, _) = window_over_copy(cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::CopyFileToClipboard, window, cx)
                .expect("runs");
        })
        .unwrap();
    let uri = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("a URI");
    assert!(uri.starts_with("file://"), "{uri}");
    let resolved = uri.trim_start_matches("file://").replace("%20", " ");
    assert_eq!(Path::new(&resolved), path.as_path());
    assert!(Path::new(&resolved).exists());
}

/// Attach to Email sends the file on disk, so it is disabled with a reason
/// while there are unsaved changes, and live again once they are saved.
#[cfg(all(unix, feature = "commands-core"))]
#[gpui::test]
fn attach_to_email_waits_for_unsaved_changes(cx: &mut TestAppContext) {
    use crate::shell::chrome::global_bar::PageCommand;

    let (_dir, _path, window, _) = window_over_copy(cx);
    let reason = |frame: &ShellFrame, cx: &App| {
        crate::shell::chrome::global_bar::main_menu_schema(frame.menu_state(cx))
            .iter()
            .flat_map(|section| section.entries.iter())
            .find(|entry| entry.command == MenuCommand::AttachToEmail)
            .expect("in the File menu")
            .availability
            .reason()
    };
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(reason(frame, cx), None, "a saved, clean file");
            frame
                .run_main_menu_command(MenuCommand::Page(PageCommand::RotateClockwise), window, cx)
                .expect("rotates");
            assert_eq!(
                reason(frame, cx),
                Some("Save first, so the email carries your changes")
            );
        })
        .unwrap();
}
