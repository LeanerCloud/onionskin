//! M4 on real windows: View > Show/Hide > Line Weights turns every stroke
//! one pixel wide in every tab of every window, the menu's check mark and
//! the Preferences dialog follow, the setting is saved, and a document
//! opened while it is off opens without line weights.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::global_bar::main_menu_schema;
use crate::shell::preferences_dialog::PreferenceChange;

/// A window over `seed` whose preferences file is `dir/preferences.json`.
fn window_saving_to(
    dir: &Path,
    seed: &str,
    cx: &mut TestAppContext,
) -> gpui::WindowHandle<ShellFrame> {
    let paths = crate::config::ConfigPaths {
        preferences: Some(dir.join("preferences.json")),
        ..Default::default()
    };
    bound_window_in(&[seed], paths, cx).0
}

fn toggle(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::LineWeights, window, cx)
                .expect("Line Weights runs");
        })
        .unwrap();
    cx.run_until_parked();
}

/// `(the preference, the menu's check mark, each tab draws hairlines)`.
fn seen(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> (bool, bool, Vec<bool>) {
    window
        .update(cx, |frame, _, cx| {
            let checked = main_menu_schema(frame.menu_state(cx))
                .into_iter()
                .flat_map(|section| section.entries)
                .find(|entry| entry.command == MenuCommand::LineWeights)
                .expect("a Line Weights entry")
                .selected;
            let hairlines = frame
                .tabs
                .tabs()
                .iter()
                .map(|tab| tab.canvas.read(cx).model.hairline_strokes())
                .collect();
            (frame.settings.preferences.line_weights, checked, hairlines)
        })
        .unwrap()
}

fn saved_line_weights(dir: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(dir.join("preferences.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    json.get("line_weights")?.as_bool()
}

#[gpui::test]
fn the_menu_toggle_redraws_the_document_and_saves_the_setting(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let window = window_saving_to(dir.path(), "two-page.pdf", cx);
    assert_eq!(seen(window, cx), (true, true, vec![false]), "on by default");

    toggle(window, cx);
    assert_eq!(seen(window, cx), (false, false, vec![true]));
    assert_eq!(saved_line_weights(dir.path()), Some(false));

    toggle(window, cx);
    assert_eq!(seen(window, cx), (true, true, vec![false]));
    assert_eq!(saved_line_weights(dir.path()), Some(true));
}

/// Two windows on one document draw from one session, so the setting is
/// the application's: the other window's pixels and check mark follow.
#[gpui::test]
fn a_second_window_follows_the_first(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let first = window_saving_to(dir.path(), "two-page.pdf", cx);
    first
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::NewWindow, window, cx)
                .expect("New Window runs");
        })
        .unwrap();
    cx.run_until_parked();
    let second = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<ShellFrame>())
        .find(|handle| *handle != first)
        .expect("a second window opened");

    toggle(first, cx);
    assert_eq!(seen(first, cx), (false, false, vec![true]));
    assert_eq!(seen(second, cx), (false, false, vec![true]));

    toggle(second, cx);
    assert_eq!(seen(first, cx), (true, true, vec![false]), "and back");
    assert_eq!(seen(second, cx), (true, true, vec![false]));
}

#[gpui::test]
fn a_document_opened_while_line_weights_are_off_opens_without_them(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(
        dir.path().join("preferences.json"),
        r#"{"line_weights": false}"#,
    )
    .expect("writes");
    let window = window_saving_to(dir.path(), "minimal.pdf", cx);
    let two_page = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
    window
        .update(cx, |frame, _, cx| {
            frame.open_document(&two_page, "", cx).expect("opens");
        })
        .unwrap();
    cx.run_until_parked();

    let (preference, checked, hairlines) = seen(window, cx);
    assert!(!preference && !checked, "the window starts from the file");
    assert_eq!(hairlines.last(), Some(&true), "the opened document");
}

/// Page Display's switch in the Preferences dialog is the same setting.
#[gpui::test]
fn the_preferences_dialog_switch_does_what_the_menu_does(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let window = window_saving_to(dir.path(), "two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(
                Activation::ChangePreference(PreferenceChange::LineWeights(false)),
                window,
                cx,
            );
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(seen(window, cx), (false, false, vec![true]));
    assert_eq!(saved_line_weights(dir.path()), Some(false));
}
