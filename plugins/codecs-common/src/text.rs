//! Plain-text export.

use onionskin_plugin_api::{
    CodecPlugin, Document, ExportError, ExportOutputKind, ExportRequest, PageIndex,
};

/// The document's text, in the order the content streams draw it.
///
/// Each page is `content`'s extraction verbatim, with a blank-line prefix on
/// every non-first request chunk so the worker can append pages into one file.
/// It does **not** reorder anything into reading order, so a two-column page
/// comes out column by column exactly as the file draws it. Accessible-text
/// ordering is an M6 row on the parity scoreboard, and claiming it here would
/// be claiming layout analysis this milestone does not do.
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
        let text = doc
            .page_text(page)
            .map_err(|source| ExportError::Page { page, source })?;
        let mut out = String::new();
        if !first_in_request {
            out.push_str(PAGE_SEPARATOR);
        }
        out.push_str(&text.flatten().text);
        Ok(out.into_bytes())
    }
}
