//! Export Selection As, from the text-selection context menu (P22, row 31's
//! file half): the selected text written to a file the user names. A name
//! ending in `.rtf` gets Rich Text with the selection's faces and sizes;
//! any other name gets the plain text.
//!
//! Writing the selection to a file is reading the document out, so an
//! encrypted document refuses it, as every export does.

use std::path::{Path, PathBuf};

use gpui::Context;
use onionskin_core::TextSelection;

use super::ShellFrame;

/// The name the save prompt suggests.
#[cfg(feature = "codecs-common")]
const SUGGESTED: &str = "Selection.rtf";
#[cfg(not(feature = "codecs-common"))]
const SUGGESTED: &str = "Selection.txt";

/// Said for a `.rtf` name in a build without the writer.
#[cfg(not(feature = "codecs-common"))]
pub(in crate::shell) const NO_RTF: &str =
    "Rich Text needs the Common Codecs plugin, which is not installed; name the file .txt";

impl ShellFrame {
    pub(super) fn export_selection(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let canvas = tab.canvas.read(cx);
        let Some(selection) = canvas.model.text_selection() else {
            return;
        };
        if let Some(refusal) = canvas.model.read_out_refusal() {
            self.notices
                .push(format!("The selection cannot be exported: {refusal}"));
            return;
        }
        let directory = tab
            .path(cx)
            .as_deref()
            .and_then(Path::parent)
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let chosen = cx.prompt_for_new_path(&directory, Some(SUGGESTED));
        cx.spawn(async move |frame, cx| {
            let Ok(Ok(Some(output))) = chosen.await else {
                return;
            };
            frame
                .update(cx, |frame, cx| {
                    frame.write_selection(&output, &selection);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Write the selection to `output` and say how it went.
    pub(super) fn write_selection(&mut self, output: &Path, selection: &TextSelection) {
        let written = selection_bytes(output, selection).and_then(|bytes| {
            super::create::write_replacing(output, &bytes)
                .map_err(|error| format!("{} was not written: {error}", output.display()))
        });
        self.notices.push(match written {
            Ok(()) => format!("Exported the selection to {}", output.display()),
            Err(message) => message,
        });
    }
}

fn is_rtf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rtf"))
}

/// The file's bytes: RTF for a `.rtf` name, the plain text otherwise.
fn selection_bytes(output: &Path, selection: &TextSelection) -> Result<Vec<u8>, String> {
    if !is_rtf(output) {
        return Ok(selection.text.clone().into_bytes());
    }
    #[cfg(feature = "codecs-common")]
    {
        Ok(onionskin_codecs_common::rtf::selection_rtf(&selection.spans).into_bytes())
    }
    #[cfg(not(feature = "codecs-common"))]
    {
        Err(NO_RTF.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> TextSelection {
        TextSelection {
            page: 0,
            quads: Vec::new(),
            text: "Bold plain".into(),
            spans: vec![
                onionskin_core::TextSpan {
                    text: "Bold ".into(),
                    font: "Helvetica-Bold".into(),
                    size: 12.0,
                },
                onionskin_core::TextSpan {
                    text: "plain".into(),
                    font: "Helvetica".into(),
                    size: 12.0,
                },
            ],
        }
    }

    #[test]
    fn a_name_that_is_not_rtf_gets_the_plain_text() {
        assert_eq!(
            selection_bytes(Path::new("/tmp/out.TXT"), &selection()),
            Ok(b"Bold plain".to_vec())
        );
        assert!(is_rtf(Path::new("a.RTF")));
        assert!(!is_rtf(Path::new("rtf")));
    }

    #[cfg(feature = "codecs-common")]
    #[test]
    fn an_rtf_name_gets_the_faces() {
        let bytes = selection_bytes(Path::new("out.rtf"), &selection()).expect("writes");
        let rtf = String::from_utf8(bytes).expect("ascii");
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(rtf.contains("{\\f0\\fs24\\b Bold }"), "{rtf}");
        assert!(rtf.contains("{\\f0\\fs24 plain}"), "{rtf}");
    }

    #[cfg(not(feature = "codecs-common"))]
    #[test]
    fn an_rtf_name_without_the_writer_is_refused() {
        assert_eq!(
            selection_bytes(Path::new("out.rtf"), &selection()),
            Err(NO_RTF.to_owned())
        );
    }
}
