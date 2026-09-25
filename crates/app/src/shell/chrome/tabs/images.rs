//! The Edit menu's image entries, and the picture the Add Image tool
//! places: the frame's half.
//!
//! Turning and flipping are registered commands. Replace Image asks for a
//! picture and puts it where the selected image is; Save Image As writes the
//! selected image as `codecs-common` extracts it. A picture is a PDF's first
//! page, or an image file made into one by a codec, so the tools in
//! `tools-edit` only ever read PDFs.

use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions};
use onionskin_plugin_api::ToolCapability;

use super::ShellFrame;
use crate::shell::chrome::image_commands::ImageCommand;
use crate::shell::Canvas;

/// What the image entries say in a build with no tool that selects images.
pub(in crate::shell) const NO_IMAGE_TOOL: &str = "No installed tool edits images";

/// What an image entry says when the Edit Image tool has not selected one.
pub(in crate::shell) const NO_IMAGE_SELECTED: &str =
    "No image is selected: click one with the Edit Image tool";

impl ShellFrame {
    pub(super) fn run_image_command(&mut self, image: ImageCommand, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        match (image, image.registry_id()) {
            (_, Some(id)) => self.run_registry_command(id, cx),
            (ImageCommand::Replace, None) => self.prompt_for_replacement(cx),
            (_, None) => self.prompt_to_save_image(cx),
        }
    }

    fn prompt_for_replacement(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        if canvas.read(cx).model.image_selection().is_none() {
            self.notices.push(NO_IMAGE_SELECTED.to_owned());
            cx.notify();
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Replace Image".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.replace_image(&canvas, &source, cx))
                .ok();
        })
        .detach();
    }

    /// Put the picture in `source` where `canvas`'s selected image is.
    pub(super) fn replace_image(
        &mut self,
        canvas: &gpui::Entity<Canvas>,
        source: &Path,
        cx: &mut Context<Self>,
    ) {
        let outcome = read_picture(source).and_then(|pdf| {
            canvas.update(cx, |canvas, cx| {
                let outcome = canvas
                    .model
                    .edit_pages(|doc| image_edits::replace(doc, pdf))
                    .map_err(|error| super::properties::sentence(&error.to_string()));
                if outcome.is_ok() {
                    canvas.handle_change(Ok(true), cx);
                }
                outcome
            })
        });
        if let Err(error) = outcome {
            self.notices.push(error);
        }
        cx.notify();
    }

    fn prompt_to_save_image(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let directory = tab
            .path(cx)
            .as_deref()
            .and_then(Path::parent)
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let extracted = selected_image_file(tab.canvas.read(cx));
        let image = match extracted {
            Ok(image) => image,
            Err(error) => {
                self.notices.push(error);
                cx.notify();
                return;
            }
        };
        let chosen = cx.prompt_for_new_path(&directory, Some(&image.0));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(output))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    frame.write_image(&output, &image.1);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Write the image's bytes to `output` and say how it went.
    pub(super) fn write_image(&mut self, output: &Path, bytes: &[u8]) {
        self.notices
            .push(match super::create::write_replacing(output, bytes) {
                Ok(()) => format!("Saved the image to {}", output.display()),
                Err(error) => format!("{} was not written: {error}", output.display()),
            });
    }

    /// Hand tool `index` of `canvas` the file at `path`: a tool that places
    /// a picture gets a PDF, made from an image file when that is what was
    /// picked, and kept in the data folder while the tool may use it.
    pub(super) fn tool_file(
        &self,
        canvas: &Canvas,
        index: usize,
        path: &Path,
    ) -> Result<PathBuf, String> {
        let places_image = canvas
            .model
            .registry()
            .tools()
            .nth(index)
            .is_some_and(|tool| tool.capabilities().contains(&ToolCapability::PlacesImage));
        if !places_image || is_pdf_file(path) {
            return Ok(path.to_path_buf());
        }
        let pdf = read_picture(path)?;
        let folder = self
            .settings
            .paths
            .data
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join("placed-images");
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{} could not be made: {error}", folder.display()))?;
        let stem = path
            .file_stem()
            .map_or_else(|| "picture".into(), |stem| stem.to_string_lossy());
        let made = folder.join(format!("{stem}.pdf"));
        std::fs::write(&made, pdf)
            .map_err(|error| format!("{} was not written: {error}", made.display()))?;
        Ok(made)
    }
}

fn is_pdf_file(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes.starts_with(b"%PDF"))
}

/// The picture in `source` as a PDF: a PDF as it is, an image through a
/// codec's import.
fn read_picture(source: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(source)
        .map_err(|error| format!("{} could not be read: {error}", source.display()))?;
    if bytes.starts_with(b"%PDF") {
        Ok(bytes)
    } else {
        super::create::import(&bytes)
    }
}

/// The selected image's suggested file name and bytes.
pub(super) fn selected_image_file(canvas: &Canvas) -> Result<(String, Vec<u8>), String> {
    let selection = canvas
        .model
        .image_selection()
        .ok_or_else(|| NO_IMAGE_SELECTED.to_owned())?;
    if let Some(refusal) = canvas.model.read_out_refusal() {
        return Err(format!("The image cannot be saved: {refusal}"));
    }
    let mut document = canvas.model.document_mut();
    image_edits::extract(&mut document, &selection)
}

/// The edits and the extraction, through the plugins when they are built
/// in, and refusals naming them when they are not.
mod image_edits {
    use onionskin_core::{Document, ImageSelection};
    use onionskin_plugin_api::CommandError;

    #[cfg(feature = "tools-edit")]
    pub(super) fn replace(doc: &mut Document, pdf: Vec<u8>) -> Result<(), CommandError> {
        onionskin_tools_edit::images::replace_selected(doc, pdf)
    }

    #[cfg(not(feature = "tools-edit"))]
    pub(super) fn replace(_: &mut Document, _: Vec<u8>) -> Result<(), CommandError> {
        Err(CommandError::Failed {
            label: "Replace Image",
            reason: super::NO_IMAGE_TOOL.to_owned(),
        })
    }

    #[cfg(feature = "codecs-common")]
    pub(super) fn extract(
        doc: &mut Document,
        selection: &ImageSelection,
    ) -> Result<(String, Vec<u8>), String> {
        let structure = doc.structure().map_err(|error| error.to_string())?;
        onionskin_codecs_common::extract_image(structure, selection.placement.image, selection.page)
            .map(|image| (image.name, image.bytes))
            .map_err(|reason| format!("The image cannot be saved: {reason}"))
    }

    #[cfg(not(feature = "codecs-common"))]
    pub(super) fn extract(
        _: &mut Document,
        _: &ImageSelection,
    ) -> Result<(String, Vec<u8>), String> {
        Err("The common codecs plugin is not installed".to_owned())
    }
}
