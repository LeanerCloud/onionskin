//! The frame's half of forms: Clear Form, the notices filling leaves on
//! the canvas, which go on the notice bar, and a field's Properties.

use gpui::Context;

use super::ShellFrame;

/// What the form entries say in a build that does not fill forms.
pub(in crate::shell) const NO_FORMS: &str = "The Forms plugin is not installed";

impl ShellFrame {
    /// Forms Auto-Complete's remembered entries, most recent first.
    pub(in crate::shell) fn autocomplete_entries(&self) -> &[String] {
        self.settings.autocomplete.entries()
    }

    /// Preferences > Forms' Remove and Clear All, written at once. The file
    /// is read again first, so an entry another window remembered is not
    /// lost.
    pub(in crate::shell) fn change_entries(
        &mut self,
        change: crate::shell::preferences_dialog::PreferenceChange,
        cx: &mut Context<Self>,
    ) {
        use crate::shell::preferences_dialog::PreferenceChange;
        let shown = self.settings.autocomplete.clone();
        self.edit_entries(cx, |list| match change {
            PreferenceChange::ForgetEntry(index) => {
                // By the entry shown, which the file may hold elsewhere.
                if let Some(entry) = shown.entries().get(index) {
                    let at = list.entries().iter().position(|each| each == entry);
                    at.is_some_and(|at| list.forget(at))
                } else {
                    false
                }
            }
            PreferenceChange::ClearEntries => {
                list.clear();
                true
            }
            _ => false,
        });
    }

    /// Change the entry list as it is on disk, save it, and hand every tab
    /// the new list.
    pub(in crate::shell) fn edit_entries(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::autocomplete::EntryList) -> bool,
    ) {
        let path = self.settings.paths.autocomplete.clone();
        let mut list = match path.as_deref() {
            Some(path) => {
                let (list, error) = crate::autocomplete::EntryList::load(Some(path));
                self.notices.extend(error);
                list
            }
            None => self.settings.autocomplete.clone(),
        };
        if change(&mut list) {
            if let Some(path) = path.as_deref() {
                if let Err(error) = list.save(path) {
                    self.notices.push(error);
                }
            }
        }
        self.settings.autocomplete = list;
        self.apply_tool_environment(cx);
        cx.notify();
    }
}

#[cfg(not(feature = "tools-form"))]
impl ShellFrame {
    /// Disabled without the plugin, saying so; reached some other way, it
    /// says so too.
    pub(super) fn run_clear_form(&mut self, cx: &mut Context<Self>) {
        self.notices.push(NO_FORMS.to_owned());
        cx.notify();
    }
}

#[cfg(feature = "tools-form")]
impl ShellFrame {
    /// Clear Form: every field back to its default, as one undoable step.
    pub(super) fn run_clear_form(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let result = canvas.update(cx, |canvas, cx| {
            let result = canvas.model.clear_form();
            canvas.handle_change(Ok(true), cx);
            result
        });
        match result {
            Ok(0) => self
                .notices
                .push("This document has no form fields to clear".to_owned()),
            Ok(_) => {}
            Err(error) => self
                .notices
                .push(super::properties::sentence(&error.to_string())),
        }
        cx.notify();
    }

    /// A screen reader picked Auto-Complete suggestion `index`.
    pub(super) fn pick_form_suggestion(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(canvas) = self.active_canvas().cloned() {
            canvas.update(cx, |canvas, cx| canvas.pick_suggestion(index, cx));
        }
    }

    /// A screen reader picked option `index` of the dropdown being filled.
    pub(super) fn choose_form_option(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(canvas) = self.active_canvas().cloned() {
            canvas.update(cx, |canvas, cx| canvas.choose_field_option_at(index, cx));
        }
    }

    /// Take what filling a field had to say: the scripts' alerts, scripts
    /// that did not run, values refused; and what was typed, for
    /// Auto-Complete to remember.
    pub(super) fn collect_form_notices(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let (notices, typed) = canvas.update(cx, |canvas, _| {
            (canvas.model.take_form_notices(), canvas.model.take_typed())
        });
        if !notices.is_empty() {
            self.notices.extend(notices);
            cx.notify();
        }
        let preferences = &self.settings.preferences;
        if preferences.autocomplete && !typed.is_empty() {
            let numbers = preferences.autocomplete_numbers;
            self.edit_entries(cx, |list| {
                // Every one remembered: `any` would stop at the first.
                let mut changed = false;
                for text in &typed {
                    changed |= list.remember(text, numbers);
                }
                changed
            });
        }
    }
}

#[cfg(feature = "tools-form")]
mod properties {
    use gpui::{Context, Window};
    use onionskin_core::forms::FieldKind;
    use onionskin_core::FieldRequest;
    use onionskin_tools_form::prepare;

    use super::super::properties::sentence;
    use super::ShellFrame;
    use crate::shell::chrome::field_dialog::{FieldAction, FieldDialogState, Shape};
    use crate::shell::dialog::ShellDialog;

    /// Which dialog a field of `kind` gets.
    fn shape(kind: &FieldKind) -> Shape {
        match kind {
            FieldKind::Text { .. } => Shape::Text,
            FieldKind::CheckBox => Shape::CheckBox,
            FieldKind::Radio { .. } => Shape::Radio,
            FieldKind::Choice { combo: true, .. } => Shape::Dropdown,
            FieldKind::Choice { .. } => Shape::ListBox,
            FieldKind::PushButton => Shape::Button,
            FieldKind::Signature => Shape::Signature,
        }
    }

    impl ShellFrame {
        pub(in crate::shell) fn field_dialog(&self) -> Option<&FieldDialogState> {
            self.field_dialog.as_ref()
        }

        /// Take a Prepare Form tool's ask for a field's Properties, for the
        /// next render.
        pub(in crate::shell::chrome::tabs) fn collect_field_request(
            &mut self,
            cx: &mut Context<Self>,
        ) {
            let Some(canvas) = self.active_canvas().cloned() else {
                return;
            };
            let request = canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().take_field_properties_request()
            });
            if let Some(request) = request {
                self.pending_field = Some(request);
                cx.notify();
            }
        }

        /// Open what was asked for. Called from render.
        pub(in crate::shell::chrome::tabs) fn run_pending_field(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            if self.dialog.is_some() {
                return;
            }
            if let Some(request) = self.pending_field.take() {
                self.open_field_dialog(request, window, cx);
            }
        }

        /// Properties for `request`'s field, as its widget shows it.
        pub(in crate::shell) fn open_field_dialog(
            &mut self,
            request: FieldRequest,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let Some(canvas) = self.active_canvas().cloned() else {
                return;
            };
            let read = canvas.update(cx, |canvas, _| {
                let mut doc = canvas.model.document_mut();
                let form = doc.form().map_err(|error| error.to_string())?;
                let kind = form
                    .field_by_ref(request.field)
                    .map(|field| field.kind.clone())
                    .ok_or_else(|| "that field is not in the form any more".to_owned())?;
                prepare::properties(&mut doc, request.field, request.widget)
                    .map(|properties| (shape(&kind), properties))
                    .map_err(|error| error.to_string())
            });
            let (shape, properties) = match read {
                Ok(read) => read,
                Err(error) => {
                    self.notices.push(sentence(&error));
                    cx.notify();
                    return;
                }
            };
            self.show_dialog(ShellDialog::FieldProperties(shape), window, cx);
            let theme = self.shell_view_state.tokens();
            self.field_dialog = Some(FieldDialogState::new(
                (request.field, request.widget),
                shape,
                properties,
                theme,
                cx,
            ));
        }

        pub(in crate::shell) fn run_field_action(
            &mut self,
            action: FieldAction,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let Some(state) = self.field_dialog.as_mut() else {
                return;
            };
            state.error = None;
            let (field, widget) = (state.field, state.widget);
            match action {
                FieldAction::Submit => match state.request(cx) {
                    Ok(properties) => self.write_field(
                        move |doc| prepare::set_properties(doc, field, widget, &properties),
                        window,
                        cx,
                    ),
                    Err(error) => state.error = Some(error),
                },
                FieldAction::Delete => {
                    self.write_field(move |doc| prepare::delete_field(doc, field), window, cx);
                }
                _ => state.apply(action, cx),
            }
            cx.notify();
        }

        /// Run a field edit, closing the dialog on success and keeping the
        /// error in it otherwise.
        fn write_field(
            &mut self,
            edit: impl FnOnce(
                &mut onionskin_core::Document,
            ) -> Result<(), onionskin_plugin_api::CommandError>,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let Some(canvas) = self.active_canvas().cloned() else {
                return;
            };
            let outcome = canvas.update(cx, |canvas, cx| {
                let outcome = edit(&mut canvas.model.document_mut());
                canvas.handle_change(Ok(true), cx);
                outcome
            });
            match outcome {
                Ok(()) => self.close_dialog(window, cx),
                Err(error) => {
                    if let Some(state) = self.field_dialog.as_mut() {
                        state.error = Some(sentence(&error.to_string()));
                    }
                }
            }
        }
    }
}
