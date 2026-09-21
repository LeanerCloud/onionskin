//! P19 on a real window: the skins panel lists the file's versions from the
//! rail, rolls back only after asking and only by truncating, refuses with
//! unsaved edits, opens an older version as a copy without touching the
//! file, and follows a save.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::dialog::ShellDialog;
use crate::shell::skins::SkinsAction;

/// A copy of `minimal.pdf` saved `saves` times, and a window over it.
fn window_over_versions(
    saves: usize,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, PathBuf, gpui::WindowHandle<ShellFrame>) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("minimal.pdf");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/minimal.pdf"),
        &path,
    )
    .expect("copies");
    let mut file = onionskin_core::DocumentFile::open(&path).expect("opens");
    for save in 0..saves {
        let (edit, base) = file.edit_mut();
        edit.apply(
            base,
            onionskin_core::DocumentEdit::SetInfoField {
                key: onionskin_cos::Name::new("Subject"),
                value: Some(onionskin_cos::Object::String(
                    format!("save {save}").into_bytes(),
                )),
            },
        )
        .expect("edits");
        file.save().expect("saves");
    }
    drop(file);
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let (window, _bindings) = bound_window_with_models(
        vec![(path.clone(), model)],
        crate::config::ConfigPaths::default(),
        cx,
    );
    (dir, path, window)
}

/// An unsaved edit to the open document, made without any plugin.
fn edit_subject(frame: &mut ShellFrame, cx: &mut Context<ShellFrame>) {
    let canvas = frame.tabs.active().unwrap().canvas.clone();
    canvas.update(cx, |canvas, cx| {
        let (edit, base) = canvas.model.document_mut().edit_mut();
        edit.apply(
            base,
            onionskin_core::DocumentEdit::SetInfoField {
                key: onionskin_cos::Name::new("Subject"),
                value: Some(onionskin_cos::Object::String(b"unsaved".to_vec())),
            },
        )
        .expect("edits");
        cx.notify();
    });
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: SkinsAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Skins(action), window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

/// The versions as the panel describes them, newest first.
fn listed(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            tree.find(&"skins-versions".into())
                .map(|versions| {
                    versions
                        .children
                        .iter()
                        .map(|row| row.label.clone())
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap()
}

#[gpui::test]
fn the_rail_opens_the_panel_listing_every_version_newest_first(cx: &mut TestAppContext) {
    let (_dir, _path, window) = window_over_versions(3, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let button = tree.find(&"tool-rail-skins".into()).expect("on the rail");
            assert_eq!(button.state.toggled, Some(false));
            let activation = button.activation.clone().expect("operable");
            frame.run_activation(activation, window, cx);
        })
        .unwrap();
    let rows = listed(window, cx);
    assert_eq!(rows.len(), 4, "three saves over the original: {rows:?}");
    assert!(
        rows[0].starts_with("Version 3, saved by Onionskin"),
        "{}",
        rows[0]
    );
    assert!(rows[0].ends_with("(current)"));
    assert!(rows[3].starts_with("Original"));
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"tool-rail-skins".into()).unwrap().state.toggled,
                Some(true)
            );
            let roll_back = tree.find(&"skins-roll-back".into()).expect("offered");
            assert!(roll_back.state.disabled, "the current version is chosen");
        })
        .unwrap();
    // Put away, and nothing of it lingers in the tree.
    act(window, SkinsAction::Toggle, cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            let lingering: Vec<String> = tree
                .walk()
                .map(|element| format!("{:?}", element.key))
                .filter(|key| key.contains("skins-"))
                .collect();
            assert!(lingering.is_empty(), "{lingering:?}");
        })
        .unwrap();
}

#[gpui::test]
fn rolling_back_asks_first_then_truncates_the_file(cx: &mut TestAppContext) {
    let (_dir, path, window) = window_over_versions(3, cx);
    let before = std::fs::read(&path).expect("reads");
    let cut = onionskin_cos::Document::open_path(&path)
        .expect("parses")
        .sections()
        .expect("sections")[2]
        .start as usize;
    act(window, SkinsAction::Toggle, cx);
    act(window, SkinsAction::Select(1), cx);
    act(window, SkinsAction::RollBack, cx);
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::RollBack));
            let tree = frame.accessible(window, cx);
            let question = tree.find(&"skins-question".into()).expect("asks");
            assert!(
                question
                    .label
                    .starts_with("Roll back to version 1? 2 newer versions"),
                "{}",
                question.label
            );
        })
        .unwrap();
    assert_eq!(
        std::fs::read(&path).expect("reads"),
        before,
        "asking changes nothing"
    );
    act(window, SkinsAction::ConfirmRollBack, cx);
    assert_eq!(
        std::fs::read(&path).expect("reads"),
        before[..cut],
        "byte for byte the file as it was when version 1 ended"
    );
    let rows = listed(window, cx);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].starts_with("Version 1") && rows[0].ends_with("(current)"));
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.dialog, None);
            assert!(frame
                .notices
                .iter()
                .any(|notice| notice.starts_with("Rolled back to version 1: 2 newer versions")));
        })
        .unwrap();
}

#[gpui::test]
fn cancelling_a_roll_back_leaves_the_file(cx: &mut TestAppContext) {
    let (_dir, path, window) = window_over_versions(2, cx);
    let before = std::fs::read(&path).expect("reads");
    act(window, SkinsAction::Toggle, cx);
    act(window, SkinsAction::Select(0), cx);
    act(window, SkinsAction::RollBack, cx);
    act(window, SkinsAction::CancelRollBack, cx);
    assert_eq!(std::fs::read(&path).expect("reads"), before);
    assert_eq!(listed(window, cx).len(), 3);
}

#[gpui::test]
fn roll_back_waits_for_unsaved_edits_to_be_saved_or_undone(cx: &mut TestAppContext) {
    let (_dir, path, window) = window_over_versions(2, cx);
    let before = std::fs::read(&path).expect("reads");
    act(window, SkinsAction::Toggle, cx);
    window
        .update(cx, |frame, _window, cx| edit_subject(frame, cx))
        .unwrap();
    cx.run_until_parked();
    act(window, SkinsAction::Select(0), cx);
    window
        .update(cx, |frame, window, cx| {
            frame.refresh_skins(cx);
            let tree = frame.accessible(window, cx);
            let roll_back = tree.find(&"skins-roll-back".into()).expect("offered");
            assert!(roll_back.state.disabled);
            assert_eq!(
                roll_back.description.as_deref(),
                Some("Save or undo your changes first")
            );
        })
        .unwrap();
    act(window, SkinsAction::RollBack, cx);
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.dialog, None, "not even asked");
        })
        .unwrap();
    assert_eq!(std::fs::read(&path).expect("reads"), before);
}

#[gpui::test]
fn opening_a_copy_of_the_original_leaves_the_file_alone(cx: &mut TestAppContext) {
    let (dir, path, window) = window_over_versions(2, cx);
    let before = std::fs::read(&path).expect("reads");
    let original_end = onionskin_cos::Document::open_path(&path)
        .expect("parses")
        .sections()
        .expect("sections")[0]
        .end as usize;
    act(window, SkinsAction::Toggle, cx);
    act(window, SkinsAction::Select(0), cx);
    act(window, SkinsAction::OpenCopy, cx);
    let copy = dir.path().join("minimal (original).pdf");
    let answer = copy.clone();
    cx.simulate_new_path_selection(move |_| Some(answer));
    cx.run_until_parked();

    assert_eq!(
        std::fs::read(&copy).expect("written"),
        before[..original_end]
    );
    assert_eq!(
        std::fs::read(&path).expect("reads"),
        before,
        "the file is untouched"
    );
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.tabs.tabs().len(), 2, "the copy opened in its own tab");
        })
        .unwrap();
}

#[gpui::test]
fn a_save_adds_a_version_to_the_open_panel(cx: &mut TestAppContext) {
    let (_dir, _path, window) = window_over_versions(1, cx);
    act(window, SkinsAction::Toggle, cx);
    assert_eq!(listed(window, cx).len(), 2);
    window
        .update(cx, |frame, window, cx| {
            edit_subject(frame, cx);
            frame
                .run_main_menu_command(MenuCommand::Save, window, cx)
                .expect("saves");
        })
        .unwrap();
    cx.run_until_parked();
    let rows = listed(window, cx);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows[0].starts_with("Version 2, saved by Onionskin"));
}
