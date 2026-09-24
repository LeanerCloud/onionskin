//! A field's properties changed, refused, and a field deleted, through the
//! plugin as the Properties dialog does it.

use onionskin_core::forms::{KindOptions, NewField};
use onionskin_core::{Document, ObjRef};
use onionskin_tools_form::prepare::{
    delete_field, other_field_names, properties, set_image, set_properties,
};

mod common;

/// The plugin's test form with a text field of our own added.
fn document() -> (Document, ObjRef, ObjRef) {
    let mut doc = Document::open_bytes(common::document()).expect("opens");
    let form = doc.form().expect("reads");
    let added = doc
        .edit_annotations("Add", |tx, structure| {
            onionskin_core::forms::add_field(
                tx,
                structure,
                &form,
                &NewField::Text,
                0,
                [300.0, 700.0, 444.0, 722.0],
            )
        })
        .expect("adds");
    (doc, added.field, added.widget)
}

#[test]
fn properties_are_read_changed_and_undone_as_one_step() {
    let (mut doc, field, widget) = document();
    let mut changed = properties(&mut doc, field, widget).expect("reads");
    assert_eq!(changed.name, "Text1");
    changed.name = "total2".into();
    changed.options = KindOptions::Text {
        align: 2,
        default: String::new(),
        multiline: false,
        password: false,
        comb: false,
        max_len: Some(8),
    };
    set_properties(&mut doc, field, widget, &changed).expect("writes");
    assert_eq!(properties(&mut doc, field, widget).expect("reads"), changed);
    assert_eq!(doc.edit().history().undo_label(), Some("Field Properties"));
    assert!(other_field_names(&mut doc, field).contains(&"qty".to_owned()));
    assert!(!other_field_names(&mut doc, field).contains(&"total2".to_owned()));
}

#[test]
fn a_taken_name_is_refused_and_a_missing_field_is_said() {
    let (mut doc, field, widget) = document();
    let mut changed = properties(&mut doc, field, widget).expect("reads");
    changed.name = "qty".into();
    let error = set_properties(&mut doc, field, widget, &changed).unwrap_err();
    assert!(
        error.to_string().contains("already a field called qty"),
        "{error}"
    );
    let gone = ObjRef::new(99, 0);
    assert!(properties(&mut doc, gone, widget)
        .unwrap_err()
        .to_string()
        .contains("not in the form"));
    assert!(properties(&mut doc, field, gone).is_err());
    assert!(set_properties(&mut doc, gone, widget, &changed).is_err());
    assert!(delete_field(&mut doc, gone).is_err());
}

#[test]
fn a_deleted_field_is_gone_in_one_step() {
    let (mut doc, field, _) = document();
    delete_field(&mut doc, field).expect("deletes");
    assert!(doc.form().expect("reads").field("Text1").is_none());
    assert_eq!(doc.edit().history().undo_label(), Some("Delete Field"));
}

#[test]
fn an_image_field_takes_an_image_and_nothing_else_does() {
    let mut doc = Document::open_bytes(common::document()).expect("opens");
    let form = doc.form().expect("reads");
    let added = doc
        .edit_annotations("Add", |tx, structure| {
            onionskin_core::forms::add_field(
                tx,
                structure,
                &form,
                &NewField::Image,
                0,
                [300.0, 600.0, 400.0, 700.0],
            )
        })
        .expect("adds");
    let picture = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 20 10] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n"
        .to_vec();
    set_image(&mut doc, added.field, added.widget, picture.clone()).expect("sets");
    assert_eq!(doc.edit().history().undo_label(), Some("Set Image"));
    let error = set_image(&mut doc, added.field, added.widget, b"not a pdf".to_vec()).unwrap_err();
    assert!(error.to_string().contains("could not be read"), "{error}");
    let qty = doc
        .form()
        .expect("reads")
        .field("qty")
        .expect("qty")
        .clone();
    assert!(set_image(&mut doc, qty.objref, qty.widgets[0].objref, picture).is_err());
}
