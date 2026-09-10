use super::*;
use crate::shell::chrome::accessible::TextField;

#[gpui::test]
fn export_defaults_are_published_as_editable_values(cx: &mut TestAppContext) {
    let window = export_settings::settings_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.start_export(ExportTarget::Png, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            frame.serve_accessibility(window, cx);
            for (key, value) in [
                ("export-first", "1"),
                ("export-last", "12"),
                ("export-dpi", "150"),
            ] {
                let node = frame.a11y.published_node(&key.into()).unwrap();
                assert_eq!(node.value(), Some(value), "{key}");
                assert_eq!(node.description(), None, "{key}");
            }
        })
        .unwrap();
}

#[gpui::test]
fn shared_inputs_publish_raw_values_and_keyboard_edits(cx: &mut TestAppContext) {
    let window = export_settings::settings_window(cx);
    for (field, key, label, export) in [
        (
            TextField::Search,
            "global-search-input",
            "Search Tools Or Document",
            false,
        ),
        (TextField::Find, FIND_INPUT_ID, "Find In Document", false),
        (TextField::Page, PAGE_ENTRY_ID, "Page Number", false),
        (TextField::ExportFirst, "export-first", "First page", true),
        (TextField::ExportLast, "export-last", "Last page", true),
        (TextField::ExportDpi, "export-dpi", "Resolution (DPI)", true),
    ] {
        window
            .update(cx, |frame, window, cx| {
                if export {
                    frame.start_export(ExportTarget::Png, window, cx);
                } else {
                    frame.open_find_bar(None, window, cx);
                }
                frame.run_activation(Activation::Focus(field), window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        for value in ["", "not a number", "é😀"] {
            window
                .update(cx, |frame, _, cx| {
                    frame.text_field(field).unwrap().update(cx, |input, cx| {
                        input.set_query(value, cx);
                    });
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |frame, window, cx| {
                    frame.serve_accessibility(window, cx);
                    let node = frame.a11y.published_node(&key.into()).unwrap();
                    assert_eq!(node.value(), Some(value), "{key}");
                    assert_eq!(node.label(), Some(label), "{key}");
                    assert_eq!(node.description().is_some(), value.is_empty(), "{key}");
                })
                .unwrap();
        }
        cx.simulate_keystrokes(window.into(), "backspace");
        cx.run_until_parked();
        window
            .update(cx, |frame, window, cx| {
                assert_eq!(frame.text_field(field).unwrap().read(cx).query(), "é");
                frame.serve_accessibility(window, cx);
                let node = frame.a11y.published_node(&key.into()).unwrap();
                assert_eq!(node.value(), Some("é"), "{key}");
                assert_eq!(node.description(), None, "{key}");
            })
            .unwrap();
    }
}
