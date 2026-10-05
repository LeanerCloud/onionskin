//! Plain-text export.

use onionskin_plugin_api::{
    CodecPlugin, Document, ExportError, ExportOutputKind, ExportRequest, PageIndex,
};

/// The document's text. A tagged document comes out in the order its structure
/// gives, with a line for each block-level element, its `/ActualText` for the
/// content it replaces and its `/Alt` for a figure. Any other document comes
/// out in the order the content streams draw it, `content`'s extraction
/// verbatim, so a two-column page is read column by column exactly as the file
/// draws it: there is no layout analysis here.
///
/// Every non-first request chunk is prefixed with a blank line so the worker can
/// append pages into one file.
pub struct TextCodec;

/// Between pages. A blank line, so a reader can see where a page ended
/// without the codec inventing a form feed the extraction never had.
const PAGE_SEPARATOR: &str = "\n\n";

impl CodecPlugin for TextCodec {
    fn id(&self) -> &'static str {
        "text"
    }

    fn name(&self) -> &'static str {
        "Plain Text"
    }

    fn extension(&self) -> &'static str {
        "txt"
    }

    fn output_kind(&self) -> ExportOutputKind {
        ExportOutputKind::Single
    }

    fn export_page(
        &self,
        doc: &mut Document,
        _request: &ExportRequest,
        page: PageIndex,
        first_in_request: bool,
    ) -> Result<Vec<u8>, ExportError> {
        // The page's text written to a file is the document read out: refused
        // on an encrypted one, like SVG. Reading text for a selection or the
        // clipboard stays allowed; that writes no file.
        if let Some(refusal) = doc.read_out_refusal() {
            return Err(ExportError::Page {
                page,
                source: onionskin_core::Error::Protected(refusal),
            });
        }
        // A document whose structure cannot be read has no structure order to
        // export, and exports as it did before there was one: in drawing order.
        let tagged = doc.reading_text(page).unwrap_or(None);
        let text = match tagged {
            Some(text) => text,
            None => {
                doc.page_text(page)
                    .map_err(|source| ExportError::Page { page, source })?
                    .flatten()
                    .text
            }
        };
        let mut out = String::new();
        if !first_in_request {
            out.push_str(PAGE_SEPARATOR);
        }
        out.push_str(&text);
        Ok(out.into_bytes())
    }
}
