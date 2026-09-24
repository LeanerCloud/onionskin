//! Answering field clicks on a real form: every kind of field, what the
//! scripts refuse and say, tabbing, and Clear Form.

use onionskin_core::forms::FieldValue;
use onionskin_core::{Document, FieldRequest, ObjRef, ViewSize};
use onionskin_plugin_api::PluginRegistry;

use super::*;

fn js(script: &str) -> String {
    format!("<< /S /JavaScript /JS ({script}) >>")
}

/// One page of fields, in object order from 4:
/// 4 qty (number), 5 agree (check box), 6 size (radio, kids 7 and 8),
/// 9 pets (multiple choice list box), 10 colour (editable dropdown),
/// 11 print (button), 12 sign (signature), 13 locked (read-only text),
/// 14 age (validated 0 to 130), 15 an appearance for the buttons.
fn document() -> Vec<u8> {
    let number = format!(
        "/AA << /K {} >>",
        js("AFNumber_Keystroke\\(0, 0, 0, 0, \"\", true\\);")
    );
    let age = format!(
        "/AA << /V {} >>",
        js("AFRange_Validate\\(true, 0, true, 130\\);")
    );
    let widget = |rest: &str| -> Vec<u8> {
        format!("<< /Type /Annot /Subtype /Widget /P 3 0 R {rest} >>").into_bytes()
    };
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 9 0 R 10 0 R \
           11 0 R 12 0 R 13 0 R 14 0 R] /DA (/Helv 10 Tf 0 g) >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R \
           12 0 R 13 0 R 14 0 R] >>"
            .to_vec(),
        widget(&format!("/FT /Tx /T (qty) /Rect [10 700 110 720] {number}")),
        widget("/FT /Btn /T (agree) /Rect [10 670 22 682] /AP << /N << /Yes 15 0 R /Off 15 0 R >> >> /AS /Off"),
        b"<< /FT /Btn /Ff 49152 /T (size) /Kids [7 0 R 8 0 R] >>".to_vec(),
        widget("/Parent 6 0 R /Rect [10 640 22 652] /AP << /N << /S 15 0 R /Off 15 0 R >> >> /AS /Off"),
        widget("/Parent 6 0 R /Rect [40 640 52 652] /AP << /N << /L 15 0 R /Off 15 0 R >> >> /AS /Off"),
        widget("/FT /Ch /Ff 2097152 /T (pets) /Rect [10 560 110 620] /Opt [(Cat) (Dog) (Fish)]"),
        widget("/FT /Ch /Ff 393216 /T (colour) /Rect [10 520 110 540] /Opt [[(r) (Red)] [(g) (Green)]] /V (g)"),
        widget("/FT /Btn /Ff 65536 /T (print) /Rect [10 490 60 510]"),
        widget("/FT /Sig /T (sign) /Rect [10 440 110 480]"),
        widget("/FT /Tx /Ff 1 /T (locked) /Rect [10 410 110 430] /V (fixed)"),
        widget(&format!("/FT /Tx /T (age) /Rect [10 380 110 400] {age}")),
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12] /Length 0 >>\nstream\n\nendstream"
            .to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
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
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn model() -> CanvasModel {
    CanvasModel::new(
        Document::open_bytes(document()).expect("opens"),
        PluginRegistry::new(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("canvas starts")
}

fn object(number: u32) -> ObjRef {
    ObjRef::new(number, 0)
}

/// Click field `field` through widget `widget` at `point`.
fn click(
    model: &mut CanvasModel,
    field: u32,
    widget: u32,
    point: (f64, f64),
) -> Option<FieldPrompt> {
    model.document_mut().request_field(FieldRequest {
        field: object(field),
        widget: object(widget),
        page: 0,
        point,
    });
    model.answer_field_request().expect("answers")
}

fn value(model: &CanvasModel, name: &str) -> FieldValue {
    model
        .document_mut()
        .form()
        .expect("reads")
        .field(name)
        .expect("the field")
        .value
        .clone()
}

#[test]
fn nothing_is_answered_without_a_click_on_a_field() {
    let mut model = model();
    assert_eq!(model.answer_field_request().expect("answers"), None);
    assert!(model.has_form());
    assert_eq!(click(&mut model, 99, 99, (0.0, 0.0)), None);
    assert_eq!(click(&mut model, 4, 5, (0.0, 0.0)), None, "not its widget");
    assert!(model.take_form_notices().is_empty());
}

#[test]
fn a_text_field_opens_an_editor_and_its_keystroke_script_decides() {
    let mut model = model();
    let prompt = click(&mut model, 4, 4, (50.0, 710.0)).expect("an editor");
    assert_eq!(prompt.name, "qty");
    assert_eq!(prompt.rect, [10.0, 700.0, 110.0, 720.0]);
    assert!(prompt.entry.typed());
    assert_eq!(prompt.entry.initial_text(), "");
    assert!(model
        .commit_field(prompt.field, prompt.entry.value_for("12"))
        .expect("commits"));
    assert_eq!(value(&model, "qty"), FieldValue::Text("12".into()));
    assert!(!model
        .commit_field(prompt.field, prompt.entry.value_for("twelve"))
        .expect("runs"));
    assert_eq!(value(&model, "qty"), FieldValue::Text("12".into()));
    let notices = model.take_form_notices();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].contains("does not match the format"),
        "{notices:?}"
    );

    let short = Entry::Text {
        value: String::new(),
        password: false,
        max_len: Some(3),
    };
    assert_eq!(
        short.value_for("12345"),
        FieldValue::Text("123".into()),
        "no more than the field holds"
    );

    model.set_form_scripts(false);
    assert!(model
        .commit_field(prompt.field, prompt.entry.value_for("twelve"))
        .expect("commits"));
    assert_eq!(value(&model, "qty"), FieldValue::Text("twelve".into()));
}

#[test]
fn a_refused_value_without_an_alert_is_still_said() {
    let mut model = model();
    model.report(
        "age",
        Filled {
            accepted: false,
            problems: vec!["age: the script failed: no".into()],
            ..Filled::default()
        },
    );
    assert_eq!(
        model.take_form_notices(),
        [
            "A form script did not run, for age: the script failed: no",
            "age did not take that value"
        ]
    );
    let age = click(&mut model, 14, 14, (50.0, 390.0)).expect("an editor");
    assert!(!model
        .commit_field(age.field, age.entry.value_for("200"))
        .expect("runs"));
    assert!(model.take_form_notices()[0].contains("less than or equal to 130"));
}

#[test]
fn check_boxes_and_radio_buttons_toggle_where_they_are_clicked() {
    let mut model = model();
    assert_eq!(click(&mut model, 5, 5, (15.0, 675.0)), None);
    assert_eq!(
        value(&model, "agree"),
        FieldValue::State(Some("Yes".into()))
    );
    click(&mut model, 6, 8, (45.0, 645.0));
    assert_eq!(value(&model, "size"), FieldValue::State(Some("L".into())));
    click(&mut model, 6, 8, (45.0, 645.0));
    assert_eq!(
        value(&model, "size"),
        FieldValue::State(Some("L".into())),
        "a radio button is not turned off"
    );
    assert!(model.take_form_notices().is_empty());
}

#[test]
fn a_list_box_takes_the_row_clicked_and_turns_it_over() {
    let mut model = model();
    // 10 pt text: rows 11.5 pt tall from 2 pt under the top at 620.
    assert_eq!(click(&mut model, 9, 9, (50.0, 612.0)), None);
    assert_eq!(
        value(&model, "pets"),
        FieldValue::Chosen(vec!["Cat".into()])
    );
    click(&mut model, 9, 9, (50.0, 590.0));
    assert_eq!(
        value(&model, "pets"),
        FieldValue::Chosen(vec!["Cat".into(), "Fish".into()])
    );
    click(&mut model, 9, 9, (50.0, 612.0));
    assert_eq!(
        value(&model, "pets"),
        FieldValue::Chosen(vec!["Fish".into()])
    );
    click(&mut model, 9, 9, (50.0, 565.0));
    assert_eq!(
        value(&model, "pets"),
        FieldValue::Chosen(vec!["Fish".into()]),
        "under the last row, nothing"
    );
}

#[test]
fn a_dropdown_offers_its_options_and_takes_typed_text_when_editable() {
    let mut model = model();
    let prompt = click(&mut model, 10, 10, (50.0, 530.0)).expect("an editor");
    let Entry::Choose {
        options,
        chosen,
        editable,
    } = &prompt.entry
    else {
        panic!("a dropdown");
    };
    assert_eq!(options.len(), 2);
    assert_eq!(chosen.as_deref(), Some("g"));
    assert!(*editable && prompt.entry.typed());
    assert_eq!(prompt.entry.initial_text(), "Green");
    assert_eq!(
        prompt.entry.value_for("Red"),
        FieldValue::Chosen(vec!["r".into()])
    );
    assert_eq!(
        prompt.entry.value_for("Mauve"),
        FieldValue::Chosen(vec!["Mauve".into()])
    );
    assert_eq!(prompt.entry.value_for(""), FieldValue::Chosen(Vec::new()));
    model
        .commit_field(prompt.field, prompt.entry.value_for("Red"))
        .expect("commits");
    assert_eq!(
        value(&model, "colour"),
        FieldValue::Chosen(vec!["r".into()])
    );
    let fixed = Entry::Choose {
        options: options.clone(),
        chosen: Some("x".into()),
        editable: false,
    };
    assert!(!fixed.typed());
    assert_eq!(fixed.initial_text(), "x", "an export with no option");
}

#[test]
fn buttons_signatures_and_read_only_fields_say_why_nothing_happens() {
    let mut model = model();
    for (number, point) in [
        (11, (20.0, 500.0)),
        (12, (50.0, 460.0)),
        (13, (50.0, 420.0)),
    ] {
        assert_eq!(click(&mut model, number, number, point), None);
    }
    let notices = model.take_form_notices();
    assert_eq!(notices.len(), 3);
    assert!(notices[0].starts_with("print is a button"));
    assert!(notices[1].starts_with("sign is a signature field"));
    assert_eq!(notices[2], "locked is read-only");
}

#[test]
fn tab_moves_between_fillable_fields_and_goes_round() {
    let mut model = model();
    let next = |model: &mut CanvasModel, widget: u32, backwards: bool| {
        model
            .field_prompt_after(object(widget), backwards)
            .expect("reads")
            .map(|prompt| prompt.name)
    };
    assert_eq!(next(&mut model, 4, false).as_deref(), Some("colour"));
    assert_eq!(next(&mut model, 10, false).as_deref(), Some("age"));
    assert_eq!(next(&mut model, 14, false).as_deref(), Some("qty"));
    assert_eq!(next(&mut model, 4, true).as_deref(), Some("age"));
    assert_eq!(next(&mut model, 99, false), None);
}

#[test]
fn clear_form_restores_every_field() {
    let mut model = model();
    click(&mut model, 5, 5, (15.0, 675.0));
    assert_eq!(
        model.clear_form().expect("clears"),
        7,
        "a button and a signature hold no value"
    );
    assert_eq!(value(&model, "agree"), FieldValue::State(None));
    assert_eq!(value(&model, "colour"), FieldValue::None);
}

#[test]
fn an_xfa_form_is_said_when_it_opens() {
    let bytes = document();
    let text = String::from_utf8_lossy(&bytes).replace(
        "/DA (/Helv 10 Tf 0 g) >>",
        "/DA (/Helv 10 Tf 0 g) /XFA 15 0 R >>",
    );
    // The offsets drift, and the reader repairs the table to open it.
    let model = CanvasModel::new(
        Document::open_bytes(text.into_bytes()).expect("opens"),
        PluginRegistry::new(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("canvas starts");
    let Some(super::super::CanvasStatus::Notice { message }) = model.status() else {
        panic!("a notice: {:?}", model.status());
    };
    assert!(message.contains("XFA"), "{message}");
    assert!(
        self::model().status().is_none(),
        "a plain form says nothing"
    );
}
