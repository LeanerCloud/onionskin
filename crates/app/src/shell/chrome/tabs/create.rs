//! File > Create's single sources and Export All Images: the frame's half.
//!
//! A new document is made by a codec's import half, found by the file's
//! signature, then written where the user chooses and opened in a tab. The
//! document is a file from the start rather than an untitled buffer, because
//! a session opened from bytes has nowhere to save to.
//!
//! Export All Images is `codecs-common`'s [`extract_images`], written into a
//! folder the user chooses. It never overwrites: a name already taken is
//! reported and skipped.
//!
//! [`extract_images`]: onionskin_codecs_common::extract_images

use std::io::Write as _;
use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions};

use super::{clipboard_image, ShellFrame};
use crate::shell::chrome::global_bar::NO_IMAGE_IMPORT;

impl ShellFrame {
    /// Create PDF From File: choose an image, make it a document.
    pub(super) fn create_from_file(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Create PDF".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.create_from_source(&source, cx))
                .ok();
        })
        .detach();
    }

    /// The chosen file, made into a document beside it.
    pub(super) fn create_from_source(&mut self, source: &Path, cx: &mut Context<Self>) {
        match std::fs::read(source) {
            Ok(bytes) => {
                let name = pdf_name_for(source);
                let directory = source.parent().map(Path::to_path_buf);
                self.create_from_bytes(&bytes, directory, &name, cx);
            }
            Err(error) => {
                self.notices
                    .push(format!("{} could not be read: {error}", source.display()));
                cx.notify();
            }
        }
    }

    /// Create PDF From Clipboard: the image on the pasteboard, as a document.
    pub(super) fn create_from_clipboard(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        match clipboard_image(cx) {
            Ok(bytes) => self.create_from_bytes(&bytes, None, "Clipboard.pdf", cx),
            Err(reason) => {
                self.notices.push(reason);
                cx.notify();
            }
        }
    }

    /// Import `bytes`, ask where the document goes, write it and open it.
    fn create_from_bytes(
        &mut self,
        bytes: &[u8],
        directory: Option<PathBuf>,
        suggested: &str,
        cx: &mut Context<Self>,
    ) {
        let pdf = match import(bytes) {
            Ok(pdf) => pdf,
            Err(reason) => {
                self.notices.push(reason);
                cx.notify();
                return;
            }
        };
        let directory = directory.unwrap_or_else(|| PathBuf::from("."));
        let chosen = cx.prompt_for_new_path(&directory, Some(suggested));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(output))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| match write_replacing(&output, &pdf) {
                    Ok(()) => frame.open_documents(&[output], cx),
                    Err(error) => {
                        frame
                            .notices
                            .push(format!("{} was not written: {error}", output.display()));
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Export All Images: choose a folder, write every image into it.
    pub(super) fn export_all_images(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        let Some(canvas) = self.tabs.active().map(|tab| tab.canvas.clone()) else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export Images Here".into()),
        });
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            frame
                .update(cx, |frame, cx| frame.export_images_to(&canvas, &folder, cx))
                .ok();
        })
        .detach();
    }

    /// Every image of `canvas`'s document, into `folder`, with a notice
    /// saying what was written and what was not.
    pub(super) fn export_images_to(
        &mut self,
        canvas: &gpui::Entity<crate::shell::Canvas>,
        folder: &Path,
        cx: &mut Context<Self>,
    ) {
        let summary = canvas.update(cx, |canvas, _| {
            extract_into(canvas.model.document_mut(), folder)
        });
        self.notices.push(summary);
        cx.notify();
    }
}

/// The registry's codecs, asked for the one that reads `bytes`.
pub(super) fn import(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let registry = crate::build_registry();
    let codec = registry.importer(bytes).ok_or_else(|| {
        if registry.codecs().any(|codec| codec.imports()) {
            "That format cannot be made into a PDF: PNG, JPEG and TIFF can".to_owned()
        } else {
            NO_IMAGE_IMPORT.to_owned()
        }
    })?;
    codec.import(bytes).map_err(|error| error.to_string())
}

/// `photo.jpg` becomes `photo.pdf`.
fn pdf_name_for(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned());
    format!("{stem}.pdf")
}

/// Write `bytes` to `path`, which the save prompt has already confirmed may
/// be replaced: into a sibling first and then renamed over it, so a failed
/// write never leaves half a file where the user's was.
pub(super) fn write_replacing(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path.parent().unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    staged.write_all(bytes)?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(feature = "codecs-common")]
fn extract_into(document: &mut onionskin_core::Document, folder: &Path) -> String {
    let extraction = match document
        .structure()
        .and_then(onionskin_codecs_common::extract_images)
    {
        Ok(extraction) => extraction,
        Err(error) => return format!("The images could not be read: {error}"),
    };
    let mut written = 0;
    let mut problems = Vec::new();
    for image in &extraction.images {
        match write_new(&folder.join(&image.name), &image.bytes) {
            Ok(()) => written += 1,
            Err(error) => problems.push(format!("{}: {error}", image.name)),
        }
    }
    problems.extend(extraction.skipped.iter().map(|skipped| {
        format!(
            "page {}, image {}: {}",
            skipped.page + 1,
            skipped.object,
            skipped.reason
        )
    }));
    summarize(written, folder, &problems)
}

#[cfg(not(feature = "codecs-common"))]
fn extract_into(_: &mut onionskin_core::Document, _: &Path) -> String {
    "The common codecs plugin is not installed".to_owned()
}

/// A file that must not already exist.
#[cfg_attr(not(feature = "codecs-common"), allow(dead_code))]
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)
}

#[cfg_attr(not(feature = "codecs-common"), allow(dead_code))]
fn summarize(written: usize, folder: &Path, problems: &[String]) -> String {
    let mut summary = match written {
        0 => format!("No images were written to {}.", folder.display()),
        1 => format!("Wrote 1 image to {}.", folder.display()),
        count => format!("Wrote {count} images to {}.", folder.display()),
    };
    if !problems.is_empty() {
        summary.push_str(&format!(" Not written: {}.", problems.join("; ")));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_created_document_is_named_after_its_source() {
        assert_eq!(
            pdf_name_for(Path::new("/scans/receipt.jpeg")),
            "receipt.pdf"
        );
        assert_eq!(pdf_name_for(Path::new("/")), "Untitled.pdf");
    }

    #[test]
    fn writing_a_new_file_never_replaces_one() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("page-1-image-4.png");
        write_new(&path, b"first").expect("writes");
        assert!(write_new(&path, b"second").is_err());
        assert_eq!(std::fs::read(&path).expect("reads"), b"first");
    }

    #[test]
    fn replacing_a_file_swaps_it_whole() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("out.pdf");
        std::fs::write(&path, b"old").expect("writes");
        write_replacing(&path, b"new document").expect("replaces");
        assert_eq!(std::fs::read(&path).expect("reads"), b"new document");
        assert_eq!(std::fs::read_dir(dir.path()).expect("lists").count(), 1);
    }

    #[test]
    fn the_summary_counts_and_names_what_was_not_written() {
        let folder = Path::new("/out");
        assert_eq!(summarize(1, folder, &[]), "Wrote 1 image to /out.");
        assert_eq!(
            summarize(3, folder, &["page 2, image 9: JBIG2Decode images are not extracted".into()]),
            "Wrote 3 images to /out. Not written: page 2, image 9: JBIG2Decode images are not extracted."
        );
        assert_eq!(summarize(0, folder, &[]), "No images were written to /out.");
    }

    #[test]
    fn bytes_no_codec_reads_are_refused_with_what_would_work() {
        let refused = import(b"GIF89a....").expect_err("refused");
        if cfg!(feature = "codecs-common") {
            assert!(refused.contains("PNG, JPEG and TIFF"), "{refused}");
        } else {
            assert_eq!(refused, NO_IMAGE_IMPORT);
        }
    }

    #[cfg(feature = "codecs-common")]
    #[test]
    fn every_image_in_a_document_lands_in_the_folder_once() {
        let pdf = onionskin_core::images::image_document(&onionskin_core::images::ImagePage {
            width: 2,
            height: 1,
            dpi: (72.0, 72.0),
            color: onionskin_core::images::ImageColor::Gray,
            data: onionskin_core::images::ImageData::Samples(vec![0, 255]),
            alpha: None,
            inverted_cmyk: false,
            icc: None,
        })
        .expect("writes");
        let mut document = onionskin_core::Document::open_bytes(pdf).expect("opens");
        let dir = tempfile::tempdir().expect("dir");
        let summary = extract_into(&mut document, dir.path());
        assert!(summary.starts_with("Wrote 1 image"), "{summary}");
        let again = extract_into(&mut document, dir.path());
        assert!(
            again.contains("Not written"),
            "a second run replaces nothing: {again}"
        );
        assert_eq!(std::fs::read_dir(dir.path()).expect("lists").count(), 1);
    }
}
