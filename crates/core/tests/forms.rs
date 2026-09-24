//! `core::forms`: the field tree read, values written with their
//! appearances, and Clear Form, each read back from a fresh parse.

use onionskin_core::forms::{
    read_form, reset_fields, set_field_value, FieldKind, FieldValue, Form,
};
use onionskin_core::{EditSession, Structure, Transaction};
use onionskin_cos::{BytesSource, Document as CosDocument, Object};

mod common;
use common::{pdf, stream};

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn apply<T>(
    original: &[u8],
    body: impl FnOnce(&mut Transaction<'_>, &Structure) -> onionskin_core::Result<T>,
) -> Vec<u8> {
    let base = open(original);
    let structure = onionskin_core::read_structure(&base).expect("the structure reads");
    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Fill", |tx| body(tx, &structure))
        .expect("the edit runs");
    let mut bytes = original.to_vec();
    if let Some(section) = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
    {
        bytes.extend_from_slice(&section);
    }
    bytes
}

fn form(bytes: &[u8]) -> Form {
    read_form(&open(bytes)).expect("the form reads")
}

/// A one-page form with a field of every kind.
fn document() -> Vec<u8> {
    pdf(&[
        // 1 catalog, 2 pages, 3 page
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R 8 0 R 9 0 R 12 0 R 13 0 R 14 0 R 15 0 R 16 0 R 17 0 R] \
           /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 4 0 R >> >> /NeedAppearances true /CO [16 0 R] /XFA 4 0 R >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [6 0 R 7 0 R 8 0 R 10 0 R 11 0 R 12 0 R 13 0 R 14 0 R 15 0 R 16 0 R 17 0 R] >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        // 5: a parent naming two text fields
        b"<< /T (person) /FT /Tx /Kids [6 0 R 7 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /T (first) /Parent 5 0 R /Rect [50 700 250 720] /V (Ada) \
           /DA (/Helv 10 Tf 0 0 1 rg) /MK << /BG [1 1 0.8] /BC [0 0 0] >> /TU (First name) /P 3 0 R >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Widget /T (last) /Parent 5 0 R /Rect [260 700 460 720] /Ff 2 /Q 2 \
           /AA << /K << /S /JavaScript /JS (AFSpecial_Keystroke(0);) >> /F << /S /JavaScript /JS (AFSpecial_Format(0);) >> >> >>"
            .to_vec(),
        // 8: a check box
        b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /Rect [50 650 62 662] /V /Yes /AS /Yes \
           /AP << /N << /Yes 4 0 R /Off 4 0 R >> >> >>"
            .to_vec(),
        // 9: a radio group of two
        b"<< /FT /Btn /Ff 49152 /T (size) /Kids [10 0 R 11 0 R] /DV /Small >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /Parent 9 0 R /Rect [50 600 62 612] /AP << /N << /Small 4 0 R /Off 4 0 R >> >> /AS /Off >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Widget /Parent 9 0 R /Rect [80 600 92 612] /AP << /N << /Large 4 0 R /Off 4 0 R >> >> /AS /Off /F 2 >>"
            .to_vec(),
        // 12: a dropdown with export and display values
        b"<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 393216 /T (colour) /Rect [50 550 150 570] \
           /Opt [[(r) (Red)] [(g) (Green)]] /V (g) >>"
            .to_vec(),
        // 13: a multi-select list box
        b"<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 2097152 /T (pets) /Rect [50 480 150 540] \
           /Opt [(Cat) (Dog) (Fish)] /V [(Cat) (Fish)] >>"
            .to_vec(),
        // 14: a push button, 15: a signature, 16: a comb, 17: a password
        b"<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (print) /Rect [400 50 450 70] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Sig /T (sign) /Rect [400 100 550 140] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 16777216 /MaxLen 5 /T (zip) /Rect [300 550 400 570] \
           /AA << /C << /S /JavaScript /JS (event.value = 1;) >> /V << /S /JavaScript /JS (true;) >> >> >>"
            .to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 12288 /T (secret) /Rect [300 500 400 540] /DV (dflt) >>".to_vec(),
    ])
}

#[test]
fn every_kind_of_field_is_read_with_what_it_inherits() {
    let form = form(&document());
    let names: Vec<&str> = form
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "person.first",
            "person.last",
            "agree",
            "size",
            "colour",
            "pets",
            "print",
            "sign",
            "zip",
            "secret"
        ]
    );
    assert!(form.need_appearances && form.xfa && form.resources.is_some());
    assert_eq!(form.calculation_order.len(), 1);

    let first = form.field("person.first").expect("first");
    assert_eq!(first.value, FieldValue::Text("Ada".to_owned()));
    assert_eq!(first.tooltip.as_deref(), Some("First name"));
    assert_eq!(first.appearance.as_deref(), Some("/Helv 10 Tf 0 0 1 rg"));
    assert_eq!(first.page(), Some(0));
    assert!(matches!(
        first.kind,
        FieldKind::Text {
            multiline: false,
            ..
        }
    ));
    let last = form.field("person.last").expect("last");
    assert!(last.flags.required && last.align == 2);
    assert_eq!(
        last.appearance.as_deref(),
        Some("/Helv 0 Tf 0 g"),
        "from the form"
    );
    assert_eq!(
        last.scripts.keystroke.as_deref(),
        Some("AFSpecial_Keystroke(0);")
    );
    assert_eq!(last.scripts.format.as_deref(), Some("AFSpecial_Format(0);"));

    let agree = form.field("agree").expect("agree");
    assert_eq!(agree.kind, FieldKind::CheckBox);
    assert_eq!(agree.value, FieldValue::State(Some("Yes".to_owned())));
    assert_eq!(agree.widgets[0].on_state.as_deref(), Some("Yes"));

    let size = form.field("size").expect("size");
    assert!(matches!(
        size.kind,
        FieldKind::Radio {
            no_toggle_to_off: true
        }
    ));
    assert_eq!(size.value, FieldValue::State(None));
    assert_eq!(size.default, FieldValue::State(Some("Small".to_owned())));
    assert_eq!(size.widgets.len(), 2);
    assert!(size.widgets[1].hidden);

    let colour = form.field("colour").expect("colour");
    let FieldKind::Choice {
        combo,
        editable,
        options,
        ..
    } = &colour.kind
    else {
        panic!("a choice");
    };
    assert!(*combo && *editable);
    assert_eq!(
        (options[1].export.as_str(), options[1].display.as_str()),
        ("g", "Green")
    );
    assert_eq!(colour.value, FieldValue::Chosen(vec!["g".to_owned()]));
    let pets = form.field("pets").expect("pets");
    assert!(matches!(
        pets.kind,
        FieldKind::Choice {
            combo: false,
            multi_select: true,
            ..
        }
    ));
    assert_eq!(
        pets.value,
        FieldValue::Chosen(vec!["Cat".to_owned(), "Fish".to_owned()])
    );
    assert_eq!(
        form.field("print").expect("print").kind,
        FieldKind::PushButton
    );
    assert_eq!(form.field("sign").expect("sign").kind, FieldKind::Signature);
    let zip = form.field("zip").expect("zip");
    assert!(matches!(
        zip.kind,
        FieldKind::Text {
            comb: true,
            max_len: Some(5),
            ..
        }
    ));
    assert_eq!(zip.scripts.calculate.as_deref(), Some("event.value = 1;"));
    assert_eq!(zip.scripts.validate.as_deref(), Some("true;"));
    assert!(matches!(
        form.field("secret").expect("secret").kind,
        FieldKind::Text {
            multiline: true,
            password: true,
            ..
        }
    ));
    assert_eq!(FieldKind::CheckBox.label(), "Check Box");
}

#[test]
fn fields_are_found_by_place_and_ordered_for_tabbing() {
    let form = form(&document());
    let (field, _) = form.field_at(0, (100.0, 710.0)).expect("a field there");
    assert_eq!(field.name, "person.first");
    assert!(
        form.field_at(0, (85.0, 605.0)).is_none(),
        "a hidden widget is not hit"
    );
    assert!(form.field_at(1, (100.0, 710.0)).is_none());
    let order: Vec<&str> = form
        .tab_order()
        .iter()
        .map(|(field, _)| form.fields[*field].name.as_str())
        .collect();
    assert_eq!(order[..3], ["person.first", "person.last", "agree"]);
    assert!(!order.contains(&"size") || order.iter().filter(|name| **name == "size").count() == 1);

    let pets = form.field("pets").expect("pets");
    let rows: Vec<Option<usize>> = [535.0, 520.0, 500.0, 490.0, 545.0]
        .map(|y| pets.list_row(&pets.widgets[0], (60.0, y)))
        .into();
    assert_eq!(rows, [Some(0), Some(1), Some(2), None, None]);
    let first = form.field("person.first").expect("a text field");
    assert_eq!(first.list_row(&first.widgets[0], (100.0, 710.0)), None);
}

#[test]
fn values_are_written_with_their_appearances() {
    let original = document();
    let before = form(&original);
    let first = before.field("person.first").expect("first").objref;
    let agree = before.field("agree").expect("agree").objref;
    let size = before.field("size").expect("size").objref;
    let colour = before.field("colour").expect("colour").objref;
    let pets = before.field("pets").expect("pets").objref;
    let zip = before.field("zip").expect("zip").objref;
    let secret = before.field("secret").expect("secret").objref;
    let saved = apply(&original, |tx, _| {
        set_field_value(
            tx,
            &before,
            first,
            &FieldValue::Text("Grace".into()),
            Some("GRACE"),
        )?;
        set_field_value(tx, &before, agree, &FieldValue::State(None), None)?;
        set_field_value(
            tx,
            &before,
            size,
            &FieldValue::State(Some("Large".into())),
            None,
        )?;
        set_field_value(
            tx,
            &before,
            colour,
            &FieldValue::Chosen(vec!["r".into()]),
            None,
        )?;
        set_field_value(
            tx,
            &before,
            pets,
            &FieldValue::Chosen(vec!["Dog".into(), "Fish".into()]),
            None,
        )?;
        set_field_value(tx, &before, zip, &FieldValue::Text("12345".into()), None)?;
        set_field_value(tx, &before, secret, &FieldValue::Text("pw".into()), None)
    });
    let after = form(&saved);
    assert_eq!(
        after.field("person.first").expect("first").value,
        FieldValue::Text("Grace".into())
    );
    assert_eq!(
        after.field("agree").expect("agree").value,
        FieldValue::State(None)
    );
    assert_eq!(
        after.field("agree").expect("agree").widgets[0]
            .state
            .as_deref(),
        Some("Off")
    );
    let size = after.field("size").expect("size");
    assert_eq!(size.value, FieldValue::State(Some("Large".into())));
    let states: Vec<_> = size
        .widgets
        .iter()
        .map(|widget| widget.state.clone())
        .collect();
    assert_eq!(states, [Some("Off".into()), Some("Large".into())]);
    assert_eq!(
        after.field("colour").expect("colour").value,
        FieldValue::Chosen(vec!["r".into()])
    );
    assert_eq!(
        after.field("pets").expect("pets").value,
        FieldValue::Chosen(vec!["Dog".into(), "Fish".into()])
    );

    // Each text and choice widget has a new appearance drawing the value.
    let doc = open(&saved);
    let appearance = |name: &str| -> String {
        let widget = after.field(name).expect("field").widgets[0].objref;
        let dict = doc.get(widget.number).expect("widget").object;
        let normal = dict
            .as_dict()
            .and_then(|dict| dict.get(b"AP"))
            .and_then(Object::as_dict)
            .and_then(|ap| ap.get(b"N"))
            .and_then(Object::as_reference)
            .expect("a normal appearance");
        let stream = doc.get(normal.number).expect("stream").object;
        String::from_utf8_lossy(&stream.as_stream().expect("a stream").raw).into_owned()
    };
    let hex = |text: &str| -> String { text.bytes().map(|byte| format!("{byte:02X}")).collect() };
    let first = appearance("person.first");
    assert!(first.contains(&hex("GRACE")), "the display text: {first}");
    assert!(
        first.contains("/Helv 10 Tf 0 0 1 rg") && first.contains("1 1 0.8 rg"),
        "{first}"
    );
    assert!(
        appearance("colour").contains(&hex("Red")),
        "a dropdown shows the display text"
    );
    let pets = appearance("pets");
    assert_eq!(
        pets.matches("re f").count(),
        2,
        "two chosen entries highlighted: {pets}"
    );
    assert_eq!(
        appearance("zip").matches("Tm").count(),
        5,
        "one cell a digit"
    );
    assert!(
        appearance("secret").contains(&hex("**")),
        "a password is not shown"
    );
}

#[test]
fn clear_form_restores_the_defaults() {
    let original = document();
    let before = form(&original);
    let first = before.field("person.first").expect("first").objref;
    let only = [first];
    let partly = apply(&original, |tx, _| reset_fields(tx, &before, Some(&only)));
    assert_eq!(
        form(&partly).field("person.first").expect("first").value,
        FieldValue::None
    );
    assert_eq!(
        form(&partly).field("agree").expect("agree").value,
        FieldValue::State(Some("Yes".into())),
        "a field not named keeps its value"
    );
    let cleared = apply(&original, |tx, _| reset_fields(tx, &before, None));
    let after = form(&cleared);
    assert_eq!(
        after.field("size").expect("size").value,
        FieldValue::State(Some("Small".into()))
    );
    assert_eq!(
        after.field("secret").expect("secret").value,
        FieldValue::Text("dflt".into())
    );
    assert_eq!(after.field("pets").expect("pets").value, FieldValue::None);
    assert_eq!(
        FieldValue::Chosen(vec!["a".into(), "b".into()]).as_text(),
        "a, b"
    );
}

#[test]
fn a_document_without_a_form_has_an_empty_one() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>".to_vec(),
        stream(""),
    ]);
    assert_eq!(form(&bytes), Form::default());
    let missing = apply(&bytes, |tx, _| {
        let result = set_field_value(
            tx,
            &Form::default(),
            onionskin_cos::ObjRef::new(3, 0),
            &FieldValue::None,
            None,
        );
        assert!(result.is_err());
        Ok(())
    });
    assert!(missing.len() >= bytes.len());
}
