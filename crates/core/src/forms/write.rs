//! Writing a field's value, and Clear Form.

use onionskin_cos::{Dict, Name, ObjRef, Object, Stream};

use super::appearance::{font, list_appearance, parse_da, text_appearance, Frame, Layout};
use super::{Field, FieldKind, FieldValue, Form};
use crate::annots::author::text_string;
use crate::edit::Transaction;
use crate::{Error, Result};

/// Give field `field` of `form` the value `value`, and every widget of it
/// the appearance for it. `display` is the text shown when a format script
/// shows the value differently from how it is stored, as `1,234.50` for
/// `1234.5`.
pub fn set_field_value(
    tx: &mut Transaction<'_>,
    form: &Form,
    field: ObjRef,
    value: &FieldValue,
    display: Option<&str>,
) -> Result<()> {
    let field = form.field_by_ref(field).ok_or(Error::NotADictionary {
        number: field.number,
    })?;
    let mut dict = dict_at(tx, field.objref)?;
    set_value_key(&mut dict, &field.kind, value);
    tx.put_object(
        field.objref.number,
        generation(tx, field.objref)?,
        Object::Dict(dict),
    )?;
    let appearances = appearances(tx, form, field, value, display)?;
    for (widget, change) in field.widgets.iter().zip(appearances) {
        let mut dict = dict_at(tx, widget.objref)?;
        match change {
            Change::State(state) => dict.set(Name::new("AS"), Object::Name(Name::new(&state))),
            Change::Normal(stream) => {
                let number = tx.reserve();
                tx.put_object(number, 0, Object::Stream(stream))?;
                let mut ap = Dict::new();
                ap.set(Name::new("N"), Object::Ref(ObjRef::new(number, 0)));
                dict.set(Name::new("AP"), Object::Dict(ap));
            }
            Change::Keep => continue,
        }
        tx.put_object(
            widget.objref.number,
            generation(tx, widget.objref)?,
            Object::Dict(dict),
        )?;
    }
    Ok(())
}

/// Clear Form: every field back to its default, or empty; only `only`
/// when it names fields. How many fields were reset.
pub fn reset_fields(
    tx: &mut Transaction<'_>,
    form: &Form,
    only: Option<&[ObjRef]>,
) -> Result<usize> {
    let mut reset = 0;
    for field in &form.fields {
        if matches!(field.kind, FieldKind::PushButton | FieldKind::Signature) {
            continue;
        }
        if only.is_some_and(|only| !only.contains(&field.objref)) {
            continue;
        }
        set_field_value(tx, form, field.objref, &field.default, None)?;
        reset += 1;
    }
    Ok(reset)
}

fn dict_at(tx: &Transaction<'_>, objref: ObjRef) -> Result<Dict> {
    tx.object(objref.number)?
        .and_then(|state| state.object.as_dict().cloned())
        .ok_or(Error::NotADictionary {
            number: objref.number,
        })
}

fn generation(tx: &Transaction<'_>, objref: ObjRef) -> Result<u16> {
    Ok(tx
        .object(objref.number)?
        .map_or(objref.generation, |state| state.generation))
}

fn set_value_key(dict: &mut Dict, kind: &FieldKind, value: &FieldValue) {
    let written = match (kind, value) {
        (FieldKind::CheckBox | FieldKind::Radio { .. }, FieldValue::State(state)) => {
            Some(Object::Name(Name::new(state.as_deref().unwrap_or("Off"))))
        }
        (FieldKind::Choice { .. }, FieldValue::Chosen(chosen)) => match chosen.as_slice() {
            [] => None,
            [one] => Some(text_string(one)),
            many => Some(Object::Array(
                many.iter().map(|each| text_string(each)).collect(),
            )),
        },
        (_, FieldValue::Text(text)) if !text.is_empty() => Some(text_string(text)),
        _ => None,
    };
    match written {
        Some(value) => dict.set(Name::new("V"), value),
        None => {
            dict.remove(b"V");
        }
    }
    // A choice's indices would disagree with a new value.
    dict.remove(b"I");
}

enum Change {
    State(String),
    Normal(Stream),
    Keep,
}

/// What each widget of `field` becomes for `value`.
fn appearances(
    tx: &Transaction<'_>,
    form: &Form,
    field: &Field,
    value: &FieldValue,
    display: Option<&str>,
) -> Result<Vec<Change>> {
    let resolve = |object: &Object| match object {
        Object::Ref(objref) => tx
            .object(objref.number)
            .ok()
            .flatten()
            .map_or(Object::Null, |state| state.object),
        other => other.clone(),
    };
    let da = parse_da(field.appearance.as_deref());
    let font = font(&da, form.resources.as_ref(), resolve);
    let shown = display.map_or_else(|| value.as_text(), str::to_owned);
    let mut out = Vec::new();
    for widget in &field.widgets {
        let dict = dict_at(tx, widget.objref)?;
        let frame = Frame::of(&dict, resolve);
        out.push(match &field.kind {
            FieldKind::CheckBox | FieldKind::Radio { .. } => {
                let on = matches!(value, FieldValue::State(Some(state)) if Some(state) == widget.on_state.as_ref());
                Change::State(if on {
                    widget.on_state.clone().unwrap_or_else(|| "Off".to_owned())
                } else {
                    "Off".to_owned()
                })
            }
            FieldKind::Text {
                multiline,
                password,
                comb,
                max_len,
            } => {
                let text = if *password {
                    "*".repeat(shown.chars().count())
                } else {
                    shown.clone()
                };
                let layout = Layout {
                    multiline: *multiline,
                    comb: comb.then_some(max_len.unwrap_or(0)),
                    align: field.align,
                };
                Change::Normal(text_appearance(&frame, &da, &font, layout, &text))
            }
            FieldKind::Choice {
                combo: true,
                options,
                ..
            } => {
                let text = display.map(str::to_owned).unwrap_or_else(|| displayed(options, value));
                let layout = Layout {
                    multiline: false,
                    comb: None,
                    align: field.align,
                };
                Change::Normal(text_appearance(&frame, &da, &font, layout, &text))
            }
            FieldKind::Choice { options, .. } => {
                let chosen = match value {
                    FieldValue::Chosen(chosen) => chosen.clone(),
                    _ => Vec::new(),
                };
                let entries: Vec<(String, bool)> = options
                    .iter()
                    .map(|option| (option.display.clone(), chosen.contains(&option.export)))
                    .collect();
                Change::Normal(list_appearance(&frame, &da, &font, &entries))
            }
            FieldKind::PushButton | FieldKind::Signature => Change::Keep,
        });
    }
    Ok(out)
}

/// A dropdown's chosen entry as it is shown: its display text.
fn displayed(options: &[super::ChoiceOption], value: &FieldValue) -> String {
    let FieldValue::Chosen(chosen) = value else {
        return String::new();
    };
    chosen
        .iter()
        .map(|export| {
            options
                .iter()
                .find(|option| &option.export == export)
                .map_or_else(|| export.clone(), |option| option.display.clone())
        })
        .collect::<Vec<_>>()
        .join(", ")
}
