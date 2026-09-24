//! Filling a form on a real window: the Hand tool's click opening a field's
//! editor, typing and Enter, a value the scripts refuse, Escape, Tab to the
//! next field, a dropdown picked by a screen reader, a check box, Clear
//! Form, and Preferences > JavaScript turning the scripts off.

use gpui::{point, px, VisualTestContext};
use onionskin_core::forms::FieldValue;

use super::*;
use crate::shell::chrome::accessible::{Activation, TextField};
use crate::shell::chrome::global_bar::MenuCommand;
use crate::shell::preferences_dialog::PreferenceChange;

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// qty takes a number; colour is a dropdown; agree a check box; name is
/// plain text. All on a small page, so they are in view.
fn document() -> Vec<u8> {
    let widget = |rest: &str| -> Vec<u8> {
        format!("<< /Type /Annot /Subtype /Widget /P 3 0 R {rest} >>").into_bytes()
    };
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 8 0 R 9 0 R] \
           /DA (/Helv 10 Tf 0 g) >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 200] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R 6 0 R 8 0 R 9 0 R] >>".to_vec(),
        widget(
            "/FT /Tx /T (qty) /Rect [10 150 110 170] /AA << /K << /S /JavaScript \
             /JS (AFNumber_Keystroke\\(0, 0, 0, 0, \"\", true\\);) >> >>",
        ),
        widget("/FT /Ch /Ff 131072 /T (colour) /Rect [150 150 250 170] /Opt [[(r) (Red)] [(g) (Green)]]"),
        widget("/FT /Btn /T (agree) /Rect [10 100 22 112] /AP << /N << /Yes 7 0 R /Off 7 0 R >> >> /AS /Off"),
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12] /Length 0 >>\nstream\n\nendstream"
            .to_vec(),
        widget("/FT /Tx /T (name) /Rect [10 50 110 70]"),
        widget("/FT /Btn /Ff 65536 /T (photo_af_image) /Rect [150 60 250 130]"),
    ])
}

/// A window on `bytes`, with the Hand tool chosen.
fn window_on(
    data: &std::path::Path,
    bytes: Vec<u8>,
    cx: &mut TestAppContext,
) -> gpui::WindowHandle<ShellFrame> {
    let path = data.join("form.pdf");
    std::fs::write(&path, bytes).expect("writes");
    let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(data), cx);
    window
        .update(cx, |frame, _, cx| {
            frame.open_documents(std::slice::from_ref(&path), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, _, cx| {
            frame.apply_tool_environment(cx);
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let index = canvas
                    .model
                    .registry()
                    .tools()
                    .position(|tool| tool.id() == "hand")
                    .expect("installed");
                canvas.model.activate_tool(index).expect("activates");
            });
        })
        .unwrap();
    window
}

fn window(data: &std::path::Path, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    window_on(data, document(), cx)
}

/// Press and release the Hand tool at the middle of page rectangle `rect`.
fn click(window: gpui::WindowHandle<ShellFrame>, rect: [f64; 4], cx: &mut VisualTestContext) {
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| {
                let (at, width, height) = canvas.model.view_rect(0, rect).expect("in view");
                let origin = canvas.model.canvas_origin();
                let at = point(
                    px(origin.x + at.x + width / 2.0),
                    px(origin.y + at.y + height / 2.0),
                );
                let modifiers = gpui::Modifiers::default();
                canvas.model.pointer_down(at, 1.0, modifiers).unwrap();
                canvas.model.pointer_up(at, 1.0, modifiers).unwrap();
                canvas.answer_field(window, cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
}

const QTY: [f64; 4] = [10.0, 150.0, 110.0, 170.0];
const COLOUR: [f64; 4] = [150.0, 150.0, 250.0, 170.0];
const AGREE: [f64; 4] = [10.0, 100.0, 22.0, 112.0];
const NAME: [f64; 4] = [10.0, 50.0, 110.0, 70.0];
const PHOTO: [f64; 4] = [150.0, 60.0, 250.0, 130.0];

fn value(
    window: gpui::WindowHandle<ShellFrame>,
    name: &str,
    cx: &mut VisualTestContext,
) -> FieldValue {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .form()
                    .expect("reads")
                    .field(name)
                    .expect("the field")
                    .value
                    .clone()
            })
        })
        .unwrap()
}

/// The open editor's field name, and whether its text box has the focus.
fn editor(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut VisualTestContext,
) -> Option<(String, bool)> {
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let canvas = canvas.read(cx);
            let editor = canvas.field_editor.as_ref()?;
            let focused = editor
                .input
                .as_ref()
                .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
            Some((editor.prompt.name.clone(), focused))
        })
        .unwrap()
}

fn notices(window: gpui::WindowHandle<ShellFrame>, cx: &mut VisualTestContext) -> Vec<String> {
    window
        .update(cx, |frame, _, _| frame.notices.clone())
        .unwrap()
}

fn visual(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> VisualTestContext {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw_window(&mut visual);
    visual
}

#[gpui::test]
fn typing_into_a_text_field_commits_on_enter_and_its_script_can_refuse(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, QTY, cx);
    assert_eq!(editor(window, cx), Some(("qty".to_owned(), true)));
    assert_eq!(
        window
            .update(cx, |frame, window, cx| frame.focused_text_field(window, cx))
            .unwrap(),
        Some(gpui::ElementId::from("form-field-editor")),
        "Enter reaches the field, not the focus ring"
    );
    cx.simulate_keystrokes("1 2 enter");
    assert_eq!(editor(window, cx), None);
    assert_eq!(value(window, "qty", cx), FieldValue::Text("12".into()));

    click(window, QTY, cx);
    cx.simulate_keystrokes("x enter");
    assert_eq!(
        editor(window, cx).map(|(name, _)| name).as_deref(),
        Some("qty"),
        "a refused value keeps the field open"
    );
    assert!(notices(window, cx)
        .iter()
        .any(|notice| notice.contains("does not match the format of the field [ qty ]")));
    cx.simulate_keystrokes("escape");
    assert_eq!(editor(window, cx), None);
    assert_eq!(value(window, "qty", cx), FieldValue::Text("12".into()));
}

#[gpui::test]
fn tab_commits_and_moves_to_the_next_field(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, QTY, cx);
    cx.simulate_keystrokes("7 tab");
    assert_eq!(value(window, "qty", cx), FieldValue::Text("7".into()));
    assert_eq!(
        editor(window, cx).map(|(name, _)| name).as_deref(),
        Some("colour")
    );
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(
        editor(window, cx).map(|(name, _)| name).as_deref(),
        Some("qty")
    );
}

#[gpui::test]
fn a_dropdown_is_offered_to_a_screen_reader_and_picked_there(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, COLOUR, cx);
    assert_eq!(
        editor(window, cx),
        Some(("colour".to_owned(), false)),
        "a dropdown that takes no typing has no text box"
    );
    let node = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| canvas.field_editor_node(1.0, cx))
        })
        .unwrap()
        .expect("published");
    assert_eq!(node.label, "colour (dropdown)");
    assert_eq!(node.role, accesskit::Role::ComboBox);
    assert!(node.bounds.is_some());
    let options: Vec<&str> = node
        .children
        .iter()
        .map(|child| child.label.as_str())
        .collect();
    assert_eq!(options, ["Red", "Green"]);
    assert_eq!(node.children[1].activation, Some(Activation::FormOption(1)));

    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::FormOption(1), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(editor(window, cx), None);
    assert_eq!(
        value(window, "colour", cx),
        FieldValue::Chosen(vec!["g".into()])
    );
}

#[gpui::test]
fn a_text_field_editor_is_published_and_focused_from_the_tree(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, QTY, cx);
    let node = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| canvas.field_editor_node(1.0, cx))
        })
        .unwrap()
        .expect("published");
    assert_eq!(node.label, "qty (text field)");
    assert_eq!(
        node.activation,
        Some(Activation::Focus(TextField::FormField))
    );
    window
        .update(cx, |frame, window, cx| {
            window.focus(frame.a11y.focus_handle());
            frame.run_activation(Activation::Focus(TextField::FormField), window, cx);
        })
        .unwrap();
    assert_eq!(editor(window, cx), Some(("qty".to_owned(), true)));
}

#[gpui::test]
fn a_check_box_toggles_and_clear_form_restores_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, AGREE, cx);
    assert_eq!(editor(window, cx), None);
    assert_eq!(
        value(window, "agree", cx),
        FieldValue::State(Some("Yes".into()))
    );
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::ClearForm, window, cx)
                .expect("runs")
        })
        .unwrap();
    assert_eq!(value(window, "agree", cx), FieldValue::State(None));
    assert!(notices(window, cx).is_empty());
}

#[gpui::test]
fn clear_form_on_a_document_without_one_says_so(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window_on(
        data.path(),
        pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 200] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        ]),
        cx,
    );
    let cx = &mut visual(window, cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::ClearForm, window, cx)
                .expect("runs")
        })
        .unwrap();
    assert_eq!(
        notices(window, cx),
        ["This document has no form fields to clear"]
    );
}

#[gpui::test]
fn with_javascript_off_a_value_is_kept_as_typed(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(PreferenceChange::JavaScript(false), cx)
        })
        .unwrap();
    click(window, QTY, cx);
    cx.simulate_keystrokes("x enter");
    assert_eq!(editor(window, cx), None);
    assert_eq!(value(window, "qty", cx), FieldValue::Text("x".into()));
    let saved = std::fs::read_to_string(data.path().join("preferences.json")).expect("saved");
    assert!(saved.contains("\"javascript\": false"), "{saved}");
}

#[gpui::test]
fn pressing_elsewhere_on_the_page_commits_the_field(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, QTY, cx);
    cx.simulate_keystrokes("4 2");
    draw_window(cx);
    let (inside, elsewhere) = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let canvas = canvas.read(cx);
            let origin = canvas.model.canvas_origin();
            let centre = |rect| {
                let (at, width, height) = canvas.model.view_rect(0, rect).expect("in view");
                point(
                    px(origin.x + at.x + width / 2.0),
                    px(origin.y + at.y + height / 2.0),
                )
            };
            (centre(QTY), centre([200.0, 20.0, 220.0, 40.0]))
        })
        .unwrap();
    cx.simulate_click(inside, gpui::Modifiers::default());
    assert!(
        editor(window, cx).is_some(),
        "a press inside the editor is the editor's"
    );
    cx.simulate_click(elsewhere, gpui::Modifiers::default());
    assert_eq!(editor(window, cx), None);
    assert_eq!(value(window, "qty", cx), FieldValue::Text("42".into()));
}

fn suggestions(window: gpui::WindowHandle<ShellFrame>, cx: &mut VisualTestContext) -> Vec<String> {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| canvas.field_suggestions(cx))
        })
        .unwrap()
}

#[gpui::test]
fn auto_complete_remembers_typed_text_and_offers_it_again(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    click(window, NAME, cx);
    cx.simulate_keystrokes("A d a enter");
    assert_eq!(value(window, "name", cx), FieldValue::Text("Ada".into()));
    click(window, QTY, cx);
    cx.simulate_keystrokes("4 2 enter");
    let saved = std::fs::read_to_string(data.path().join("autocomplete.json")).expect("saved");
    assert!(
        saved.contains("Ada") && !saved.contains("42"),
        "no numbers by default: {saved}"
    );

    click(window, NAME, cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).field_editor_input().expect("a text box");
            input.update(cx, |input, cx| input.set_query("a".to_owned(), cx));
        })
        .unwrap();
    assert_eq!(suggestions(window, cx), ["Ada"]);
    let node = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| canvas.field_editor_node(1.0, cx))
        })
        .unwrap()
        .expect("published");
    assert_eq!(node.children[0].label, "Ada");
    assert_eq!(
        node.children[0].activation,
        Some(Activation::FormSuggestion(0))
    );
    draw_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::FormSuggestion(0), window, cx)
        })
        .unwrap();
    assert_eq!(
        editor(window, cx).map(|(name, _)| name).as_deref(),
        Some("name")
    );
    let typed = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).field_editor_input().expect("a text box");
            input.read(cx).query().to_owned()
        })
        .unwrap();
    assert_eq!(typed, "Ada");
    cx.simulate_keystrokes("escape");

    // Preferences > Forms lists it, and forgets it.
    let rows = window
        .update(cx, |frame, _, _| {
            crate::shell::preferences_dialog::rows_for(
                frame.preferences(),
                frame.autocomplete_entries(),
                crate::preferences::PreferenceCategory::Forms,
            )
            .into_iter()
            .map(|row| row.label)
            .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(rows[2..], ["1 entry remembered", "Ada"]);
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(PreferenceChange::KeepEntry(0), cx);
            frame.change_preference(PreferenceChange::ForgetEntry(0), cx);
            frame.change_preference(PreferenceChange::ForgetEntry(7), cx);
        })
        .unwrap();
    assert!(window
        .update(cx, |frame, _, _| frame.autocomplete_entries().is_empty())
        .unwrap());
    let saved = std::fs::read_to_string(data.path().join("autocomplete.json")).expect("saved");
    assert!(!saved.contains("Ada"));
}

#[gpui::test]
fn auto_complete_off_offers_nothing_and_clear_all_forgets_everything(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(PreferenceChange::AutoCompleteNumbers(true), cx)
        })
        .unwrap();
    click(window, QTY, cx);
    cx.simulate_keystrokes("4 2 enter");
    assert_eq!(
        window
            .update(cx, |frame, _, _| frame.autocomplete_entries().to_vec())
            .unwrap(),
        ["42"]
    );
    window
        .update(cx, |frame, _, cx| {
            frame.change_preference(PreferenceChange::AutoComplete(false), cx)
        })
        .unwrap();
    click(window, QTY, cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).field_editor_input().expect("a text box");
            input.update(cx, |input, cx| input.set_query("4".to_owned(), cx));
        })
        .unwrap();
    assert!(suggestions(window, cx).is_empty(), "off");
    cx.simulate_keystrokes("3 enter");
    window
        .update(cx, |frame, _, cx| {
            assert_eq!(
                frame.autocomplete_entries(),
                ["42"],
                "not remembered while off"
            );
            frame.change_preference(PreferenceChange::ClearEntries, cx);
            assert!(frame.autocomplete_entries().is_empty());
        })
        .unwrap();
}

/// The raw normal appearance of `name`'s widget.
fn appearance(
    window: gpui::WindowHandle<ShellFrame>,
    name: &str,
    cx: &mut VisualTestContext,
) -> String {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let mut doc = canvas.model.document_mut();
                let widget = doc
                    .form()
                    .expect("reads")
                    .field(name)
                    .expect("the field")
                    .widgets[0]
                    .objref;
                let cos = doc.structure().expect("structure");
                let dict = cos.get(widget.number).expect("widget").object;
                let Some(normal) = dict
                    .as_dict()
                    .and_then(|dict| dict.get(b"AP"))
                    .and_then(|ap| cos.resolve(ap).ok())
                    .and_then(|ap| ap.as_dict().and_then(|ap| ap.get(b"N")).cloned())
                    .and_then(|normal| cos.resolve(&normal).ok())
                else {
                    return String::new();
                };
                String::from_utf8_lossy(&normal.as_stream().expect("a stream").raw).into_owned()
            })
        })
        .unwrap()
}

#[gpui::test]
fn an_image_field_shows_the_image_chosen_for_it(cx: &mut TestAppContext) {
    use image::ImageEncoder as _;
    let data = tempfile::tempdir().expect("dir");
    let window = window(data.path(), cx);
    let cx = &mut visual(window, cx);
    // The click asks the platform for a file, which the test platform
    // cannot show, so the model is asked what the click answers.
    let asked = window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .request_field(onionskin_core::FieldRequest {
                        field: onionskin_core::ObjRef::new(9, 0),
                        widget: onionskin_core::ObjRef::new(9, 0),
                        page: 0,
                        point: (200.0, 100.0),
                    });
                canvas.model.answer_field_request().expect("answers")
            })
        })
        .unwrap()
        .expect("a prompt");
    assert_eq!(asked.entry, crate::shell::canvas::Entry::Image);

    let path = data.path().join("face.png");
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&[0, 90, 180, 255], 2, 2, image::ExtendedColorType::L8)
        .expect("encodes");
    std::fs::write(&path, png).expect("writes");
    let prompt = asked;
    assert_eq!(prompt.rect, PHOTO);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| {
                canvas.take_image_file(&prompt, None, cx);
                canvas.take_image_file(&prompt, Some(path.clone()), cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert!(appearance(window, "photo_af_image", cx).contains("/Img Do"));
    assert!(notices(window, cx).is_empty());

    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| {
                canvas.take_image_file(&prompt, Some(path.with_file_name("gone.png")), cx);
                assert!(
                    canvas.model.status().is_some(),
                    "an unreadable file is said"
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn detect_form_fields_places_fields_where_a_page_is_filled_in(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let content = "BT /F1 12 Tf 20 150 Td (Name: ________) Tj ET";
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 200] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ]);
    let window = window_on(data.path(), bytes, cx);
    let cx = &mut visual(window, cx);
    let detect = |cx: &mut VisualTestContext| {
        window
            .update(cx, |frame, window, cx| {
                frame
                    .run_main_menu_command(MenuCommand::DetectFields, window, cx)
                    .expect("runs")
            })
            .unwrap();
    };
    detect(cx);
    assert_eq!(
        notices(window, cx).last().map(String::as_str),
        Some("Placed 1 field where the pages are filled in")
    );
    assert_eq!(
        value(window, "Name", cx),
        FieldValue::None,
        "named by its label"
    );
    detect(cx);
    assert_eq!(
        notices(window, cx).last().map(String::as_str),
        Some("No places to fill in were found")
    );
}
