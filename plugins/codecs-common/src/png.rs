//! PNG page export.

use image::{ExtendedColorType, ImageEncoder};
use onionskin_plugin_api::{
    BaseRaster, CodecPlugin, Document, ExportError, ExportRequest, ExportedFile,
};

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

    fn export(
        &self,
        doc: &mut Document,
        request: &ExportRequest,
    ) -> Result<Vec<ExportedFile>, ExportError> {
        let zoom = request.zoom()?;
        let mut out = Vec::new();
        for page in request.pages.pages() {
            let rendered = doc
                .render_page_now(page, zoom)
                .map_err(|source| ExportError::Page { page, source })?;
            out.push(ExportedFile {
                page: Some(page),
                bytes: encode(&rendered.raster, page)?,
            });
        }
        Ok(out)
    }
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
