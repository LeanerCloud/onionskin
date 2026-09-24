//! Filling a form through the plugin: keystroke and validate scripts that
//! refuse, calculations that follow, formats that show, and Clear Form.

use onionskin_core::forms::FieldValue;
use onionskin_core::{Document, ObjRef};
use onionskin_cos::Object;
use onionskin_tools_form::fill::{clear_form, fill, toggle, FillOptions};

fn js(script: &str) -> String {
    format!("<< /S /JavaScript /JS ({script}) >>")
}

/// qty times price is total, shown as dollars; age between 0 and 130; a
/// check box, a radio pair, and a field whose calculation cannot run.
fn document() -> Vec<u8> {
    let number = format!(
        "/AA << /K {} /F {} >>",
        js("AFNumber_Keystroke\\(2, 0, 0, 0, \"\", true\\);"),
        js("AFNumber_Format\\(2, 0, 0, 0, \"\", true\\);")
    );
    let total = format!(
        "/AA << /C {} /F {} >>",
        js("AFSimple_Calculate\\(\"PRD\", new Array \\(\"qty\", \"price\"\\)\\);"),
        js("AFNumber_Format\\(2, 0, 0, 0, \"$\", true\\);")
    );
    let age = format!(
        "/AA << /V {} >>",
        js("AFRange_Validate\\(true, 0, true, 130\\);")
    );
    let broken = format!("/AA << /C {} >>", js("event.value = this.mailForm\\(\\);"));
    let widget = |rest: &str| -> Vec<u8> {
        format!("<< /Type /Annot /Subtype /Widget /P 3 0 R {rest} >>").into_bytes()
    };
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 12 0 R] \
           /DA (/Helv 0 Tf 0 g) /CO [6 0 R 12 0 R] >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 10 0 R 11 0 R 12 0 R] >>".to_vec(),
        widget("/FT /Tx /T (qty) /Rect [10 700 110 720] /V (1)"),
        widget(&format!("/FT /Tx /T (price) /Rect [10 670 110 690] {number}")),
        widget(&format!("/FT /Tx /T (total) /Rect [10 640 110 660] {total}")),
        widget(&format!("/FT /Tx /T (age) /Rect [10 610 110 630] {age}")),
        widget("/FT /Btn /T (agree) /Rect [10 580 22 592] /AP << /N << /Yes 13 0 R /Off 13 0 R >> >> /AS /Off"),
        b"<< /FT /Btn /Ff 49152 /T (size) /Kids [10 0 R 11 0 R] >>".to_vec(),
        widget("/Parent 9 0 R /Rect [10 550 22 562] /AP << /N << /S 13 0 R /Off 13 0 R >> >> /AS /Off"),
        widget("/Parent 9 0 R /Rect [40 550 52 562] /AP << /N << /L 13 0 R /Off 13 0 R >> >> /AS /Off"),
        widget(&format!("/FT /Tx /T (broken) /Rect [10 520 110 540] /V (old) {broken}")),
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12] /Length 0 >>\nstream\n\nendstream".to_vec(),
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

fn open() -> Document {
    Document::open_bytes(document()).expect("opens")
}

fn field(doc: &mut Document, name: &str) -> ObjRef {
    doc.form()
        .expect("reads")
        .field(name)
        .expect("the field")
        .objref
}

fn value(doc: &mut Document, name: &str) -> FieldValue {
    doc.form()
        .expect("reads")
        .field(name)
        .expect("the field")
        .value
        .clone()
}

/// The text a widget's appearance draws, decoded from its hex strings.
fn shown(doc: &mut Document, name: &str) -> String {
    let form = doc.form().expect("reads");
    let widget = form.field(name).expect("the field").widgets[0].objref;
    let structure = doc.structure().expect("structure");
    let dict = structure.get(widget.number).expect("widget").object;
    let normal = dict
        .as_dict()
        .and_then(|dict| dict.get(b"AP"))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .and_then(Object::as_reference)
        .expect("an appearance");
    let stream = structure.get(normal.number).expect("stream").object;
    let raw = String::from_utf8_lossy(&stream.as_stream().expect("a stream").raw).into_owned();
    let start = raw.find('<').expect("a string") + 1;
    let end = raw[start..].find('>').expect("its end") + start;
    (start..end)
        .step_by(2)
        .map(|at| u8::from_str_radix(&raw[at..at + 2], 16).expect("hex") as char)
        .collect()
}

#[test]
fn a_value_is_kept_calculations_follow_and_formats_show() {
    let mut doc = open();
    let price = field(&mut doc, "price");
    let filled = fill(
        &mut doc,
        price,
        FieldValue::Text("1,234.5".into()),
        FillOptions::default(),
    )
    .expect("fills");
    assert!(filled.accepted);
    assert_eq!(value(&mut doc, "price"), FieldValue::Text("1,234.5".into()));
    assert_eq!(
        value(&mut doc, "total"),
        FieldValue::Text("1234.5".into()),
        "qty 1 times price"
    );
    assert_eq!(shown(&mut doc, "price"), "1,234.50");
    assert_eq!(shown(&mut doc, "total"), "$1,234.50");
    assert!(filled.changed.contains(&"total".to_owned()));
    assert_eq!(filled.problems.len(), 1, "the broken calculation is named");
    assert!(
        filled.problems[0].starts_with("broken:"),
        "{:?}",
        filled.problems
    );
    assert_eq!(
        value(&mut doc, "broken"),
        FieldValue::Text("old".into()),
        "left as it was"
    );

    let qty = field(&mut doc, "qty");
    fill(
        &mut doc,
        qty,
        FieldValue::Text("2".into()),
        FillOptions::default(),
    )
    .expect("fills");
    assert_eq!(value(&mut doc, "total"), FieldValue::Text("2469".into()));
    assert_eq!(shown(&mut doc, "total"), "$2,469.00");

    assert_eq!(doc.edit().history().undo_label(), Some("Fill Field"));
    let (edit, base) = doc.edit_mut();
    edit.undo(base).expect("undoes");
    assert_eq!(
        value(&mut doc, "total"),
        FieldValue::Text("1234.5".into()),
        "one step undoes it all"
    );
}

#[test]
fn a_keystroke_or_validate_script_refuses_a_value() {
    let mut doc = open();
    let price = field(&mut doc, "price");
    let refused = fill(
        &mut doc,
        price,
        FieldValue::Text("abc".into()),
        FillOptions::default(),
    )
    .expect("runs");
    assert!(!refused.accepted);
    assert_eq!(
        refused.alerts,
        ["The value entered does not match the format of the field [ price ]"]
    );
    assert_eq!(
        value(&mut doc, "price"),
        FieldValue::None,
        "nothing was written"
    );

    let age = field(&mut doc, "age");
    let old = fill(
        &mut doc,
        age,
        FieldValue::Text("200".into()),
        FillOptions::default(),
    )
    .expect("runs");
    assert!(!old.accepted);
    assert!(old.alerts[0].contains("less than or equal to 130"));
    assert!(
        fill(
            &mut doc,
            age,
            FieldValue::Text("42".into()),
            FillOptions::default()
        )
        .expect("runs")
        .accepted
    );

    let scripts_off = FillOptions { scripts: false };
    let kept = fill(&mut doc, price, FieldValue::Text("abc".into()), scripts_off).expect("runs");
    assert!(kept.accepted && kept.alerts.is_empty());
    assert_eq!(value(&mut doc, "price"), FieldValue::Text("abc".into()));
    assert_eq!(
        value(&mut doc, "total"),
        FieldValue::Text("0".into()),
        "computed when age was filled, and not again with scripts off"
    );
}

#[test]
fn check_boxes_toggle_and_radio_buttons_hold() {
    let mut doc = open();
    let form = doc.form().expect("reads");
    let agree = form.field("agree").expect("agree");
    let (agree_field, agree_widget) = (agree.objref, agree.widgets[0].objref);
    let size = form.field("size").expect("size");
    let (size_field, small, large) = (size.objref, size.widgets[0].objref, size.widgets[1].objref);

    toggle(&mut doc, agree_field, agree_widget, FillOptions::default()).expect("toggles");
    assert_eq!(
        value(&mut doc, "agree"),
        FieldValue::State(Some("Yes".into()))
    );
    toggle(&mut doc, agree_field, agree_widget, FillOptions::default()).expect("toggles");
    assert_eq!(value(&mut doc, "agree"), FieldValue::State(None));

    toggle(&mut doc, size_field, small, FillOptions::default()).expect("toggles");
    assert_eq!(value(&mut doc, "size"), FieldValue::State(Some("S".into())));
    toggle(&mut doc, size_field, small, FillOptions::default()).expect("toggles");
    assert_eq!(
        value(&mut doc, "size"),
        FieldValue::State(Some("S".into())),
        "no toggle to off"
    );
    toggle(&mut doc, size_field, large, FillOptions::default()).expect("toggles");
    assert_eq!(value(&mut doc, "size"), FieldValue::State(Some("L".into())));
    let missing =
        toggle(&mut doc, ObjRef::new(99, 0), large, FillOptions::default()).expect("runs");
    assert!(!missing.accepted);
}

#[test]
fn clear_form_empties_the_form_and_a_missing_field_is_refused() {
    let mut doc = open();
    let qty = field(&mut doc, "qty");
    fill(
        &mut doc,
        qty,
        FieldValue::Text("5".into()),
        FillOptions { scripts: false },
    )
    .expect("fills");
    assert_eq!(clear_form(&mut doc).expect("clears"), 7);
    assert_eq!(value(&mut doc, "qty"), FieldValue::None);
    assert_eq!(doc.edit().history().undo_label(), Some("Clear Form"));
    let error = fill(
        &mut doc,
        ObjRef::new(99, 0),
        FieldValue::None,
        FillOptions::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("not in the form"), "{error}");
}
