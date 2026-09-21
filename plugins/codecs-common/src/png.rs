//! PNG page export, and a page from a PNG.

use std::io::Cursor;

use image::codecs::png::PngDecoder;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder};
use onionskin_core::images::ImageColor;
use onionskin_plugin_api::{
    BaseRaster, CodecPlugin, Document, ExportError, ExportOutputKind, ExportRequest, ImportError,
    PageIndex,
};

use crate::import::{decode_error, document, dpi_or_default, Decoded};

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// One PNG per page, rasterized by the renderer that draws the canvas.
///
/// The pixels come from [`Document::render_page_now`], so an exported page is
/// the page on screen at that zoom, annotations and all, rather than a second
/// rasterizer's reading of the file.
pub struct PngCodec;

impl CodecPlugin for PngCodec {
    fn id(&self) -> &'static str {
        "png"
    }

    fn name(&self) -> &'static str {
        "PNG Image"
    }

    fn extension(&self) -> &'static str {
        "png"
    }

    fn output_kind(&self) -> ExportOutputKind {
        ExportOutputKind::PerPage
    }

    fn export_page(
        &self,
        doc: &mut Document,
        request: &ExportRequest,
        page: PageIndex,
        _first_in_request: bool,
    ) -> Result<Vec<u8>, ExportError> {
        let zoom = request.zoom()?;
        let rendered = doc
            .render_page_now(page, zoom)
            .map_err(|source| ExportError::Page { page, source })?;
        encode(&rendered.raster, page)
    }

    fn imports(&self) -> bool {
        true
    }

    fn reads(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(SIGNATURE)
    }

    /// Gray stays gray and colour becomes RGB, at 8 bits; a palette is
    /// expanded; transparency becomes a soft mask; the resolution is the
    /// file's `pHYs`, and an embedded ICC profile is kept.
    fn import(&self, bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        let mut decoder = PngDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
        let icc = decoder.icc_profile().ok().flatten();
        let image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
        let (width, height) = (image.width(), image.height());
        let has_alpha = image.color().has_alpha();
        let (color, samples) = match (image.color().has_color(), has_alpha) {
            (false, false) => (ImageColor::Gray, image.into_luma8().into_raw()),
            (false, true) => (ImageColor::Gray, image.into_luma_alpha8().into_raw()),
            (true, false) => (ImageColor::Rgb, image.to_rgb8().into_raw()),
            (true, true) => (ImageColor::Rgb, image.into_rgba8().into_raw()),
        };
        let decoded = Decoded {
            width,
            height,
            color,
            has_alpha,
            samples,
            dpi: dpi_or_default(physical_dpi(bytes)),
            icc,
        };
        document(vec![decoded.into_page()])
    }
}

/// Pixels per inch from the `pHYs` chunk, when it states metres. A `pHYs`
/// with unit 0 is an aspect ratio, not a size, and says nothing here.
fn physical_dpi(bytes: &[u8]) -> Option<(f64, f64)> {
    let (_, data) = chunks(bytes).find(|(kind, _)| kind == b"pHYs")?;
    if data.len() < 9 || data[8] != 1 {
        return None;
    }
    let per_inch = |per_metre: u32| f64::from(per_metre) * 0.0254;
    Some((per_inch(be32(&data[0..4])), per_inch(be32(&data[4..8]))))
}

/// The chunks of a PNG, as `(type, data)`, until the first malformed one.
fn chunks(bytes: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut at = SIGNATURE.len();
    std::iter::from_fn(move || {
        let header = bytes.get(at..at + 8)?;
        let length = be32(&header[0..4]) as usize;
        let kind: [u8; 4] = header[4..8].try_into().ok()?;
        let data = bytes.get(at + 8..at.checked_add(8 + length)?)?;
        at += 8 + length + 4;
        Some((kind, data))
    })
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn encode(raster: &BaseRaster, page: usize) -> Result<Vec<u8>, ExportError> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &straight_alpha(raster.rgba()),
            raster.width(),
            raster.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|source| ExportError::Encode {
            page,
            source: Box::new(source),
        })?;
    Ok(bytes)
}

/// A [`BaseRaster`] holds premultiplied RGBA; PNG stores straight alpha.
///
/// Today every base raster is opaque, because the renderer fills the page
/// with white before it draws, which makes this the identity and the exported
/// pixels bit-for-bit what the canvas composites. It is still the conversion
/// the format calls for, and the day a render arrives with a transparent
/// background it is the difference between correct colours and dark fringes.
fn straight_alpha(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premultiplied.len());
    for pixel in premultiplied.as_chunks::<4>().0 {
        let alpha = pixel[3];
        if alpha == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for channel in &pixel[..3] {
            out.push((u16::from(*channel) * 255 / u16::from(alpha)).min(255) as u8);
        }
        out.push(alpha);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG with a `pHYs` chunk of `per_metre` pixels, written by hand so
    /// the chunk is exactly what the test says.
    fn png_with_phys(per_metre: u32, unit: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[0, 0, 0, 0], 2, 2, ExtendedColorType::L8)
            .expect("encodes");
        let mut phys = per_metre.to_be_bytes().to_vec();
        phys.extend_from_slice(&per_metre.to_be_bytes());
        phys.push(unit);
        let mut chunk = 9u32.to_be_bytes().to_vec();
        chunk.extend_from_slice(b"pHYs");
        chunk.extend_from_slice(&phys);
        chunk.extend_from_slice(&[0; 4]);
        // After the signature and the 25-byte IHDR chunk.
        let at = SIGNATURE.len() + 25;
        bytes.splice(at..at, chunk);
        bytes
    }

    #[test]
    fn a_phys_in_metres_is_a_resolution_and_an_aspect_ratio_is_not() {
        let dpi = physical_dpi(&png_with_phys(11_811, 1)).expect("stated");
        assert!((dpi.0 - 300.0).abs() < 0.01, "{dpi:?}");
        assert_eq!(physical_dpi(&png_with_phys(11_811, 0)), None);
    }

    #[test]
    fn a_truncated_png_ends_its_chunks_rather_than_panicking() {
        let bytes = png_with_phys(11_811, 1);
        assert_eq!(chunks(&bytes[..SIGNATURE.len() + 12]).count(), 0);
        assert_eq!(physical_dpi(&bytes[..SIGNATURE.len() + 4]), None);
    }

    #[test]
    fn an_opaque_raster_survives_the_alpha_conversion_untouched() {
        let opaque = vec![10, 200, 30, 255, 0, 0, 0, 255];

        assert_eq!(straight_alpha(&opaque), opaque);
    }

    #[test]
    fn a_half_transparent_pixel_is_divided_back_out_and_a_clear_one_is_dropped() {
        let premultiplied = vec![64, 32, 16, 128, 9, 9, 9, 0];

        assert_eq!(
            straight_alpha(&premultiplied),
            vec![127, 63, 31, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn an_encoded_raster_decodes_to_the_pixels_it_was_given() {
        let raster = BaseRaster::new(2, 1, 1.0, vec![10, 20, 30, 255, 40, 50, 60, 255]);

        let bytes = encode(&raster, 0).expect("raster encodes");

        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .expect("the export is a PNG")
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.into_raw(), raster.rgba());
    }
}
