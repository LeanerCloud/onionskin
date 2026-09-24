//! The frame's half of Watermark, Background, Header & Footer and Bates
//! Numbering: opening the dialog, the file prompts, and running what it
//! asks for through `tools-edit`.

use std::path::PathBuf;

use gpui::{Context, Window};
use onionskin_core::pages::MarkKind;
use onionskin_core::Document;
use onionskin_plugin_api::CommandError;
use onionskin_tools_edit::marks::{self as edit_marks, Art};

use super::properties::sentence;
use super::ShellFrame;
use crate::shell::chrome::marks_dialog::{
    iso_date, title, ArtChoice, Checked, MarkAction, MarkForm, MarkRequest, MarksDialogState,
};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// Open the dialog for `kind` on the grid's selection or the page on
    /// screen, saying whether the document already has such a mark.
    pub(super) fn open_marks_dialog(
        &mut self,
        kind: MarkKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pages = self.target_pages(cx);
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let (page_count, existing, saved) = canvas.update(cx, |canvas, _| {
            let count = canvas.model.viewport().page_count();
            let mut document = canvas.model.document_mut();
            let marked = edit_marks::marked_pages(&mut document, kind);
            let saved = edit_marks::saved_settings(&mut document, kind)
                .ok()
                .flatten();
            (count, marked.is_ok_and(|pages| !pages.is_empty()), saved)
        });
        self.show_dialog(ShellDialog::Marks(kind), window, cx);
        let theme = self.shell_view_state.tokens();
        let form = MarkForm::new(kind, existing);
        let mut state = MarksDialogState::new(form, pages, page_count, theme, cx);
        if let Some(saved) = saved.filter(|_| existing) {
            state.restore(&saved, cx);
        }
        self.marks = Some(state);
    }

    pub(in crate::shell) fn marks_dialog(&self) -> Option<&MarksDialogState> {
        self.marks.as_ref()
    }

    pub(super) fn run_marks_action(
        &mut self,
        action: MarkAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.marks.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            MarkAction::Submit => {
                let date = iso_date(&onionskin_core::pdf_date(unix_now()));
                match state.request(&date, cx) {
                    Ok(checked) => self.mark_pages(checked, window, cx),
                    Err(error) => state.error = Some(error),
                }
            }
            MarkAction::Remove => self.remove_marks(window, cx),
            MarkAction::ChooseFile => self.prompt_for_mark_files(false, cx),
            MarkAction::AddFiles => self.prompt_for_mark_files(true, cx),
            _ => state.form.apply(action),
        }
        cx.notify();
    }

    /// Choose the art's PDF, or more files to number.
    fn prompt_for_mark_files(&mut self, several: bool, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: several,
            prompt: Some(if several { "Add Files" } else { "Choose" }.into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    frame.take_mark_files(paths, several);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// What a file prompt chose, into the form.
    pub(super) fn take_mark_files(&mut self, paths: Vec<PathBuf>, several: bool) {
        let Some(state) = self.marks.as_mut() else {
            return;
        };
        if several {
            state.form.other_files.extend(paths);
        } else {
            state.form.file = paths.into_iter().next();
        }
    }

    /// Add or update the mark, then close; a failure stays in the dialog.
    fn mark_pages(&mut self, checked: Checked, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.marks.as_ref() else {
            return;
        };
        let (kind, replace) = (state.form.kind, state.form.existing);
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let pages = checked.pages.clone();
        let mut done = String::new();
        let outcome = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.edit_pages(|doc| {
                done = apply(doc, kind, replace, &pages, &checked)?;
                Ok(())
            });
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome
        });
        match outcome {
            Ok(()) => {
                self.close_dialog(window, cx);
                self.notices.push(done);
            }
            Err(error) => {
                if let Some(state) = self.marks.as_mut() {
                    state.error = Some(sentence(&error.to_string()));
                }
            }
        }
    }

    /// Remove the kind's marks from every page.
    fn remove_marks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.marks.as_ref().map(|state| state.form.kind) else {
            return;
        };
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let mut removed = 0;
        let outcome = canvas.update(cx, |canvas, cx| {
            let pages: Vec<usize> = (0..canvas.model.viewport().page_count()).collect();
            let outcome = canvas.model.edit_pages(|doc| {
                removed = edit_marks::remove_marks(doc, kind, &pages)?;
                Ok(())
            });
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome
        });
        match outcome {
            Ok(()) => {
                self.close_dialog(window, cx);
                self.notices.push(format!(
                    "Removed the {} from {}.",
                    noun(kind),
                    pages_said(removed)
                ));
            }
            Err(error) => {
                if let Some(state) = self.marks.as_mut() {
                    state.error = Some(sentence(&error.to_string()));
                }
            }
        }
    }
}

/// Run `checked` on `doc`, saying what was done.
fn apply(
    doc: &mut Document,
    kind: MarkKind,
    replace: bool,
    pages: &[usize],
    checked: &Checked,
) -> Result<String, CommandError> {
    let verb = if replace { "Updated" } else { "Added" };
    let settings = checked.settings.as_str();
    match &checked.request {
        MarkRequest::HeaderFooter(header) => {
            edit_marks::add_header_footer(doc, pages, header, replace, settings)?;
        }
        MarkRequest::Bates {
            bates,
            others,
            naming,
        } => {
            let (first, last) = edit_marks::add_bates(doc, pages, bates, settings)?;
            let mut said = format!(
                "Numbered {} from {first} to {last}.",
                pages_said(pages.len())
            );
            if !others.is_empty() {
                let next = edit_marks::Bates {
                    start: bates.start + pages.len() as u64,
                    ..bates.clone()
                };
                let written = edit_marks::number_files(others, &next, naming)?;
                for file in written {
                    said.push_str(&format!(
                        " {} is {} to {}.",
                        file.output.display(),
                        file.first,
                        file.last
                    ));
                }
            }
            return Ok(said);
        }
        MarkRequest::Art { art, appearance } => {
            let art = match art {
                ArtChoice::Text { text, style } => Art::Text {
                    text: text.clone(),
                    style: *style,
                },
                ArtChoice::Color(color) => Art::Color(*color),
                ArtChoice::File { path, scale } => Art::Page {
                    pdf: std::sync::Arc::new(std::fs::read(path).map_err(|error| {
                        CommandError::Failed {
                            label: "Choose PDF File",
                            reason: format!("{}: {error}", path.display()),
                        }
                    })?),
                    page: 0,
                    scale: *scale,
                },
            };
            match kind {
                MarkKind::Background => {
                    edit_marks::add_background(doc, pages, &art, *appearance, replace, settings)?
                }
                _ => edit_marks::add_watermark(doc, pages, &art, *appearance, replace, settings)?,
            }
        }
    }
    Ok(format!(
        "{verb} the {} on {}.",
        noun(kind),
        pages_said(pages.len())
    ))
}

/// The kind, as a sentence names it.
fn noun(kind: MarkKind) -> String {
    match kind {
        MarkKind::HeaderFooter => "header and footer".to_owned(),
        other => title(other).to_lowercase(),
    }
}

fn pages_said(count: usize) -> String {
    match count {
        1 => "1 page".to_owned(),
        count => format!("{count} pages"),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notices_name_the_kind_and_the_pages() {
        assert_eq!(noun(MarkKind::HeaderFooter), "header and footer");
        assert_eq!(noun(MarkKind::Bates), "bates numbering");
        assert_eq!(pages_said(1), "1 page");
        assert_eq!(pages_said(0), "0 pages");
        assert!(unix_now() > 1_700_000_000);
    }
}
