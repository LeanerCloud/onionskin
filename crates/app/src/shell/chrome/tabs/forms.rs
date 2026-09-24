//! The frame's half of filling forms: Clear Form, and the notices filling
//! leaves on the canvas, which go on the notice bar.

use gpui::Context;

use super::ShellFrame;

/// What the form entries say in a build that does not fill forms.
pub(in crate::shell) const NO_FORMS: &str = "The Forms plugin is not installed";

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

    /// A screen reader picked option `index` of the dropdown being filled.
    pub(super) fn choose_form_option(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(canvas) = self.active_canvas().cloned() {
            canvas.update(cx, |canvas, cx| canvas.choose_field_option_at(index, cx));
        }
    }

    /// Take what filling a field had to say: the scripts' alerts, scripts
    /// that did not run, values refused.
    pub(super) fn collect_form_notices(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let notices = canvas.update(cx, |canvas, _| canvas.model.take_form_notices());
        if !notices.is_empty() {
            self.notices.extend(notices);
            cx.notify();
        }
    }
}
