//! The Combine and Split dialogs' half in the frame: opening them, the file
//! pickers, and running what they ask for.
//!
//! The work itself is `commands-core`'s. This build may not have it - the
//! plugin is optional - and then the File menu entries that open these
//! dialogs are disabled, saying so; the handlers still compile, and report the
//! plugin missing if they are reached some other way.

use std::path::PathBuf;

use gpui::{Context, PathPromptOptions, Window};

use super::ShellFrame;
use crate::shell::chrome::combine_dialog::{
    CombineAction, CombineDialogState, CombineEntry, CombineEntryPoint,
};
use crate::shell::chrome::split_dialog::{SplitAction, SplitChoice, SplitDialogState};
use crate::shell::dialog::ShellDialog;

/// The organize dialogs' state. Only the one on screen is `Some`.
#[derive(Default)]
pub(in crate::shell) struct OrganizeDialogs {
    pub(in crate::shell) combine: Option<CombineDialogState>,
    pub(in crate::shell) split: Option<SplitDialogState>,
}

/// What a build without `commands-core` says when asked to combine or split.
pub(in crate::shell) const NO_CORE_COMMANDS: &str = "The core commands plugin is not installed";

impl ShellFrame {
    pub(in crate::shell) fn combine_dialog(&self) -> Option<&CombineDialogState> {
        self.organize.combine.as_ref()
    }

    pub(in crate::shell) fn split_dialog(&self) -> Option<&SplitDialogState> {
        self.organize.split.as_ref()
    }

    pub(super) fn open_combine_dialog(
        &mut self,
        entry_point: CombineEntryPoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.shell_view_state.tokens();
        let state = CombineDialogState::new(entry_point, theme, cx);
        self.show_dialog(ShellDialog::Combine(entry_point), window, cx);
        self.organize.combine = Some(state);
    }

    pub(super) fn open_split_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = self.shell_view_state.tokens();
        let state = SplitDialogState::new(theme, cx);
        self.show_dialog(ShellDialog::Split, window, cx);
        self.organize.split = Some(state);
    }

    pub(in crate::shell) fn run_combine_action(
        &mut self,
        action: CombineAction,
        cx: &mut Context<Self>,
    ) {
        let pages = self.combine_dialog().map(|state| state.pages_text(cx));
        let Some(state) = self.organize.combine.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            CombineAction::AddFiles => self.prompt_for_inputs(false, cx),
            CombineAction::AddFolder => self.prompt_for_inputs(true, cx),
            CombineAction::Submit => self.prompt_for_combined_output(cx),
            CombineAction::Select(index) => state.list.select(index),
            CombineAction::MoveUp => state.list.move_selected(true),
            CombineAction::MoveDown => state.list.move_selected(false),
            CombineAction::Remove => state.list.remove_selected(),
            CombineAction::ApplyPages => {
                if let Err(error) = state.list.set_pages(&pages.unwrap_or_default()) {
                    state.error = Some(error);
                }
            }
        }
        cx.notify();
    }

    pub(in crate::shell) fn run_split_action(
        &mut self,
        action: SplitAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.organize.split.as_mut() else {
            return;
        };
        state.error = None;
        match action {
            SplitAction::SetMode(mode) => state.mode = mode,
            SplitAction::Submit => match state.choice(cx) {
                Ok(choice) => self.split_active_document(choice, window, cx),
                Err(error) => state.error = Some(error),
            },
        }
        cx.notify();
    }

    /// Add Files, or Add Folder: pick, then add what was picked, each with
    /// its page count read for the row's preview.
    fn prompt_for_inputs(&mut self, folder: bool, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: !folder,
            prompt: Some(if folder { "Add Folder" } else { "Add Files" }.into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    let entries = expand_inputs(&paths, folder);
                    if let Some(state) = frame.organize.combine.as_mut() {
                        match entries {
                            Ok(entries) => state.list.add(entries),
                            Err(error) => state.error = Some(error),
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Combine: ask where, then combine in the background and open the result.
    fn prompt_for_combined_output(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.organize.combine.as_mut() else {
            return;
        };
        let inputs = state.list.inputs();
        let directory = state
            .list
            .output_directory()
            .map_or_else(|| PathBuf::from("."), PathBuf::from);
        state.running = true;
        let chosen = cx.prompt_for_new_path(&directory, Some("Combined.pdf"));
        cx.spawn(async move |frame, cx| {
            let output = match chosen.await {
                Ok(Ok(Some(path))) => path,
                _ => {
                    frame
                        .update(cx, |frame, cx| frame.combine_finished(None, cx))
                        .ok();
                    return;
                }
            };
            let job_output = output.clone();
            let outcome = cx
                .background_executor()
                .spawn(async move { combine_files(&inputs, &job_output) })
                .await;
            frame
                .update(cx, |frame, cx| {
                    frame.combine_finished(Some((output, outcome)), cx)
                })
                .ok();
        })
        .detach();
    }

    fn combine_finished(
        &mut self,
        outcome: Option<(PathBuf, Result<String, String>)>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.organize.combine.as_mut() else {
            return;
        };
        state.running = false;
        match outcome {
            None => {}
            Some((output, Ok(summary))) => {
                self.notices.push(summary);
                self.dialog = None;
                self.organize.combine = None;
                self.open_documents(&[output], cx);
            }
            Some((_, Err(error))) => state.error = Some(error),
        }
        cx.notify();
    }

    /// Split the active document beside itself.
    fn split_active_document(
        &mut self,
        choice: SplitChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let outcome = canvas.update(cx, |canvas, _| {
            split_document(canvas.model.document_mut(), choice)
        });
        match outcome {
            Ok(summary) => {
                self.notices.push(summary);
                self.close_dialog(window, cx);
            }
            Err(error) => {
                if let Some(state) = self.organize.split.as_mut() {
                    state.error = Some(error);
                }
            }
        }
    }
}

#[cfg(feature = "commands-core")]
fn expand_inputs(paths: &[PathBuf], folder: bool) -> Result<Vec<CombineEntry>, String> {
    use onionskin_commands_core::combine;

    let files = if folder {
        let mut files = Vec::new();
        for dir in paths {
            files.extend(
                combine::pdfs_in_folder(dir)
                    .map_err(|error| format!("{}: {error}", dir.display()))?,
            );
        }
        files
    } else {
        paths.to_vec()
    };
    Ok(files
        .into_iter()
        .map(|path| CombineEntry {
            page_count: combine::page_count(&path).map_err(|error| error.to_string()),
            path,
            pages: None,
        })
        .collect())
}

#[cfg(not(feature = "commands-core"))]
fn expand_inputs(_: &[PathBuf], _: bool) -> Result<Vec<CombineEntry>, String> {
    Err(NO_CORE_COMMANDS.to_owned())
}

#[cfg(feature = "commands-core")]
fn combine_files(
    inputs: &[(PathBuf, Option<Vec<usize>>)],
    output: &std::path::Path,
) -> Result<String, String> {
    use onionskin_commands_core::combine::{self, Input};
    use onionskin_core::pages::Tagging;

    let inputs: Vec<Input> = inputs
        .iter()
        .map(|(path, pages)| Input {
            path: path.clone(),
            pages: pages.clone(),
        })
        .collect();
    let combined = combine::combine(&inputs, output).map_err(|error| error.to_string())?;
    let tagging = match combined.tagging {
        Tagging::Tagged => "",
        Tagging::Untagged(_) => " It is untagged: not every input was a whole tagged document.",
    };
    Ok(format!(
        "Combined {} pages into {}.{tagging}",
        combined.page_count,
        output.display()
    ))
}

#[cfg(not(feature = "commands-core"))]
fn combine_files(
    _: &[(PathBuf, Option<Vec<usize>>)],
    _: &std::path::Path,
) -> Result<String, String> {
    Err(NO_CORE_COMMANDS.to_owned())
}

#[cfg(feature = "commands-core")]
fn split_document(
    document: &mut onionskin_core::Document,
    choice: SplitChoice,
) -> Result<String, String> {
    use onionskin_commands_core::split::{self, SplitBy};

    let path = document
        .path()
        .ok_or("Save the document to a file first: the parts are written beside it")?
        .to_path_buf();
    let folder = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = path
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    let by = match choice {
        SplitChoice::PageCount(count) => SplitBy::PageCount(count),
        SplitChoice::FileSize { bytes } => SplitBy::FileSize(bytes),
        SplitChoice::TopLevelBookmarks => SplitBy::TopLevelBookmarks,
    };
    let written = split::split(document, by, folder, &stem).map_err(|error| error.to_string())?;
    let mut summary = format!(
        "Split into {} files in {}.",
        written.files.len(),
        folder.display()
    );
    if !written.unresolved.is_empty() {
        summary.push_str(&format!(
            " These bookmarks name no page and did not start a file: {}.",
            written.unresolved.join(", ")
        ));
    }
    Ok(summary)
}

#[cfg(not(feature = "commands-core"))]
fn split_document(_: &mut onionskin_core::Document, _: SplitChoice) -> Result<String, String> {
    Err(NO_CORE_COMMANDS.to_owned())
}
