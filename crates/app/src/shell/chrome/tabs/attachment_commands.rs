//! The Attachments pane's commands that need the frame: Open, which opens
//! a PDF attachment in a tab of its own, Edit Description, and Search
//! Attachments.
//!
//! **Only PDF attachments open.** Anything else would be handed to another
//! program, which is what Acrobat's Trust Manager asks about file by file;
//! here it is refused, and Save puts the file where the user can open it
//! themselves.

use gpui::{Context, Focusable as _, Window};

use super::ShellFrame;
use crate::shell::chrome::description_dialog::DescriptionState;
use crate::shell::dialog::ShellDialog;

/// Said when a listed attachment is not a PDF.
pub(in crate::shell) const NOT_A_PDF: &str =
    "Only PDF attachments open here. Save the attachment to open it with another program.";

impl ShellFrame {
    pub(in crate::shell) fn description_dialog(&self) -> Option<&DescriptionState> {
        self.description.as_ref()
    }

    /// Open the listed attachment at `index` in a new tab, when it is a PDF.
    pub(super) fn open_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(attachment) = self.navigation.attachment(index).cloned() else {
            return;
        };
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let bytes = match canvas.update(cx, |canvas, _| canvas.model.attachment_bytes(index)) {
            Ok(bytes) => bytes,
            Err(error) => return self.say(error.to_string(), cx),
        };
        self.open_attachment_bytes(attachment.file_name(), bytes, cx);
    }

    fn say(&mut self, notice: String, cx: &mut Context<Self>) {
        self.notices.push(notice);
        cx.notify();
    }

    /// Edit Description on the listed attachment at `index`.
    pub(super) fn open_description_dialog(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(attachment) = self.navigation.attachment(index).cloned() else {
            return;
        };
        self.show_dialog(ShellDialog::AttachmentDescription, window, cx);
        let theme = self.shell_view_state.tokens();
        let state = DescriptionState::new(
            attachment.stream,
            attachment.description.as_deref().unwrap_or_default(),
            theme,
            cx,
        );
        window.focus(&state.text.read(cx).focus_handle(cx));
        self.description = Some(state);
    }

    /// Write the typed description as one undoable step, and read the pane
    /// again.
    pub(super) fn submit_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.description.as_ref() else {
            return;
        };
        let stream = state.stream;
        let text = state.text.read(cx).query().to_owned();
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let result = canvas.update(cx, |canvas, cx| {
            let result = canvas
                .model
                .document_mut()
                .edit_document("Edit Description", |tx| {
                    onionskin_core::set_attachment_description(tx, stream, &text)
                });
            if result.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            result
        });
        match result {
            Ok(_) => {
                self.close_dialog(window, cx);
                self.navigation.reread(&canvas, cx);
            }
            Err(error) => {
                if let Some(state) = self.description.as_mut() {
                    state.error = Some(error.to_string());
                }
            }
        }
        cx.notify();
    }

    /// Search Attachments: Advanced Search with its PDF attachments
    /// included.
    pub(super) fn search_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_advanced_search(window, cx);
        if let Some(state) = self.advanced_search.as_mut() {
            state.form.include_attachments = true;
        }
        cx.notify();
    }
}
