//! Page-to-SVG conversion: the same interpreter as the rasterizer, writing
//! vector output instead of pixels.

use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::{InterpreterSettings, InterpreterWarning};

use crate::base::{Document, RenderError, RenderOptions};

/// One page converted to SVG, and everything the interpreter gave up on while
/// producing it.
pub struct PageSvg {
    pub svg: String,
    /// Content the interpreter skipped, for the same reason [`PageRender`]
    /// carries it: an export missing what the file asked for is a bug to
    /// report, not to swallow.
    ///
    /// [`PageRender`]: crate::PageRender
    pub warnings: Vec<InterpreterWarning>,
}

impl Document {
    /// Convert one page to an SVG document.
    ///
    /// There is no zoom: the output is vector, sized in the same points
    /// [`Document::page_geometry`] reports. The SVG paints no background, so
    /// it composites onto whatever the consumer puts behind it, where
    /// [`RenderSession::render_page`] rasterizes onto white.
    ///
    /// [`RenderSession::render_page`]: crate::RenderSession::render_page
    pub fn render_page_svg(
        &self,
        index: usize,
        options: &RenderOptions,
    ) -> Result<PageSvg, RenderError> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(RenderError::NoSuchPage {
            index,
            count: pages.len(),
        })?;

        let warnings = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&warnings);
        let svg = hayro_svg::convert(
            page,
            &hayro_svg::RenderCache::new(),
            &InterpreterSettings {
                warning_sink: Arc::new(move |warning| {
                    sink.lock().expect("sink is never poisoned").push(warning)
                }),
                render_annotations: options.render_annotations,
                ocg_overrides: Arc::new(options.layer_visibility.clone()),
                ..Default::default()
            },
            &hayro_svg::SvgRenderSettings::default(),
        );

        let warnings = std::mem::take(&mut *warnings.lock().expect("sink is never poisoned"));
        Ok(PageSvg { svg, warnings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_page_converts_to_an_svg_with_its_glyphs() {
        let document = Document::open(seed("hello.pdf")).expect("seed opens");

        let page = document
            .render_page_svg(0, &RenderOptions::default())
            .expect("seed page converts");

        assert!(
            page.svg.starts_with("<svg"),
            "{}",
            &page.svg[..40.min(page.svg.len())]
        );
        assert!(
            page.svg.contains("<path"),
            "no glyph outlines in the export"
        );
        assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    }

    /// The viewBox is the page's render dimensions, which is what makes an
    /// exported SVG line up with the same page's raster.
    #[test]
    fn the_view_box_is_the_pages_render_size() {
        let document = Document::open(seed("hello.pdf")).expect("seed opens");
        let (width, height) = document
            .page_geometry(0)
            .expect("seed has a page")
            .render_size;

        let page = document
            .render_page_svg(0, &RenderOptions::default())
            .expect("seed page converts");

        assert!(
            page.svg
                .contains(&format!("viewBox=\"0 0 {width} {height}\"")),
            "{}",
            &page.svg[..120.min(page.svg.len())]
        );
    }

    #[test]
    fn a_page_past_the_end_names_the_document_length() {
        let document = Document::open(seed("hello.pdf")).expect("seed opens");

        assert_eq!(
            document.render_page_svg(7, &RenderOptions::default()).err(),
            Some(RenderError::NoSuchPage { index: 7, count: 1 })
        );
    }

    fn seed(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/seeds")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
    }
}
