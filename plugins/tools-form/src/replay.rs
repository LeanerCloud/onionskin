//! Replaying a recorded form session: guarantee test 7's harness.
//!
//! A scenario is the ordered field interactions someone made in Acrobat;
//! the expectation is the state Acrobat was left in, as recorded there.
//! [`replay`] makes the same interactions through [`crate::fill`] and says
//! every place the result differs. The file formats are the ones
//! `corpus/js-forms/README.md` sets out: `<stem>.scenario.json` and
//! `<stem>.expected.json` beside each PDF.

use std::collections::BTreeMap;

use onionskin_core::forms::{FieldKind, FieldValue};
use onionskin_core::Document;
use onionskin_scripting::{run, EventKind, Invocation};
use serde_json::Value;

use crate::fill::{fill, toggle, FillOptions};

/// One interaction, as a scenario file lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub field: String,
    pub action: Action,
    pub value: String,
}

/// What was done to the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Typed and committed with Enter.
    Enter,
    /// Typed and committed by leaving the field.
    Blur,
    /// A check box or radio button clicked: the value names its on state.
    Check,
    /// A dropdown or list box option chosen, by export value.
    Choose,
}

/// What Acrobat showed for one field after the last step. Each is a list,
/// so a field two Acrobat versions disagree on passes on either.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expected {
    pub value: Vec<String>,
    pub display: Vec<String>,
}

/// A recorded session's outcome.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expectation {
    pub fields: BTreeMap<String, Expected>,
    /// What the scripts said, in order, when recorded.
    pub alerts: Option<Vec<String>>,
}

/// A file that is not in the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn bad(what: impl Into<String>) -> FormatError {
    FormatError(what.into())
}

/// A `<stem>.scenario.json`: a list of `{field, action, value}`.
pub fn parse_scenario(json: &str) -> Result<Vec<Step>, FormatError> {
    let parsed: Value = serde_json::from_str(json).map_err(|error| bad(error.to_string()))?;
    let steps = parsed
        .as_array()
        .ok_or_else(|| bad("a scenario is a list of steps"))?;
    steps
        .iter()
        .enumerate()
        .map(|(index, step)| parse_step(index, step))
        .collect()
}

fn parse_step(index: usize, step: &Value) -> Result<Step, FormatError> {
    let text = |key: &str| {
        step.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| bad(format!("step {}: `{key}` must be a string", index + 1)))
    };
    let action = match text("action")?.as_str() {
        "enter" => Action::Enter,
        "blur" => Action::Blur,
        "check" => Action::Check,
        "choose" => Action::Choose,
        other => {
            return Err(bad(format!(
                "step {}: `{other}` is not enter, blur, check or choose",
                index + 1
            )))
        }
    };
    Ok(Step {
        field: text("field")?,
        action,
        value: step
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

/// A `<stem>.expected.json`: `{"fields": {name: {"value", "display"}},
/// "alerts": [...]}`, each value a string or a list of the versions'.
pub fn parse_expectation(json: &str) -> Result<Expectation, FormatError> {
    let parsed: Value = serde_json::from_str(json).map_err(|error| bad(error.to_string()))?;
    let fields = parsed
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("an expectation has a `fields` object"))?;
    let mut expectation = Expectation::default();
    for (name, expected) in fields {
        expectation.fields.insert(
            name.clone(),
            Expected {
                value: strings(expected.get("value"), name)?,
                display: strings(expected.get("display"), name)?,
            },
        );
    }
    expectation.alerts = match parsed.get("alerts") {
        None => None,
        Some(alerts) => Some(strings(Some(alerts), "alerts")?),
    };
    Ok(expectation)
}

fn strings(value: Option<&Value>, name: &str) -> Result<Vec<String>, FormatError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::String(one)) => Ok(vec![one.clone()]),
        Some(Value::Array(many)) => many
            .iter()
            .map(|each| {
                each.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| bad(format!("{name}: every version is a string")))
            })
            .collect(),
        Some(_) => Err(bad(format!("{name}: a string or a list of strings"))),
    }
}

/// Make `steps` on `doc` and compare with `expected`: every difference, in
/// words. Empty means Onionskin filled the form the way Acrobat did.
pub fn replay(doc: &mut Document, steps: &[Step], expected: &Expectation) -> Vec<String> {
    let mut differences = Vec::new();
    let mut alerts = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        match make(doc, step) {
            Ok(said) => alerts.extend(said),
            Err(why) => differences.push(format!("step {}: {why}", index + 1)),
        }
    }
    for (name, wanted) in &expected.fields {
        match observed(doc, name) {
            Some((value, display)) => {
                differ(&mut differences, name, "value", &wanted.value, &value);
                differ(&mut differences, name, "display", &wanted.display, &display);
            }
            None => differences.push(format!("{name}: not a field of this form")),
        }
    }
    if let Some(wanted) = &expected.alerts {
        if *wanted != alerts {
            differences.push(format!("alerts: expected {wanted:?}, got {alerts:?}"));
        }
    }
    differences
}

fn differ(differences: &mut Vec<String>, name: &str, what: &str, wanted: &[String], got: &str) {
    if !wanted.is_empty() && !wanted.iter().any(|each| each == got) {
        differences.push(format!("{name}: {what} expected {wanted:?}, got {got:?}"));
    }
}

/// One step, and what the scripts said.
fn make(doc: &mut Document, step: &Step) -> Result<Vec<String>, String> {
    let form = doc.form().map_err(|error| error.to_string())?;
    let field = form
        .field(&step.field)
        .ok_or_else(|| format!("{} is not a field of this form", step.field))?;
    let options = FillOptions::default();
    let filled = match step.action {
        Action::Enter | Action::Blur => {
            let value = match field.kind {
                FieldKind::Choice { .. } => FieldValue::Chosen(vec![step.value.clone()]),
                _ => FieldValue::Text(step.value.clone()),
            };
            fill(doc, field.objref, value, options)
        }
        Action::Choose => fill(
            doc,
            field.objref,
            FieldValue::Chosen(vec![step.value.clone()]),
            options,
        ),
        Action::Check => {
            let widget = field
                .widgets
                .iter()
                .find(|widget| widget.on_state.as_deref() == Some(step.value.as_str()))
                .ok_or_else(|| format!("{} has no {} button", step.field, step.value))?;
            toggle(doc, field.objref, widget.objref, options)
        }
    }
    .map_err(|error| error.to_string())?;
    Ok(filled.alerts)
}

/// A field's value and display as Acrobat's `value` and formatted text
/// read them: an unchecked button is "Off".
fn observed(doc: &mut Document, name: &str) -> Option<(String, String)> {
    let form = doc.form().ok()?;
    let field = form.field(name)?;
    let value = match &field.value {
        FieldValue::State(None) => "Off".to_owned(),
        other => other.as_text(),
    };
    let display = match field.scripts.format.as_deref() {
        Some(script) => {
            let values: BTreeMap<String, String> = form
                .fields
                .iter()
                .map(|each| (each.name.clone(), each.value.as_text()))
                .collect();
            run(
                script,
                &Invocation {
                    kind: EventKind::Format,
                    target: name,
                    value: &value,
                    change: "",
                    will_commit: true,
                    fields: &values,
                },
            )
            .map_or_else(|_| value.clone(), |outcome| outcome.value)
        }
        None => value.clone(),
    };
    Some((value, display))
}
