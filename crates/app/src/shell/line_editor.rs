//! Editing a line of text where it is: the editor the Edit Text tool opens
//! over the line it was clicked on, holding what the line says.
//!
//! Enter keeps what was typed, Escape leaves the line as it was, and a
//! press anywhere else on the canvas keeps it, as leaving the line in
//! Acrobat does. A change the document refuses (a character no font can
//! draw, a line that changed meanwhile) is said on the canvas's status
//! line, and the line stays as it was.

use gpui::{
    actions, div, px, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, ParentElement as _, Styled as _, Window,
};

use onionskin_core::{TextEditRequest, ViewRect, ViewSize};

use super::chrome::accessible::{Element as A11yElement, Rects, TextField};
use super::chrome::{SearchInput, ThemeTokens};
use super::Canvas;

actions!(onionskin_line_editor, [CommitLine, CancelLine]);

const LINE_KEY_CONTEXT: &str = "OnionskinLineEditor";

/// The id the editor's text box publishes.
pub(super) const LINE_EDITOR_ID: &str = "line-editor";

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("enter", CommitLine, Some(LINE_KEY_CONTEXT)),
        KeyBinding::new("escape", CancelLine, Some(LINE_KEY_CONTEXT)),
    ]);
}

/// The open editor: the line it edits and its text box.
pub(super) struct LineEditor {
    pub(super) request: TextEditRequest,
    pub(super) input: Entity<SearchInput>,
}

impl Canvas {
    /// Open the editor on the line a tool asked for, if one did.
    pub(super) fn answer_text_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.model.document_mut().take_text_edit_request() else {
            return;
        };
        let theme = self.theme;
        let text = request.text.clone();
        let input = cx.new(|cx| {
            let mut input =
                SearchInput::with_placeholder(LINE_EDITOR_ID, "Line of text", theme, cx);
            input.set_query(text, cx);
            input
        });
        window.focus(&input.read(cx).focus_handle(cx));
        self.line_editor = Some(LineEditor { request, input });
        cx.notify();
    }

    /// Write what the editor holds into the line, and close it.
    pub(super) fn commit_line_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.line_editor.take() else {
            return;
        };
        let typed = editor.input.read(cx).query().to_owned();
        let result = self.model.edit_text_line(&editor.request, &typed);
        self.handle_change(result, cx);
    }

    /// Leave the line as it was.
    pub(super) fn cancel_line_editor(&mut self, cx: &mut Context<Self>) {
        if self.line_editor.take().is_some() {
            cx.notify();
        }
    }

    /// The editor's text box, while one is open.
    pub(in crate::shell) fn line_editor_input(&self) -> Option<Entity<SearchInput>> {
        Some(self.line_editor.as_ref()?.input.clone())
    }

    /// What the editor tells a screen reader: a text box over the line.
    pub(super) fn line_editor_node(&self, scale: f32, cx: &gpui::App) -> Option<A11yElement> {
        let editor = self.line_editor.as_ref()?;
        let mut node = editor
            .input
            .read(cx)
            .accessible("Line of text", TextField::LineText);
        node.bounds = self
            .model
            .view_rect(editor.request.page, editor.request.bounds)
            .map(|(origin, width, height)| {
                let rect = ViewRect {
                    origin,
                    size: ViewSize { width, height },
                };
                Rects::view_rect(rect, self.model.canvas_origin(), scale)
            });
        Some(node)
    }

    /// The editor, over the line in canvas coordinates.
    pub(super) fn render_line_editor(
        &self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let editor = self.line_editor.as_ref()?;
        let (at, width, height) = self
            .model
            .view_rect(editor.request.page, editor.request.bounds)?;
        Some(
            div()
                .key_context(LINE_KEY_CONTEXT)
                .on_action(cx.listener(|canvas, _: &CommitLine, _window, cx| {
                    canvas.commit_line_editor(cx);
                }))
                .on_action(cx.listener(|canvas, _: &CancelLine, _window, cx| {
                    canvas.cancel_line_editor(cx);
                }))
                // A press inside places the cursor; it is not the press
                // elsewhere that keeps the change.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .absolute()
                .left(px(at.x))
                .top(px(at.y))
                .w(px(width.max(160.0)))
                .min_h(px(height))
                .p_1()
                .rounded_sm()
                .bg(theme.raised)
                .border_1()
                .border_color(theme.selected)
                .child(editor.input.clone()),
        )
    }
}
