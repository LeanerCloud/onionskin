use super::*;
use crate::shell::chrome::accessible::TextField;
use crate::shell::chrome::export_dialog::{
    items, Item, DEFAULT_EXPORT_DPI, DEFAULT_EXPORT_QUALITY,
};

#[gpui::test]
fn export_settings_published_bounds_follow_fields_buttons_and_error_rows(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for target in [ExportTarget::Png, ExportTarget::Text] {
        window
            .update(&mut visual, |frame, window, cx| {
                frame.start_export(target, window, cx)
            })
            .unwrap();
        visual.run_until_parked();
        for has_error in [false, true, false] {
            window
                .update(&mut visual, |frame, _, cx| {
                    set_field(
                        frame,
                        TextField::ExportFirst,
                        if has_error { "bad" } else { "1" },
                        cx,
                    );
                })
                .unwrap();
            visual.run_until_parked();
            if has_error {
                window
                    .update(&mut visual, |frame, window, cx| {
                        frame.submit_export(window, cx)
                    })
                    .unwrap();
            }
            draw_window(&mut visual);
            draw_window(&mut visual);
            window
                .update(&mut visual, |frame, window, cx| {
                    frame.serve_accessibility(window, cx);
                    let tree = frame.accessible(window, cx);
                    let dialog = tree.find(&"dialog".into()).unwrap();
                    let mut last_bottom = 0.0;
                    for node in &dialog.children {
                        let published = frame.a11y.published_node(&node.key).unwrap();
                        let bounds = published
                            .bounds()
                            .unwrap_or_else(|| panic!("{} has no bounds", node.key));
                        assert!(bounds.x1 > bounds.x0 && bounds.y1 > bounds.y0);
                        assert!(
                            bounds.y0 >= last_bottom,
                            "{} overlaps the previous row",
                            node.key
                        );
                        last_bottom = bounds.y1;
                    }
                    assert_eq!(tree.find(&"export-error".into()).is_some(), has_error);
                    assert_eq!(
                        tree.find(&"export-dpi".into()).is_some(),
                        target.is_raster()
                    );
                })
                .unwrap();
        }
    }
}

struct SettingsCodec(ExportTarget);

impl CodecPlugin for SettingsCodec {
    fn id(&self) -> &'static str {
        self.0.codec()
    }

    fn name(&self) -> &'static str {
        "Settings test"
    }

    fn extension(&self) -> &'static str {
        "test"
    }

    fn output_kind(&self) -> ExportOutputKind {
        match self.0 {
            ExportTarget::Text => ExportOutputKind::Single,
            ExportTarget::Png | ExportTarget::Svg | ExportTarget::Jpeg | ExportTarget::Tiff => {
                ExportOutputKind::PerPage
            }
        }
    }

    fn export_page(
        &self,
        _: &mut Document,
        request: &ExportRequest,
        page: PageIndex,
        first: bool,
    ) -> Result<Vec<u8>, ExportError> {
        Ok(format!("{page}:{}:{first}", request.dpi).into_bytes())
    }
}

pub(super) fn settings_window(cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let models = ["settings.pdf", "other.pdf"]
        .into_iter()
        .map(|name| {
            let mut registry = PluginRegistry::new();
            for target in ExportTarget::ALL {
                registry.register_codec(Box::new(SettingsCodec(target)));
            }
            let model = CanvasModel::new(
                Document::open_bytes(crate::shell::fixtures::many_pages_pdf(12)).unwrap(),
                registry,
                ViewSize {
                    width: 800.0,
                    height: 600.0,
                },
            )
            .unwrap();
            (PathBuf::from(name), model)
        })
        .collect();
    bound_window_with_models(models, crate::config::ConfigPaths::default(), cx).0
}

fn set_field(frame: &ShellFrame, field: TextField, value: &str, cx: &mut Context<ShellFrame>) {
    frame
        .text_field(field)
        .unwrap()
        .update(cx, |input, cx| input.set_query(value, cx));
}

#[gpui::test]
fn export_settings_defaults_and_visible_items_match_the_accessible_modal(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    for target in ExportTarget::ALL {
        window
            .update(cx, |frame, window, cx| {
                frame
                    .run_main_menu_command(MenuCommand::Export(target), window, cx)
                    .unwrap();
                let dialog = frame.export.dialog.as_ref().unwrap();
                let request = dialog.request(cx).unwrap();
                assert_eq!(request.pages.pages(), 0..=11);
                assert_eq!(request.dpi, DEFAULT_EXPORT_DPI);
                assert_eq!(
                    request.quality,
                    target.is_lossy().then_some(DEFAULT_EXPORT_QUALITY)
                );
                assert!(dialog.first.read(cx).focus_handle(cx).is_focused(window));
                let tree = frame.accessible(window, cx);
                assert!(tree.find(&"page-controls".into()).is_none());
                let body = tree.find(&"dialog".into()).unwrap();
                assert_eq!(body.children.len(), items(target, false).len() + 1);
                assert_eq!(body.children[0].activation, Some(Activation::CloseDialog));
                for (item, node) in items(target, false).iter().zip(&body.children[1..]) {
                    let (key, role, activation) = match item {
                        Item::Target => ("export-target", Role::Label, None),
                        Item::First => (
                            "export-first",
                            Role::NumberInput,
                            Some(Activation::Focus(TextField::ExportFirst)),
                        ),
                        Item::Last => (
                            "export-last",
                            Role::NumberInput,
                            Some(Activation::Focus(TextField::ExportLast)),
                        ),
                        Item::Dpi => (
                            "export-dpi",
                            Role::NumberInput,
                            Some(Activation::Focus(TextField::ExportDpi)),
                        ),
                        Item::Quality => (
                            "export-quality",
                            Role::NumberInput,
                            Some(Activation::Focus(TextField::ExportQuality)),
                        ),
                        Item::Export => (
                            "export-submit",
                            Role::Button,
                            Some(Activation::SubmitExport),
                        ),
                        Item::Cancel => {
                            ("export-cancel", Role::Button, Some(Activation::CloseDialog))
                        }
                        Item::Error => panic!("no error expected"),
                    };
                    assert_eq!(node.key, gpui::ElementId::from(key));
                    assert_eq!(node.role, role);
                    assert_eq!(node.activation, activation);
                }
                assert_eq!(
                    tree.find(&"export-target".into()).unwrap().label,
                    target.label()
                );
                assert_eq!(
                    tree.find(&"export-dpi".into()).is_some(),
                    target.is_raster()
                );
                assert_eq!(
                    tree.find(&"export-quality".into()).is_some(),
                    target.is_lossy()
                );
                if !target.is_raster() {
                    dialog_hidden_dpi_is_ignored(frame, cx);
                }
                frame.close_dialog(window, cx);
            })
            .unwrap();
        assert!(!cx.did_prompt_for_new_path());
    }
}

fn dialog_hidden_dpi_is_ignored(frame: &ShellFrame, cx: &mut Context<ShellFrame>) {
    let dialog = frame.export.dialog.as_ref().unwrap();
    dialog
        .dpi
        .update(cx, |input, cx| input.set_query("not a number", cx));
    assert_eq!(dialog.request(cx).unwrap().dpi, DEFAULT_EXPORT_DPI);
    assert!(frame.text_field(TextField::ExportDpi).is_none());
}

/// JPEG asks for a quality and refuses one the encoder cannot take; the
/// request carries what the user typed, not the codec's default.
#[gpui::test]
fn a_jpeg_export_takes_a_quality_from_1_to_100(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Jpeg, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    for bad in ["", "0", "101", "high", "-5", "7.5"] {
        window
            .update(cx, |frame, _, cx| {
                set_field(frame, TextField::ExportQuality, bad, cx);
                let dialog = frame.export.dialog.as_ref().unwrap();
                let refused = dialog.request(cx).expect_err("refused").to_string();
                assert!(refused.starts_with("Quality"), "{bad:?}: {refused}");
            })
            .unwrap();
    }
    window
        .update(cx, |frame, _, cx| {
            set_field(frame, TextField::ExportQuality, " 40 ", cx);
            let request = frame.export.dialog.as_ref().unwrap().request(cx).unwrap();
            assert_eq!(request.quality, Some(40));
        })
        .unwrap();
}

#[gpui::test]
fn export_settings_reject_invalid_fields_and_recover_after_editing(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    for (first, last, dpi) in [
        ("", "12", "150"),
        ("no", "12", "150"),
        ("0", "12", "150"),
        ("13", "12", "150"),
        ("4", "2", "150"),
        ("1", "", "150"),
        ("1", "no", "150"),
        ("1", "0", "150"),
        ("1", "13", "150"),
        ("1", "12", ""),
        ("1", "12", "bad"),
        ("1", "12", "0"),
        ("1", "12", "-1"),
        ("1", "12", "NaN"),
        ("1", "12", "inf"),
        ("1", "12", "-inf"),
    ] {
        window
            .update(cx, |frame, _, cx| {
                set_field(frame, TextField::ExportFirst, first, cx);
                set_field(frame, TextField::ExportLast, last, cx);
                set_field(frame, TextField::ExportDpi, dpi, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                frame.submit_export(window, cx);
                assert_eq!(frame.dialog, Some(ShellDialog::Export));
                let tree = frame.accessible(window, cx);
                assert_eq!(tree.find(&"export-error".into()).unwrap().role, Role::Alert);
            })
            .unwrap();
        cx.run_until_parked();
        assert!(!cx.did_prompt_for_new_path(), "{first}, {last}, {dpi}");
        window
            .update(cx, |frame, window, cx| {
                assert!(frame
                    .accessible(window, cx)
                    .find(&"export-error".into())
                    .is_some());
            })
            .unwrap();
    }
    window
        .update(cx, |frame, _, cx| {
            set_field(frame, TextField::ExportFirst, "2", cx);
            set_field(frame, TextField::ExportLast, "4", cx);
            set_field(frame, TextField::ExportDpi, "72", cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert!(frame.export.dialog.as_ref().unwrap().error.is_none());
            let request = frame.export.dialog.as_ref().unwrap().request(cx).unwrap();
            assert_eq!(request.pages.pages(), 1..=3);
            assert_eq!(request.dpi, 72.0);
            frame.submit_export(window, cx);
            assert!(frame.dialog.is_none());
            assert!(frame.export.dialog.is_none());
            assert!(frame.a11y.focus_handle().is_focused(window));
        })
        .unwrap();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
}

#[gpui::test]
fn export_settings_enter_submits_each_visible_field(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    for target in ExportTarget::ALL {
        for field in [
            TextField::ExportFirst,
            TextField::ExportLast,
            TextField::ExportDpi,
            TextField::ExportQuality,
        ] {
            let visible = match field {
                TextField::ExportDpi => target.is_raster(),
                TextField::ExportQuality => target.is_lossy(),
                _ => true,
            };
            if !visible {
                continue;
            }
            window
                .update(cx, |frame, window, cx| {
                    frame.start_export(target, window, cx);
                    frame.run_activation(Activation::Focus(field), window, cx);
                })
                .unwrap();
            cx.run_until_parked();
            cx.simulate_keystrokes(window.into(), "enter");
            cx.run_until_parked();
            assert!(cx.did_prompt_for_new_path(), "{target:?} {field:?}");
            window
                .update(cx, |frame, window, _| {
                    assert!(frame.export.dialog.is_none());
                    assert!(frame.a11y.focus_handle().is_focused(window));
                })
                .unwrap();
            cx.simulate_new_path_selection(|_| None);
            cx.run_until_parked();
        }
    }
}

#[gpui::test]
fn export_settings_tab_order_tracks_field_focus_and_space_stays_in_the_field(
    cx: &mut TestAppContext,
) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    for key in [
        "export-last",
        "export-dpi",
        "export-submit",
        "export-cancel",
        "dialog-close",
        "export-first",
    ] {
        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                frame.serve_accessibility(window, cx);
                assert_eq!(frame.a11y.published_focus(), Some(key.into()));
                if let Some(Activation::Focus(field)) = frame.a11y.focused_activation() {
                    assert!(frame
                        .text_field(field)
                        .unwrap()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window));
                } else {
                    assert!(frame.a11y.focus_handle().is_focused(window));
                }
            })
            .unwrap();
    }
    for key in [
        "dialog-close",
        "export-cancel",
        "export-submit",
        "export-dpi",
        "export-last",
        "export-first",
    ] {
        cx.simulate_keystrokes(window.into(), "shift-tab");
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                frame.serve_accessibility(window, cx);
                assert_eq!(frame.a11y.published_focus(), Some(key.into()));
                if let Some(Activation::Focus(field)) = frame.a11y.focused_activation() {
                    assert!(frame
                        .text_field(field)
                        .unwrap()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window));
                } else {
                    assert!(frame.a11y.focus_handle().is_focused(window));
                }
            })
            .unwrap();
    }
    cx.simulate_keystrokes(window.into(), "space");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Export));
            assert!(frame
                .export
                .dialog
                .as_ref()
                .unwrap()
                .first
                .read(cx)
                .query()
                .contains(' '));
            assert!(frame.text_field_focused(window, cx));
        })
        .unwrap();
    assert!(!cx.did_prompt_for_new_path());
}

#[gpui::test]
fn export_settings_cancel_escape_and_replacement_restore_focus(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    for route in 0..3 {
        window
            .update(cx, |frame, window, cx| {
                frame.start_export(ExportTarget::Png, window, cx);
                if route == 0 {
                    frame.run_activation(Activation::CloseDialog, window, cx);
                } else if route == 2 {
                    frame.show_dialog(ShellDialog::About, window, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        if route == 1 {
            cx.simulate_keystrokes(window.into(), "escape");
            cx.run_until_parked();
        }
        window
            .update(cx, |frame, window, cx| {
                assert!(frame.export.dialog.is_none());
                assert!(frame.a11y.focus_handle().is_focused(window));
                for field in [
                    TextField::ExportFirst,
                    TextField::ExportLast,
                    TextField::ExportDpi,
                ] {
                    frame.run_activation(Activation::Focus(field), window, cx);
                    assert!(frame.a11y.focus_handle().is_focused(window));
                }
                frame.close_dialog(window, cx);
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "tab");
        cx.run_until_parked();
        window
            .update(cx, |frame, _, _| {
                assert!(frame.a11y.focused_activation().is_some())
            })
            .unwrap();
        assert!(!cx.did_prompt_for_new_path());
    }
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx);
            frame.submit_export(window, cx);
        })
        .unwrap();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
}

#[gpui::test]
fn export_settings_backdrop_restores_keyboard_operation(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx)
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw_window(&mut visual);
    visual.simulate_click(gpui::point(px(2.0), px(2.0)), gpui::Modifiers::default());
    visual.run_until_parked();
    window
        .update(&mut visual, |frame, window, _| {
            assert!(frame.export.dialog.is_none());
            assert!(frame.a11y.focus_handle().is_focused(window));
        })
        .unwrap();
    visual.simulate_keystrokes("tab");
    visual.run_until_parked();
    window
        .update(&mut visual, |frame, _, _| {
            assert!(frame.a11y.focused_activation().is_some())
        })
        .unwrap();
}

#[gpui::test]
fn export_settings_competing_job_reports_a_reachable_alert(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let origin = frame.export.dialog.as_ref().unwrap().origin;
            install_test_export_job(frame, origin);
            frame.submit_export(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Export));
            let tree = frame.accessible(window, cx);
            let error = tree.find(&"export-error".into()).unwrap();
            assert_eq!(error.role, Role::Alert);
            assert_eq!(error.label, "an export is already in progress");
        })
        .unwrap();
    assert!(!cx.did_prompt_for_new_path());
}

#[gpui::test]
fn export_settings_closed_or_changed_origin_never_prompts(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx);
            frame.activate(1, cx);
            frame.submit_export(window, cx);
            assert!(frame.export.dialog.is_none());
            frame.activate(0, cx);
            frame.start_export(ExportTarget::Png, window, cx);
            frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
            frame.submit_export(window, cx);
            assert!(frame.export.dialog.is_none());
            assert!(frame.a11y.focus_handle().is_focused(window));
        })
        .unwrap();
    assert!(!cx.did_prompt_for_new_path());
}

#[gpui::test]
fn export_settings_selected_range_and_dpi_reach_the_real_worker(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("subset.test");
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Export(ExportTarget::Png), window, cx)
                .unwrap();
            set_field(frame, TextField::ExportFirst, "2", cx);
            set_field(frame, TextField::ExportLast, "4", cx);
            set_field(frame, TextField::ExportDpi, "96", cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| frame.submit_export(window, cx))
        .unwrap();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(destination));
    cx.run_until_parked();
    let mut names = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        ["subset-02.test", "subset-03.test", "subset-04.test"]
    );
    assert_eq!(
        std::fs::read(dir.path().join("subset-02.test")).unwrap(),
        b"1:96:true"
    );
    assert_eq!(
        std::fs::read(dir.path().join("subset-04.test")).unwrap(),
        b"3:96:false"
    );
}

#[gpui::test]
fn export_settings_single_output_keeps_the_request_captured_before_another_modal_opens(
    cx: &mut TestAppContext,
) {
    let window = settings_window(cx);
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("text.test");
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Export(ExportTarget::Text), window, cx)
                .unwrap();
            set_field(frame, TextField::ExportFirst, "2", cx);
            set_field(frame, TextField::ExportLast, "4", cx);
            frame
                .export
                .dialog
                .as_ref()
                .unwrap()
                .dpi
                .update(cx, |input, cx| input.set_query("invalid hidden DPI", cx));
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.submit_export(window, cx);
            frame
                .run_main_menu_command(MenuCommand::Export(ExportTarget::Png), window, cx)
                .unwrap();
            set_field(frame, TextField::ExportFirst, "5", cx);
            set_field(frame, TextField::ExportLast, "6", cx);
            set_field(frame, TextField::ExportDpi, "72", cx);
        })
        .unwrap();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(destination.clone()));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"1:150:true2:150:false3:150:false"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    window
        .update(cx, |frame, _, cx| {
            let dialog = frame.export.dialog.as_ref().unwrap();
            assert_eq!(dialog.target, ExportTarget::Png);
            assert_eq!(dialog.request(cx).unwrap().pages.pages(), 4..=5);
        })
        .unwrap();
}

#[gpui::test]
fn export_settings_canvas_boundary_revalidates_live_page_count(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = &frame.tabs.tabs()[0].canvas;
            let request = ExportRequest {
                pages: PageRange::new(10, 15, 20).unwrap(),
                dpi: 72.0,
                quality: None,
            };
            assert!(canvas
                .read(cx)
                .model
                .prepare_export(ExportTarget::Png.codec(), request)
                .is_err());
            let request = ExportRequest {
                pages: PageRange::new(1, 3, 12).unwrap(),
                dpi: 96.0,
                quality: None,
            };
            let prepared = canvas
                .read(cx)
                .model
                .prepare_export(ExportTarget::Png.codec(), request)
                .unwrap();
            assert_eq!(prepared.request.pages.pages(), 1..=3);
            assert_eq!(prepared.request.dpi, 96.0);
            assert_eq!(prepared.page_count, 12);
        })
        .unwrap();
}

#[gpui::test]
fn export_settings_cursor_and_theme_changes_preserve_the_validation_alert(cx: &mut TestAppContext) {
    let window = settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, _, cx| {
            set_field(frame, TextField::ExportFirst, "bad", cx);
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert!(
                frame
                    .accessible(window, cx)
                    .find(&"export-error".into())
                    .is_some(),
                "Enter must expose the invalid First field"
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "left");
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.set_theme(ThemePreference::Light, cx);
            assert!(frame
                .accessible(window, cx)
                .find(&"export-error".into())
                .is_some());
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert!(frame
                .accessible(window, cx)
                .find(&"export-error".into())
                .is_some());
            set_field(frame, TextField::ExportFirst, "2", cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert!(frame
                .accessible(window, cx)
                .find(&"export-error".into())
                .is_none());
        })
        .unwrap();
    assert!(!cx.did_prompt_for_new_path());
}

#[cfg(feature = "codecs-common")]
#[gpui::test]
fn export_settings_png_subset_dimensions_use_the_selected_dpi(cx: &mut TestAppContext) {
    let (window, _) = bound_window_from_bytes(
        vec![("resolution.pdf", crate::shell::fixtures::many_pages_pdf(12))],
        cx,
    );
    let dir = tempfile::tempdir().unwrap();
    for (dpi, dimensions) in [(72, (200, 100)), (144, (400, 200))] {
        window
            .update(cx, |frame, window, cx| {
                frame
                    .run_main_menu_command(MenuCommand::Export(ExportTarget::Png), window, cx)
                    .unwrap();
                set_field(frame, TextField::ExportFirst, "2", cx);
                set_field(frame, TextField::ExportLast, "3", cx);
                set_field(frame, TextField::ExportDpi, &dpi.to_string(), cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| Some(dir.path().join(format!("dpi-{dpi}.png"))));
        cx.run_until_parked();
        for page in [2, 3] {
            let bytes = std::fs::read(dir.path().join(format!("dpi-{dpi}-{page:02}.png"))).unwrap();
            let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .unwrap()
                .to_rgba8();
            assert_eq!(image.dimensions(), dimensions);
        }
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);
}
