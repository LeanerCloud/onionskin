//! Typing a comment where it is, the way Acrobat does: a pop-up beside a
//! sticky note, the cursor in a text box.
//!
//! When a text tool places a comment, the canvas opens a text field on it and
//! focuses it. Enter or Escape finishes, and so does pressing anywhere else on
//! the canvas; what was typed becomes the comment's text as one undoable
//! step. The field is single-line: a text box's own newlines are typed in the
//! Comments pane.

use gpui::{
    actions, div, px, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Styled as _, Window,
};
use onionskin_core::ObjRef;

use super::chrome::{SearchInput, ThemeTokens};
use super::Canvas;

actions!(onionskin_inline_text, [FinishInlineText]);

/// The key context the field's own keys are bound in, more specific than the
/// shell's, so Enter finishes the comment instead of activating the focus
/// ring.
const INLINE_KEY_CONTEXT: &str = "OnionskinInlineText";

/// The id the field publishes.
pub(super) const INLINE_TEXT_ID: &str = "inline-comment-text";

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("enter", FinishInlineText, Some(INLINE_KEY_CONTEXT)),
        KeyBinding::new("escape", FinishInlineText, Some(INLINE_KEY_CONTEXT)),
    ]);
}

/// The open field, and the comment it writes into.
pub(super) struct InlineText {
    pub(super) annotation: ObjRef,
    pub(super) input: Entity<SearchInput>,
}

impl Canvas {
    /// Open the field when a text tool has just placed a comment, and focus
    /// it so typing goes straight in.
    pub(super) fn sync_inline_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.model.text_target().map(|target| target.annotation);
        match (target, self.inline.as_ref().map(|open| open.annotation)) {
            (Some(wanted), Some(open)) if wanted == open => {}
            (Some(wanted), _) => {
                let theme = self.theme;
                let input = cx.new(|cx| {
                    SearchInput::with_placeholder(INLINE_TEXT_ID, "Type your comment", theme, cx)
                });
                window.focus(&input.read(cx).focus_handle(cx));
                self.inline = Some(InlineText {
                    annotation: wanted,
                    input,
                });
            }
            (None, Some(_)) => self.inline = None,
            (None, None) => {}
        }
    }

    /// Write what was typed into the comment and close the field.
    pub(super) fn finish_inline_text(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.inline.take() else {
            return;
        };
        let text = open.input.read(cx).query().to_owned();
        let result = self.model.finish_text(&text);
        self.handle_change(result, cx);
    }

    /// The field, placed over the comment in canvas coordinates.
    pub(super) fn render_inline_text(
        &self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let open = self.inline.as_ref()?;
        let (at, width, height) = self.model.text_target_rect()?;
        Some(
            div()
                .key_context(INLINE_KEY_CONTEXT)
                .on_action(cx.listener(|canvas, _: &FinishInlineText, _window, cx| {
                    canvas.finish_inline_text(cx);
                }))
                .absolute()
                .left(px(at.x))
                .top(px(at.y))
                .w(px(width))
                .min_h(px(height))
                .p_1()
                .rounded_sm()
                .bg(theme.raised)
                .border_1()
                .border_color(theme.selected)
                .child(open.input.clone()),
        )
    }
}
