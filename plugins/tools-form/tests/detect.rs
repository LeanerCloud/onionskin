//! Detecting where a form is filled in: underscores, rules, boxes and
//! labels, and what is not a place to write.

use onionskin_core::forms::{FieldKind, NewField};
use onionskin_core::Document;
use onionskin_tools_form::detect::{detect_fields, detect_page};

/// One Letter page drawing `content` in Helvetica as `/F1`.
fn page(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

const FORM: &str = "\
BT /F1 12 Tf 72 700 Td (Name: ________________) Tj ET \
BT /F1 12 Tf 72 650 Td (Date of birth) Tj ET 160 648 m 300 648 l S \
72 600 10 10 re S BT /F1 12 Tf 90 601 Td (I agree) Tj ET \
BT /F1 12 Tf 20 545 Td (Address) Tj ET 72 540 200 20 re S \
BT /F1 12 Tf 72 480 Td (Title) Tj ET 72 478 m 110 478 l S \
0 450 m 612 450 l S \
72 400 300 1 re f \
0.5 w 72 300 30 30 re S";

#[test]
fn underscores_rules_and_boxes_become_fields_named_by_their_labels() {
    let mut doc = Document::open_bytes(page(FORM)).expect("opens");
    let found = detect_page(&mut doc, 0).expect("detects");
    let summary: Vec<(String, Option<String>)> = found
        .iter()
        .map(|place| (format!("{:?}", place.kind), place.label.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Text".to_owned(), Some("Name".to_owned())),
            ("Text".to_owned(), Some("Date of birth".to_owned())),
            ("CheckBox".to_owned(), None),
            ("Text".to_owned(), Some("Address".to_owned())),
            ("Text".to_owned(), None),
        ],
        "{found:#?}"
    );
    let name = &found[0];
    assert!(name.rect[0] > 100.0, "over the underscores, not the label");
    assert_eq!(found[2].rect, [72.0, 600.0, 82.0, 610.0]);
    assert_eq!(found[3].rect, [73.0, 541.0, 271.0, 559.0], "inside the box");
    assert_eq!(found[4].rect[1], 400.5, "standing on the filled rule");

    assert_eq!(detect_fields(&mut doc).expect("adds"), 5);
    assert_eq!(
        doc.edit().history().undo_label(),
        Some("Detect Form Fields")
    );
    let form = doc.form().expect("reads");
    let names: Vec<&str> = form
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Name", "Date of birth", "Check Box1", "Address", "Text1"]
    );
    assert_eq!(form.fields[2].kind, FieldKind::CheckBox);
    assert_eq!(
        detect_fields(&mut doc).expect("runs"),
        0,
        "nothing over a field the page has"
    );
}

#[test]
fn a_label_taken_is_numbered_and_a_page_with_nothing_finds_nothing() {
    let twice = "\
BT /F1 12 Tf 72 700 Td (Name: ________) Tj ET \
BT /F1 12 Tf 72 650 Td (Name: ________) Tj ET";
    let mut doc = Document::open_bytes(page(twice)).expect("opens");
    assert_eq!(detect_fields(&mut doc).expect("adds"), 2);
    let form = doc.form().expect("reads");
    let names: Vec<&str> = form
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(names, ["Name", "Name1"]);

    let mut plain =
        Document::open_bytes(page("BT /F1 12 Tf 72 700 Td (Hello) Tj ET")).expect("opens");
    assert_eq!(detect_fields(&mut plain).expect("runs"), 0);
    assert!(plain.form().expect("reads").fields.is_empty());
    assert_eq!(NewField::Text.base_name(), "Text");
}
