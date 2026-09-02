//! SVG page export.

use onionskin_plugin_api::{
    CodecPlugin, Document, ExportError, ExportOutputKind, ExportRequest, PageIndex,
};

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

    fn output_kind(&self) -> ExportOutputKind {
        ExportOutputKind::PerPage
    }

    fn export_page(
        &self,
        doc: &mut Document,
        _request: &ExportRequest,
        page: PageIndex,
        _first_in_request: bool,
    ) -> Result<Vec<u8>, ExportError> {
        let converted = doc
            .page_svg(page)
            .map_err(|source| ExportError::Page { page, source })?;
        Ok(converted.svg.into_bytes())
    }
}
