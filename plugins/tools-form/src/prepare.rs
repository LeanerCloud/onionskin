//! Changing and taking away a field, as Prepare Form's Properties dialog
//! and its Delete do: each one undoable step.

use onionskin_core::forms::{
    properties_refusal, remove_field, set_button_icon, set_field_properties, FieldProperties,
};
use onionskin_core::{Document, ObjRef};
use onionskin_plugin_api::CommandError;

use crate::fill::failed;

const PROPERTIES: &str = "Field Properties";
const DELETE: &str = "Delete Field";

fn missing(label: &'static str) -> CommandError {
    CommandError::Failed {
        label,
        reason: "that field is not in the form any more".to_owned(),
    }
}

/// The properties of `field` as its widget `widget` shows them.
pub fn properties(
    doc: &mut Document,
    field: ObjRef,
    widget: ObjRef,
) -> Result<FieldProperties, CommandError> {
    let form = doc.form().map_err(failed(PROPERTIES))?;
    let target = form
        .field_by_ref(field)
        .ok_or_else(|| missing(PROPERTIES))?;
    let shown = target
        .widgets
        .iter()
        .find(|each| each.objref == widget)
        .ok_or_else(|| missing(PROPERTIES))?;
    Ok(FieldProperties::of(target, shown))
}

/// Write `properties` to `field`, as widget `widget` shows it, or say why
/// not.
pub fn set_properties(
    doc: &mut Document,
    field: ObjRef,
    widget: ObjRef,
    properties: &FieldProperties,
) -> Result<(), CommandError> {
    let form = doc.form().map_err(failed(PROPERTIES))?;
    let target = form
        .field_by_ref(field)
        .ok_or_else(|| missing(PROPERTIES))?;
    if let Some(reason) = properties_refusal(&form, target, properties) {
        return Err(CommandError::Failed {
            label: PROPERTIES,
            reason,
        });
    }
    doc.edit_content(PROPERTIES, |tx, _| {
        set_field_properties(tx, &form, field, widget, properties)
    })
    .map_err(failed(PROPERTIES))
}

/// Take `field` away.
pub fn delete_field(doc: &mut Document, field: ObjRef) -> Result<(), CommandError> {
    let form = doc.form().map_err(failed(DELETE))?;
    if form.field_by_ref(field).is_none() {
        return Err(missing(DELETE));
    }
    doc.edit_content(DELETE, |tx, _| remove_field(tx, &form, field))
        .map_err(failed(DELETE))
}

/// The names of every field but `field`, for a calculation to pick from.
pub fn other_field_names(doc: &mut Document, field: ObjRef) -> Vec<String> {
    doc.form()
        .map(|form| {
            form.fields
                .iter()
                .filter(|each| each.objref != field)
                .map(|each| each.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Show the image `document` (a PDF, as every image import makes one) on
/// image field `field` through its widget `widget`, as one undo step.
pub fn set_image(
    doc: &mut Document,
    field: ObjRef,
    widget: ObjRef,
    document: Vec<u8>,
) -> Result<(), CommandError> {
    const LABEL: &str = "Set Image";
    let unreadable = |error: onionskin_core::Error| CommandError::Failed {
        label: LABEL,
        reason: format!("the image could not be read: {error}"),
    };
    let mut image = Document::open_bytes(document).map_err(unreadable)?;
    let source = image.structure().map_err(unreadable)?;
    let form = doc.form().map_err(failed(LABEL))?;
    if !form
        .field_by_ref(field)
        .is_some_and(|target| target.is_image())
    {
        return Err(missing(LABEL));
    }
    doc.edit_content(LABEL, |tx, _| {
        set_button_icon(tx, &form, field, widget, source)
    })
    .map_err(failed(LABEL))
}
