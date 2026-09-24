//! The frame's half of redaction: the Edit menu's commands, the Redact
//! tool's click on a mark, the canvas menu's Redact Text, and each of the
//! redaction dialog's panels run through the `redact` plugin.

use super::ShellFrame;

/// What the redaction entries say in a build without the plugin.
pub(in crate::shell) const NO_REDACT: &str = "The Redact plugin is not installed";

/// A redaction command from the Edit menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum RedactCommand {
    MarkPages,
    Find,
    Properties,
    Apply,
    Sanitize,
}

impl RedactCommand {
    pub(in crate::shell) const ALL: [RedactCommand; 5] = [
        RedactCommand::MarkPages,
        RedactCommand::Find,
        RedactCommand::Properties,
        RedactCommand::Apply,
        RedactCommand::Sanitize,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::MarkPages => "Mark Pages for Redaction…",
            Self::Find => "Find Text & Redact…",
            Self::Properties => "Redaction Properties…",
            Self::Apply => "Apply Redactions…",
            Self::Sanitize => "Remove Hidden Information…",
        }
    }

    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::MarkPages => "redact.mark-pages",
            Self::Find => "redact.find",
            Self::Properties => "redact.properties",
            Self::Apply => "redact.apply",
            Self::Sanitize => "redact.sanitize",
        }
    }

    /// Whether it writes a new file rather than marking: it then needs the
    /// document's content to be readable out, not editable.
    pub(in crate::shell) fn writes_a_file(self) -> bool {
        matches!(self, Self::Apply | Self::Sanitize)
    }
}

#[cfg(not(feature = "redact"))]
impl ShellFrame {
    pub(super) fn run_redact_command(
        &mut self,
        _command: RedactCommand,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.notices.push(NO_REDACT.to_owned());
        cx.notify();
    }

    pub(super) fn collect_redaction_request(&mut self, _cx: &mut gpui::Context<Self>) {}

    pub(super) fn run_pending_redaction(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) {
    }
}

#[cfg(feature = "redact")]
mod dialog {
    use std::path::{Path, PathBuf};

    use gpui::{Context, EntityId, PathPromptOptions, Window};
    use onionskin_core::redactions::RedactionLook;
    use onionskin_core::ObjRef;
    use onionskin_redact::codes::{built_in, CodeLibrary, CodeSet};
    use onionskin_redact::find::find;
    use onionskin_redact::look::{default_of, look_of};
    use onionskin_redact::mark::{mark_found, mark_pages, mark_text, marks, set_look, unmark};
    use onionskin_redact::{apply_with, Applied, ApplyOptions};

    use super::super::properties::sentence;
    use super::{RedactCommand, ShellFrame};
    use crate::shell::chrome::combine_dialog::parse_page_list;
    use crate::shell::chrome::redact_dialog::{
        codes, look, query, Panel, PropertiesForm, RedactAction, RedactDialogState, RedactField,
    };
    use crate::shell::dialog::ShellDialog;

    impl ShellFrame {
        pub(in crate::shell::chrome::tabs) fn run_redact_command(
            &mut self,
            command: RedactCommand,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let panel = match command {
                RedactCommand::MarkPages => Panel::Pages,
                RedactCommand::Find => Panel::Find,
                RedactCommand::Properties => Panel::Properties { mark: None },
                RedactCommand::Apply => Panel::Apply { sanitize: false },
                RedactCommand::Sanitize => Panel::Apply { sanitize: true },
            };
            self.open_redact_dialog(panel, window, cx);
        }

        pub(in crate::shell) fn redact_dialog(&self) -> Option<&RedactDialogState> {
            self.redact.as_ref()
        }

        /// The look new marks take.
        fn default_look(&self) -> RedactionLook {
            self.settings
                .preferences
                .redaction
                .as_ref()
                .map(look_of)
                .unwrap_or_default()
        }

        fn code_library(&self) -> Option<CodeLibrary> {
            self.settings
                .paths
                .data
                .as_deref()
                .map(CodeLibrary::in_data_dir)
        }

        fn code_sets(&self) -> Vec<CodeSet> {
            self.code_library()
                .map_or_else(built_in, |library| library.sets())
        }

        fn open_redact_dialog(
            &mut self,
            panel: Panel,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let Some(canvas) = self.active_canvas().cloned() else {
                return;
            };
            let (page_count, current, look) = canvas.update(cx, |canvas, _| {
                let viewport = canvas.model.viewport();
                let (count, current) = (viewport.page_count(), viewport.current_page());
                let look = match panel {
                    Panel::Properties {
                        mark: Some((mark, _)),
                    } => marks(&mut canvas.model.document_mut())
                        .ok()
                        .and_then(|marks| marks.into_iter().find(|each| each.objref == mark))
                        .map(|mark| mark.look),
                    _ => None,
                };
                (count, current, look)
            });
            let look = look.unwrap_or_else(|| self.default_look());
            let (properties, overlay, size) = PropertiesForm::of(&look, self.code_sets());
            let texts = [
                (RedactField::OverlayText, overlay),
                (RedactField::FontSize, size),
                (RedactField::Pages, (current + 1).to_string()),
            ];
            self.show_dialog(ShellDialog::Redact(panel), window, cx);
            let theme = self.shell_view_state.tokens();
            self.redact = Some(RedactDialogState::new(
                panel, properties, page_count, &texts, theme, cx,
            ));
        }

        pub(in crate::shell::chrome::tabs) fn run_redact_action(
            &mut self,
            action: RedactAction,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let Some(state) = self.redact.as_mut() else {
                return;
            };
            state.error = None;
            let outcome = match action {
                RedactAction::Save => self.save_properties(window, cx),
                RedactAction::RemoveMark => self.remove_mark(window, cx),
                RedactAction::UseCode => {
                    self.use_code(cx);
                    Ok(())
                }
                RedactAction::SaveSet | RedactAction::RenameSet | RedactAction::RemoveSet => {
                    self.change_code_set(action, cx)
                }
                RedactAction::ImportSet => {
                    self.prompt_for_code_set(cx);
                    Ok(())
                }
                RedactAction::ExportSet => {
                    self.prompt_for_code_export(cx);
                    Ok(())
                }
                RedactAction::Find => self.find_to_redact(cx),
                RedactAction::MarkChecked => self.mark_checked(window, cx),
                RedactAction::MarkPages => self.mark_redaction_pages(window, cx),
                RedactAction::HiddenInformation => {
                    state.hidden_information = !state.hidden_information;
                    Ok(())
                }
                RedactAction::Apply => {
                    self.prompt_for_redacted_copy(cx);
                    Ok(())
                }
                RedactAction::Patterns(_)
                | RedactAction::NextPattern
                | RedactAction::WholeWord
                | RedactAction::MatchCase
                | RedactAction::Toggle(_) => {
                    state.find.apply(action);
                    Ok(())
                }
                _ => {
                    state.properties.apply(action);
                    Ok(())
                }
            };
            if let Err(error) = outcome {
                if let Some(state) = self.redact.as_mut() {
                    state.error = Some(error);
                }
            }
            cx.notify();
        }

        /// Save: the mark's new look, or the default for new marks.
        fn save_properties(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Result<(), String> {
            let state = self.redact.as_ref().ok_or("")?;
            let chosen = look(
                &state.properties,
                &state.text(RedactField::OverlayText, cx),
                &state.text(RedactField::FontSize, cx),
            )?;
            match state.panel {
                Panel::Properties {
                    mark: Some((mark, _)),
                } => {
                    self.edit_marks(|doc| set_look(doc, mark, &chosen).map(|()| 1), cx)?;
                }
                _ => {
                    self.settings.preferences.redaction = Some(default_of(&chosen));
                    self.apply_tool_environment(cx);
                    self.save_preferences();
                    self.notices
                        .push("New redaction marks will look like this.".to_owned());
                }
            }
            self.close_dialog(window, cx);
            Ok(())
        }

        fn remove_mark(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Result<(), String> {
            let Some(Panel::Properties {
                mark: Some((mark, page)),
            }) = self.redact.as_ref().map(|state| state.panel)
            else {
                return Ok(());
            };
            self.edit_marks(|doc| unmark(doc, page, mark).map(|()| 1), cx)?;
            self.close_dialog(window, cx);
            Ok(())
        }

        fn use_code(&mut self, cx: &mut Context<Self>) {
            let Some(state) = self.redact.as_mut() else {
                return;
            };
            let Some(code) = state.properties.current_code().map(str::to_owned) else {
                return;
            };
            state.properties.overlay = true;
            if let Some((_, input)) = state
                .inputs
                .iter()
                .find(|(field, _)| *field == RedactField::OverlayText)
            {
                input.update(cx, |input, cx| input.set_query(code, cx));
            }
        }

        /// Save Code Set, Rename Code Set and Remove Code Set.
        fn change_code_set(
            &mut self,
            action: RedactAction,
            cx: &mut Context<Self>,
        ) -> Result<(), String> {
            let library = self
                .code_library()
                .ok_or("There is no folder to keep code sets in")?;
            let state = self.redact.as_mut().ok_or("")?;
            let name = state.text(RedactField::SetName, cx).trim().to_owned();
            let current = state
                .properties
                .current_set()
                .map(|set| set.name.clone())
                .unwrap_or_default();
            let chosen = match action {
                RedactAction::SaveSet => {
                    let typed = codes(&state.text(RedactField::Codes, cx));
                    if typed.is_empty() {
                        return Err("Type at least one code, separated by commas.".to_owned());
                    }
                    library
                        .save(&name, &typed)
                        .map_err(|error| sentence(&error.to_string()))?;
                    name
                }
                RedactAction::RenameSet => {
                    library
                        .rename(&current, &name)
                        .map_err(|error| sentence(&error.to_string()))?;
                    name
                }
                _ => {
                    library
                        .remove(&current)
                        .map_err(|error| sentence(&error.to_string()))?;
                    String::new()
                }
            };
            state.properties.choose_set(library.sets(), &chosen);
            Ok(())
        }

        fn prompt_for_code_set(&mut self, cx: &mut Context<Self>) {
            let chosen = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Import".into()),
            });
            cx.spawn(async move |frame, cx| {
                let Ok(Ok(Some(paths))) = chosen.await else {
                    return;
                };
                let Some(file) = paths.into_iter().next() else {
                    return;
                };
                frame
                    .update(cx, |frame, cx| {
                        frame.import_code_set(&file);
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }

        /// Import Code Set, from the file the prompt chose.
        pub(in crate::shell::chrome::tabs) fn import_code_set(&mut self, file: &Path) {
            let outcome = self
                .code_library()
                .ok_or_else(|| "There is no folder to keep code sets in".to_owned())
                .and_then(|library| {
                    let set = library
                        .import(file)
                        .map_err(|error| sentence(&error.to_string()))?;
                    Ok((library.sets(), set.name))
                });
            let Some(state) = self.redact.as_mut() else {
                return;
            };
            match outcome {
                Ok((sets, name)) => state.properties.choose_set(sets, &name),
                Err(error) => state.error = Some(error),
            }
        }

        fn prompt_for_code_export(&mut self, cx: &mut Context<Self>) {
            let Some(set) = self
                .redact
                .as_ref()
                .and_then(|state| state.properties.current_set().cloned())
            else {
                return;
            };
            let directory = self
                .settings
                .paths
                .home
                .clone()
                .unwrap_or_else(|| PathBuf::from("."));
            let chosen = cx.prompt_for_new_path(&directory, Some(&format!("{}.txt", set.name)));
            cx.spawn(async move |frame, cx| {
                let Ok(Ok(Some(path))) = chosen.await else {
                    return;
                };
                frame
                    .update(cx, |frame, cx| {
                        frame.export_code_set(&set, &path);
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }

        /// Export Code Set, to the file the prompt chose.
        pub(in crate::shell::chrome::tabs) fn export_code_set(
            &mut self,
            set: &CodeSet,
            file: &Path,
        ) {
            match CodeLibrary::export(set, file) {
                Ok(()) => {
                    self.notices
                        .push(format!("Exported {} to {}.", set.name, file.display()))
                }
                Err(error) => {
                    if let Some(state) = self.redact.as_mut() {
                        state.error = Some(sentence(&error.to_string()));
                    }
                }
            }
        }

        fn find_to_redact(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
            let state = self.redact.as_ref().ok_or("")?;
            let wanted = query(&state.find, &state.text(RedactField::FindText, cx))?;
            let canvas = self.active_canvas().cloned().ok_or("No document is open")?;
            let found = canvas
                .update(cx, |canvas, _| {
                    find(&mut canvas.model.document_mut(), &wanted)
                })
                .map_err(|error| sentence(&error.to_string()))?;
            if let Some(state) = self.redact.as_mut() {
                state.find.show(found);
            }
            Ok(())
        }

        fn mark_checked(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Result<(), String> {
            let chosen = self
                .redact
                .as_ref()
                .map(|state| state.find.chosen())
                .unwrap_or_default();
            if chosen.is_empty() {
                return Err("Check at least one result to mark.".to_owned());
            }
            let look = self.default_look();
            let marked = self.edit_marks(|doc| mark_found(doc, &chosen, &look), cx)?;
            self.notices.push(marked_said(marked));
            self.close_dialog(window, cx);
            Ok(())
        }

        fn mark_redaction_pages(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> Result<(), String> {
            let state = self.redact.as_ref().ok_or("")?;
            let pages = parse_page_list(&state.text(RedactField::Pages, cx), state.page_count)?
                .ok_or("Name the pages to mark.")?;
            let look = self.default_look();
            let marked = self.edit_marks(|doc| mark_pages(doc, &pages, &look), cx)?;
            self.notices.push(marked_said(marked));
            self.close_dialog(window, cx);
            Ok(())
        }

        /// Runs a marking edit on the document in front, as one undo step.
        fn edit_marks(
            &mut self,
            edit: impl FnOnce(
                &mut onionskin_core::Document,
            ) -> Result<usize, onionskin_plugin_api::CommandError>,
            cx: &mut Context<Self>,
        ) -> Result<usize, String> {
            let canvas = self.active_canvas().cloned().ok_or("No document is open")?;
            canvas.update(cx, |canvas, cx| {
                let outcome = edit(&mut canvas.model.document_mut());
                if outcome.is_ok() {
                    canvas.handle_change(Ok(true), cx);
                }
                outcome.map_err(|error| sentence(&error.to_string()))
            })
        }

        /// The canvas menu's Redact Text: the selected text marked. `false`
        /// with nothing selected, for the Redact tool to be chosen instead.
        pub(in crate::shell::chrome::tabs) fn redact_selection(
            &mut self,
            cx: &mut Context<Self>,
        ) -> bool {
            let Some(canvas) = self.active_canvas().cloned() else {
                return false;
            };
            let look = self.default_look();
            let marked = canvas.update(cx, |canvas, cx| {
                let quads = canvas
                    .model
                    .document_mut()
                    .selection()
                    .text_quads()
                    .to_vec();
                let page = quads.first()?.page;
                let on_page: Vec<_> = quads.into_iter().filter(|quad| quad.page == page).collect();
                let outcome = mark_text(&mut canvas.model.document_mut(), page, &on_page, &look);
                if outcome.is_ok() {
                    canvas.handle_change(Ok(true), cx);
                }
                Some(outcome)
            });
            let Some(marked) = marked else {
                return false;
            };
            self.notices.push(match marked {
                Ok(_) => marked_said(1),
                Err(error) => sentence(&error.to_string()),
            });
            cx.notify();
            true
        }

        /// Take the Redact tool's click on a mark, for the next render.
        pub(in crate::shell::chrome::tabs) fn collect_redaction_request(
            &mut self,
            cx: &mut Context<Self>,
        ) {
            let Some(canvas) = self.active_canvas().cloned() else {
                return;
            };
            let request = canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().take_redaction_request()
            });
            if let Some(mark) = request {
                self.pending_redaction = Some(mark);
                cx.notify();
            }
        }

        /// Open the clicked mark's properties. Called from render.
        pub(in crate::shell::chrome::tabs) fn run_pending_redaction(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            if self.dialog.is_some() {
                return;
            }
            let Some(mark) = self.pending_redaction.take() else {
                return;
            };
            let page = self.mark_page(mark, cx);
            if let Some(page) = page {
                self.open_redact_dialog(
                    Panel::Properties {
                        mark: Some((mark, page)),
                    },
                    window,
                    cx,
                );
            }
        }

        fn mark_page(&mut self, mark: ObjRef, cx: &mut Context<Self>) -> Option<usize> {
            let canvas = self.active_canvas().cloned()?;
            canvas.update(cx, |canvas, _| {
                marks(&mut canvas.model.document_mut())
                    .ok()?
                    .into_iter()
                    .find(|each| each.objref == mark)
                    .map(|each| each.page)
            })
        }

        /// Apply Redactions or Remove Hidden Information: ask where the new
        /// file goes.
        fn prompt_for_redacted_copy(&mut self, cx: &mut Context<Self>) {
            let Some(canvas) = self.active_canvas_entity() else {
                return;
            };
            let origin = canvas.entity_id();
            let current = canvas
                .read(cx)
                .model
                .path()
                .unwrap_or_else(|| PathBuf::from("Untitled.pdf"));
            let directory = current
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            let chosen = cx.prompt_for_new_path(&directory, Some(&redacted_name(&current)));
            cx.spawn(async move |frame, cx| {
                let Ok(Ok(Some(path))) = chosen.await else {
                    return;
                };
                frame
                    .update(cx, |frame, cx| frame.redact_into(origin, &path, cx))
                    .ok();
            })
            .detach();
        }

        /// The second half: apply, verify, write the new file and open it.
        pub(in crate::shell::chrome::tabs) fn redact_into(
            &mut self,
            origin: EntityId,
            path: &Path,
            cx: &mut Context<Self>,
        ) {
            let Some(state) = self.redact.as_ref() else {
                return;
            };
            let options = ApplyOptions {
                remove_hidden_information: matches!(state.panel, Panel::Apply { sanitize: true })
                    || state.hidden_information,
            };
            let Some((_, canvas)) = self.canvas_by_id(origin) else {
                return;
            };
            let applied = canvas
                .update(cx, |canvas, _| {
                    apply_with(&mut canvas.model.document_mut(), &options)
                })
                .map_err(|error| sentence(&error.to_string()))
                .and_then(|applied| {
                    write_new(path, &applied.bytes).map_err(|error| {
                        format!("{} could not be written: {error}", path.display())
                    })?;
                    Ok(applied)
                });
            match applied {
                Ok(applied) => {
                    self.redact = None;
                    self.dialog = None;
                    self.notices.push(applied_said(&applied, path));
                    self.open_documents(&[path.to_path_buf()], cx);
                }
                Err(error) => {
                    if let Some(state) = self.redact.as_mut() {
                        state.error = Some(error);
                    }
                }
            }
            cx.notify();
        }
    }

    /// Writes `bytes` to `path` through a partial file, so a failure leaves
    /// nothing half written.
    fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let partial = path.with_extension("pdf.part");
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, path)
    }

    /// `report.pdf` becomes `report_Redacted.pdf`, as Acrobat names it.
    fn redacted_name(current: &Path) -> String {
        let stem = current
            .file_stem()
            .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
        format!("{stem}_Redacted.pdf")
    }

    fn marked_said(count: usize) -> String {
        match count {
            1 => {
                "Marked 1 area for redaction. Nothing is removed until Apply Redactions.".to_owned()
            }
            count => format!(
                "Marked {count} areas for redaction. Nothing is removed until Apply Redactions."
            ),
        }
    }

    /// What applying says it did.
    fn applied_said(applied: &Applied, path: &Path) -> String {
        let report = &applied.report;
        let counts = report.counts;
        let mut parts = Vec::new();
        let mut count = |number: usize, one: &str, many: &str| match number {
            0 => {}
            1 => parts.push(format!("1 {one}")),
            number => parts.push(format!("{number} {many}")),
        };
        count(counts.glyphs, "character", "characters");
        count(counts.images + counts.inline_images, "image", "images");
        count(counts.paths, "drawing", "drawings");
        count(report.annotations, "annotation", "annotations");
        let removed = if parts.is_empty() {
            "nothing under the marks".to_owned()
        } else {
            parts.join(", ")
        };
        let pages = report.pages.len();
        let mut said = format!(
            "Saved {} with {removed} removed on {pages} page{}. The check found nothing left.",
            path.display(),
            if pages == 1 { "" } else { "s" },
        );
        if let Some(sanitized) = report.sanitized {
            let hidden = sanitized.metadata
                + sanitized.scripts
                + sanitized.actions
                + sanitized.attachments
                + sanitized.comments
                + sanitized.private_data;
            said.push_str(&format!(
                " Hidden information removed: {hidden} items, and {} hidden layer drawings.",
                sanitized.hidden_layers
            ));
        }
        said
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use onionskin_redact::{Report, Sanitized, Verification};

        #[test]
        fn names_and_notices() {
            assert_eq!(
                redacted_name(Path::new("/a/report.pdf")),
                "report_Redacted.pdf"
            );
            assert!(marked_said(1).starts_with("Marked 1 area"));
            assert!(marked_said(3).starts_with("Marked 3 areas"));
            let mut report = Report {
                pages: vec![0],
                ..Report::default()
            };
            report.counts.glyphs = 6;
            report.counts.images = 1;
            let applied = Applied {
                bytes: Vec::new(),
                report,
                verification: Verification::default(),
            };
            assert_eq!(
                applied_said(&applied, Path::new("x.pdf")),
                "Saved x.pdf with 6 characters, 1 image removed on 1 page. The check found nothing left."
            );
            let mut sanitized = applied.clone();
            sanitized.report.pages = vec![0, 1];
            sanitized.report.counts = Default::default();
            sanitized.report.sanitized = Some(Sanitized {
                metadata: 2,
                hidden_layers: 4,
                ..Sanitized::default()
            });
            let said = applied_said(&sanitized, Path::new("x.pdf"));
            assert!(
                said.contains("nothing under the marks removed on 2 pages"),
                "{said}"
            );
            assert!(
                said.ends_with("2 items, and 4 hidden layer drawings."),
                "{said}"
            );
        }
    }
}
