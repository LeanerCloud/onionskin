//! `core::forms`: the field tree read, values written with their
//! appearances, Clear Form, and fields added and taken away, each read back
//! from a fresh parse.

use onionskin_core::forms::{
    add_field, properties_refusal, read_form, remove_field, reset_fields, set_field_properties,
    set_field_value, unique_name, ChoiceOption, FieldKind, FieldProperties, FieldScripts,
    FieldValue, Form, KindOptions, NewField,
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
    assert!(form
        .notice()
        .expect("XFA is said")
        .contains("standard fields"));
    let xfa_only = Form {
        xfa: true,
        ..Form::default()
    };
    assert!(xfa_only.notice().expect("said").contains("read-only"));
    assert_eq!(Form::default().notice(), None);
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

/// One blank page and no form.
fn blank() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
    ])
}

/// `bytes` with a field of `kind` added over `rect` on the first page.
fn with_field(bytes: &[u8], kind: NewField, rect: [f64; 4]) -> Vec<u8> {
    let before = form(bytes);
    apply(bytes, |tx, structure| {
        add_field(tx, structure, &before, &kind, 0, rect).map(|_| ())
    })
}

/// The raw normal appearance of `name`'s first widget, or of its `state`.
fn normal_appearance(bytes: &[u8], name: &str, state: Option<&str>) -> String {
    let doc = open(bytes);
    let form = read_form(&doc).expect("reads");
    let widget = form.field(name).expect("the field").widgets[0].objref;
    let dict = doc.get(widget.number).expect("widget").object;
    let ap = doc
        .resolve(
            dict.as_dict()
                .and_then(|dict| dict.get(b"AP"))
                .expect("an /AP"),
        )
        .expect("resolves");
    let mut normal = doc
        .resolve(ap.as_dict().and_then(|ap| ap.get(b"N")).expect("an /N"))
        .expect("resolves");
    if let Some(state) = state {
        normal = doc
            .resolve(
                normal
                    .as_dict()
                    .and_then(|n| n.get(state.as_bytes()))
                    .expect("the state"),
            )
            .expect("resolves");
    }
    let stream = normal.as_stream().expect("a stream").clone();
    String::from_utf8_lossy(&stream.raw).into_owned()
}

#[test]
fn a_field_of_every_kind_is_added_to_a_document_without_a_form() {
    let mut bytes = blank();
    let kinds = [
        (NewField::Text, [100.0, 700.0, 244.0, 722.0]),
        (NewField::Date, [300.0, 700.0, 444.0, 722.0]),
        (NewField::CheckBox, [100.0, 650.0, 114.0, 664.0]),
        (
            NewField::Radio { group: None },
            [100.0, 600.0, 114.0, 614.0],
        ),
        (
            NewField::Radio {
                group: Some("Group1".to_owned()),
            },
            [130.0, 600.0, 144.0, 614.0],
        ),
        (NewField::ListBox, [100.0, 500.0, 244.0, 572.0]),
        (NewField::Dropdown, [300.0, 500.0, 444.0, 522.0]),
        (NewField::Button, [100.0, 450.0, 172.0, 472.0]),
        (NewField::Signature, [100.0, 380.0, 280.0, 416.0]),
    ];
    for (kind, rect) in kinds {
        bytes = with_field(&bytes, kind, rect);
    }
    let form = form(&bytes);
    let names: Vec<&str> = form
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Text1",
            "Date1",
            "Check Box1",
            "Group1",
            "List Box1",
            "Dropdown1",
            "Button1",
            "Signature1"
        ]
    );
    assert!(form
        .fields
        .iter()
        .all(|field| field.widgets.iter().all(|widget| widget.page == Some(0))));
    let text = form.field("Text1").expect("text");
    assert!(matches!(
        text.kind,
        FieldKind::Text {
            multiline: false,
            ..
        }
    ));
    assert_eq!(text.widgets[0].rect, [100.0, 700.0, 244.0, 722.0]);
    assert_eq!(text.appearance.as_deref(), Some("/Helv 0 Tf 0 g"));
    let drawn = normal_appearance(&bytes, "Text1", None);
    assert!(drawn.contains("0 0 0 RG 1 w"), "a black border: {drawn}");

    let date = form.field("Date1").expect("date");
    assert_eq!(
        date.scripts.format.as_deref(),
        Some("AFDate_FormatEx(\"mm/dd/yyyy\");")
    );
    assert!(date.scripts.keystroke.is_some());

    let check = form.field("Check Box1").expect("check box");
    assert_eq!(check.kind, FieldKind::CheckBox);
    assert_eq!(check.widgets[0].on_state.as_deref(), Some("Yes"));
    assert_eq!(check.value, FieldValue::State(None));
    assert!(normal_appearance(&bytes, "Check Box1", Some("Yes")).contains("/ZaDb"));

    let group = form.field("Group1").expect("the radio group");
    assert_eq!(
        group.kind,
        FieldKind::Radio {
            no_toggle_to_off: true
        }
    );
    let states: Vec<_> = group
        .widgets
        .iter()
        .map(|widget| widget.on_state.as_deref())
        .collect();
    assert_eq!(states, [Some("Choice1"), Some("Choice2")]);

    assert!(matches!(
        form.field("List Box1").expect("list").kind,
        FieldKind::Choice { combo: false, .. }
    ));
    assert!(matches!(
        form.field("Dropdown1").expect("dropdown").kind,
        FieldKind::Choice { combo: true, .. }
    ));
    assert_eq!(
        form.field("Button1").expect("button").kind,
        FieldKind::PushButton
    );
    // "Button" in WinAnsi hex, centred on the grey face.
    assert!(normal_appearance(&bytes, "Button1", None).contains("<427574746F6E>"));
    assert_eq!(
        form.field("Signature1").expect("signature").kind,
        FieldKind::Signature
    );

    let fonts = form
        .resources
        .as_ref()
        .and_then(|dr| dr.get(b"Font"))
        .and_then(|fonts| fonts.as_dict().cloned())
        .expect("fonts in /DR");
    assert!(fonts.contains(b"Helv") && fonts.contains(b"ZaDb"));
    let doc = open(&bytes);
    let page = doc.page(0).expect("page");
    let annots = doc
        .resolve(page.dict.get(b"Annots").expect("/Annots"))
        .expect("resolves");
    assert_eq!(
        annots.as_array().map(<[Object]>::len),
        Some(9),
        "every widget"
    );
}

#[test]
fn names_are_numbered_past_those_taken() {
    let bytes = with_field(&blank(), NewField::Text, [0.0, 0.0, 10.0, 10.0]);
    let bytes = with_field(&bytes, NewField::Text, [0.0, 20.0, 10.0, 30.0]);
    let form = form(&bytes);
    assert!(form.field("Text2").is_some());
    assert_eq!(unique_name(&form, "Text"), "Text3");
    assert_eq!(unique_name(&form, "Check Box"), "Check Box1");
    assert_eq!(NewField::Text.base_name(), "Text");
    assert_eq!(NewField::CheckBox.default_size(), (14.0, 14.0));
    assert_eq!(NewField::Signature.default_size(), (180.0, 36.0));
}

#[test]
fn a_field_taken_away_leaves_the_page_the_form_and_the_calculation_order() {
    let mut bytes = with_field(&blank(), NewField::Text, [0.0, 0.0, 10.0, 10.0]);
    bytes = with_field(
        &bytes,
        NewField::Radio { group: None },
        [0.0, 20.0, 10.0, 30.0],
    );
    bytes = with_field(
        &bytes,
        NewField::Radio {
            group: Some("Group1".into()),
        },
        [20.0, 20.0, 30.0, 30.0],
    );
    let before = form(&bytes);
    let text = before.field("Text1").expect("text").objref;
    let group = before.field("Group1").expect("group").objref;
    bytes = apply(&bytes, |tx, _| remove_field(tx, &before, text));
    let middle = form(&bytes);
    assert!(middle.field("Text1").is_none());
    assert!(middle.field("Group1").is_some());
    bytes = apply(&bytes, |tx, _| remove_field(tx, &middle, group));
    assert_eq!(form(&bytes).fields, Vec::new());
    let doc = open(&bytes);
    let page = doc.page(0).expect("page");
    let left = page
        .dict
        .get(b"Annots")
        .map(|annots| doc.resolve(annots).expect("resolves"))
        .and_then(|annots| annots.as_array().map(<[Object]>::len))
        .unwrap_or(0);
    assert_eq!(left, 0, "no widget is left on the page");
    let missing = apply(&bytes, |tx, _| {
        assert!(remove_field(tx, &Form::default(), text).is_err());
        Ok(())
    });
    assert!(missing.len() >= bytes.len());

    let every_kind = document();
    let before = form(&every_kind);
    let calculated = before.calculation_order[0];
    let name = before
        .field_by_ref(calculated)
        .expect("in the form")
        .name
        .clone();
    let after = form(&apply(&every_kind, |tx, _| {
        remove_field(tx, &before, calculated)
    }));
    assert!(after.field(&name).is_none());
    assert!(after.calculation_order.is_empty(), "out of /CO too");
    assert_eq!(after.fields.len(), before.fields.len() - 1);
}

/// `bytes` with `change` made to `name`'s properties, as its first widget
/// shows them.
fn with_properties(bytes: &[u8], name: &str, change: impl FnOnce(&mut FieldProperties)) -> Vec<u8> {
    let before = form(bytes);
    let field = before.field(name).expect("the field").clone();
    let widget = field.widgets[0].clone();
    let mut properties = FieldProperties::of(&field, &widget);
    change(&mut properties);
    assert_eq!(properties_refusal(&before, &field, &properties), None);
    apply(bytes, |tx, _| {
        set_field_properties(tx, &before, field.objref, widget.objref, &properties)
    })
}

fn properties(bytes: &[u8], name: &str) -> FieldProperties {
    let form = form(bytes);
    let field = form.field(name).expect("the field");
    FieldProperties::of(field, &field.widgets[0])
}

#[test]
fn a_text_field_s_properties_are_written_and_read_back() {
    let bytes = with_field(&blank(), NewField::Text, [100.0, 700.0, 244.0, 722.0]);
    let fresh = properties(&bytes, "Text1");
    assert_eq!(fresh.border, Some([0.0; 3]));
    assert_eq!(fresh.fill, None);
    assert_eq!(fresh.font_size, 0.0);
    let wanted = FieldProperties {
        name: "email".into(),
        tooltip: "Your e-mail".into(),
        hidden: true,
        read_only: true,
        required: true,
        border: None,
        fill: Some([1.0, 1.0, 0.8]),
        font_size: 10.0,
        text_color: [1.0, 0.0, 0.0],
        rect: [110.0, 690.0, 300.0, 712.0],
        options: KindOptions::Text {
            align: 1,
            default: "a@b.c".into(),
            multiline: false,
            password: false,
            comb: true,
            max_len: Some(20),
        },
        scripts: FieldScripts {
            keystroke: None,
            format: Some("AFSpecial_Format(0);".into()),
            validate: Some("true;".into()),
            calculate: Some("event.value = 1;".into()),
        },
    };
    let bytes = with_properties(&bytes, "Text1", |properties| *properties = wanted.clone());
    assert_eq!(properties(&bytes, "email"), wanted);
    let form_after = form(&bytes);
    let email = form_after.field("email").expect("renamed");
    assert_eq!(form_after.calculation_order, [email.objref]);
    assert!(email.widgets[0].hidden);
    let drawn = normal_appearance(&bytes, "email", None);
    assert!(drawn.contains("1 1 0.8 rg"), "the fill: {drawn}");
    assert!(!drawn.contains("RG"), "no border: {drawn}");

    let bytes = with_properties(&bytes, "email", |properties| {
        properties.scripts.calculate = None;
        properties.scripts.format = Some("  ".into());
        properties.hidden = false;
        properties.options = KindOptions::Text {
            align: 0,
            default: String::new(),
            multiline: true,
            password: true,
            comb: false,
            max_len: None,
        };
    });
    let after = properties(&bytes, "email");
    assert_eq!(after.scripts.format, None, "a blank script is none");
    assert!(!after.hidden);
    assert!(form(&bytes).calculation_order.is_empty(), "out of /CO");
    assert!(matches!(
        after.options,
        KindOptions::Text {
            multiline: true,
            password: true,
            max_len: None,
            ..
        }
    ));
}

#[test]
fn a_check_box_s_export_value_renames_its_on_state() {
    let bytes = with_field(&blank(), NewField::CheckBox, [100.0, 650.0, 114.0, 664.0]);
    let bytes = with_properties(&bytes, "Check Box1", |properties| {
        properties.options = KindOptions::Button {
            export: "Agree".into(),
            on_by_default: true,
            no_toggle_to_off: None,
        };
    });
    let form_after = form(&bytes);
    let field = form_after.field("Check Box1").expect("the check box");
    assert_eq!(field.widgets[0].on_state.as_deref(), Some("Agree"));
    assert_eq!(field.default, FieldValue::State(Some("Agree".into())));
    assert!(normal_appearance(&bytes, "Check Box1", Some("Agree")).contains("/ZaDb"));
    let bytes = with_properties(&bytes, "Check Box1", |properties| {
        properties.options = KindOptions::Button {
            export: "Agree".into(),
            on_by_default: false,
            no_toggle_to_off: None,
        };
    });
    assert_eq!(
        form(&bytes).field("Check Box1").expect("it").default,
        FieldValue::State(None)
    );
}

#[test]
fn a_radio_button_s_export_value_is_its_own() {
    let bytes = with_field(
        &blank(),
        NewField::Radio { group: None },
        [0.0, 0.0, 14.0, 14.0],
    );
    let bytes = with_field(
        &bytes,
        NewField::Radio {
            group: Some("Group1".into()),
        },
        [20.0, 0.0, 34.0, 14.0],
    );
    let before = form(&bytes);
    let group = before.field("Group1").expect("group").clone();
    let second = group.widgets[1].clone();
    let mut changed = FieldProperties::of(&group, &second);
    changed.options = KindOptions::Button {
        export: "Large".into(),
        on_by_default: false,
        no_toggle_to_off: Some(false),
    };
    let bytes = apply(&bytes, |tx, _| {
        set_field_properties(tx, &before, group.objref, second.objref, &changed)
    });
    let after = form(&bytes);
    let group = after.field("Group1").expect("group");
    let states: Vec<_> = group
        .widgets
        .iter()
        .map(|widget| widget.on_state.as_deref())
        .collect();
    assert_eq!(states, [Some("Choice1"), Some("Large")]);
    assert_eq!(
        group.kind,
        FieldKind::Radio {
            no_toggle_to_off: false
        }
    );
}

#[test]
fn a_dropdown_s_options_and_a_button_s_caption_are_written() {
    let bytes = with_field(&blank(), NewField::Dropdown, [0.0, 0.0, 144.0, 22.0]);
    let options = vec![
        ChoiceOption {
            export: "r".into(),
            display: "Red".into(),
        },
        ChoiceOption {
            export: "g".into(),
            display: "Green".into(),
        },
    ];
    let bytes = with_properties(&bytes, "Dropdown1", |properties| {
        properties.options = KindOptions::Choice {
            options: options.clone(),
            editable: true,
            multi_select: false,
            default: Some("g".into()),
        };
    });
    assert_eq!(
        properties(&bytes, "Dropdown1").options,
        KindOptions::Choice {
            options,
            editable: true,
            multi_select: false,
            default: Some("g".into()),
        }
    );

    let bytes = with_field(&bytes, NewField::Button, [0.0, 50.0, 72.0, 72.0]);
    let bytes = with_properties(&bytes, "Button1", |properties| {
        properties.options = KindOptions::PushButton {
            caption: "Print".into(),
        };
    });
    assert_eq!(
        properties(&bytes, "Button1").options,
        KindOptions::PushButton {
            caption: "Print".into()
        }
    );
    assert!(normal_appearance(&bytes, "Button1", None).contains("<5072696E74>"));
    let bytes = with_field(&bytes, NewField::Signature, [0.0, 100.0, 180.0, 136.0]);
    assert_eq!(
        properties(&bytes, "Signature1").options,
        KindOptions::Signature
    );
}

#[test]
fn properties_that_cannot_be_written_say_why() {
    let bytes = with_field(&blank(), NewField::Text, [0.0, 0.0, 144.0, 22.0]);
    let bytes = with_field(&bytes, NewField::CheckBox, [0.0, 50.0, 14.0, 64.0]);
    let form = form(&bytes);
    let text = form.field("Text1").expect("text");
    let base = FieldProperties::of(text, &text.widgets[0]);
    let refused = |change: &dyn Fn(&mut FieldProperties)| {
        let mut properties = base.clone();
        change(&mut properties);
        properties_refusal(&form, text, &properties).unwrap_or_default()
    };
    assert_eq!(refused(&|p| p.name = " ".into()), "A field needs a name");
    assert!(refused(&|p| p.name = "a.b".into()).contains("full stop"));
    assert_eq!(
        refused(&|p| p.name = "Check Box1".into()),
        "There is already a field called Check Box1"
    );
    assert!(refused(&|p| p.rect = [0.0, 0.0, 0.5, 10.0]).contains("at least a point"));
    assert!(refused(&|p| {
        p.options = KindOptions::Button {
            export: "Off".into(),
            on_by_default: false,
            no_toggle_to_off: None,
        }
    })
    .contains("export value"));
    assert_eq!(refused(&|_| {}), "");
    let missing = apply(&bytes, |tx, _| {
        let wrong = onionskin_cos::ObjRef::new(99, 0);
        assert!(set_field_properties(tx, &form, wrong, wrong, &base).is_err());
        assert!(set_field_properties(tx, &form, text.objref, wrong, &base).is_err());
        Ok(())
    });
    assert!(missing.len() >= bytes.len());
}
