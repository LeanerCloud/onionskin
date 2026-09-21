//! What every image import shares: the resolution a file that states none is
//! taken at, pages from decoded samples, and several pages joined into one
//! document.
//!
//! The formats themselves - reading a JPEG's header, a PNG's `pHYs`, a TIFF's
//! resolution tags - are each codec's own. What they hand here is already
//! `core::images`' shape.

use onionskin_core::images::{image_document, ImageColor, ImageData, ImagePage};
use onionskin_core::pages::Assembly;
use onionskin_cos::{BytesSource, Document as CosDocument};
use onionskin_plugin_api::ImportError;

/// The resolution of an image that states none: one pixel to one point,
/// which is what every PDF producer assumes.
pub(crate) const DEFAULT_DPI: f64 = 72.0;

/// A resolution as stated, or the default when a file states nothing usable.
pub(crate) fn dpi_or_default(stated: Option<(f64, f64)>) -> (f64, f64) {
    match stated {
        Some((x, y)) if usable(x) && usable(y) => (x, y),
        _ => (DEFAULT_DPI, DEFAULT_DPI),
    }
}

fn usable(dpi: f64) -> bool {
    dpi.is_finite() && dpi >= 1.0
}

/// Decoded 8-bit samples with `channels` per pixel, the last of them alpha
/// when `has_alpha`.
pub(crate) struct Decoded {
    pub width: u32,
    pub height: u32,
    pub color: ImageColor,
    pub has_alpha: bool,
    pub samples: Vec<u8>,
    pub dpi: (f64, f64),
    pub icc: Option<Vec<u8>>,
}

impl Decoded {
    /// The page this image becomes. An alpha channel that is opaque
    /// everywhere is dropped rather than written as a soft mask that does
    /// nothing.
    pub fn into_page(self) -> ImagePage {
        let (samples, alpha) = if self.has_alpha {
            split_alpha(self.samples, self.color.components())
        } else {
            (self.samples, None)
        };
        ImagePage {
            width: self.width,
            height: self.height,
            dpi: self.dpi,
            color: self.color,
            data: ImageData::Samples(samples),
            alpha,
            inverted_cmyk: false,
            icc: self.icc,
        }
    }
}

/// Colour samples and alpha, separated; `None` for alpha that is opaque at
/// every pixel.
fn split_alpha(interleaved: Vec<u8>, color_components: usize) -> (Vec<u8>, Option<Vec<u8>>) {
    let stride = color_components + 1;
    let pixels = interleaved.len() / stride;
    let mut color = Vec::with_capacity(pixels * color_components);
    let mut alpha = Vec::with_capacity(pixels);
    for pixel in interleaved.chunks_exact(stride) {
        color.extend_from_slice(&pixel[..color_components]);
        alpha.push(pixel[color_components]);
    }
    let opaque = alpha.iter().all(|&coverage| coverage == u8::MAX);
    (color, (!opaque).then_some(alpha))
}

/// One document with a page per image, in order.
pub(crate) fn document(pages: Vec<ImagePage>) -> Result<Vec<u8>, ImportError> {
    let mut written = Vec::with_capacity(pages.len());
    for page in &pages {
        written.push(image_document(page)?);
    }
    if written.len() == 1 {
        return Ok(written.remove(0));
    }
    let mut assembly = Assembly::new();
    for bytes in written {
        let (source, _) = CosDocument::open_repairing(Box::new(BytesSource::new(bytes)))
            .map_err(|error| ImportError::Document(error.into()))?;
        assembly.append(&source, &[0])?;
    }
    Ok(assembly.finish()?.bytes)
}

/// A decoder's error, as the import error a user reads.
pub(crate) fn decode_error(error: impl std::fmt::Display) -> ImportError {
    ImportError::Decode(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_alpha_channel_is_dropped_and_a_real_one_kept() {
        let (color, alpha) = split_alpha(vec![1, 2, 3, 255, 4, 5, 6, 255], 3);
        assert_eq!(color, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(alpha, None);

        let (color, alpha) = split_alpha(vec![9, 0, 7, 128], 1);
        assert_eq!(color, vec![9, 7]);
        assert_eq!(alpha, Some(vec![0, 128]));
    }

    #[test]
    fn a_missing_or_nonsensical_resolution_is_72() {
        assert_eq!(dpi_or_default(None), (72.0, 72.0));
        assert_eq!(dpi_or_default(Some((0.0, 300.0))), (72.0, 72.0));
        assert_eq!(dpi_or_default(Some((f64::NAN, 300.0))), (72.0, 72.0));
        assert_eq!(dpi_or_default(Some((300.0, 150.0))), (300.0, 150.0));
    }

    #[test]
    fn several_images_are_one_document_of_that_many_pages() {
        let page = |shade| ImagePage {
            width: 2,
            height: 2,
            dpi: (72.0, 72.0),
            color: ImageColor::Gray,
            data: ImageData::Samples(vec![shade; 4]),
            alpha: None,
            inverted_cmyk: false,
            icc: None,
        };
        let bytes = document(vec![page(0), page(128), page(255)]).expect("assembles");
        let (doc, _) =
            CosDocument::open_repairing(Box::new(BytesSource::new(bytes))).expect("opens");
        assert_eq!(doc.page_count().expect("counts"), 3);
    }
}
