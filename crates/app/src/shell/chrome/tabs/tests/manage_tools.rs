//! P22 on a real window: View > Manage Tools lists the rail's tools as
//! checkboxes, a cleared one leaves the rail and the preferences file keeps
//! it, and the selected tool stays on the rail even when hidden.

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::dialog::ShellDialog;

/// A window over `minimal.pdf` whose preferences file is in `dir`.
fn window_with_preferences(dir: &Path, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/minimal.pdf");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let paths = crate::config::ConfigPaths {
        preferences: Some(dir.join("preferences.json")),
        ..Default::default()
    };
    let (window, _bindings) = bound_window_with_models(vec![(path, model)], paths, cx);
    window
}

/// The rail's tool ids, expanded so no group hides a member.
fn rail_ids(frame: &ShellFrame, cx: &App) -> Vec<&'static str> {
    frame
        .rail_entries(cx)
        .iter()
        .map(|entry| entry.id)
        .collect()
}

/// The dialog's rows as (label, checked, what activating does).
fn rows(
    frame: &mut ShellFrame,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) -> Vec<(String, Option<bool>, Activation)> {
    let tree = frame.accessible(window, cx);
    tree.walk()
        .filter(|element| format!("{:?}", element.key).contains("manage-tools-row"))
        .map(|row| {
            (
                row.label.clone(),
                row.state.toggled,
                row.activation.clone().expect("operable"),
            )
        })
        .collect()
}

#[gpui::test]
fn a_cleared_tool_leaves_the_rail_and_the_choice_is_kept(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let window = window_with_preferences(dir.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame.toggle_rail_expanded(cx);
            frame
                .run_main_menu_command(MenuCommand::ManageTools, window, cx)
                .expect("opens");
            assert_eq!(frame.dialog, Some(ShellDialog::ManageTools));
            let listed = rows(frame, window, cx);
            let rail = rail_ids(frame, cx);
            assert_eq!(listed.len(), rail.len(), "one row per rail tool");
            assert!(listed.iter().all(|(_, checked, _)| *checked == Some(true)));

            // Clear the last tool, which is not the selected one.
            let (label, _, clear) = listed.last().cloned().expect("a tool");
            let Activation::ToggleToolShown(id) = clear else {
                panic!("{label} does not toggle a tool: {clear:?}");
            };
            frame.run_activation(clear.clone(), window, cx);
            assert!(
                !rail_ids(frame, cx).contains(&id),
                "{id} is still on the rail"
            );
            assert_eq!(rows(frame, window, cx).last().unwrap().1, Some(false));
            let written =
                std::fs::read_to_string(dir.path().join("preferences.json")).expect("saved");
            assert!(
                written.contains("hidden_tools") && written.contains(id),
                "{written}"
            );

            // Checking it again puts it back and empties the list.
            frame.run_activation(clear, window, cx);
            assert!(rail_ids(frame, cx).contains(&id));
            assert!(frame.preferences().hidden_tools.is_empty());
        })
        .unwrap();
}

/// Hiding the tool in use would leave the user with no button saying what
/// is selected, so the rail keeps it until another tool is chosen.
#[gpui::test]
fn the_selected_tool_stays_on_the_rail_when_hidden(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let window = window_with_preferences(dir.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame.toggle_rail_expanded(cx);
            let active = frame
                .rail_entries(cx)
                .into_iter()
                .find(|entry| entry.active)
                .expect("a tool is selected");
            frame.run_activation(Activation::ToggleToolShown(active.id), window, cx);
            assert!(frame.preferences().hidden_tools.contains(active.id));
            assert!(rail_ids(frame, cx).contains(&active.id));
        })
        .unwrap();
}

/// The preferences file's list is what the rail reads on the next start.
#[gpui::test]
fn a_hidden_tool_in_the_file_is_off_the_rail_at_start(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let registry = crate::build_registry();
    let hidden = registry
        .tools()
        .filter(|tool| tool.in_rail())
        .last()
        .expect("a rail tool")
        .id();
    std::fs::write(
        dir.path().join("preferences.json"),
        format!(r#"{{"hidden_tools": ["{hidden}"]}}"#),
    )
    .expect("writes");
    let window = window_with_preferences(dir.path(), cx);
    window
        .update(cx, |frame, _window, cx| {
            frame.toggle_rail_expanded(cx);
            assert!(!rail_ids(frame, cx).contains(&hidden));
        })
        .unwrap();
}
