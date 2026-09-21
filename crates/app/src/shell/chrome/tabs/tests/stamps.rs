//! The Stamps dialog, Paste Clipboard Image as Stamp and Attach File's file,
//! on a real window, through `run_activation`.
//!
//! The file pickers are the platform's, which the test platform does not
//! implement, so the tests start from the path a picker would hand back.

use gpui::{ClipboardItem, Image, ImageFormat};
use image::ImageEncoder as _;
use onionskin_plugin_api::{tool_with, ToolCapability};

use super::*;
use crate::shell::chrome::stamps_dialog::{Row, StampAction};

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[0, 90, 180, 255], 2, 2, image::ExtendedColorType::L8)
        .expect("encodes");
    bytes
}

/// A window on hello.pdf whose tools keep their files under `data`.
fn stamp_window(data: &std::path::Path, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let (window, _) = bound_window_in(&["hello.pdf"], crate::config::ConfigPaths::in_dir(data), cx);
    window
        .update(cx, |frame, _, cx| {
            let environment = frame.settings.tool_environment();
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            canvas.update(cx, |canvas, _| canvas.model.configure_tools(&environment));
        })
        .unwrap();
    window
}

/// The stamp tool's index, whether it is the active tool, and its choice.
fn stamp_state(frame: &ShellFrame, cx: &App) -> (bool, Option<String>) {
    let canvas = frame.tabs.active().expect("a tab").canvas.read(cx);
    let registry = canvas.model.registry();
    let index = tool_with(registry, ToolCapability::Stamp).expect("a stamp tool");
    let chosen = registry.tools().nth(index).and_then(|tool| tool.chosen());
    (canvas.model.active_tool() == Some(index), chosen)
}

#[gpui::test]
fn the_stamps_dialog_lists_every_stamp_and_choosing_one_arms_the_tool(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = stamp_window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Stamps, window, cx)
                .expect("live");
            assert_eq!(frame.dialog, Some(ShellDialog::Stamps));
            let rows = frame.stamps_rows(cx);
            let stamps = rows
                .iter()
                .filter(|row| matches!(row, Row::Stamp { .. }))
                .count();
            assert_eq!(stamps, 22);
            assert!(
                !rows.iter().any(|row| matches!(row, Row::Delete(_))),
                "a built-in stamp cannot be deleted"
            );
            let headings: Vec<&str> = rows
                .iter()
                .filter_map(|row| match row {
                    Row::Category(name) => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(headings, ["Standard Business", "Sign Here", "Dynamic"]);

            let tree = frame.accessible(window, cx);
            let dialog = tree.find(&"dialog".into()).expect("described");
            assert_eq!(dialog.label, "Stamps");

            frame.run_activation(
                Activation::Stamps(StampAction::Choose("sign-sign-here".into())),
                window,
                cx,
            );
            assert_eq!(frame.dialog, None, "choosing closes the dialog");
            assert_eq!(
                stamp_state(frame, cx),
                (true, Some("sign-sign-here".into()))
            );
        })
        .unwrap();
}

#[gpui::test]
fn a_pasted_image_becomes_the_stamp_the_tool_places(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = stamp_window(data.path(), cx);
    cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
        ImageFormat::Png,
        png(),
    )));
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::PasteStamp, window, cx)
                .expect("live");
            assert!(frame.notices.is_empty(), "{:?}", frame.notices);
            assert_eq!(
                stamp_state(frame, cx),
                (true, Some("custom:Pasted/Clipboard Image".into()))
            );
        })
        .unwrap();
    assert!(data
        .path()
        .join("data/stamps/Pasted/Clipboard Image.pdf")
        .is_file());

    // A second paste replaces the first rather than refusing the name.
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::PasteStamp, window, cx)
                .expect("live");
            assert!(frame.notices.is_empty(), "{:?}", frame.notices);
        })
        .unwrap();
}

#[gpui::test]
fn an_empty_clipboard_pastes_no_stamp_and_says_so(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = stamp_window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::PasteStamp, window, cx)
                .expect("live");
            assert_eq!(frame.notices, ["The clipboard is empty"]);
            assert!(!stamp_state(frame, cx).0, "the stamp tool was not chosen");
        })
        .unwrap();
}

#[gpui::test]
fn a_custom_stamp_from_a_file_is_listed_under_custom_and_can_be_deleted(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = stamp_window(data.path(), cx);
    let image = data.path().join("Receipt.png");
    std::fs::write(&image, png()).expect("writes");
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Stamps, window, cx)
                .expect("live");
            frame.create_custom_stamp(&image, cx);
            let rows = frame.stamps_rows(cx);
            let delete = rows
                .iter()
                .find_map(|row| match row {
                    Row::Delete(choice) => Some(choice.clone()),
                    _ => None,
                })
                .expect("the custom stamp has a Delete");
            assert_eq!(delete.label, "Receipt");
            assert_eq!(delete.category, "Custom");

            // The same name again is refused, in the dialog.
            frame.create_custom_stamp(&image, cx);
            assert!(frame
                .stamps_rows(cx)
                .iter()
                .any(|row| matches!(row, Row::Error(_))));

            frame.run_activation(
                Activation::Stamps(StampAction::Delete(delete.id.clone())),
                window,
                cx,
            );
            assert!(!frame
                .stamps_rows(cx)
                .iter()
                .any(|row| matches!(row, Row::Delete(_))));

            // A built-in's id is not a custom stamp, and nothing is deleted.
            frame.run_activation(
                Activation::Stamps(StampAction::Delete("business-approved".into())),
                window,
                cx,
            );
            assert!(frame
                .stamps_rows(cx)
                .iter()
                .any(|row| matches!(row, Row::Error(error) if error.contains("custom"))));
            assert_eq!(
                frame
                    .stamps_rows(cx)
                    .iter()
                    .filter(|row| matches!(row, Row::Stamp { .. }))
                    .count(),
                22
            );
        })
        .unwrap();
}

#[gpui::test]
fn the_file_a_file_placing_tool_is_given_is_what_it_places(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = stamp_window(data.path(), cx);
    let file = data.path().join("notes.txt");
    std::fs::write(&file, b"notes").expect("writes");
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            let index = tool_with(
                canvas.read(cx).model.registry(),
                ToolCapability::ChoosesFile,
            )
            .expect("attach file is installed");
            frame.give_tool_file(&canvas, index, &file, cx);
            let chosen = canvas
                .read(cx)
                .model
                .registry()
                .tools()
                .nth(index)
                .and_then(|tool| tool.chosen());
            assert_eq!(chosen.as_deref(), file.to_str());

            frame.give_tool_file(&canvas, index, &data.path().join("missing.txt"), cx);
            assert!(frame.notices.last().unwrap().contains("cannot be used"));
        })
        .unwrap();
}
