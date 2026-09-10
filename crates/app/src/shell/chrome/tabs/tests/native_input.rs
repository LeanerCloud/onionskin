use super::*;
use crate::shell::chrome::accessible::TextField;
use crate::shell::chrome::global_bar::RunCommand;

struct InputTestCodec;

impl CodecPlugin for InputTestCodec {
    fn id(&self) -> &'static str {
        ExportTarget::Png.codec()
    }
    fn name(&self) -> &'static str {
        "Input test PNG"
    }
    fn extension(&self) -> &'static str {
        "png"
    }
    fn output_kind(&self) -> ExportOutputKind {
        ExportOutputKind::PerPage
    }
    fn export_page(
        &self,
        _: &mut Document,
        _: &ExportRequest,
        _: PageIndex,
        _: bool,
    ) -> Result<Vec<u8>, ExportError> {
        unreachable!("input tests never submit an export")
    }
}

fn input_window(cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let mut registry = crate::build_registry();
    if registry.codec(ExportTarget::Png.codec()).is_none() {
        registry.register_codec(Box::new(InputTestCodec));
    }
    let model = CanvasModel::new(
        Document::open_bytes(crate::shell::fixtures::text_pages_pdf()).unwrap(),
        registry,
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .unwrap();
    bound_window_with_models(
        vec![(PathBuf::from("input.pdf"), model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0
}

#[gpui::test]
fn native_select_all_replaces_the_complete_visible_field_and_preserves_document_selection(
    cx: &mut TestAppContext,
) {
    let window = input_window(cx);
    for field in [
        TextField::ExportFirst,
        TextField::ExportLast,
        TextField::ExportDpi,
        TextField::Find,
        TextField::Search,
        TextField::Page,
    ] {
        window
            .update(cx, |frame, window, cx| {
                frame.close_dialog(window, cx);
                if matches!(
                    field,
                    TextField::ExportFirst | TextField::ExportLast | TextField::ExportDpi
                ) {
                    frame.start_export(ExportTarget::Png, window, cx);
                } else if field == TextField::Find {
                    frame.open_find_bar(None, window, cx);
                }
                let input = frame.text_field(field).unwrap();
                input.update(cx, |input, cx| input.set_query("a😀é invalid 123", cx));
                frame.run_activation(Activation::Focus(field), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.dispatch_action(
            window.into(),
            RunCommand {
                command: MenuCommand::SelectAll,
            },
        );
        window
            .update(cx, |frame, _, cx| {
                let input = frame.text_field(field).unwrap().read(cx);
                assert_eq!(
                    input.selected_range(),
                    0.."a😀é invalid 123".len(),
                    "{field:?}"
                );
                assert_eq!(
                    frame
                        .tabs
                        .active()
                        .unwrap()
                        .canvas
                        .read(cx)
                        .model
                        .selection_text(),
                    None,
                    "{field:?}"
                );
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "7");
        cx.run_until_parked();
        window
            .update(cx, |frame, _, cx| {
                assert_eq!(
                    frame.text_field(field).unwrap().read(cx).query(),
                    "7",
                    "{field:?}"
                );
            })
            .unwrap();
    }
}

#[gpui::test]
fn native_select_all_selects_document_without_visible_field_focus_and_deselect_still_works(
    cx: &mut TestAppContext,
) {
    let window = input_window(cx);
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let input = &frame.find_input;
            input.update(cx, |input, cx| input.set_query("hidden 😀", cx));
            window.focus(&input.read(cx).focus_handle(cx));
            assert!(frame
                .accessible(window, cx)
                .find(&input.read(cx).element_id().into())
                .is_none());
        })
        .unwrap();
    cx.run_until_parked();
    cx.dispatch_action(
        window.into(),
        RunCommand {
            command: MenuCommand::SelectAll,
        },
    );
    window
        .update(cx, |frame, window, cx| {
            assert!(frame
                .tabs
                .active()
                .unwrap()
                .canvas
                .read(cx)
                .model
                .selection_text()
                .is_some_and(|text| !text.is_empty()));
            assert!(frame.find_input.read(cx).selected_range().is_empty());
            window.focus(frame.a11y.focus_handle());
        })
        .unwrap();
    cx.run_until_parked();
    cx.dispatch_action(
        window.into(),
        RunCommand {
            command: MenuCommand::DeselectAll,
        },
    );
    window
        .update(cx, |frame, _, cx| {
            assert_eq!(
                frame
                    .tabs
                    .active()
                    .unwrap()
                    .canvas
                    .read(cx)
                    .model
                    .selection_text(),
                None
            );
        })
        .unwrap();
    cx.dispatch_action(
        window.into(),
        RunCommand {
            command: MenuCommand::SelectAll,
        },
    );
    window
        .update(cx, |frame, _, cx| {
            assert!(frame
                .tabs
                .active()
                .unwrap()
                .canvas
                .read(cx)
                .model
                .selection_text()
                .is_some_and(|text| !text.is_empty()));
        })
        .unwrap();
}

#[gpui::test]
fn native_select_all_on_a_modal_button_or_hidden_field_never_selects_the_document(
    cx: &mut TestAppContext,
) {
    let window = input_window(cx);
    for hidden_field in [false, true] {
        window
            .update(cx, |frame, window, cx| {
                frame.start_export(ExportTarget::Png, window, cx);
                if hidden_field {
                    frame
                        .tool_search
                        .search_input
                        .update(cx, |input, cx| input.set_query("hidden 😀", cx));
                    window.focus(&frame.tool_search.search_input.read(cx).focus_handle(cx));
                } else {
                    window.focus(frame.a11y.focus_handle());
                }
            })
            .unwrap();
        cx.run_until_parked();
        if !hidden_field {
            window
                .update(cx, |frame, window, cx| {
                    assert!(frame.a11y.focus_key(&"export-submit".into()));
                    frame.focus_ring_target(window, cx);
                })
                .unwrap();
        }
        cx.dispatch_action(
            window.into(),
            RunCommand {
                command: MenuCommand::SelectAll,
            },
        );
        window
            .update(cx, |frame, _, cx| {
                assert_eq!(
                    frame
                        .tabs
                        .active()
                        .unwrap()
                        .canvas
                        .read(cx)
                        .model
                        .selection_text(),
                    None
                );
                assert!(frame
                    .tool_search
                    .search_input
                    .read(cx)
                    .selected_range()
                    .is_empty());
                assert!(frame
                    .export
                    .dialog
                    .as_ref()
                    .unwrap()
                    .first
                    .read(cx)
                    .selected_range()
                    .is_empty());
            })
            .unwrap();
    }
}
