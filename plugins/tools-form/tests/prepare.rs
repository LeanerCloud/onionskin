//! A field's properties changed, refused, and a field deleted, through the
//! plugin as the Properties dialog does it.

use onionskin_core::forms::{KindOptions, NewField};
use onionskin_core::{Document, ObjRef};
use onionskin_tools_form::prepare::{delete_field, other_field_names, properties, set_properties};

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
