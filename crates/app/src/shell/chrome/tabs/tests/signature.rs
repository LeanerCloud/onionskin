//! Add Signature and Add Initials on a real window: typed, drawn and image
//! signatures saved into the library, the Sign tool armed and placing one,
//! and Clear Saved.
//!
//! The image picker is the platform's, which the test platform does not
//! implement, so the image tests start from the path a picker would hand
//! back.

use gpui::{point, px};
use image::ImageEncoder as _;
use onionskin_core::Subtype;
use onionskin_plugin_api::{tool_with, ToolCapability};

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::signature_dialog::{Method, PadEvent, SignatureAction};
use crate::shell::dialog::ShellDialog;

/// A window on hello.pdf whose tools keep their files under `data`.
fn window(data: &std::path::Path, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
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

fn open(window: gpui::WindowHandle<ShellFrame>, initials: bool, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Signature { initials }, window, cx)
                .expect("live");
            assert_eq!(frame.dialog, Some(ShellDialog::Signature { initials }));
        })
        .unwrap();
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: SignatureAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Signature(action), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
}

fn error(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<String> {
    window
        .update(cx, |frame, _, _| {
            frame
                .signature_dialog()
                .and_then(|state| state.error.clone())
        })
        .unwrap()
}

fn last_notice(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> String {
    window
        .update(cx, |frame, _, _| {
            frame.notices.last().cloned().unwrap_or_default()
        })
        .unwrap()
}

fn saved(data: &std::path::Path, file: &str) -> bool {
    data.join("data/signatures").join(file).exists()
}

/// Whether the Sign tool is active, and what it has chosen.
fn sign_tool(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> (bool, Option<String>) {
    window
        .update(cx, |frame, _, cx| sign_tool_of(frame, cx))
        .unwrap()
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[0, 90, 180, 255], 2, 2, image::ExtendedColorType::L8)
        .expect("encodes");
    bytes
}

#[gpui::test]
fn a_typed_signature_is_saved_arms_the_sign_tool_and_is_placed(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    open(window, false, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let dialog = tree.find(&"dialog".into()).expect("described");
            assert_eq!(dialog.label, "Add Signature");
            assert!(
                tree.find(&"signature-name".into()).is_some(),
                "the name field"
            );
            assert!(tree.find(&"signature-clear-saved".into()).is_none());
        })
        .unwrap();

    act(window, SignatureAction::Save, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Type the name to sign with.")
    );

    window
        .update(cx, |frame, _, cx| {
            let input = frame.signature_dialog().expect("open").name.clone();
            input.update(cx, |input, cx| {
                input.set_query("Ada Lovelace".to_owned(), cx)
            });
        })
        .unwrap();
    act(window, SignatureAction::Save, cx);
    assert_eq!(window.update(cx, |frame, _, _| frame.dialog).unwrap(), None);
    assert!(saved(data.path(), "signature.pdf"));
    assert_eq!(
        last_notice(window, cx),
        "Saved your signature. Click where it goes."
    );
    assert_eq!(sign_tool(window, cx), (true, Some("signature".to_owned())));

    let placed = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            canvas.update(cx, |canvas, _| {
                let page = canvas.model.viewport().visible_pages().unwrap()[0].rect;
                let at = point(
                    px(page.origin.x + page.size.width / 2.0),
                    px(page.origin.y + page.size.height / 2.0),
                );
                let modifiers = gpui::Modifiers::default();
                canvas.model.pointer_down(at, 1.0, modifiers).unwrap();
                canvas.model.pointer_up(at, 1.0, modifiers).unwrap();
                canvas.model.annotations().expect("reads")
            })
        })
        .unwrap();
    let signature = placed
        .iter()
        .find(|annotation| annotation.subtype == Some(Subtype::Stamp))
        .expect("the Sign tool placed the signature");
    assert_eq!(signature.contents.as_deref(), Some("Signature"));
}

#[gpui::test]
fn drawn_initials_are_saved_and_cleared(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    open(window, true, cx);
    act(window, SignatureAction::SetMethod(Method::Draw), cx);
    act(window, SignatureAction::Save, cx);
    assert_eq!(error(window, cx).as_deref(), Some("Draw on the pad first."));

    window
        .update(cx, |frame, _, cx| {
            // Before the pad is painted a pointer event has nowhere to land.
            frame.signature_pad(PadEvent::Down(point(px(110.0), px(110.0))), cx);
            let origin = point(px(100.0), px(100.0));
            frame
                .signature_dialog()
                .expect("open")
                .pad_origin
                .set(Some(origin));
            frame.signature_pad(PadEvent::Down(point(px(110.0), px(110.0))), cx);
            frame.signature_pad(PadEvent::Move(point(px(150.0), px(130.0))), cx);
            frame.signature_pad(PadEvent::Up, cx);
            let form = &frame.signature_dialog().expect("open").form;
            assert_eq!(form.strokes, [vec![(10.0, 10.0), (50.0, 30.0)]]);
            assert!(!form.drawing);
        })
        .unwrap();
    act(window, SignatureAction::Save, cx);
    assert!(saved(data.path(), "initials.pdf"));
    assert_eq!(sign_tool(window, cx), (true, Some("initials".to_owned())));

    open(window, true, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let clear = tree
                .find(&"signature-clear-saved".into())
                .expect("offered once one is saved");
            assert_eq!(clear.label, "Clear Saved Initials");
        })
        .unwrap();
    act(window, SignatureAction::ClearSaved, cx);
    assert!(!saved(data.path(), "initials.pdf"));
    assert_eq!(last_notice(window, cx), "Cleared your saved initials.");
    let offered = window
        .update(cx, |frame, _, _| {
            frame.signature_dialog().expect("open").form.saved
        })
        .unwrap();
    assert!(!offered);
}

#[gpui::test]
fn an_image_or_a_pdf_makes_a_signature_and_a_missing_one_is_named(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    open(window, false, cx);
    act(window, SignatureAction::SetMethod(Method::Image), cx);
    act(window, SignatureAction::Save, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Choose the image to sign with.")
    );

    let missing = data.path().join("gone.png");
    let choose = |file: std::path::PathBuf, cx: &mut TestAppContext| {
        window
            .update(cx, |frame, _, _| frame.take_signature_image(Some(file)))
            .unwrap();
    };
    choose(missing.clone(), cx);
    act(window, SignatureAction::Save, cx);
    let said = error(window, cx).expect("an error");
    assert!(
        said.starts_with(&format!("{} could not be read", missing.display())),
        "{said}"
    );

    let image = data.path().join("sig.png");
    std::fs::write(&image, png()).expect("writes");
    choose(image, cx);
    act(window, SignatureAction::Save, cx);
    assert_eq!(error(window, cx), None);
    assert!(saved(data.path(), "signature.pdf"));

    open(window, true, cx);
    act(window, SignatureAction::SetMethod(Method::Image), cx);
    let pdf = data.path().join("initials-source.pdf");
    std::fs::write(
        &pdf,
        onionskin_tools_fill_sign::signature::typed("AL").expect("made"),
    )
    .expect("writes");
    choose(pdf, cx);
    act(window, SignatureAction::Save, cx);
    assert!(saved(data.path(), "initials.pdf"));
}

#[gpui::test]
fn without_a_data_folder_nothing_is_kept(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    open(window, false, cx);
    act(window, SignatureAction::SetMethod(Method::Draw), cx);
    act(window, SignatureAction::SetMethod(Method::Type), cx);
    window
        .update(cx, |frame, _, cx| {
            let input = frame.signature_dialog().expect("open").name.clone();
            input.update(cx, |input, cx| input.set_query("Ada".to_owned(), cx));
        })
        .unwrap();
    act(window, SignatureAction::Save, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("There is no folder to keep signatures in")
    );
    act(window, SignatureAction::ClearSaved, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("There is no folder to keep signatures in")
    );
    act(window, SignatureAction::ClearPad, cx);
    assert_eq!(error(window, cx), None);
}

/// The quick action bar's Fill Text Fields and Add Sign choose the plugin's
/// tools, and Add Text opens a field on the box it places.
#[gpui::test]
fn the_quick_actions_choose_the_fill_and_sign_tools(cx: &mut TestAppContext) {
    use crate::shell::chrome::quick_actions::QuickAction;

    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    window
        .update(cx, |frame, _, cx| {
            let entries = frame.all_quick_action_entries(cx);
            let entry = |action: QuickAction| {
                *entries
                    .iter()
                    .find(|entry| entry.action == action)
                    .expect("listed")
            };
            let sign = entry(QuickAction::AddSignature);
            assert!(sign.availability.is_enabled(), "{sign:?}");
            frame.select_quick_action(sign, cx);
            assert_eq!(
                sign_tool_of(frame, cx),
                (true, Some("signature".to_owned()))
            );

            let text = entry(QuickAction::FillTextFields);
            assert!(text.availability.is_enabled(), "{text:?}");
            frame.select_quick_action(text, cx);
            let canvas = frame.tabs.active().expect("a tab").canvas.clone();
            canvas.update(cx, |canvas, _| {
                let active = canvas.model.active_tool().expect("a tool");
                let tool = canvas.model.registry().tools().nth(active).expect("tool");
                assert_eq!(tool.id(), "fill-sign.text");
                let page = canvas.model.viewport().visible_pages().unwrap()[0].rect;
                let at = point(
                    px(page.origin.x + page.size.width / 2.0),
                    px(page.origin.y + page.size.height / 2.0),
                );
                let modifiers = gpui::Modifiers::default();
                canvas.model.pointer_down(at, 1.0, modifiers).unwrap();
                canvas.model.pointer_up(at, 1.0, modifiers).unwrap();
                let target = canvas.model.text_target().expect("a field opens");
                assert!(!target.popup, "typed in place, not in a pop-up");
            });
        })
        .unwrap();
}

fn sign_tool_of(frame: &ShellFrame, cx: &App) -> (bool, Option<String>) {
    let canvas = frame.tabs.active().expect("a tab").canvas.read(cx);
    let registry = canvas.model.registry();
    let index = tool_with(registry, ToolCapability::AddSignature).expect("a sign tool");
    let chosen = registry.tools().nth(index).and_then(|tool| tool.chosen());
    (canvas.model.active_tool() == Some(index), chosen)
}
