//! Document Properties, Save as Other and Layer Properties on a real window.

use onionskin_core::metadata::{write_initial_view, InitialView, OpenFit, PageLayout, PageMode};
use onionskin_core::DocumentFile;

use super::*;
use crate::shell::chrome::accessible::TextField;
use crate::shell::chrome::properties_dialog::{FitChoice, PropertiesAction, PropertiesTab};
use crate::shell::panes::{LayerAction, LayersCommand, NavigationPane, PaneAction};

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

/// A window whose one tab is a copy of `name`, so an edit cannot reach the
/// corpus.
fn window_on_copy(
    name: &str,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, PathBuf, gpui::WindowHandle<ShellFrame>) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(name);
    std::fs::copy(seed(name), &path).expect("copies");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let window = bound_window_with_models(
        vec![(path.clone(), model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0;
    (dir, path, window)
}

fn act(
    frame: &mut ShellFrame,
    action: PropertiesAction,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) {
    frame.run_activation(Activation::Properties(action), window, cx);
}

fn type_into(frame: &ShellFrame, field: TextField, value: &str, cx: &mut Context<ShellFrame>) {
    frame
        .text_field(field)
        .unwrap_or_else(|| panic!("{field:?} is not showing"))
        .clone()
        .update(cx, |input, cx| input.set_query(value, cx));
}

fn with_document<T>(
    frame: &ShellFrame,
    cx: &mut Context<ShellFrame>,
    read: impl FnOnce(&mut Document) -> T,
) -> T {
    let canvas = frame.tabs.active().expect("a tab").canvas.clone();
    canvas.update(cx, |canvas, _| read(&mut canvas.model.document_mut()))
}

#[gpui::test]
fn a_description_and_a_custom_property_are_one_undo_step_in_both_copies(cx: &mut TestAppContext) {
    let (_dir, _path, window) = window_on_copy("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Properties, window, cx)
                .expect("live");
            assert_eq!(frame.dialog, Some(ShellDialog::Properties));
            type_into(frame, TextField::PropertiesTitle, "Quarterly Report", cx);
            type_into(frame, TextField::PropertiesAuthor, "Ana; Bo", cx);
            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::Custom),
                window,
                cx,
            );
            type_into(frame, TextField::PropertiesCustomKey, "Department", cx);
            type_into(frame, TextField::PropertiesCustomValue, "Finance", cx);
            act(frame, PropertiesAction::AddCustom, window, cx);
            assert_eq!(
                frame.properties_dialog().expect("open").custom,
                [("Department".to_owned(), "Finance".to_owned())]
            );
            act(frame, PropertiesAction::Apply, window, cx);
            assert_eq!(frame.dialog, None, "Apply closes the dialog");

            let (info, xmp, reach) = with_document(frame, cx, |document| {
                (
                    document.info().expect("reads"),
                    document.xmp().expect("reads").expect("a packet"),
                    document.edit().history().reach(),
                )
            });
            assert_eq!(info.description.title.as_deref(), Some("Quarterly Report"));
            assert_eq!(
                info.custom,
                [("Department".to_owned(), "Finance".to_owned())]
            );
            assert_eq!(xmp.title.as_deref(), Some("Quarterly Report"));
            assert_eq!(xmp.authors, ["Ana", "Bo"]);
            assert_eq!(reach, 1, "one Apply, one undo step");
        })
        .unwrap();
}

#[gpui::test]
fn a_dialog_nothing_was_changed_in_writes_nothing(cx: &mut TestAppContext) {
    let (_dir, _path, window) = window_on_copy("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Properties, window, cx)
                .expect("live");
            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::InitialView),
                window,
                cx,
            );
            act(frame, PropertiesAction::Apply, window, cx);
            assert_eq!(frame.dialog, None);
            let reach = with_document(frame, cx, |document| document.edit().history().reach());
            assert_eq!(reach, 0, "looking is not editing");
        })
        .unwrap();
}

#[gpui::test]
fn a_standard_key_as_a_custom_property_is_refused_in_the_dialog(cx: &mut TestAppContext) {
    let (_dir, _path, window) = window_on_copy("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Properties, window, cx)
                .expect("live");
            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::Custom),
                window,
                cx,
            );
            type_into(frame, TextField::PropertiesCustomKey, "Producer", cx);
            act(frame, PropertiesAction::AddCustom, window, cx);
            act(frame, PropertiesAction::Apply, window, cx);
            assert_eq!(frame.dialog, Some(ShellDialog::Properties), "still open");
            let error = frame
                .properties_dialog()
                .and_then(|state| state.error.clone())
                .expect("an error in the dialog");
            assert!(error.contains("Producer"), "{error}");
            let tree = frame.accessible(window, cx);
            assert_eq!(
                tree.find(&"properties-error".into()).map(|node| node.role),
                Some(Role::Alert)
            );
        })
        .unwrap();
}

#[gpui::test]
fn the_initial_view_the_dialog_writes_is_the_one_the_session_reports(cx: &mut TestAppContext) {
    let (_dir, _path, window) = window_on_copy("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Properties, window, cx)
                .expect("live");
            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::InitialView),
                window,
                cx,
            );
            act(
                frame,
                PropertiesAction::Layout(Some(PageLayout::TwoPageLeft)),
                window,
                cx,
            );
            act(
                frame,
                PropertiesAction::Mode(Some(PageMode::UseThumbs)),
                window,
                cx,
            );
            act(frame, PropertiesAction::Fit(FitChoice::Width), window, cx);
            type_into(frame, TextField::PropertiesOpenPage, "9", cx);
            act(frame, PropertiesAction::Apply, window, cx);
            let error = frame
                .properties_dialog()
                .and_then(|state| state.error.clone());
            assert!(
                error.is_some_and(|error| error.contains("1 to 2")),
                "a page past the end"
            );
            type_into(frame, TextField::PropertiesOpenPage, "2", cx);
            act(frame, PropertiesAction::Apply, window, cx);
            assert_eq!(frame.dialog, None);
            let view = with_document(frame, cx, |document| {
                document.initial_view().expect("reads")
            });
            assert_eq!(
                view,
                InitialView {
                    layout: Some(PageLayout::TwoPageLeft),
                    mode: Some(PageMode::UseThumbs),
                    page: Some(1),
                    fit: OpenFit::Width,
                }
            );
        })
        .unwrap();
}

/// Row 32's claim, through the session rather than the dialog: a file whose
/// catalog says "page 2, fit width, two-up, thumbnails" opens that way.
#[gpui::test]
fn a_document_opens_at_the_page_fit_layout_and_pane_it_asks_for(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("opens-at-two.pdf");
    std::fs::copy(seed("two-page.pdf"), &path).expect("copies");
    let mut file = DocumentFile::open(&path).expect("opens");
    let view = InitialView {
        layout: Some(PageLayout::TwoColumnRight),
        mode: Some(PageMode::UseThumbs),
        page: Some(1),
        fit: OpenFit::Width,
    };
    file.document_mut()
        .edit_document("Initial View", |tx| write_initial_view(tx, &view))
        .expect("writes");
    file.save().expect("saves");
    drop(file);

    let (window, _) = bound_window(&[], cx);
    window
        .update(cx, |frame, _, cx| {
            frame.open_documents(std::slice::from_ref(&path), cx);
            let state = frame.active_view_state(cx).expect("a tab");
            assert_eq!(state.current_page, 1);
            assert_eq!(state.fit_mode(), Some(onionskin_core::FitMode::Width));
            assert_eq!(
                state.layout_mode,
                onionskin_core::PageLayoutMode::TwoPageContinuous
            );
            assert!(state.show_cover, "Right puts page one alone");
            assert_eq!(frame.navigation.active(), Some(NavigationPane::Thumbnails));
        })
        .unwrap();
}

/// The Security tab looks read-only to a screen reader: every row is a
/// read-only field with no action on it.
#[gpui::test]
fn the_security_tab_is_read_only_in_the_tree_and_the_dialog_leaves_it_on_close(
    cx: &mut TestAppContext,
) {
    let (_dir, _path, window) = window_on_copy("two-page.pdf", cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Properties, window, cx)
                .expect("live");
            let tree = frame.accessible(window, cx);
            let tabs = tree.find(&"properties-tabs".into()).expect("the tab list");
            assert_eq!(tabs.role, Role::TabList);
            let labels: Vec<&str> = tabs.children.iter().map(|tab| tab.label.as_str()).collect();
            assert_eq!(
                labels,
                ["Description", "Security", "Fonts", "Initial View", "Custom"]
            );

            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::Security),
                window,
                cx,
            );
            let tree = frame.accessible(window, cx);
            let method = tree
                .find(&("properties-security", 0usize).into())
                .expect("the first security row");
            assert_eq!(method.label, "Security Method");
            assert_eq!(method.value.as_deref(), Some("No Security"));
            for index in 0..6usize {
                let row = tree
                    .find(&("properties-security", index).into())
                    .expect("a security row");
                assert!(row.state.read_only, "{} is read-only", row.label);
                assert_eq!(row.activation, None, "{} does nothing", row.label);
            }

            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::Fonts),
                window,
                cx,
            );
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&"properties-fonts".into()).is_some()
                    || tree.find(&"properties-fonts-none".into()).is_some()
            );

            frame.run_activation(Activation::CloseDialog, window, cx);
            let tree = frame.accessible(window, cx);
            assert!(tree.find(&"properties-tabs".into()).is_none());
            assert!(frame.properties_dialog().is_none());
        })
        .unwrap();
}

#[gpui::test]
fn an_encrypted_document_shows_its_permissions_and_cannot_apply(cx: &mut TestAppContext) {
    let bytes = std::fs::read(onionskin_corpus_testing::encrypted_fixture(
        "r4-aes-128.pdf",
    ))
    .expect("reads");
    let (window, _) = bound_window_from_bytes(vec![("locked.pdf", bytes)], cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Properties, window, cx)
                .expect("reading properties is not an edit");
            let tree = frame.accessible(window, cx);
            let apply = tree.find(&"properties-apply".into()).expect("Apply");
            assert!(apply.state.disabled);
            assert_eq!(
                apply.description.as_deref(),
                Some(onionskin_core::protection::Refusal::EncryptedSource.reason())
            );
            act(
                frame,
                PropertiesAction::Tab(PropertiesTab::Security),
                window,
                cx,
            );
            let tree = frame.accessible(window, cx);
            let method = tree
                .find(&("properties-security", 0usize).into())
                .expect("the first security row");
            assert_eq!(method.value.as_deref(), Some("Password Security"));
        })
        .unwrap();
}

/// Save as Other lists exactly what the registry exports to, and nothing
/// Onionskin does not write.
#[gpui::test]
fn save_as_other_offers_what_the_registry_exports_and_nothing_else(cx: &mut TestAppContext) {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            let exporters: Vec<&str> = crate::build_registry()
                .codecs()
                .map(|codec| codec.name())
                .collect();
            let unavailable = frame.command_unavailable(MenuCommand::SaveAsOther, cx);
            if exporters.is_empty() {
                assert_eq!(unavailable, Some("No installed codec exports"));
                return;
            }
            frame
                .run_main_menu_command(MenuCommand::SaveAsOther, window, cx)
                .expect("live");
            let sections = frame.menu_panel_sections(cx);
            assert_eq!(sections.len(), 1);
            let entries = &sections[0].entries;
            assert_eq!(
                entries.len(),
                exporters.len(),
                "one entry per exporting codec"
            );
            let registry = crate::build_registry();
            for entry in entries {
                let MenuCommand::Export(target) = entry.command else {
                    panic!("{} is not an export", entry.label);
                };
                assert!(registry.codec(target.codec()).is_some(), "{}", entry.label);
                assert!(entry.availability.is_enabled(), "{}", entry.label);
            }
            let tree = frame.accessible(window, cx);
            let menu = tree.find(&"main-menu-panel".into()).expect("the panel");
            assert_eq!(menu.label, "Save as Other");
            for absent in ["PDF/X", "Reader Extended", "Reader-Extended"] {
                assert!(
                    !menu
                        .children
                        .iter()
                        .any(|entry| entry.label.contains(absent)),
                    "{absent} is not offered"
                );
            }
        })
        .unwrap();
}

#[gpui::test]
fn layer_properties_opens_from_the_layers_pane_and_names_each_layer(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("layers.pdf", crate::shell::fixtures::optional_content_pdf())],
        cx,
    );
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Layers), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Layer(LayerAction::Run(
                    LayersCommand::Properties,
                ))),
                window,
                cx,
            );
            assert_eq!(frame.dialog, Some(ShellDialog::LayerProperties));
            let rows = frame.layer_property_rows();
            assert_eq!(rows[0], ("Stamp".to_owned(), "Visible".to_owned()));
            let tree = frame.accessible(window, cx);
            let row = tree.find(&("dialog-row", 0usize).into()).expect("a row");
            assert_eq!(row.label, "Stamp: Visible");
        })
        .unwrap();
}
