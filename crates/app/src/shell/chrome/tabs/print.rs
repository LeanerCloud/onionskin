//! The Print dialog's half in the frame: opening it on the active
//! document, applying its choices, and sending the job to a backend.
//!
//! Save as PDF is the file backend and works everywhere. A printer is the
//! macOS backend; Linux and Windows printing arrive at M4, and until then
//! the dialog lists only Save as PDF there rather than printers it cannot
//! reach.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use onionskin_print::PrintJob;

use super::ShellFrame;
use crate::shell::chrome::print_dialog::{
    apply, apply_setup, Destination, PageSetup, PrintAction, PrintDialogState, Printed,
};
use crate::shell::dialog::ShellDialog;

/// What an encrypted document's Print as Image box says, locked on.
pub(in crate::shell) const IMAGE_ONLY: &str = "Encrypted documents print only as images until M6";

impl ShellFrame {
    pub(in crate::shell) fn print_dialog(&self) -> Option<&PrintDialogState> {
        self.print.as_ref()
    }

    pub(in crate::shell) fn page_setup(&self) -> PageSetup {
        self.page_setup
    }

    /// File > Print: the dialog, on the active document.
    pub(super) fn open_print_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let printed = canvas.update(cx, |canvas, _| {
            let current_page = canvas.model.viewport().current_page();
            let document = canvas.model.document_mut();
            let image_only = document.read_out_refusal().map(|_| IMAGE_ONLY);
            (0..document.page_count())
                .map(|page| {
                    document
                        .page_geometry(page)
                        .map(|geometry| geometry.render_size)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|page_sizes| Printed {
                    page_sizes,
                    current_page,
                    image_only,
                })
        });
        let printed = match printed {
            Ok(printed) => printed,
            Err(error) => {
                self.notices
                    .push(format!("This document cannot be printed: {error}"));
                cx.notify();
                return;
            }
        };
        self.show_dialog(ShellDialog::Print, window, cx);
        let theme = self.shell_view_state.tokens();
        self.print = Some(PrintDialogState::new(printed, destinations(), theme, cx));
    }

    /// File > Page Setup.
    pub(super) fn open_page_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_dialog(ShellDialog::PageSetup, window, cx);
    }

    pub(in crate::shell) fn run_print_action(
        &mut self,
        action: PrintAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if apply_setup(&mut self.page_setup, action) {
            self.after_print_setting(cx);
            return;
        }
        let setup = self.page_setup;
        let Some(state) = self.print.as_mut() else {
            return;
        };
        if apply(&mut state.settings, action) {
            self.after_print_setting(cx);
            return;
        }
        match action {
            PrintAction::PreviewPrevious => {
                state.preview_sheet = state.preview_sheet.saturating_sub(1);
            }
            PrintAction::PreviewNext => {
                let count = state.preview(setup, cx).map_or(0, |sheets| sheets.len());
                state.preview_sheet = (state.preview_sheet + 1).min(count.saturating_sub(1));
            }
            PrintAction::Print => self.submit_print(cx),
            PrintAction::Cancel => self.close_dialog(window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// A choice changed: the preview starts again at the first sheet, and
    /// an error about the old choices is gone.
    fn after_print_setting(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.print.as_mut() {
            state.preview_sheet = 0;
            state.error = None;
        }
        cx.notify();
    }

    /// Print: check the choices, then send the job where it was asked to go.
    fn submit_print(&mut self, cx: &mut Context<Self>) {
        let setup = self.page_setup;
        let Some(state) = self.print.as_mut() else {
            return;
        };
        let job = match state.job(setup, cx) {
            Ok(job) => job,
            Err(error) => {
                state.error = Some(error);
                return;
            }
        };
        match state.destinations.get(state.settings.destination).cloned() {
            Some(Destination::Printer(_)) => self.print_to_printer(job, cx),
            _ => self.prompt_for_print_file(job, cx),
        }
    }

    /// Save as PDF: where the printed file goes.
    fn prompt_for_print_file(&mut self, job: PrintJob, cx: &mut Context<Self>) {
        let Some(source) = self.tabs.active().map(|tab| tab.source.clone()) else {
            return;
        };
        let directory = source
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let chosen = cx.prompt_for_new_path(&directory, Some(&printed_name(&source)));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(output))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.print_to_path(&output, &job, cx))
                .ok();
        })
        .detach();
    }

    /// Write the job's sheets to `output`, and say so; or say why not, in
    /// the dialog.
    pub(super) fn print_to_path(&mut self, output: &Path, job: &PrintJob, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let printed = canvas.update(cx, |canvas, _| {
            onionskin_print::print_to_file(canvas.model.document_mut(), job)
        });
        let written = printed
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                super::create::write_replacing(output, &bytes)
                    .map_err(|error| format!("{} was not written: {error}", output.display()))
            });
        self.finish_print(
            written.map(|()| format!("Printed to {}", output.display())),
            cx,
        );
    }

    #[cfg(target_os = "macos")]
    fn print_to_printer(&mut self, job: PrintJob, cx: &mut Context<Self>) {
        use onionskin_print::{impose, MacBackend, PrintBackend};

        let Some(tab) = self.tabs.active() else {
            return;
        };
        let (canvas, title) = (tab.canvas.clone(), tab.title().to_owned());
        let sent = canvas.update(cx, |canvas, _| {
            let bytes = canvas
                .model
                .document_mut()
                .preview_bytes(job.comments)
                .map_err(|error| error.to_string())?;
            let mut backend = MacBackend::new(bytes, title).map_err(|error| error.to_string())?;
            let sizes = backend.page_sizes().map_err(|error| error.to_string())?;
            backend
                .print(&job, &impose(&job, &sizes))
                .map_err(|error| error.to_string())
        });
        let printer = job.printer.clone().unwrap_or_default();
        self.finish_print(sent.map(|()| format!("Sent to {printer}")), cx);
    }

    #[cfg(not(target_os = "macos"))]
    fn print_to_printer(&mut self, _job: PrintJob, cx: &mut Context<Self>) {
        self.finish_print(
            Err("Printing to a printer on this platform arrives in M4; use Save as PDF".to_owned()),
            cx,
        );
    }

    /// A print that went: the dialog closes and a notice says where. One that
    /// did not: the dialog stays, saying why.
    fn finish_print(&mut self, outcome: Result<String, String>, cx: &mut Context<Self>) {
        match outcome {
            Ok(notice) => {
                self.dialog = None;
                self.print = None;
                self.notices.push(notice);
            }
            Err(error) => match self.print.as_mut() {
                Some(state) => state.error = Some(error),
                None => self.notices.push(error),
            },
        }
        cx.notify();
    }

    /// The sheets the open dialog would print now, for tests that hold the
    /// preview against the output.
    #[cfg(test)]
    pub(super) fn print_preview_sheets(
        &self,
        cx: &gpui::App,
    ) -> Result<Vec<onionskin_print::Sheet>, String> {
        let state = self.print.as_ref().ok_or("no print dialog")?;
        state.preview(self.page_setup, cx)
    }
}

/// Where a print can go: a PDF file always, and the platform's printers
/// where there is a backend for them.
fn destinations() -> Vec<Destination> {
    std::iter::once(Destination::SaveAsPdf)
        .chain(platform_printers().into_iter().map(Destination::Printer))
        .collect()
}

#[cfg(target_os = "macos")]
fn platform_printers() -> Vec<String> {
    onionskin_print::printers()
}

#[cfg(not(target_os = "macos"))]
fn platform_printers() -> Vec<String> {
    Vec::new()
}

/// `report.pdf` printed is `report (printed).pdf`.
fn printed_name(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    format!("{stem} (printed).pdf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printed_copy_is_named_after_the_document() {
        assert_eq!(
            printed_name(Path::new("/a/report.pdf")),
            "report (printed).pdf"
        );
        assert_eq!(printed_name(Path::new("")), "Document (printed).pdf");
    }

    #[test]
    fn save_as_pdf_is_always_the_first_destination() {
        assert_eq!(destinations()[0], Destination::SaveAsPdf);
        if cfg!(not(target_os = "macos")) {
            assert_eq!(destinations().len(), 1, "no printer without a backend");
        }
    }
}
