//! The skins panel's half in the frame: opening it on the active document,
//! keeping its rows current when the file changes, and running Open a Copy
//! and Roll Back.

use std::path::{Path, PathBuf};

use gpui::{Context, Entity, Window};

use super::ShellFrame;
use crate::shell::chrome::accessible::Element;
use crate::shell::chrome::ThemeTokens;
use crate::shell::dialog::ShellDialog;
use crate::shell::skins::{self, SkinsAction, SkinsState};

impl ShellFrame {
    pub(in crate::shell) fn skins_state(&self) -> Option<&SkinsState> {
        self.skins.as_ref()
    }

    pub(in crate::shell) fn run_skins_action(
        &mut self,
        action: SkinsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SkinsAction::Toggle => self.toggle_skins(cx),
            SkinsAction::Select(index) => {
                if let Some(state) = self.skins.as_mut() {
                    state.selected = index.min(state.rows.len().saturating_sub(1));
                    state.error = None;
                }
            }
            SkinsAction::OpenCopy => self.prompt_for_version_copy(cx),
            SkinsAction::RollBack => {
                if self
                    .skins
                    .as_ref()
                    .is_some_and(|state| state.roll_back_refusal().is_none())
                {
                    self.show_dialog(ShellDialog::RollBack, window, cx);
                }
            }
            SkinsAction::ConfirmRollBack => {
                self.close_dialog(window, cx);
                self.roll_back(cx);
            }
            SkinsAction::CancelRollBack => self.close_dialog(window, cx),
        }
        cx.notify();
    }

    /// Open the panel on the active document, opening the side panel with
    /// it; or put it away.
    fn toggle_skins(&mut self, cx: &mut Context<Self>) {
        if self.skins.take().is_some() {
            return;
        }
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        match read_skins(&canvas, cx) {
            Ok(state) => {
                self.skins = Some(state);
                if !self.side_panel_state.is_open() {
                    self.side_panel_state.toggle();
                }
            }
            Err(error) => self.notices.push(error),
        }
    }

    /// Read the rows again if the file under them changed, or if another
    /// tab is now the active one. Cheap when nothing did: two comparisons.
    pub(super) fn refresh_skins(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.skins.as_ref() else {
            return;
        };
        let Some(canvas) = self.active_canvas().cloned() else {
            self.skins = None;
            return;
        };
        let model = &canvas.read(cx).model;
        let dirty = model.history_facts().dirty;
        let unchanged =
            state.canvas == canvas.entity_id() && state.read_at == model.byte_generation();
        if unchanged {
            if let Some(state) = self.skins.as_mut() {
                state.dirty = dirty;
            }
            return;
        }
        match read_skins(&canvas, cx) {
            Ok(fresh) => {
                let state = self.skins.as_mut().expect("checked above");
                if state.canvas == fresh.canvas {
                    state.reread(fresh.read_at, fresh.rows);
                    state.dirty = fresh.dirty;
                } else {
                    *state = fresh;
                }
            }
            Err(error) => {
                self.skins = None;
                self.notices.push(error);
            }
        }
    }

    /// Open a Copy of This Version: where the copy goes, then write it and
    /// open it. The open document and its file are not touched.
    fn prompt_for_version_copy(&mut self, cx: &mut Context<Self>) {
        let (Some(state), Some(tab)) = (self.skins.as_ref(), self.tabs.active()) else {
            return;
        };
        let index = state.selected;
        let bytes = match tab.canvas.read(cx).model.version_bytes(index) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.set_skins_error(error.to_string());
                return;
            }
        };
        let source = tab.source.clone();
        let directory = source
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let chosen = cx.prompt_for_new_path(&directory, Some(&copy_name(&source, index)));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(output))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    match super::create::write_replacing(&output, &bytes) {
                        Ok(()) => frame.open_documents(&[output], cx),
                        Err(error) => frame.set_skins_error(format!(
                            "{} was not written: {error}",
                            output.display()
                        )),
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Roll the active document's file back to the chosen version.
    fn roll_back(&mut self, cx: &mut Context<Self>) {
        let (Some(state), Some(canvas)) = (self.skins.as_ref(), self.active_canvas().cloned())
        else {
            return;
        };
        let keep = state.selected;
        let removed = state.rows.len().saturating_sub(keep + 1);
        let rolled = canvas.update(cx, |canvas, cx| {
            let result = canvas.model.roll_back_to(keep);
            cx.notify();
            result
        });
        match rolled {
            Ok(()) => {
                let version = if keep == 0 {
                    "the original".to_owned()
                } else {
                    format!("version {keep}")
                };
                self.notices.push(format!(
                    "Rolled back to {version}: {removed} newer {} removed from the file",
                    if removed == 1 { "version" } else { "versions" }
                ));
                self.refresh_skins(cx);
            }
            Err(error) => self.set_skins_error(super::properties::sentence(&error.to_string())),
        }
    }

    fn set_skins_error(&mut self, error: String) {
        match self.skins.as_mut() {
            Some(state) => state.error = Some(error),
            None => self.notices.push(error),
        }
    }

    /// What the side panel shows instead of the tool's help: the skins when
    /// they are open, a chosen comment's properties otherwise.
    pub(super) fn accessible_side_panel_content(&self, cx: &gpui::App) -> Option<Element> {
        if let Some(state) = &self.skins {
            return Some(skins::accessible(state));
        }
        self.accessible_inspector(cx).map(|inspector| {
            Element::new("inspector", accesskit::Role::Group, "Comment Properties")
                .with_children(inspector)
        })
    }

    pub(super) fn render_side_panel_content(
        &mut self,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if let Some(state) = &self.skins {
            return Some(skins::render(state, theme, cx));
        }
        self.render_inspector(theme, cx)
    }
}

/// The panel's state for `canvas`, read from its file.
fn read_skins(canvas: &Entity<crate::shell::Canvas>, cx: &gpui::App) -> Result<SkinsState, String> {
    let model = &canvas.read(cx).model;
    let rows = model
        .generation_details()
        .map_err(|error| format!("The versions of this file could not be read: {error}"))?;
    Ok(SkinsState::new(
        canvas.entity_id(),
        model.byte_generation(),
        rows,
        model.history_facts().dirty,
    ))
}

/// `report.pdf` version 2 is `report (version 2).pdf`.
fn copy_name(source: &Path, index: usize) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    if index == 0 {
        format!("{stem} (original).pdf")
    } else {
        format!("{stem} (version {index}).pdf")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_named_after_the_document_and_its_version() {
        assert_eq!(
            copy_name(Path::new("/a/report.pdf"), 2),
            "report (version 2).pdf"
        );
        assert_eq!(
            copy_name(Path::new("/a/report.pdf"), 0),
            "report (original).pdf"
        );
    }
}
