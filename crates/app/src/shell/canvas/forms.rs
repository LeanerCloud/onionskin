//! Filling a form on the canvas: what a Hand tool click on a field does.
//!
//! A check box or radio button toggles, and a list box takes the row that
//! was clicked, straight away. A text field or a dropdown opens an editor
//! over the widget ([`FieldPrompt`]), and what is typed or picked is
//! committed through the form's own scripts, as Acrobat commits it. What the
//! scripts said with `app.alert`, what they could not run, and a value they
//! refused all become notices, so a form is never left computed wrong
//! without a word.

use onionskin_core::forms::{ChoiceOption, Field, FieldKind, FieldValue, Form, Widget};
use onionskin_core::{FieldRequest, ObjRef, PageIndex};
use onionskin_tools_form::fill::{clear_form, fill, toggle, FillOptions, Filled};

use super::{CanvasError, CanvasModel};
use crate::autocomplete::EntryList;

/// How the canvas fills: whether the form's scripts run, and what filling
/// had to say that the frame has not shown yet.
#[derive(Debug)]
pub(in crate::shell) struct FormFilling {
    scripts: bool,
    notices: Vec<String>,
    /// Auto-Complete's entries, when it is on.
    autocomplete: Option<EntryList>,
    /// Text typed into fields and kept, for Auto-Complete to remember.
    typed: Vec<String>,
}

impl Default for FormFilling {
    fn default() -> Self {
        FormFilling {
            scripts: true,
            notices: Vec::new(),
            autocomplete: None,
            typed: Vec::new(),
        }
    }
}

/// A field waiting for a value: where its editor goes, and what it offers.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPrompt {
    pub field: ObjRef,
    pub widget: ObjRef,
    pub page: PageIndex,
    /// The widget's rectangle on the page.
    pub rect: [f64; 4],
    pub name: String,
    pub entry: Entry,
}

/// What the editor offers.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A text field: its value to edit.
    Text {
        value: String,
        password: bool,
        max_len: Option<usize>,
    },
    /// A dropdown: its options, the one chosen by export value, and whether
    /// text may be typed instead.
    Choose {
        options: Vec<ChoiceOption>,
        chosen: Option<String>,
        editable: bool,
    },
}

impl Entry {
    /// Whether the editor has a text box.
    pub fn typed(&self) -> bool {
        matches!(
            self,
            Entry::Text { .. } | Entry::Choose { editable: true, .. }
        )
    }

    /// The text the editor's box starts with.
    pub fn initial_text(&self) -> String {
        match self {
            Entry::Text { value, .. } => value.clone(),
            Entry::Choose {
                options, chosen, ..
            } => chosen
                .as_deref()
                .map(|export| display_of(options, export))
                .unwrap_or_default(),
        }
    }

    /// What the typed `text` commits as.
    pub fn value_for(&self, text: &str) -> FieldValue {
        match self {
            Entry::Text { max_len, .. } => FieldValue::Text(match max_len {
                Some(most) => text.chars().take(*most).collect(),
                None => text.to_owned(),
            }),
            Entry::Choose { .. } if text.is_empty() => FieldValue::Chosen(Vec::new()),
            Entry::Choose { options, .. } => {
                let export = options
                    .iter()
                    .find(|option| option.display == text)
                    .map_or(text, |option| option.export.as_str());
                FieldValue::Chosen(vec![export.to_owned()])
            }
        }
    }
}

fn display_of(options: &[ChoiceOption], export: &str) -> String {
    options
        .iter()
        .find(|option| option.export == export)
        .map_or_else(|| export.to_owned(), |option| option.display.clone())
}

/// The editor for `widget` of `field`, when its kind is filled in one.
fn prompt(field: &Field, widget: &Widget) -> Option<FieldPrompt> {
    let entry = match &field.kind {
        FieldKind::Text {
            password, max_len, ..
        } => Entry::Text {
            value: field.value.as_text(),
            password: *password,
            max_len: *max_len,
        },
        FieldKind::Choice {
            combo: true,
            editable,
            options,
            ..
        } => Entry::Choose {
            options: options.clone(),
            chosen: match &field.value {
                FieldValue::Chosen(chosen) => chosen.first().cloned(),
                FieldValue::Text(text) => Some(text.clone()),
                _ => None,
            },
            editable: *editable,
        },
        _ => return None,
    };
    Some(FieldPrompt {
        field: field.objref,
        widget: widget.objref,
        page: widget.page?,
        rect: widget.rect,
        name: field.name.clone(),
        entry,
    })
}

/// Why a click on `field` fills nothing, when it does not.
fn refusal(field: &Field) -> Option<String> {
    let name = &field.name;
    match field.kind {
        FieldKind::PushButton => Some(format!(
            "{name} is a button, and Onionskin does not run button actions"
        )),
        FieldKind::Signature => Some(format!(
            "{name} is a signature field, and signing with a digital ID is not available yet"
        )),
        _ if field.flags.read_only => Some(format!("{name} is read-only")),
        _ => None,
    }
}

/// The list box value after clicking option `row`: that option alone, or
/// for a multiple choice, the chosen ones with that option turned over.
fn list_value(field: &Field, row: usize) -> Option<FieldValue> {
    let FieldKind::Choice {
        options,
        multi_select,
        ..
    } = &field.kind
    else {
        return None;
    };
    let export = options.get(row)?.export.clone();
    if !multi_select {
        return Some(FieldValue::Chosen(vec![export]));
    }
    let mut chosen = match &field.value {
        FieldValue::Chosen(chosen) => chosen.clone(),
        _ => Vec::new(),
    };
    if let Some(at) = chosen.iter().position(|each| *each == export) {
        chosen.remove(at);
    } else {
        chosen.push(export);
    }
    Some(FieldValue::Chosen(chosen))
}

impl CanvasModel {
    /// Whether the form's scripts run: Preferences > JavaScript.
    pub fn set_form_scripts(&mut self, on: bool) {
        self.forms.scripts = on;
    }

    /// Auto-Complete's entries, or `None` when it is off.
    pub fn set_autocomplete(&mut self, entries: Option<EntryList>) {
        self.forms.autocomplete = entries;
    }

    /// What Auto-Complete offers for `typed` in a field `entry`: nothing for
    /// a password, or when it is off.
    pub fn suggestions(&self, entry: &Entry, typed: &str) -> Vec<String> {
        match (&self.forms.autocomplete, entry) {
            (
                Some(list),
                Entry::Text {
                    password: false, ..
                },
            ) => list.suggest(typed),
            _ => Vec::new(),
        }
    }

    /// Text typed into fields and kept since the frame last asked.
    pub fn take_typed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.forms.typed)
    }

    /// Commit what was typed into `prompt`'s editor. A text field's value,
    /// kept, is remembered for Auto-Complete unless it is a password.
    pub fn commit_typed(&mut self, prompt: &FieldPrompt, typed: &str) -> Result<bool, CanvasError> {
        let accepted = self.commit_field(prompt.field, prompt.entry.value_for(typed))?;
        if accepted
            && matches!(
                prompt.entry,
                Entry::Text {
                    password: false,
                    ..
                }
            )
        {
            self.forms.typed.push(typed.to_owned());
        }
        Ok(accepted)
    }

    /// What filling had to say since the frame last asked.
    pub fn take_form_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.forms.notices)
    }

    /// Answer the field a tool clicked, if it clicked one: toggle, choose,
    /// or the editor to open.
    pub fn answer_field_request(&mut self) -> Result<Option<FieldPrompt>, CanvasError> {
        let request = self.document_mut().take_field_request();
        let Some(request) = request else {
            return Ok(None);
        };
        self.answer(request)
    }

    fn answer(&mut self, request: FieldRequest) -> Result<Option<FieldPrompt>, CanvasError> {
        let form = self.document_mut().form()?;
        let Some(field) = form.field_by_ref(request.field) else {
            return Ok(None);
        };
        let Some(widget) = field
            .widgets
            .iter()
            .find(|widget| widget.objref == request.widget)
        else {
            return Ok(None);
        };
        if let Some(notice) = refusal(field) {
            self.forms.notices.push(notice);
            return Ok(None);
        }
        match &field.kind {
            FieldKind::CheckBox | FieldKind::Radio { .. } => {
                let options = self.fill_options();
                let filled = toggle(
                    &mut self.document_mut(),
                    field.objref,
                    widget.objref,
                    options,
                )
                .map_err(CanvasError::Command)?;
                self.report(&field.name, filled);
                Ok(None)
            }
            FieldKind::Choice { combo: false, .. } => {
                let value = field
                    .list_row(widget, request.point)
                    .and_then(|row| list_value(field, row));
                if let Some(value) = value {
                    self.commit_field(field.objref, value)?;
                }
                Ok(None)
            }
            _ => Ok(prompt(field, widget)),
        }
    }

    fn fill_options(&self) -> FillOptions {
        FillOptions {
            scripts: self.forms.scripts,
        }
    }

    /// Commit `value` to `field` through its scripts. `true` when it was
    /// kept; a refused value leaves the field as it was, with a notice.
    pub fn commit_field(&mut self, field: ObjRef, value: FieldValue) -> Result<bool, CanvasError> {
        let options = self.fill_options();
        let filled =
            fill(&mut self.document_mut(), field, value, options).map_err(CanvasError::Command)?;
        let accepted = filled.accepted;
        let name = self
            .document_mut()
            .form()
            .ok()
            .and_then(|form| form.field_by_ref(field).map(|field| field.name.clone()))
            .unwrap_or_default();
        self.report(&name, filled);
        Ok(accepted)
    }

    fn report(&mut self, name: &str, filled: Filled) {
        let said = !filled.alerts.is_empty();
        self.forms.notices.extend(filled.alerts);
        self.forms.notices.extend(
            filled
                .problems
                .into_iter()
                .map(|problem| format!("A form script did not run, for {problem}")),
        );
        if !filled.accepted && !said {
            self.forms
                .notices
                .push(format!("{name} did not take that value"));
        }
    }

    /// The editor for the next field in tab order after `widget`, or the
    /// one before it: text fields and dropdowns that can be filled, going
    /// round from the last to the first.
    pub fn field_prompt_after(
        &mut self,
        widget: ObjRef,
        backwards: bool,
    ) -> Result<Option<FieldPrompt>, CanvasError> {
        let form = self.document_mut().form()?;
        Ok(neighbour(&form, widget, backwards))
    }

    /// Edit > Clear Form: every field back to its default, as one step.
    pub fn clear_form(&mut self) -> Result<usize, CanvasError> {
        clear_form(&mut self.document_mut()).map_err(CanvasError::Command)
    }

    /// Whether the document has a form to clear.
    pub fn has_form(&self) -> bool {
        self.document_mut()
            .form()
            .is_ok_and(|form| !form.fields.is_empty())
    }
}

/// The prompt for the fillable widget after (or before) `widget` in tab
/// order.
fn neighbour(form: &Form, widget: ObjRef, backwards: bool) -> Option<FieldPrompt> {
    let mut order = form.tab_order();
    if backwards {
        order.reverse();
    }
    let at = order
        .iter()
        .position(|&(field, index)| form.fields[field].widgets[index].objref == widget)?;
    order
        .iter()
        .cycle()
        .skip(at + 1)
        .take(order.len() - 1)
        .find_map(|&(field, index)| {
            let field = &form.fields[field];
            if refusal(field).is_some() {
                return None;
            }
            prompt(field, &field.widgets[index])
        })
}

#[cfg(test)]
mod tests;
