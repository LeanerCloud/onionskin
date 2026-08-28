//! Base-page rasterization: the CPU reference, delegated to hayro.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::{InterpreterSettings, InterpreterWarning};
use hayro::hayro_syntax::object::ObjectIdentifier;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::{RenderCache, RenderSettings};

/// hayro sizes its pixmaps with `u16`, so a page is unrenderable once either
/// axis exceeds this at the requested zoom. A letter page reaches it at about
/// 80x zoom.
const MAX_RASTER_AXIS: f64 = u16::MAX as f64;

/// Return the dimensions hayro will allocate for a page at `zoom`.
///
/// hayro multiplies its `f32` dimensions and scale, floors the result, then
/// converts to `u16`; keep that order here so validation and allocation agree.
pub fn raster_size(width: f32, height: f32, zoom: f32) -> Result<(u16, u16), RenderError> {
    let scaled_width = f64::from(width * zoom);
    let scaled_height = f64::from(height * zoom);
    let width = scaled_width.floor();
    let height = scaled_height.floor();

    if !(1.0..=MAX_RASTER_AXIS).contains(&width) || !(1.0..=MAX_RASTER_AXIS).contains(&height) {
        return Err(RenderError::UnrenderableSize {
            width: scaled_width,
            height: scaled_height,
        });
    }

    Ok((width as u16, height as u16))
}

/// An opened PDF, held only for display. Nothing here is the semantic
/// contract: `cos` owns what a save writes.
pub struct Document {
    pdf: Pdf,
}

/// The page-to-device mapping hayro uses at 72 dpi.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageTransform {
    coefficients: [f64; 6],
}

impl PageTransform {
    pub fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.coefficients;
        (a * x + c * y + e, b * x + d * y + f)
    }

    pub fn apply_inverse(self, x: f64, y: f64) -> Result<(f64, f64), TransformError> {
        let [a, b, c, d, e, f] = self.coefficients;
        let determinant = a * d - b * c;
        if determinant == 0.0 || !determinant.is_finite() {
            return Err(TransformError::NonInvertible { determinant });
        }

        let x = x - e;
        let y = y - f;
        Ok((
            (d * x - c * y) / determinant,
            (-b * x + a * y) / determinant,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformError {
    NonInvertible { determinant: f64 },
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonInvertible { determinant } => write!(
                f,
                "page transform is not invertible; determinant is {determinant}"
            ),
        }
    }
}

impl Error for TransformError {}

/// Owned page metadata that can cross the render-worker channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRenderGeometry {
    pub render_size: (f64, f64),
    pub transform: PageTransform,
}

/// What a render includes beyond the page's own marks.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Draw annotation appearance streams over the page content. On, as a
    /// viewer needs; M3's edit mode turns it off to draw its own.
    pub render_annotations: bool,
    /// Optional content groups to show or hide against the document's own
    /// default configuration, keyed by the object the group lives in. Empty
    /// means the file decides, which is what a viewer without a layers pane
    /// wants.
    pub layer_visibility: HashMap<ObjectIdentifier, bool>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            render_annotations: true,
            layer_visibility: HashMap::new(),
        }
    }
}

impl Document {
    pub fn open(bytes: Vec<u8>) -> Result<Self, RenderError> {
        Self::from_shared(Arc::new(bytes))
    }

    /// Open the bytes the rest of the session already holds. hayro stores them
    /// as an `Arc` internally, so this shares the buffer rather than copying
    /// it: one allocation serves `cos` and the render thread both.
    pub fn from_shared(bytes: Arc<Vec<u8>>) -> Result<Self, RenderError> {
        Ok(Self {
            pdf: Pdf::new(bytes).map_err(RenderError::Load)?,
        })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    pub fn page_geometry(&self, index: usize) -> Result<PageRenderGeometry, RenderError> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(RenderError::NoSuchPage {
            index,
            count: pages.len(),
        })?;
        let (width, height) = page.render_dimensions();
        Ok(PageRenderGeometry {
            render_size: (f64::from(width), f64::from(height)),
            transform: PageTransform {
                coefficients: page.initial_transform(true).as_coeffs(),
            },
        })
    }

    /// Run rendering with one hayro cache for the duration of the callback.
    ///
    /// The cache borrows this document, so keeping it beside `Document` would
    /// require self-referential storage. A worker can keep this session alive
    /// while it processes its request loop instead.
    pub fn with_render_session<R>(&self, f: impl for<'a> FnOnce(&mut RenderSession<'a>) -> R) -> R {
        f(&mut RenderSession {
            document: self,
            cache: RenderCache::new(),
        })
    }

    /// Rasterize one page at `zoom` (1.0 = 72 dpi) onto white.
    ///
    /// A short-lived session keeps the public convenience API compatible while
    /// allowing workers to retain a cache across several pages.
    pub fn render_page(
        &self,
        index: usize,
        zoom: f32,
        options: &RenderOptions,
    ) -> Result<PageRender, RenderError> {
        self.with_render_session(|session| session.render_page(index, zoom, options))
    }
}

/// A borrowed PDF and its reusable hayro render cache.
pub struct RenderSession<'a> {
    document: &'a Document,
    cache: RenderCache<'a>,
}

impl RenderSession<'_> {
    /// Rasterize one page using this session's shared cache.
    pub fn render_page(
        &mut self,
        index: usize,
        zoom: f32,
        options: &RenderOptions,
    ) -> Result<PageRender, RenderError> {
        let document = self.document;
        let pages = document.pdf.pages();
        let page = pages.get(index).ok_or(RenderError::NoSuchPage {
            index,
            count: pages.len(),
        })?;

        let (pt_width, pt_height) = page.render_dimensions();
        let (px_width, px_height) = raster_size(pt_width, pt_height, zoom)?;

        // hayro drops content it cannot interpret and carries on, so the sink
        // is the only way to tell a correct page from a quietly wrong one.
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&warnings);

        let pixmap = hayro::render(
            page,
            &self.cache,
            &InterpreterSettings {
                warning_sink: Arc::new(move |warning| {
                    sink.lock().expect("sink is never poisoned").push(warning)
                }),
                render_annotations: options.render_annotations,
                ocg_overrides: Arc::new(options.layer_visibility.clone()),
                ..Default::default()
            },
            &RenderSettings {
                x_scale: zoom,
                y_scale: zoom,
                width: Some(px_width),
                height: Some(px_height),
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            },
        );

        let warnings = std::mem::take(&mut *warnings.lock().expect("sink is never poisoned"));
        Ok(PageRender {
            raster: BaseRaster::new(
                u32::from(pixmap.width()),
                u32::from(pixmap.height()),
                zoom,
                pixmap.data_as_u8_slice().to_vec(),
            ),
            warnings,
        })
    }
}

/// A rasterized page and everything hayro gave up on while producing it.
pub struct PageRender {
    pub raster: BaseRaster,
    /// Content the interpreter skipped. Non-empty means the page on screen
    /// is missing something the file asked for, which is a display bug to
    /// report rather than swallow.
    pub warnings: Vec<InterpreterWarning>,
}

/// One page rasterized at one zoom: premultiplied RGBA8, row-major, top-left
/// origin. Held whole because hayro cannot rasterize a sub-rectangle.
#[derive(Clone)]
pub struct BaseRaster {
    width: u32,
    height: u32,
    zoom: f32,
    rgba: Arc<[u8]>,
}

impl BaseRaster {
    /// Panics on an empty raster, a zoom that is not a positive finite scale,
    /// or an `rgba` that is not exactly `width * height * 4` bytes: a caller
    /// that gets any of them wrong has a bug, and a silently resized buffer
    /// would shear every row below the mistake.
    pub fn new(width: u32, height: u32, zoom: f32, rgba: Vec<u8>) -> Self {
        assert!(
            width > 0 && height > 0,
            "a base raster needs at least one pixel, got {width}x{height}"
        );
        // Overlay coordinates are in page points, so a zoom of zero, NaN or a
        // negative places every overlay somewhere meaningless instead of
        // failing, and the tile store keys its caches on this value.
        assert!(
            zoom.is_finite() && zoom > 0.0,
            "a base raster's zoom must be a positive finite scale, got {zoom}"
        );
        let expected = width as usize * height as usize * 4;
        assert_eq!(
            rgba.len(),
            expected,
            "base raster is {}x{} so it needs {expected} bytes, got {}",
            width,
            height,
            rgba.len()
        );
        Self {
            width,
            height,
            zoom,
            rgba: Arc::from(rgba),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The scale this was rasterized at. Overlay coordinates are in page
    /// points, so the tile cache needs it to place them.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderError {
    Load(LoadPdfError),
    NoSuchPage {
        index: usize,
        count: usize,
    },
    /// The page rasterizes to nothing, or to more pixels than hayro can
    /// address on one axis.
    UnrenderableSize {
        width: f64,
        height: f64,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // hayro's `DecryptionError` carries no `Display`, and `{e:?}` in a
            // message a user reads is a leak, not a diagnosis. Matched
            // exhaustively so a new upstream variant fails the build instead
            // of falling into a vague catch-all.
            Self::Load(LoadPdfError::Decryption(e)) => {
                let reason = match e {
                    DecryptionError::MissingIDEntry => "its /ID entry is missing",
                    DecryptionError::PasswordProtected => "it needs a password",
                    DecryptionError::InvalidEncryption => "its encryption dictionary is invalid",
                    DecryptionError::UnsupportedAlgorithm => {
                        "it uses an encryption algorithm we do not support"
                    }
                };
                write!(f, "the document is encrypted and {reason}")
            }
            Self::Load(LoadPdfError::Invalid) => write!(f, "the document could not be parsed"),
            Self::NoSuchPage { index, count } => {
                write!(f, "page {index} requested, document has {count}")
            }
            Self::UnrenderableSize { width, height } => write!(
                f,
                "page rasterizes to {width:.0}x{height:.0}, outside the 1..={MAX_RASTER_AXIS:.0} px hayro can address"
            ),
        }
    }
}

impl Error for RenderError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The render worker builds its `Document` inside the spawned closure from
    /// the session's shared bytes, so nothing but the bytes crosses a thread
    /// boundary. That design holds only if what it builds can live there;
    /// asserted rather than assumed.
    #[test]
    fn a_document_can_live_on_the_render_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Pdf>();
        assert_send_sync::<Document>();
    }

    #[test]
    fn every_decryption_message_stays_prose() {
        for (error, expected) in [
            (
                DecryptionError::MissingIDEntry,
                "the document is encrypted and its /ID entry is missing",
            ),
            (
                DecryptionError::PasswordProtected,
                "the document is encrypted and it needs a password",
            ),
            (
                DecryptionError::InvalidEncryption,
                "the document is encrypted and its encryption dictionary is invalid",
            ),
            (
                DecryptionError::UnsupportedAlgorithm,
                "the document is encrypted and it uses an encryption algorithm we do not support",
            ),
        ] {
            let message = RenderError::Load(LoadPdfError::Decryption(error)).to_string();
            assert_eq!(message, expected);
        }
    }

    #[test]
    #[should_panic(expected = "positive finite scale")]
    fn a_base_raster_refuses_a_zoom_that_places_nothing() {
        BaseRaster::new(2, 2, 0.0, vec![255; 16]);
    }

    #[test]
    fn raster_size_matches_hayros_floor_and_u16_limits() {
        assert_eq!(raster_size(200.9, 100.1, 1.0).unwrap(), (200, 100));
        assert_eq!(raster_size(65_535.9, 2.0, 1.0).unwrap(), (65_535, 2));
        assert!(raster_size(65_536.0, 2.0, 1.0).is_err());
        assert!(raster_size(0.9, 2.0, 1.0).is_err());
    }

    #[test]
    fn a_non_invertible_transform_reports_a_typed_failure() {
        let transform = PageTransform {
            coefficients: [1.0, 2.0, 2.0, 4.0, 10.0, 20.0],
        };

        assert!(matches!(
            transform.apply_inverse(10.0, 20.0),
            Err(TransformError::NonInvertible { determinant: 0.0 })
        ));
    }

    #[test]
    fn a_render_session_reuses_one_cache_for_multiple_pages() {
        let document = Document::open(minimal_pdf()).expect("fixture opens");
        document.with_render_session(|session| {
            let first = session
                .render_page(0, 1.0, &RenderOptions::default())
                .expect("first page renders");
            let second = session
                .render_page(1, 1.0, &RenderOptions::default())
                .expect("second page render reuses the session");
            assert_eq!(first.raster.width(), second.raster.width());
            assert_eq!(first.raster.height(), second.raster.height());
        });
    }

    #[test]
    fn cloning_a_base_raster_shares_its_pixels() {
        let raster = BaseRaster::new(2, 2, 1.0, vec![255; 16]);
        let clone = raster.clone();
        assert!(Arc::ptr_eq(&raster.rgba, &clone.rgba));
        assert_eq!(raster.rgba(), clone.rgba());
    }

    fn minimal_pdf() -> Vec<u8> {
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_vec(),
        ];
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        pdf
    }
}
