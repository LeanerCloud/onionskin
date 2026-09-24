//! The Attachments pane's commands that need the frame: Open, which opens
//! a PDF attachment in a tab of its own, Edit Description, and Search
//! Attachments.
//!
//! **Only PDF attachments open.** Anything else would be handed to another
//! program, which is what Acrobat's Trust Manager asks about file by file;
//! here it is refused, and Save puts the file where the user can open it
//! themselves.

use std::path::{Path, PathBuf};

use gpui::{Context, Focusable as _, Window};

use super::ShellFrame;
use crate::shell::chrome::description_dialog::DescriptionState;
use crate::shell::dialog::ShellDialog;

/// Said when a listed attachment is not a PDF.
pub(in crate::shell) const NOT_A_PDF: &str =
    "Only PDF attachments open here. Save the attachment to open it with another program.";

/// Where an opened attachment is written: its own folder under the
/// temporary directory, so two attachments with one name do not collide.
pub(in crate::shell) fn opened_attachment_path(
    root: &Path,
    stream: u32,
    file_name: &str,
) -> PathBuf {
    root.join(format!("{stream}")).join(file_name)
}

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
        let bytes = canvas.update(cx, |canvas, _| canvas.model.attachment_bytes(index));
        let bytes = match bytes {
            Ok(bytes) if bytes.starts_with(b"%PDF") => bytes,
            Ok(_) => return self.say(NOT_A_PDF.to_owned(), cx),
            Err(error) => return self.say(error.to_string(), cx),
        };
        let root = std::env::temp_dir()
            .join("onionskin-attachments")
            .join(std::process::id().to_string());
        let path = opened_attachment_path(&root, attachment.stream, &attachment.file_name());
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, bytes));
        match written {
            Ok(()) => self.open_documents(&[path], cx),
            Err(error) => self.say(format!("The attachment could not be opened: {error}"), cx),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_attachment_gets_a_folder_of_its_own() {
        assert_eq!(
            opened_attachment_path(Path::new("/t"), 12, "report.pdf"),
            Path::new("/t/12/report.pdf")
        );
    }
}
