//! Filling a form field where it is, as Acrobat does with the Hand tool: a
//! text box over a text field, the options under a dropdown.
//!
//! The canvas opens the editor when a click on a field asks for one (see
//! `canvas::forms`). Enter commits, Escape leaves the field as it was, and
//! Tab or Shift-Tab commits and moves to the next or previous field. A
//! press anywhere else on the canvas commits, as leaving a field in
//! Acrobat does. A value the form's scripts refuse keeps the editor open on
//! what was typed, with the scripts' alert on the notice bar.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    actions, div, px, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyBinding, MouseButton, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window,
};

use accesskit::Role;
use onionskin_core::{ViewRect, ViewSize};

use super::canvas::{Entry, FieldPrompt};
use super::chrome::accessible::{Activation, Element as A11yElement, Rects, TextField};
use super::chrome::{SearchInput, ThemeTokens};
use super::Canvas;
use crate::a11y::State;

actions!(
    onionskin_field_editor,
    [CommitField, CancelField, NextField, PreviousField]
);

/// The key context the editor's keys are bound in, more specific than the
/// shell's, so Tab moves between fields rather than around the chrome.
const FIELD_KEY_CONTEXT: &str = "OnionskinFieldEditor";

/// The id the editor's text box publishes.
pub(super) const FIELD_EDITOR_ID: &str = "form-field-editor";

fn kind_label(entry: &Entry) -> &'static str {
    match entry {
        Entry::Text { password: true, .. } => "password",
        Entry::Text { .. } => "text field",
        Entry::Choose { .. } => "dropdown",
        Entry::Image => "image field",
    }
}

/// The tallest a dropdown's list of options is drawn, in rows.
const MAX_ROWS: usize = 8;
const ROW_HEIGHT: f32 = 22.0;

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("enter", CommitField, Some(FIELD_KEY_CONTEXT)),
        KeyBinding::new("escape", CancelField, Some(FIELD_KEY_CONTEXT)),
        KeyBinding::new("tab", NextField, Some(FIELD_KEY_CONTEXT)),
        KeyBinding::new("shift-tab", PreviousField, Some(FIELD_KEY_CONTEXT)),
    ]);
}

/// The open editor: the field it fills, and its text box when it has one.
pub(super) struct FieldEditor {
    pub(super) prompt: FieldPrompt,
    pub(super) input: Option<Entity<SearchInput>>,
    focus: FocusHandle,
    /// Draws the editor again as its text changes, for Auto-Complete's
    /// suggestions to follow the typing.
    _typing: Option<gpui::Subscription>,
}

impl Canvas {
    /// Answer a click a tool made on a field: toggle or choose it straight
    /// away, or open its editor.
    pub(super) fn answer_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.model.answer_field_request() {
            Ok(Some(prompt)) if prompt.entry == Entry::Image => self.prompt_for_image(prompt, cx),
            Ok(Some(prompt)) => self.open_field_editor(prompt, window, cx),
            Ok(None) => {}
            Err(error) => self.record_error(error, cx),
        }
        self.handle_change(Ok(true), cx);
    }

    fn open_field_editor(
        &mut self,
        prompt: FieldPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        let input = prompt.entry.typed().then(|| {
            let theme = self.theme;
            let name = prompt.name.clone();
            let initial = prompt.entry.initial_text();
            let masked = matches!(prompt.entry, Entry::Text { password: true, .. });
            cx.new(|cx| {
                let mut input = SearchInput::with_placeholder(FIELD_EDITOR_ID, name, theme, cx);
                input.set_query(initial, cx);
                input.set_masked(masked);
                input
            })
        });
        match &input {
            Some(input) => window.focus(&input.read(cx).focus_handle(cx)),
            None => window.focus(&focus),
        }
        let typing = input
            .as_ref()
            .map(|input| cx.observe(input, |_, _, cx| cx.notify()));
        self.field_editor = Some(FieldEditor {
            prompt,
            input,
            focus,
            _typing: typing,
        });
        cx.notify();
    }

    /// Ask for the image an image field shows, as Acrobat does on its
    /// click.
    fn prompt_for_image(&mut self, prompt: FieldPrompt, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Image".into()),
        });
        cx.spawn(async move |canvas, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            canvas
                .update(cx, |canvas, cx| {
                    canvas.take_image_file(&prompt, paths.into_iter().next(), cx);
                })
                .ok();
        })
        .detach();
    }

    /// What the image prompt chose, shown on the field.
    pub(in crate::shell) fn take_image_file(
        &mut self,
        prompt: &FieldPrompt,
        path: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = path else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                self.model.set_field_image(prompt, bytes);
            }
            Err(error) => {
                self.record_error(format!("{} could not be read: {error}", path.display()), cx)
            }
        }
        self.handle_change(Ok(true), cx);
    }

    /// Commit what the editor holds. `true` when the field took it, or
    /// there was nothing to commit; a refused value leaves the editor open.
    pub(super) fn commit_field_editor(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.field_editor.take() else {
            return true;
        };
        let Some(input) = &editor.input else {
            cx.notify();
            return true;
        };
        let typed = input.read(cx).query().to_owned();
        let result = self.model.commit_typed(&editor.prompt, &typed);
        let accepted = matches!(result, Ok(true));
        if matches!(result, Ok(false)) {
            self.field_editor = Some(editor);
        }
        self.handle_change(result.map(|_| true), cx);
        accepted
    }

    /// Leave the field as it was.
    pub(super) fn cancel_field_editor(&mut self, cx: &mut Context<Self>) {
        if self.field_editor.take().is_some() {
            cx.notify();
        }
    }

    /// Commit, then open the next field in tab order, or the one before.
    pub(super) fn move_field_editor(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(widget) = self
            .field_editor
            .as_ref()
            .map(|editor| editor.prompt.widget)
        else {
            return;
        };
        if !self.commit_field_editor(cx) {
            return;
        }
        match self.model.field_prompt_after(widget, backwards) {
            Ok(Some(prompt)) => self.open_field_editor(prompt, window, cx),
            Ok(None) => {}
            Err(error) => self.record_error(error, cx),
        }
    }

    /// A dropdown's option picked: it is the value, and the editor closes.
    pub(super) fn choose_field_option(&mut self, export: String, cx: &mut Context<Self>) {
        let Some(editor) = self.field_editor.take() else {
            return;
        };
        let value = onionskin_core::forms::FieldValue::Chosen(vec![export]);
        let result = self.model.commit_field(editor.prompt.field, value);
        self.handle_change(result.map(|_| true), cx);
    }

    /// The editor's text box, while one is open.
    pub(in crate::shell) fn field_editor_input(&self) -> Option<Entity<SearchInput>> {
        self.field_editor.as_ref()?.input.clone()
    }

    /// What Auto-Complete offers for the text the editor holds.
    pub(in crate::shell) fn field_suggestions(&self, cx: &App) -> Vec<String> {
        let Some(editor) = self.field_editor.as_ref() else {
            return Vec::new();
        };
        let Some(input) = editor.input.as_ref() else {
            return Vec::new();
        };
        self.model
            .suggestions(&editor.prompt.entry, input.read(cx).query())
    }

    /// Put suggestion `index` in the editor, to be committed as typed text
    /// is.
    pub(in crate::shell) fn pick_suggestion(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(chosen) = self.field_suggestions(cx).into_iter().nth(index) else {
            return;
        };
        if let Some(input) = self
            .field_editor
            .as_ref()
            .and_then(|editor| editor.input.clone())
        {
            input.update(cx, |input, cx| input.set_query(chosen, cx));
        }
        cx.notify();
    }

    /// Option `index` of the open dropdown picked, as a screen reader
    /// activates it.
    pub(in crate::shell) fn choose_field_option_at(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let export = self
            .field_editor
            .as_ref()
            .and_then(|editor| match &editor.prompt.entry {
                Entry::Choose { options, .. } => {
                    options.get(index).map(|option| option.export.clone())
                }
                Entry::Text { .. } | Entry::Image => None,
            });
        if let Some(export) = export {
            self.choose_field_option(export, cx);
        }
    }

    /// What the editor tells a screen reader: the field by name, what is
    /// typed in it, and a dropdown's options, each of which picks itself.
    pub(super) fn field_editor_node(&self, scale: f32, cx: &App) -> Option<A11yElement> {
        let editor = self.field_editor.as_ref()?;
        let prompt = &editor.prompt;
        let mut node = match &editor.input {
            Some(input) => input
                .read(cx)
                .accessible("Form field", TextField::FormField),
            None => A11yElement::new(FIELD_EDITOR_ID, Role::ComboBox, "Form field")
                .with_value(prompt.entry.initial_text()),
        };
        node.label = format!("{} ({})", prompt.name, kind_label(&prompt.entry));
        node.bounds =
            self.model
                .view_rect(prompt.page, prompt.rect)
                .map(|(origin, width, height)| {
                    let rect = ViewRect {
                        origin,
                        size: ViewSize { width, height },
                    };
                    Rects::view_rect(rect, self.model.canvas_origin(), scale)
                });
        for (index, suggestion) in self.field_suggestions(cx).into_iter().enumerate() {
            node = node.child(
                A11yElement::new(
                    ("form-field-suggestion", index),
                    Role::ListBoxOption,
                    suggestion,
                )
                .with_description("Auto-Complete")
                .with_activation(Activation::FormSuggestion(index)),
            );
        }
        if let Entry::Choose {
            options, chosen, ..
        } = &prompt.entry
        {
            for (index, option) in options.iter().enumerate() {
                let picked = chosen.as_deref() == Some(option.export.as_str());
                node = node.child(
                    A11yElement::new(
                        ("form-field-option", index),
                        Role::ListBoxOption,
                        option.display.clone(),
                    )
                    .with_state(State::selected(picked))
                    .with_activation(Activation::FormOption(index)),
                );
            }
        }
        Some(node)
    }

    /// The editor, over the field's widget in canvas coordinates.
    pub(super) fn render_field_editor(
        &self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let editor = self.field_editor.as_ref()?;
        let (at, width, height) = self
            .model
            .view_rect(editor.prompt.page, editor.prompt.rect)?;
        let mut body = div()
            .id("form-field-editor-frame")
            .key_context(FIELD_KEY_CONTEXT)
            .track_focus(&editor.focus)
            .on_action(cx.listener(|canvas, _: &CommitField, _window, cx| {
                canvas.commit_field_editor(cx);
            }))
            .on_action(cx.listener(|canvas, _: &CancelField, _window, cx| {
                canvas.cancel_field_editor(cx);
            }))
            .on_action(cx.listener(|canvas, _: &NextField, window, cx| {
                canvas.move_field_editor(false, window, cx);
            }))
            .on_action(cx.listener(|canvas, _: &PreviousField, window, cx| {
                canvas.move_field_editor(true, window, cx);
            }))
            // A press inside the editor places the cursor; it is not the
            // press elsewhere that commits.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .left(px(at.x))
            .top(px(at.y))
            .w(px(width.max(120.0)))
            .flex()
            .flex_col()
            .rounded_sm()
            .bg(theme.raised)
            .border_1()
            .border_color(theme.selected);
        match &editor.input {
            Some(input) => {
                body = body.child(div().min_h(px(height)).p_1().child(input.clone()));
            }
            None => {
                body = body.child(
                    div()
                        .min_h(px(height))
                        .p_1()
                        .child(editor.prompt.entry.initial_text()),
                );
            }
        }
        let suggestions = self.field_suggestions(cx);
        if !suggestions.is_empty() {
            let mut list = div().id("form-field-suggestions").flex().flex_col();
            for (index, suggestion) in suggestions.into_iter().enumerate() {
                list = list.child(
                    div()
                        .id(("form-field-suggestion", index))
                        .h(px(ROW_HEIGHT))
                        .px_2()
                        .text_color(theme.secondary_text)
                        .hover(|row| row.bg(theme.hover))
                        .child(suggestion)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |canvas, _, _window, cx| {
                                cx.stop_propagation();
                                canvas.pick_suggestion(index, cx);
                            }),
                        ),
                );
            }
            body = body.child(list);
        }
        if let Entry::Choose {
            options, chosen, ..
        } = &editor.prompt.entry
        {
            let mut list = div()
                .id("form-field-options")
                .flex()
                .flex_col()
                .max_h(px(ROW_HEIGHT * MAX_ROWS as f32))
                .overflow_y_scroll();
            for (index, option) in options.iter().enumerate() {
                let export = option.export.clone();
                let picked = chosen.as_deref() == Some(option.export.as_str());
                let row = div()
                    .id(("form-field-option", index))
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .when(picked, |row| row.bg(theme.selected))
                    .hover(|row| row.bg(theme.hover))
                    .child(option.display.clone())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |canvas, _, _window, cx| {
                            cx.stop_propagation();
                            canvas.choose_field_option(export.clone(), cx);
                        }),
                    );
                list = list.child(row);
            }
            body = body.child(list);
        }
        Some(body)
    }
}
