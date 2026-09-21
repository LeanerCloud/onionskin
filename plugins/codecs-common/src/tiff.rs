//! TIFF page export, and pages from a TIFF.
//!
//! Through the `tiff` crate directly: `image` converts CMYK to RGB on the way
//! in and cannot write resolution tags on the way out. A multi-page TIFF - a
//! fax, a scanned stack - becomes a document of that many pages.

use std::io::Cursor;

use onionskin_core::images::{ImageColor, ImagePage};
use onionskin_plugin_api::{
    BaseRaster, CodecPlugin, Document, ExportError, ExportOutputKind, ExportRequest, ImportError,
    PageIndex,
};
use tiff::decoder::{Decoder, DecodingResult};
use tiff::encoder::{colortype, Compression, DeflateLevel, Rational, TiffEncoder};
use tiff::tags::{ResolutionUnit, Tag};
use tiff::ColorType;

use crate::import::{decode_error, document, dpi_or_default, Decoded};
use crate::raster::{rgb_over_white, whole_dpi};

/// How many pages one TIFF may become, far past a real scan stack and short
/// of a file whose directory chain is a loop.
const MAX_PAGES: usize = 10_000;

/// One RGB TIFF per page, Deflate-compressed, carrying its resolution.
pub struct TiffCodec;

impl CodecPlugin for TiffCodec {
    fn id(&self) -> &'static str {
        "tiff"
    }

    fn name(&self) -> &'static str {
        "TIFF Image"
    }

    fn extension(&self) -> &'static str {
        "tif"
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
        encode_rgb(&rendered.raster, whole_dpi(request.dpi)).map_err(|source| ExportError::Encode {
            page,
            source: Box::new(source),
        })
    }

    fn reads(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*")
    }

    fn import(&self, bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(decode_error)?;
        let mut pages = vec![read_page(&mut decoder)?];
        while decoder.more_images() && pages.len() < MAX_PAGES {
            decoder.next_image().map_err(decode_error)?;
            pages.push(read_page(&mut decoder)?);
        }
        document(pages)
    }
}

fn encode_rgb(raster: &BaseRaster, dpi: u16) -> tiff::TiffResult<Vec<u8>> {
    encode::<colortype::RGB8>(
        raster.width(),
        raster.height(),
        &rgb_over_white(raster),
        dpi,
    )
}

/// An 8-bit TIFF of `samples` in the colour type `C`, at `dpi`.
pub(crate) fn encode<C: colortype::ColorType<Inner = u8>>(
    width: u32,
    height: u32,
    samples: &[u8],
    dpi: u16,
) -> tiff::TiffResult<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = TiffEncoder::new(Cursor::new(&mut bytes))?
        .with_compression(Compression::Deflate(DeflateLevel::Balanced));
    let mut image = encoder.new_image::<C>(width, height)?;
    image.resolution(
        ResolutionUnit::Inch,
        Rational {
            n: u32::from(dpi),
            d: 1,
        },
    );
    image.write_data(samples)?;
    Ok(bytes)
}

/// The decoder's current image as a page.
fn read_page<R: std::io::Read + std::io::Seek>(
    decoder: &mut Decoder<R>,
) -> Result<ImagePage, ImportError> {
    let (width, height) = decoder.dimensions().map_err(decode_error)?;
    let color_type = decoder.colortype().map_err(decode_error)?;
    let white_is_zero = decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
        .ok()
        .flatten()
        == Some(0);
    let dpi = dpi_or_default(resolution(decoder));
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile).ok();
    let (color, has_alpha, bits) = layout(color_type)?;
    let raw = decoder.read_image().map_err(decode_error)?;
    let mut samples = match (raw, bits) {
        (DecodingResult::U8(packed), 1) => unpack_bits(&packed, width, height),
        (DecodingResult::U8(samples), 8) => samples,
        (DecodingResult::U16(samples), 16) => {
            samples.iter().map(|value| (value >> 8) as u8).collect()
        }
        _ => return Err(unsupported(color_type)),
    };
    if white_is_zero {
        samples
            .iter_mut()
            .for_each(|sample| *sample = u8::MAX - *sample);
    }
    Ok(Decoded {
        width,
        height,
        color,
        has_alpha,
        samples,
        dpi,
        icc,
    }
    .into_page())
}

/// The colour, whether an alpha channel follows it, and the bit depth.
fn layout(color_type: ColorType) -> Result<(ImageColor, bool, u8), ImportError> {
    let (color, alpha, bits) = match color_type {
        ColorType::Gray(bits) => (ImageColor::Gray, false, bits),
        ColorType::GrayA(bits) => (ImageColor::Gray, true, bits),
        ColorType::RGB(bits) => (ImageColor::Rgb, false, bits),
        ColorType::RGBA(bits) => (ImageColor::Rgb, true, bits),
        ColorType::CMYK(bits) => (ImageColor::Cmyk, false, bits),
        ColorType::CMYKA(bits) => (ImageColor::Cmyk, true, bits),
        other => return Err(unsupported(other)),
    };
    match (color, alpha, bits) {
        (_, _, 8 | 16) | (ImageColor::Gray, false, 1) => Ok((color, alpha, bits)),
        _ => Err(unsupported(color_type)),
    }
}

fn unsupported(color_type: ColorType) -> ImportError {
    ImportError::Decode(format!("{color_type:?} TIFF images are not read"))
}

/// Pixels per inch from the resolution tags; centimetres are converted, and
/// a unit of "none" is an aspect ratio, which states no size.
fn resolution<R: std::io::Read + std::io::Seek>(decoder: &mut Decoder<R>) -> Option<(f64, f64)> {
    let unit = decoder
        .find_tag_unsigned::<u16>(Tag::ResolutionUnit)
        .ok()
        .flatten()
        .unwrap_or(2);
    let scale = match unit {
        2 => 1.0,
        3 => 2.54,
        _ => return None,
    };
    let mut axis = |tag| match decoder.find_tag(tag).ok().flatten()? {
        tiff::decoder::ifd::Value::Rational(n, d) if d != 0 => Some(f64::from(n) / f64::from(d)),
        _ => None,
    };
    Some((
        axis(Tag::XResolution)? * scale,
        axis(Tag::YResolution)? * scale,
    ))
}

/// 1-bit rows, each padded to a byte, as 8-bit gray with 1 as white - the
/// TIFF default, `BlackIsZero`; `WhiteIsZero` is inverted after.
fn unpack_bits(packed: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width as usize).div_ceil(8);
    let mut out = Vec::with_capacity(width as usize * height as usize);
    for row in 0..height as usize {
        for column in 0..width as usize {
            let byte = packed
                .get(row * row_bytes + column / 8)
                .copied()
                .unwrap_or(0);
            let bit = (byte >> (7 - column % 8)) & 1;
            out.push(if bit == 1 { u8::MAX } else { 0 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_a_page_can_hold_is_read() {
        assert!(layout(ColorType::CMYK(8)).is_ok());
        assert!(layout(ColorType::RGBA(16)).is_ok());
        assert!(layout(ColorType::Gray(1)).is_ok());
        assert!(layout(ColorType::RGB(1)).is_err());
        assert!(layout(ColorType::Gray(4)).is_err());
        let refused = layout(ColorType::Palette(8))
            .expect_err("refused")
            .to_string();
        assert!(refused.contains("Palette"), "{refused}");
    }

    #[test]
    fn one_bit_rows_are_padded_to_a_byte() {
        assert_eq!(
            unpack_bits(&[0b1010_0000, 0b0100_0000], 3, 2),
            vec![255, 0, 255, 0, 255, 0]
        );
    }

    #[test]
    fn the_signature_is_either_byte_order() {
        assert!(TiffCodec.reads(b"II*\0...."));
        assert!(TiffCodec.reads(b"MM\0*...."));
        assert!(!TiffCodec.reads(b"%PDF-1.7"));
    }
}
