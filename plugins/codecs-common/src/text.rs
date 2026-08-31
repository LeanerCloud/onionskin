//! Plain-text export.

use onionskin_plugin_api::{CodecPlugin, Document, ExportError, ExportRequest, ExportedFile};

/// The document's text, in the order the content streams draw it.
///
/// This is `content`'s extraction verbatim: the codec joins pages and adds
/// nothing. In particular it does **not** reorder anything into reading
/// order, so a two-column page comes out column by column exactly as the file
/// draws it. Accessible-text ordering is an M6 row on the parity scoreboard,
/// and claiming it here would be claiming layout analysis this milestone does
/// not do.
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

    fn export(
        &self,
        doc: &mut Document,
        request: &ExportRequest,
    ) -> Result<Vec<ExportedFile>, ExportError> {
        let mut out = String::new();
        for (position, page) in request.pages.pages().enumerate() {
            let text = doc
                .page_text(page)
                .map_err(|source| ExportError::Page { page, source })?;
            // Keyed on the page's position, not on what came out of it: a
            // page with no text is still a page the reader passed.
            if position > 0 {
                out.push_str(PAGE_SEPARATOR);
            }
            out.push_str(&text.flatten().text);
        }
        Ok(vec![ExportedFile {
            page: None,
            bytes: out.into_bytes(),
        }])
    }
}
