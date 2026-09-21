//! The Stamps dialog's half in the frame, Paste Clipboard Image as Stamp,
//! and the file a file-placing tool asks for.
//!
//! The stamp tool is found by its capability, and what it can place is its
//! own list: the frame keeps no copy. The custom stamp library is
//! `tools-comment`'s, in the folder the shell hands every tool; this module
//! adds to it and deletes from it, and a build without the plugin shows the
//! entries that open this disabled.

use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions, Window};
use onionskin_plugin_api::{tool_with, ToolCapability};

use super::{clipboard_image, ShellFrame};
use crate::shell::chrome::stamps_dialog::{rows, Row, StampAction, StampsDialogState};
use crate::shell::dialog::ShellDialog;
use crate::shell::Canvas;

/// What the Stamps entries say in a build with no stamp tool.
pub(in crate::shell) const NO_STAMP_TOOL: &str = "No installed tool places stamps";

impl ShellFrame {
    pub(super) fn open_stamps_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_dialog(ShellDialog::Stamps, window, cx);
        self.stamps = Some(StampsDialogState::default());
    }

    /// The active tab's stamp tool: its canvas and its index.
    fn stamp_tool(&self, cx: &gpui::App) -> Option<(gpui::Entity<Canvas>, usize)> {
        let canvas = self.tabs.active()?.canvas.clone();
        let index = tool_with(canvas.read(cx).model.registry(), ToolCapability::Stamp)?;
        Some((canvas, index))
    }

    /// The dialog's rows, from the stamp tool's own list.
    pub(in crate::shell) fn stamps_rows(&self, cx: &gpui::App) -> Vec<Row> {
        let error = self
            .stamps
            .as_ref()
            .and_then(|state| state.error.as_deref());
        let Some((canvas, index)) = self.stamp_tool(cx) else {
            return rows(&[], None, Some(NO_STAMP_TOOL));
        };
        let canvas = canvas.read(cx);
        let tool = canvas.model.registry().tools().nth(index);
        let choices = tool.map(|tool| tool.choices()).unwrap_or_default();
        let chosen = tool.and_then(|tool| tool.chosen());
        rows(&choices, chosen.as_deref(), error)
    }

    pub(in crate::shell) fn run_stamp_action(
        &mut self,
        action: StampAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.stamps.as_mut() {
            state.error = None;
        }
        let outcome = match action {
            StampAction::Choose(id) => self.choose_stamp(&id, cx).map(|()| {
                self.close_dialog(window, cx);
            }),
            StampAction::Delete(id) => delete_custom(&self.settings.paths.data, &id),
            StampAction::CreateCustom => {
                self.prompt_for_custom_stamp(cx);
                Ok(())
            }
            StampAction::PasteClipboard => self.paste_clipboard_stamp(cx).map(|()| {
                self.close_dialog(window, cx);
            }),
        };
        if let Err(error) = outcome {
            self.report_stamp_error(error, cx);
        }
        cx.notify();
    }

    /// Paste Clipboard Image as Stamp, from the menu: the image becomes the
    /// stamp the tool places, and the tool is chosen.
    pub(super) fn paste_stamp_from_menu(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        if let Err(error) = self.paste_clipboard_stamp(cx) {
            self.notices.push(error);
        }
        cx.notify();
    }

    fn report_stamp_error(&mut self, error: String, cx: &mut Context<Self>) {
        match self.stamps.as_mut() {
            Some(state) => state.error = Some(error),
            None => self.notices.push(error),
        }
        cx.notify();
    }

    /// Choose `id` on the stamp tool and switch to it.
    fn choose_stamp(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let (canvas, index) = self.stamp_tool(cx).ok_or(NO_STAMP_TOOL)?;
        let chosen = canvas.update(cx, |canvas, _| canvas.model.choose_tool(index, id));
        if !chosen {
            return Err(format!("There is no stamp {id:?} any more"));
        }
        let entry = self.active_rail_entry(index, cx);
        self.activate_canvas_tool(index, "Stamp", entry, cx);
        Ok(())
    }

    fn paste_clipboard_stamp(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let bytes = clipboard_image(cx)?;
        let pdf = super::create::import(&bytes)?;
        let id = add_custom(
            &self.settings.paths.data,
            "Pasted",
            "Clipboard Image",
            &pdf,
            true,
        )?;
        self.choose_stamp(&id, cx)
    }

    fn prompt_for_custom_stamp(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Create Stamp".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.create_custom_stamp(&source, cx))
                .ok();
        })
        .detach();
    }

    /// A custom stamp from `source`: a PDF's first page, or an image made
    /// into one. Named after the file, under Custom.
    pub(super) fn create_custom_stamp(&mut self, source: &Path, cx: &mut Context<Self>) {
        let made = std::fs::read(source)
            .map_err(|error| format!("{} could not be read: {error}", source.display()))
            .and_then(|bytes| {
                if bytes.starts_with(b"%PDF") {
                    Ok(bytes)
                } else {
                    super::create::import(&bytes)
                }
            })
            .and_then(|pdf| {
                let name = source
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Stamp".to_owned());
                add_custom(&self.settings.paths.data, "Custom", &name, &pdf, false)
            });
        if let Err(error) = made {
            self.report_stamp_error(error, cx);
        }
        cx.notify();
    }

    /// A tool that places a file the user picks has just been chosen: ask
    /// which file, and hand the tool its path.
    pub(super) fn prompt_for_tool_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    frame.give_tool_file(&canvas, index, &path, cx)
                })
                .ok();
        })
        .detach();
    }

    pub(super) fn give_tool_file(
        &mut self,
        canvas: &gpui::Entity<Canvas>,
        index: usize,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let taken = canvas.update(cx, |canvas, _| {
            canvas.model.choose_tool(index, &path.to_string_lossy())
        });
        if !taken {
            self.notices
                .push(format!("{} cannot be used here", path.display()));
            cx.notify();
        }
    }
}

/// Whether tool `index` of `canvas` asks for a file when chosen.
pub(super) fn chooses_file(canvas: &Canvas, index: usize) -> bool {
    canvas
        .model
        .registry()
        .tools()
        .nth(index)
        .is_some_and(|tool| tool.capabilities().contains(&ToolCapability::ChoosesFile))
}

#[cfg(feature = "tools-comment")]
fn library(data: &Option<PathBuf>) -> Result<onionskin_tools_comment::StampLibrary, String> {
    data.as_deref()
        .map(onionskin_tools_comment::library_in)
        .ok_or_else(|| "There is no folder to keep custom stamps in".to_owned())
}

#[cfg(feature = "tools-comment")]
fn add_custom(
    data: &Option<PathBuf>,
    category: &str,
    name: &str,
    pdf: &[u8],
    replace: bool,
) -> Result<String, String> {
    library(data)?
        .add(category, name, pdf, 0, replace)
        .map(|stamp| stamp.id())
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tools-comment")]
fn delete_custom(data: &Option<PathBuf>, id: &str) -> Result<(), String> {
    let library = library(data)?;
    let stamp = library
        .find(id)
        .ok_or_else(|| "Only a custom stamp can be deleted".to_owned())?;
    library.remove(&stamp).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tools-comment"))]
fn add_custom(_: &Option<PathBuf>, _: &str, _: &str, _: &[u8], _: bool) -> Result<String, String> {
    Err(NO_STAMP_TOOL.to_owned())
}

#[cfg(not(feature = "tools-comment"))]
fn delete_custom(_: &Option<PathBuf>, _: &str) -> Result<(), String> {
    Err(NO_STAMP_TOOL.to_owned())
}
