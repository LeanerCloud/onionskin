//! Summarize Comments' half in the frame: the dialog, the save prompt, and
//! the summary written and opened.
//!
//! The work is `tools-comment`'s. A build without it shows the Edit menu
//! entry disabled, through the registry query for its command; the handlers
//! here still compile and say the plugin is missing if reached another way.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use super::ShellFrame;
use crate::shell::chrome::summary_dialog::{SummaryAction, SummaryChoice, SummaryDialogState};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    pub(super) fn open_summary_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_dialog(ShellDialog::Summary, window, cx);
        self.summary = Some(SummaryDialogState::default());
    }

    pub(in crate::shell) fn summary_dialog(&self) -> Option<&SummaryDialogState> {
        self.summary.as_ref()
    }

    pub(in crate::shell) fn run_summary_action(
        &mut self,
        action: SummaryAction,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.summary.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            SummaryAction::Choose(choice) => state.choice = choice,
            SummaryAction::Submit => self.prompt_for_summary_output(cx),
        }
        cx.notify();
    }

    /// Where the summary goes: beside the document, named after it, unless
    /// the user says otherwise.
    fn prompt_for_summary_output(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.tabs.active().map(|tab| tab.source.clone()) else {
            return;
        };
        let directory = source
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let suggested = summary_name(&source);
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(output))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.summarize_to(&output, cx))
                .ok();
        })
        .detach();
    }

    /// Summarize the active document into `output` and open it; or say why
    /// not, in the dialog.
    pub(super) fn summarize_to(&mut self, output: &Path, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let choice = self
            .summary
            .as_ref()
            .map(|state| state.choice)
            .unwrap_or_default();
        let made = canvas.update(cx, |canvas, _| {
            summarize(canvas.model.document_mut(), choice)
        });
        let written = made.and_then(|bytes| {
            super::create::write_replacing(output, &bytes)
                .map_err(|error| format!("{} was not written: {error}", output.display()))
        });
        match written {
            Ok(()) => {
                self.dialog = None;
                self.summary = None;
                self.open_documents(&[output.to_path_buf()], cx);
            }
            Err(error) => match self.summary.as_mut() {
                Some(state) => state.error = Some(error),
                None => self.notices.push(error),
            },
        }
        cx.notify();
    }
}

/// `report.pdf`'s summary is `report - Comments.pdf`.
fn summary_name(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    format!("{stem} - Comments.pdf")
}

#[cfg(feature = "tools-comment")]
fn summarize(
    document: &mut onionskin_core::Document,
    choice: SummaryChoice,
) -> Result<Vec<u8>, String> {
    use onionskin_tools_comment::SummaryLayout;

    let layout = match choice {
        SummaryChoice::CommentsOnly => SummaryLayout::CommentsOnly,
        SummaryChoice::DocumentAndComments => SummaryLayout::DocumentAndComments,
    };
    onionskin_tools_comment::summarize(document, layout)
        .map(|summary| summary.bytes)
        .map_err(|error| super::properties::sentence(&error.to_string()))
}

#[cfg(not(feature = "tools-comment"))]
fn summarize(_: &mut onionskin_core::Document, _: SummaryChoice) -> Result<Vec<u8>, String> {
    Err("The comment tools plugin is not installed".to_owned())
}
