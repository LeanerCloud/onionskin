//! Base-page rasterization: the CPU reference, delegated to hayro.

use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::{InterpreterSettings, InterpreterWarning};
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::{RenderCache, RenderSettings};

/// hayro sizes its pixmaps with `u16`, so a page is unrenderable once either
/// axis exceeds this at the requested zoom. A letter page reaches it at about
/// 80x zoom.
const MAX_RASTER_AXIS: f64 = u16::MAX as f64;

/// An opened PDF, held only for display. Nothing here is the semantic
/// contract: `cos` owns what a save writes.
pub struct Document {
    pdf: Pdf,
}

impl Document {
    pub fn open(bytes: Vec<u8>) -> Result<Self, RenderError> {
        Ok(Self {
            pdf: Pdf::new(bytes).map_err(RenderError::Load)?,
        })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Rasterize one page at `zoom` (1.0 = 72 dpi) onto white.
    ///
    /// A fresh [`RenderCache`] per call, which throws away hayro's font and
    /// outline caching between pages: `RenderCache<'a>` borrows the `Pdf`, so
    /// storing one beside the document it caches for is self-referential. A
    /// viewer that pages through a document needs that fixed.
    pub fn render_page(&self, index: usize, zoom: f32) -> Result<PageRender, RenderError> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(RenderError::NoSuchPage {
            index,
            count: pages.len(),
        })?;

        let (pt_width, pt_height) = page.render_dimensions();
        let (px_width, px_height) = (
            pt_width as f64 * zoom as f64,
            pt_height as f64 * zoom as f64,
        );
        // A malformed `/MediaBox` can round to nothing, and hayro answers with
        // an empty pixmap rather than an error; the tile grid has no useful
        // reading of that, so it is rejected here instead of panicking later.
        if !(1.0..=MAX_RASTER_AXIS).contains(&px_width)
            || !(1.0..=MAX_RASTER_AXIS).contains(&px_height)
        {
            return Err(RenderError::UnrenderableSize {
                width: px_width,
                height: px_height,
            });
        }

        // hayro drops content it cannot interpret and carries on, so the sink
        // is the only way to tell a correct page from a quietly wrong one.
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&warnings);

        let pixmap = hayro::render(
            page,
            &RenderCache::new(),
            &InterpreterSettings {
                warning_sink: Arc::new(move |warning| {
                    sink.lock().expect("sink is never poisoned").push(warning)
                }),
                ..Default::default()
            },
            &RenderSettings {
                x_scale: zoom,
                y_scale: zoom,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
                ..Default::default()
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
    rgba: Vec<u8>,
}

impl BaseRaster {
    /// Panics on an empty raster, or if `rgba` is not exactly
    /// `width * height * 4` bytes: a caller that gets either wrong has a bug,
    /// and a silently resized buffer would shear every row below the mistake.
    pub fn new(width: u32, height: u32, zoom: f32, rgba: Vec<u8>) -> Self {
        assert!(
            width > 0 && height > 0,
            "a base raster needs at least one pixel, got {width}x{height}"
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
            rgba,
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
            Self::Load(LoadPdfError::Decryption(e)) => {
                write!(f, "the document is encrypted and could not be decrypted: {e:?}")
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
