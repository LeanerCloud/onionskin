//! PDF JavaScript for forms: the Acrobat forms API subset - field
//! calculation, validation and formatting - on a pure-Rust engine,
//! sandboxed with no I/O, no network and a fuel budget. Real AcroForms
//! compute, so a form whose scripts never run is filled wrong; a form
//! whose scripts fail gets a visible notice rather than silent wrong
//! values. Document-level and interactive JS beyond forms is out of scope.
//!
//! [`run`] runs one field script for one event. The API a script can
//! reach is written in JavaScript in `prelude.js`, on top of Boa's
//! language: `event`, `this.getField`, `app.alert`, `util.printf`,
//! `util.printd`, `util.scand` and the `AF` functions Acrobat's Format,
//! Keystroke, Validate and Calculate panels write. A name outside it is
//! [`ScriptError::Unsupported`].

use std::collections::BTreeMap;

use boa_engine::{Context, Source};

const PRELUDE: &str = include_str!("prelude.js");

/// Loop iterations a script may run: far past any form's arithmetic, short
/// of a hang.
const LOOP_LIMIT: u64 = 1_000_000;
const RECURSION_LIMIT: usize = 256;

/// Which event a script runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A key typed, or with `will_commit`, the value about to be kept.
    Keystroke,
    /// The value about to be shown.
    Format,
    /// The value about to be kept.
    Validate,
    /// The field's value computed from others.
    Calculate,
}

impl EventKind {
    fn name(self) -> &'static str {
        match self {
            Self::Keystroke => "Keystroke",
            Self::Format => "Format",
            Self::Validate => "Validate",
            Self::Calculate => "Calculate",
        }
    }
}

/// One event: the field it is for, and every field's value by name.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation<'a> {
    pub kind: EventKind,
    pub target: &'a str,
    pub value: &'a str,
    /// What a keystroke adds.
    pub change: &'a str,
    pub will_commit: bool,
    pub fields: &'a BTreeMap<String, String>,
}

/// What a script left.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// `event.value` after the script.
    pub value: String,
    /// `event.rc`: `false` rejects a keystroke or a value.
    pub accepted: bool,
    /// What `app.alert` said, in order.
    pub alerts: Vec<String>,
    /// Other fields the script set, by name.
    pub changed: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// The script used something the forms subset does not have.
    Unsupported(String),
    /// The script threw.
    Failed(String),
    /// The script ran out of its budget.
    Exhausted,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => {
                write!(f, "the script uses what Onionskin does not run: {what}")
            }
            Self::Failed(why) => write!(f, "the script failed: {why}"),
            Self::Exhausted => write!(f, "the script ran too long and was stopped"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// Runs `script` for `invocation`, in a fresh sandbox.
pub fn run(script: &str, invocation: &Invocation) -> Result<Outcome, ScriptError> {
    let mut context = Context::default();
    let limits = context.runtime_limits_mut();
    limits.set_loop_iteration_limit(LOOP_LIMIT);
    limits.set_recursion_limit(RECURSION_LIMIT);
    let input = serde_json::json!({
        "kind": invocation.kind.name(),
        "target": invocation.target,
        "value": invocation.value,
        "change": invocation.change,
        "willCommit": invocation.will_commit,
        "fields": invocation.fields,
    });
    let program = format!(
        "var __input = {input};\n{PRELUDE}\n(function () {{\n{script}\n}}).call(__doc);\n__output();"
    );
    let result = context
        .eval(Source::from_bytes(program.as_bytes()))
        .map_err(|error| classify(&error.to_string()))?;
    let text = result
        .to_string(&mut context)
        .map_err(|error| classify(&error.to_string()))?
        .to_std_string_escaped();
    parse(&text)
}

fn classify(message: &str) -> ScriptError {
    if message.contains("RuntimeLimitError") || message.contains("stack size") {
        return ScriptError::Exhausted;
    }
    let unknown = [
        "ReferenceError",
        "is not a function",
        "not a callable",
        "not defined",
    ];
    if unknown.iter().any(|sign| message.contains(sign)) {
        return ScriptError::Unsupported(message.to_owned());
    }
    ScriptError::Failed(message.to_owned())
}

fn parse(text: &str) -> Result<Outcome, ScriptError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| ScriptError::Failed(error.to_string()))?;
    let strings = |value: Option<&serde_json::Value>| -> Vec<String> {
        value
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(Outcome {
        value: value["value"].as_str().unwrap_or_default().to_owned(),
        accepted: value["rc"].as_bool().unwrap_or(true),
        alerts: strings(value.get("alerts")),
        changed: value["changed"]
            .as_object()
            .map(|changed| {
                changed
                    .iter()
                    .map(|(name, value)| {
                        (name.clone(), value.as_str().unwrap_or_default().to_owned())
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}
