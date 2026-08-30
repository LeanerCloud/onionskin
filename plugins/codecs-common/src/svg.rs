//! SVG page export.

use onionskin_plugin_api::{CodecPlugin, Document, ExportError, ExportRequest, ExportedFile};

/// One SVG per page, converted by the same interpreter that rasterizes the
/// canvas, so annotations and layer state match what is on screen.
///
/// The output is vector, so [`ExportRequest::dpi`] does not apply: the SVG is
/// sized in the page's own points and scales without loss.
pub struct SvgCodec;

impl CodecPlugin for SvgCodec {
    fn id(&self) -> &'static str {
        "svg"
    }

    fn name(&self) -> &'static str {
        "SVG Image"
    }

    fn extension(&self) -> &'static str {
        "svg"
    }

    fn export(
        &self,
        doc: &mut Document,
        request: &ExportRequest,
    ) -> Result<Vec<ExportedFile>, ExportError> {
        let mut out = Vec::new();
        for page in request.pages.pages() {
            let converted = doc
                .page_svg(page)
                .map_err(|source| ExportError::Page { page, source })?;
            out.push(ExportedFile {
                page: Some(page),
                bytes: converted.svg.into_bytes(),
            });
        }
        Ok(out)
    }
}
