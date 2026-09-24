//! Filling a form: a value committed the way Acrobat commits one.
//!
//! A value goes through the field's keystroke script with `willCommit`, then
//! its validate script; either can refuse it. Accepted, it is kept, every
//! field in the calculation order recomputes from the new values, and each
//! field that changed is shown through its format script. All of it is one
//! undo step. With document JavaScript turned off, the value is simply
//! kept.

use std::collections::BTreeMap;

use onionskin_core::forms::{reset_fields, set_field_value, Field, FieldKind, FieldValue, Form};
use onionskin_core::{Document, ObjRef};
use onionskin_plugin_api::CommandError;
use onionskin_scripting::{run, EventKind, Invocation, Outcome};

/// How to fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillOptions {
    /// Run the form's JavaScript: Preferences > JavaScript.
    pub scripts: bool,
}

impl Default for FillOptions {
    fn default() -> Self {
        FillOptions { scripts: true }
    }
}

/// What filling did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filled {
    /// Whether the value was kept. A refused value changes nothing.
    pub accepted: bool,
    /// What the scripts said with `app.alert`.
    pub alerts: Vec<String>,
    /// Scripts that could not run, by field, which the shell shows so a
    /// value is never silently left uncomputed.
    pub problems: Vec<String>,
    /// The fields written, by name.
    pub changed: Vec<String>,
}

fn failed(label: &'static str) -> impl Fn(onionskin_core::Error) -> CommandError {
    move |source| CommandError::Edit { label, source }
}

const FILL: &str = "Fill Field";

/// Commit `value` to field `field`.
pub fn fill(
    doc: &mut Document,
    field: ObjRef,
    value: FieldValue,
    options: FillOptions,
) -> Result<Filled, CommandError> {
    let form = doc.form().map_err(failed(FILL))?;
    let target = form
        .field_by_ref(field)
        .ok_or_else(|| CommandError::Failed {
            label: FILL,
            reason: "that field is not in the form any more".to_owned(),
        })?;
    if target.flags.read_only {
        return Err(CommandError::Failed {
            label: FILL,
            reason: format!("{} is read-only", target.name),
        });
    }
    let mut values: BTreeMap<String, String> = form
        .fields
        .iter()
        .map(|field| (field.name.clone(), field.value.as_text()))
        .collect();
    let mut filled = Filled::default();
    let mut value = value;
    if options.scripts && takes_text(&target.kind) {
        let mut typed = value.as_text();
        for (kind, script) in [
            (EventKind::Keystroke, target.scripts.keystroke.as_deref()),
            (EventKind::Validate, target.scripts.validate.as_deref()),
        ] {
            let Some(script) = script else { continue };
            match invoke(kind, script, target, &typed, &values) {
                Ok(outcome) => {
                    filled.alerts.extend(outcome.alerts);
                    if !outcome.accepted {
                        return Ok(filled);
                    }
                    typed = outcome.value;
                }
                Err(problem) => filled.problems.push(problem),
            }
        }
        value = as_value(&target.kind, &typed, &value);
    }
    values.insert(target.name.clone(), value.as_text());
    let mut changed: BTreeMap<ObjRef, FieldValue> = BTreeMap::from([(target.objref, value)]);
    if options.scripts {
        calculate(&form, &mut values, &mut changed, &mut filled);
    }
    let displays = if options.scripts {
        formats(&form, &changed, &values, &mut filled)
    } else {
        BTreeMap::new()
    };
    doc.edit_annotations(FILL, |tx, _| {
        for (objref, value) in &changed {
            set_field_value(
                tx,
                &form,
                *objref,
                value,
                displays.get(objref).map(String::as_str),
            )?;
        }
        Ok(())
    })
    .map_err(failed(FILL))?;
    filled.accepted = true;
    filled.changed = changed
        .keys()
        .filter_map(|objref| form.field_by_ref(*objref).map(|field| field.name.clone()))
        .collect();
    Ok(filled)
}

/// A check box or radio button clicked: on becomes off, off becomes on,
/// except a radio button that cannot be turned off.
pub fn toggle(
    doc: &mut Document,
    field: ObjRef,
    widget: ObjRef,
    options: FillOptions,
) -> Result<Filled, CommandError> {
    let form = doc.form().map_err(failed(FILL))?;
    let Some(target) = form.field_by_ref(field) else {
        return Ok(Filled::default());
    };
    let on = target
        .widgets
        .iter()
        .find(|each| each.objref == widget)
        .and_then(|each| each.on_state.clone());
    let current = match &target.value {
        FieldValue::State(state) => state.clone(),
        _ => None,
    };
    let next = match (&target.kind, current == on) {
        (
            FieldKind::Radio {
                no_toggle_to_off: true,
            },
            true,
        ) => current,
        (_, true) => None,
        (_, false) => on,
    };
    fill(doc, field, FieldValue::State(next), options)
}

/// Clear Form: every field back to its default. How many were reset.
pub fn clear_form(doc: &mut Document) -> Result<usize, CommandError> {
    let label = "Clear Form";
    let form = doc.form().map_err(failed(label))?;
    doc.edit_annotations(label, |tx, _| reset_fields(tx, &form, None))
        .map_err(failed(label))
}

fn takes_text(kind: &FieldKind) -> bool {
    matches!(kind, FieldKind::Text { .. } | FieldKind::Choice { .. })
}

/// `typed` as the value of a field of `kind`, keeping a multiple choice as
/// it was chosen.
fn as_value(kind: &FieldKind, typed: &str, chosen: &FieldValue) -> FieldValue {
    match (kind, chosen) {
        (
            FieldKind::Choice {
                multi_select: true, ..
            },
            FieldValue::Chosen(_),
        ) => chosen.clone(),
        (FieldKind::Choice { .. }, _) if typed.is_empty() => FieldValue::Chosen(Vec::new()),
        (FieldKind::Choice { .. }, _) => FieldValue::Chosen(vec![typed.to_owned()]),
        (FieldKind::CheckBox | FieldKind::Radio { .. }, _) => {
            FieldValue::State((!typed.is_empty() && typed != "Off").then(|| typed.to_owned()))
        }
        _ => FieldValue::Text(typed.to_owned()),
    }
}

fn invoke(
    kind: EventKind,
    script: &str,
    field: &Field,
    value: &str,
    values: &BTreeMap<String, String>,
) -> Result<Outcome, String> {
    run(
        script,
        &Invocation {
            kind,
            target: &field.name,
            value,
            change: "",
            will_commit: true,
            fields: values,
        },
    )
    .map_err(|error| format!("{}: {error}", field.name))
}

/// The fields in the calculation order recompute, each from the values as
/// the ones before it left them.
fn calculate(
    form: &Form,
    values: &mut BTreeMap<String, String>,
    changed: &mut BTreeMap<ObjRef, FieldValue>,
    filled: &mut Filled,
) {
    let order: Vec<ObjRef> = if form.calculation_order.is_empty() {
        form.fields
            .iter()
            .filter(|field| field.scripts.calculate.is_some())
            .map(|field| field.objref)
            .collect()
    } else {
        form.calculation_order.clone()
    };
    for objref in order {
        let Some(field) = form.field_by_ref(objref) else {
            continue;
        };
        let Some(script) = field.scripts.calculate.as_deref() else {
            continue;
        };
        let current = values.get(&field.name).cloned().unwrap_or_default();
        match invoke(EventKind::Calculate, script, field, &current, values) {
            Ok(outcome) => {
                filled.alerts.extend(outcome.alerts);
                for (name, value) in outcome.changed {
                    if let Some(other) = form.field(&name) {
                        changed.insert(other.objref, as_value(&other.kind, &value, &other.value));
                        values.insert(name, value);
                    }
                }
                if outcome.value != current {
                    changed.insert(
                        field.objref,
                        as_value(&field.kind, &outcome.value, &field.value),
                    );
                    values.insert(field.name.clone(), outcome.value);
                }
            }
            Err(problem) => filled.problems.push(problem),
        }
    }
}

/// How each changed field is shown, through its format script.
fn formats(
    form: &Form,
    changed: &BTreeMap<ObjRef, FieldValue>,
    values: &BTreeMap<String, String>,
    filled: &mut Filled,
) -> BTreeMap<ObjRef, String> {
    let mut out = BTreeMap::new();
    for objref in changed.keys() {
        let Some(field) = form.field_by_ref(*objref) else {
            continue;
        };
        let Some(script) = field.scripts.format.as_deref() else {
            continue;
        };
        let value = values.get(&field.name).cloned().unwrap_or_default();
        match invoke(EventKind::Format, script, field, &value, values) {
            Ok(outcome) => {
                filled.alerts.extend(outcome.alerts);
                out.insert(*objref, outcome.value);
            }
            Err(problem) => filled.problems.push(problem),
        }
    }
    out
}
