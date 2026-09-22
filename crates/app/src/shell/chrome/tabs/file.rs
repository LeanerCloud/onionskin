//! The frame's half of P18: Save, Save As, Revert, Undo and Redo on the
//! active document; the question before closing unsaved documents; autosave
//! and the recovery offer.
//!
//! Every one of these is reached through [`ShellFrame::run_menu_command`] or
//! an [`Activation`], never from a global action listener directly, so each
//! runs deferred and inside the window's own update.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Context, Entity, EntityId, Window};

use super::ShellFrame;
use crate::shell::canvas::rank_offers;
use crate::shell::chrome::file_dialogs::{FileAction, PendingClose, RecoverState, UnsavedState};
use crate::shell::chrome::global_bar::refresh_native_menus;
use crate::shell::dialog::ShellDialog;
use crate::shell::Canvas;

/// How often dirty documents are written to their recovery file.
pub(super) const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

impl ShellFrame {
    fn active_canvas_entity(&self) -> Option<Entity<Canvas>> {
        self.tabs.active().map(|tab| tab.canvas.clone())
    }

    fn canvas_by_id(&self, id: EntityId) -> Option<(usize, Entity<Canvas>)> {
        self.tabs
            .tabs()
            .iter()
            .position(|tab| tab.canvas.entity_id() == id)
            .map(|index| (index, self.tabs.tabs()[index].canvas.clone()))
    }

    /// After anything that changed the document or its file: the pane, the
    /// menus and the tab's dirty mark all read it again. The pane is read
    /// again rather than emptied, so the comment chosen in the Comments pane
    /// stays chosen across an Undo.
    fn after_file_change(&mut self, cx: &mut Context<Self>) {
        if let Some(canvas) = self.active_canvas_entity() {
            self.navigation.reread(&canvas, cx);
        }
        self.observed_view_state = self.active_view_state(cx);
        self.sync_page_entry(cx);
        refresh_native_menus(cx, self.menu_state(cx));
        cx.notify();
    }

    fn report(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        if let Err(message) = result {
            self.notices.push(message);
        }
        self.after_file_change(cx);
    }

    /// Reduce File Size: ask where the copy goes, then compress into it.
    fn reduce_active_file_size(&mut self, cx: &mut Context<Self>) {
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
        let suggested = reduced_name(&current);
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.reduce_into(origin, &path, cx))
                .ok();
        })
        .detach();
    }

    /// The second half of Reduce File Size, for the document the dialog was
    /// opened on.
    pub(in crate::shell) fn reduce_into(
        &mut self,
        origin: EntityId,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let Some((_, canvas)) = self.canvas_by_id(origin) else {
            return;
        };
        let message = canvas.update(cx, |canvas, _| {
            reduce_document(&mut canvas.model.document_mut(), path)
        });
        self.notices.push(match message {
            Ok(message) | Err(message) => message,
        });
        cx.notify();
    }

    /// File > Attach to Email: the saved file, handed to the mail client.
    pub(super) fn attach_active_to_email(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_saved_path(cx) else {
            return;
        };
        if let Err(message) = crate::shell::share::attach_to_email(&path) {
            self.notices.push(message);
        }
        cx.notify();
    }

    /// File > Copy File to Clipboard: the saved file's `file://` URI, which
    /// a file manager or a mail client resolves back to the file.
    pub(super) fn copy_active_file_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_saved_path(cx) else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            crate::shell::share::file_uri(&path),
        ));
    }

    /// Edit > Cut, Copy, Paste or Delete, run by the active tool.
    pub(super) fn run_edit_verb(
        &mut self,
        verb: onionskin_plugin_api::EditVerb,
        cx: &mut Context<Self>,
    ) {
        let Some(canvas) = self.active_canvas_entity() else {
            return;
        };
        let pasted = cx.read_from_clipboard().and_then(|item| item.text());
        let copied = canvas.update(cx, |canvas, cx| {
            let copied = canvas.model.run_edit_verb(verb, pasted.as_deref());
            canvas.handle_change(Ok(true), cx);
            copied
        });
        if let Some(text) = copied {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
        cx.notify();
    }

    /// The active document's file, when it has one on disk.
    fn active_saved_path(&self, cx: &gpui::App) -> Option<std::path::PathBuf> {
        let canvas = self.active_canvas_entity()?;
        canvas.read(cx).model.path()
    }

    pub(super) fn undo_active(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas_entity() else {
            return;
        };
        let result = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.undo().map(|_| ());
            canvas.handle_change(Ok(true), cx);
            outcome.map_err(|error| format!("Undo failed: {error}"))
        });
        self.report(result, cx);
    }

    pub(super) fn redo_active(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas_entity() else {
            return;
        };
        let result = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.redo().map(|_| ());
            canvas.handle_change(Ok(true), cx);
            outcome.map_err(|error| format!("Redo failed: {error}"))
        });
        self.report(result, cx);
    }

    /// Save: to the document's own file, or, for one that has none, the same
    /// question Save As asks.
    pub(super) fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas_entity() else {
            return;
        };
        if canvas.read(cx).model.path().is_none() {
            self.save_active_as(cx);
            return;
        }
        let result = save(&canvas, cx);
        self.report(result, cx);
    }

    /// Save As: ask where, then write there. The tab goes on showing the
    /// same document, which now belongs to the new file.
    pub(super) fn save_active_as(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas_entity() else {
            return;
        };
        let origin = canvas.entity_id();
        let current = canvas.read(cx).model.path();
        let directory = current
            .as_deref()
            .and_then(Path::parent)
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let suggested = current.as_deref().and_then(Path::file_name).map_or_else(
            || "Untitled.pdf".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.save_canvas_as(origin, &path, cx))
                .ok();
        })
        .detach();
    }

    /// The second half of Save As, for the document the dialog was opened
    /// on, whichever tab is active by the time it answers.
    pub(in crate::shell) fn save_canvas_as(
        &mut self,
        origin: EntityId,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let Some((index, canvas)) = self.canvas_by_id(origin) else {
            return;
        };
        let result = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.save_as(path);
            canvas.handle_change(Ok(true), cx);
            outcome.map_err(|error| format!("{} was not saved: {error}", path.display()))
        });
        if result.is_ok() {
            self.tabs.retitle(index, path.to_path_buf());
        }
        self.report(result, cx);
    }

    /// Revert: throw the unsaved edits away and read the file again.
    pub(super) fn revert_active(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas_entity() else {
            return;
        };
        let result = canvas.update(cx, |canvas, cx| {
            let outcome = canvas.model.revert();
            canvas.handle_change(Ok(true), cx);
            outcome.map_err(|error| format!("Revert failed: {error}"))
        });
        self.report(result, cx);
    }

    /// Whether the tab holding `canvas` has unsaved changes.
    pub(super) fn is_dirty(canvas: &Entity<Canvas>, cx: &gpui::App) -> bool {
        canvas.read(cx).model.history_facts().dirty
    }

    /// Whether closing the tab holding `canvas` would lose unsaved changes:
    /// it is dirty and no other window shows the document. Closing one of
    /// two windows on a document loses nothing; the other still has it.
    pub(super) fn close_loses_changes(canvas: &Entity<Canvas>, cx: &gpui::App) -> bool {
        Self::is_dirty(canvas, cx) && canvas.read(cx).model.other_windows() == 0
    }

    /// Before a close: the documents it would lose unsaved changes in. When
    /// there are any, the question is asked and `true` returned, and the close
    /// waits for the answer.
    pub(super) fn ask_before_closing(
        &mut self,
        close: PendingClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let unsaved: Vec<(EntityId, String)> = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| match close {
                PendingClose::Tab(id) => tab.canvas.entity_id() == id,
                PendingClose::Others(kept) => tab.canvas.entity_id() != kept,
                PendingClose::All => true,
            })
            .filter(|tab| Self::close_loses_changes(&tab.canvas, cx))
            .map(|tab| (tab.canvas.entity_id(), tab.title().to_owned()))
            .collect();
        if unsaved.is_empty() {
            return false;
        }
        self.show_dialog(ShellDialog::UnsavedChanges, window, cx);
        self.unsaved = Some(UnsavedState {
            close,
            unsaved,
            error: None,
        });
        true
    }

    pub(in crate::shell) fn unsaved_dialog(&self) -> Option<&UnsavedState> {
        self.unsaved.as_ref()
    }

    pub(in crate::shell) fn recover_dialog(&self) -> Option<&RecoverState> {
        self.recover.as_ref()
    }

    pub(in crate::shell) fn run_file_action(
        &mut self,
        action: FileAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            FileAction::CancelClose | FileAction::CancelReduce => self.close_dialog(window, cx),
            FileAction::ReduceFileSize => {
                self.close_dialog(window, cx);
                self.reduce_active_file_size(cx);
            }
            FileAction::SaveAndClose | FileAction::DiscardAndClose => {
                let Some(state) = self.unsaved.take() else {
                    return;
                };
                if action == FileAction::SaveAndClose {
                    if let Err(error) = self.save_all(&state.unsaved, cx) {
                        self.unsaved = Some(UnsavedState {
                            error: Some(error),
                            ..state
                        });
                        cx.notify();
                        return;
                    }
                }
                self.close_dialog(window, cx);
                self.finish_close(state.close, window, cx);
            }
            FileAction::Recover | FileAction::DiscardRecovery => {
                let Some(state) = self.recover.take() else {
                    return;
                };
                self.close_dialog(window, cx);
                if let Some((_, canvas)) = self.canvas_by_id(state.canvas) {
                    let result = canvas.update(cx, |canvas, cx| {
                        let outcome = if action == FileAction::Recover {
                            canvas.model.accept_recovery(&state.offer)
                        } else {
                            canvas.model.discard_recovery()
                        };
                        canvas.handle_change(Ok(true), cx);
                        outcome.map_err(|error| format!("Recovery failed: {error}"))
                    });
                    self.report(result, cx);
                }
                self.offer_next_recovery(window, cx);
            }
        }
    }

    /// Save every listed document. A document with no file cannot be saved
    /// without asking where, which a close does not stop to do: it is named
    /// in the error instead, and nothing closes.
    fn save_all(
        &mut self,
        unsaved: &[(EntityId, String)],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        for (id, title) in unsaved {
            let Some((_, canvas)) = self.canvas_by_id(*id) else {
                continue;
            };
            if canvas.read(cx).model.path().is_none() {
                return Err(format!(
                    "{title} has no file yet: use Save As, then close it"
                ));
            }
            save(&canvas, cx)?;
        }
        Ok(())
    }

    /// Run the close the dialog was asked about, now that it has an answer.
    fn finish_close(&mut self, close: PendingClose, window: &mut Window, cx: &mut Context<Self>) {
        use super::TabCommand;

        let (command, index) = match close {
            PendingClose::Tab(id) => match self.canvas_by_id(id) {
                Some((index, _)) => (TabCommand::Close, index),
                None => return,
            },
            PendingClose::Others(id) => match self.canvas_by_id(id) {
                Some((index, _)) => (TabCommand::CloseOthers, index),
                None => return,
            },
            PendingClose::All => match self.tabs.active_index() {
                Some(index) => (TabCommand::CloseAll, index),
                None => return,
            },
        };
        let _ = window;
        if let Err(error) = self.run_tab_command(command, index, cx) {
            self.notices.push(error.to_string());
        }
    }

    /// A close the user asked for: asks first when it would lose unsaved
    /// changes, and closes straight away otherwise. `run_tab_command` itself
    /// never asks, which is what the dialog's answer runs.
    pub(super) fn request_tab_command(
        &mut self,
        command: super::TabCommand,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), super::TabError> {
        use super::TabCommand;

        let id = self
            .tabs
            .tabs()
            .get(index)
            .map(|tab| tab.canvas.entity_id());
        let close = match (command, id) {
            (TabCommand::Close, Some(id)) => Some(PendingClose::Tab(id)),
            (TabCommand::CloseOthers, Some(id)) => Some(PendingClose::Others(id)),
            (TabCommand::CloseAll, _) => Some(PendingClose::All),
            _ => None,
        };
        if let Some(close) = close {
            self.menus.main_menu_open = false;
            self.context_menus.tab_context_menu = None;
            if self.ask_before_closing(close, window, cx) {
                return Ok(());
            }
        }
        self.run_tab_command(command, index, cx)
    }

    /// Turn autosave on for the documents just opened, and queue the offer
    /// of any recovery waiting for them. A store that refuses to open, most
    /// often a directory someone widened, is said once and autosave stays
    /// off: document content does not go into a directory others can read.
    pub(super) fn attach_recovery(&mut self, opened: &[EntityId], cx: &mut Context<Self>) {
        let Some(dir) = self.settings.paths.recovery.clone() else {
            return;
        };
        let store = match onionskin_core::RecoveryStore::open(&dir) {
            Ok(store) => store,
            Err(error) => {
                self.notices.push(format!("Autosave is off: {error}"));
                return;
            }
        };
        for id in opened {
            if let Some((_, canvas)) = self.canvas_by_id(*id) {
                canvas.update(cx, |canvas, _| canvas.model.set_recovery(store.clone()));
            }
        }
        self.queue_recovery_offers(opened, cx);
    }

    /// Autosave every so often, for as long as the window is open.
    pub(super) fn start_autosave(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |frame, cx| loop {
            cx.background_executor().timer(AUTOSAVE_INTERVAL).await;
            if frame
                .update(cx, |frame, cx| frame.autosave_all(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    /// Autosave every dirty document. Save is synchronous on this thread, so
    /// an autosave cannot run in the middle of one.
    pub(super) fn autosave_all(&mut self, cx: &mut Context<Self>) {
        for canvas in self.canvases() {
            if let Err(error) = canvas.read(cx).model.autosave() {
                self.notices.push(format!("Autosave failed: {error}"));
                cx.notify();
            }
        }
    }

    /// Queue the recovery offers for the documents just opened, most recent
    /// first, and ask about the first.
    pub(super) fn queue_recovery_offers(&mut self, opened: &[EntityId], cx: &mut Context<Self>) {
        let mut offers = Vec::new();
        for id in opened {
            let Some((index, canvas)) = self.canvas_by_id(*id) else {
                continue;
            };
            match canvas.read(cx).model.recovery_offer() {
                Ok(Some(offer)) => {
                    offers.push((offer, *id, self.tabs.tabs()[index].title().to_owned()))
                }
                Ok(None) => {}
                Err(error) => self
                    .notices
                    .push(format!("Recovery could not be read: {error}")),
            }
        }
        let ranked = rank_offers(offers.iter().map(|(offer, _, _)| offer.clone()).collect());
        for offer in ranked {
            if let Some((_, id, title)) = offers.iter().find(|(o, _, _)| *o == offer) {
                self.pending_recoveries.push_back(RecoverState {
                    canvas: *id,
                    title: title.clone(),
                    offer: offer.clone(),
                });
            }
        }
        cx.notify();
    }

    /// Ask about the next queued recovery, when no other dialog is up.
    /// Called from render, which is the first place a window is at hand
    /// after documents open.
    pub(super) fn offer_next_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        let Some(next) = self.pending_recoveries.pop_front() else {
            return;
        };
        self.show_dialog(ShellDialog::Recover, window, cx);
        self.recover = Some(next);
    }
}

fn save(canvas: &Entity<Canvas>, cx: &mut Context<ShellFrame>) -> Result<(), String> {
    canvas.update(cx, |canvas, cx| {
        let outcome = canvas.model.save();
        canvas.handle_change(Ok(true), cx);
        outcome.map_err(|error| {
            let name = canvas.model.path().map_or_else(
                || "The document".to_owned(),
                |path| path.display().to_string(),
            );
            format!("{name} was not saved: {error}")
        })
    })
}

/// The name Reduce File Size suggests for the copy.
fn reduced_name(current: &Path) -> String {
    let stem = current
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    format!("{stem} (reduced).pdf")
}

/// Compress `document` into a new file at `path`, and say what happened.
#[cfg(feature = "commands-core")]
fn reduce_document(document: &mut onionskin_core::Document, path: &Path) -> Result<String, String> {
    use onionskin_commands_core::compress::{
        compress, CompressOptions, Compressed, NOTHING_TO_COMPRESS,
    };

    match compress(document, &CompressOptions::default()).map_err(|error| error.to_string())? {
        Compressed::Nothing => Err(NOTHING_TO_COMPRESS.to_owned()),
        Compressed::Smaller { bytes, before, .. } => {
            let after = bytes.len();
            onionskin_commands_core::publish::publish(&[(path.to_path_buf(), bytes)])
                .map_err(|error| error.to_string())?;
            Ok(format!(
                "Saved a reduced copy as {}: {} KB, from {} KB. It keeps no editing history.",
                path.display(),
                after.div_ceil(1024),
                before.div_ceil(1024)
            ))
        }
    }
}

#[cfg(not(feature = "commands-core"))]
fn reduce_document(_: &mut onionskin_core::Document, _: &Path) -> Result<String, String> {
    Err(super::NO_CORE_COMMANDS.to_owned())
}
