//! Prepare Form's field Properties (M5): General, Appearance, Position,
//! Options, and for a text field or dropdown, Format, Validate and
//! Calculate.
//!
//! What the choices mean is plain data in [`form`], tested without a
//! window; the frame writes the field through `tools-form`.

mod form;
mod view;

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::forms::FieldProperties;
use onionskin_core::ObjRef;

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) use form::{FieldAction, FieldForm, FieldInput, Shape};
#[cfg(test)]
pub(in crate::shell) use form::{FormatKind, Tab};
pub(in crate::shell) use view::{accessible, render};

/// The dialog, open on one field as one of its widgets shows it.
pub(in crate::shell) struct FieldDialogState {
    pub(in crate::shell) field: ObjRef,
    pub(in crate::shell) widget: ObjRef,
    pub(in crate::shell) form: FieldForm,
    inputs: Vec<(FieldInput, Entity<SearchInput>)>,
    pub(in crate::shell) error: Option<String>,
}

impl FieldDialogState {
    pub(in crate::shell) fn new(
        (field, widget): (ObjRef, ObjRef),
        shape: Shape,
        properties: FieldProperties,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let (form, typed) = FieldForm::of(shape, properties);
        let inputs = FieldInput::ALL
            .into_iter()
            .map(|which| {
                let text = typed
                    .iter()
                    .find(|(input, _)| *input == which)
                    .map(|(_, text)| text.clone())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    let mut input =
                        SearchInput::with_placeholder(which.id(), which.label(), theme, cx);
                    input.set_query(text, cx);
                    input
                });
                (which, input)
            })
            .collect();
        Self {
            field,
            widget,
            form,
            inputs,
            error: None,
        }
    }

    fn input(&self, which: FieldInput) -> Option<&Entity<SearchInput>> {
        self.inputs
            .iter()
            .find(|(each, _)| *each == which)
            .map(|(_, input)| input)
    }

    /// A typed field, when the tab shown has it.
    pub(in crate::shell) fn text_field(&self, which: FieldInput) -> Option<&Entity<SearchInput>> {
        if !self.form.inputs().contains(&which) {
            return None;
        }
        self.input(which)
    }

    /// What is typed in `which`, whichever tab it is on.
    pub(in crate::shell) fn typed(&self, which: FieldInput, cx: &gpui::App) -> String {
        self.input(which)
            .map(|input| input.read(cx).query().to_owned())
            .unwrap_or_default()
    }

    /// Run a control that is not Save or Delete.
    pub(in crate::shell) fn apply(&mut self, action: FieldAction, cx: &mut Context<ShellFrame>) {
        let typed: Vec<(FieldInput, String)> = FieldInput::ALL
            .into_iter()
            .map(|which| (which, self.typed(which, cx)))
            .collect();
        let read = |which: FieldInput| lookup(&typed, which);
        self.form.apply(action, &read);
        if action == FieldAction::AddOption {
            for which in [FieldInput::OptionItem, FieldInput::OptionExport] {
                if let Some(input) = self.input(which) {
                    input.update(cx, |input, cx| input.set_query(String::new(), cx));
                }
            }
        }
    }

    /// The properties to write, or what is wrong.
    pub(in crate::shell) fn request(&self, cx: &gpui::App) -> Result<FieldProperties, String> {
        let typed: Vec<(FieldInput, String)> = FieldInput::ALL
            .into_iter()
            .map(|which| (which, self.typed(which, cx)))
            .collect();
        self.form.request(&|which| lookup(&typed, which))
    }
}

fn lookup(typed: &[(FieldInput, String)], which: FieldInput) -> String {
    typed
        .iter()
        .find(|(each, _)| *each == which)
        .map(|(_, text)| text.clone())
        .unwrap_or_default()
}

/// The dialog's text fields, for the focus ring.
pub(in crate::shell) fn text_fields() -> impl Iterator<Item = TextField> {
    FieldInput::ALL.into_iter().map(TextField::Field)
}

#[cfg(test)]
mod tests;
