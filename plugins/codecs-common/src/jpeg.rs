//! JPEG page export, and a page from a JPEG.
//!
//! Import never decodes: the file goes into the PDF as it is, `/DCTDecode`,
//! and only its header is read, for the size, the colour, the resolution and
//! the profile. Decoding and re-encoding would lose quality for nothing, and
//! would turn a CMYK file meant for print into RGB.

use std::io::Cursor;

use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::ExtendedColorType;
use onionskin_core::images::{ImageColor, ImageData, ImagePage};
use onionskin_plugin_api::{
    CodecPlugin, Document, ExportError, ExportOutputKind, ExportRequest, ImportError, PageIndex,
};

use crate::import::{document, dpi_or_default};
use crate::raster::{rgb_over_white, whole_dpi};

/// One JPEG per page, at the quality the codec was made with.
pub struct JpegCodec {
    quality: u8,
}

impl JpegCodec {
    /// What the registered codec uses: high enough that text stays clean.
    pub const DEFAULT_QUALITY: u8 = 90;

    /// A codec encoding at `quality`, 1 (smallest) to 100 (best).
    pub fn new(quality: u8) -> Self {
        Self {
            quality: quality.clamp(1, 100),
        }
    }

    pub fn quality(&self) -> u8 {
        self.quality
    }
}

impl Default for JpegCodec {
    fn default() -> Self {
        Self::new(Self::DEFAULT_QUALITY)
    }
}

impl CodecPlugin for JpegCodec {
    fn id(&self) -> &'static str {
        "jpeg"
    }

    fn name(&self) -> &'static str {
        "JPEG Image"
    }

    fn extension(&self) -> &'static str {
        "jpg"
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
        let raster = &rendered.raster;
        let mut bytes = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(Cursor::new(&mut bytes), self.quality);
        encoder.set_pixel_density(PixelDensity::dpi(whole_dpi(request.dpi)));
        encoder
            .encode(
                &rgb_over_white(raster),
                raster.width(),
                raster.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|source| ExportError::Encode {
                page,
                source: Box::new(source),
            })?;
        Ok(bytes)
    }

    fn reads(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    }

    fn import(&self, bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        document(vec![page(bytes)?])
    }
}

/// The page a JPEG becomes, from its header alone.
pub(crate) fn page(bytes: &[u8]) -> Result<ImagePage, ImportError> {
    let header = header(bytes).map_err(ImportError::Decode)?;
    let color = match header.components {
        1 => ImageColor::Gray,
        3 => ImageColor::Rgb,
        4 => ImageColor::Cmyk,
        other => {
            return Err(ImportError::Decode(format!(
                "a JPEG with {other} colour components"
            )))
        }
    };
    if header.precision != 8 {
        return Err(ImportError::Decode(format!(
            "a {}-bit JPEG, which a PDF cannot carry as it is",
            header.precision
        )));
    }
    Ok(ImagePage {
        width: u32::from(header.width),
        height: u32::from(header.height),
        dpi: dpi_or_default(header.dpi),
        color,
        data: ImageData::Jpeg(bytes.to_vec()),
        alpha: None,
        inverted_cmyk: header.adobe && color == ImageColor::Cmyk,
        icc: header.icc,
    })
}

/// What a JPEG's markers say, up to its first scan.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Header {
    pub width: u16,
    pub height: u16,
    pub components: u8,
    pub precision: u8,
    /// From the JFIF segment, when it states a unit. EXIF's resolution is
    /// not read: in practice it is 72 whatever the image, which is the
    /// default anyway.
    pub dpi: Option<(f64, f64)>,
    /// An Adobe APP14 segment, which marks CMYK stored inverted.
    pub adobe: bool,
    /// The APP2 `ICC_PROFILE` chunks, joined in sequence order.
    pub icc: Option<Vec<u8>>,
}

pub(crate) fn header(bytes: &[u8]) -> Result<Header, String> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".to_owned());
    }
    let mut header = Header::default();
    let mut icc_chunks = Vec::new();
    let mut found_frame = false;
    let mut at = 2;
    while let Some((marker, segment, next)) = segment(bytes, at)? {
        match marker {
            0xE0 => header.dpi = jfif_dpi(segment).or(header.dpi),
            0xE2 if segment.starts_with(b"ICC_PROFILE\0") && segment.len() > 14 => {
                icc_chunks.push((segment[12], segment[14..].to_vec()));
            }
            0xEE if segment.starts_with(b"Adobe") => header.adobe = true,
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                frame(segment, &mut header)?;
                found_frame = true;
            }
            _ => {}
        }
        at = next;
    }
    if !found_frame {
        return Err("a JPEG with no frame header".to_owned());
    }
    if !icc_chunks.is_empty() {
        icc_chunks.sort_by_key(|(sequence, _)| *sequence);
        header.icc = Some(icc_chunks.into_iter().flat_map(|(_, data)| data).collect());
    }
    Ok(header)
}

/// A marker, its segment's data, and where the next segment starts.
type Segment<'a> = (u8, &'a [u8], usize);

/// The segment at `at`. `None` at the first scan or the end of the image,
/// past which there are no headers.
fn segment(bytes: &[u8], mut at: usize) -> Result<Option<Segment<'_>>, String> {
    loop {
        if bytes.get(at) != Some(&0xFF) {
            return Err("the JPEG's markers end before its first scan".to_owned());
        }
        let marker = *bytes.get(at + 1).ok_or("the JPEG ends inside a marker")?;
        match marker {
            0xFF => at += 1,
            0x01 | 0xD0..=0xD7 => at += 2,
            0xD9 | 0xDA => return Ok(None),
            _ => break,
        }
    }
    let length = bytes
        .get(at + 2..at + 4)
        .map(|length| usize::from(u16::from_be_bytes([length[0], length[1]])))
        .filter(|length| *length >= 2)
        .ok_or("a JPEG segment with no length")?;
    let data = bytes
        .get(at + 4..at + 2 + length)
        .ok_or("a JPEG segment runs past the end of the file")?;
    Ok(Some((bytes[at + 1], data, at + 2 + length)))
}

fn frame(segment: &[u8], header: &mut Header) -> Result<(), String> {
    if segment.len() < 6 {
        return Err("a JPEG frame header that is too short".to_owned());
    }
    header.precision = segment[0];
    header.height = u16::from_be_bytes([segment[1], segment[2]]);
    header.width = u16::from_be_bytes([segment[3], segment[4]]);
    header.components = segment[5];
    if header.width == 0 || header.height == 0 {
        return Err("a JPEG whose height is given after its first scan".to_owned());
    }
    Ok(())
}

/// The JFIF segment's density, in pixels per inch, when its unit is inches
/// or centimetres. Unit 0 is an aspect ratio.
fn jfif_dpi(segment: &[u8]) -> Option<(f64, f64)> {
    if !segment.starts_with(b"JFIF\0") || segment.len() < 12 {
        return None;
    }
    let x = f64::from(u16::from_be_bytes([segment[8], segment[9]]));
    let y = f64::from(u16::from_be_bytes([segment[10], segment[11]]));
    match segment[7] {
        1 => Some((x, y)),
        2 => Some((x * 2.54, y * 2.54)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(code: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, code];
        out.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    fn jpeg(segments: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for segment in segments {
            out.extend_from_slice(segment);
        }
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        out
    }

    fn sof(width: u16, height: u16, components: u8) -> Vec<u8> {
        let mut data = vec![8];
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&width.to_be_bytes());
        data.push(components);
        marker(0xC2, &data)
    }

    fn jfif(unit: u8, x: u16, y: u16) -> Vec<u8> {
        let mut data = b"JFIF\0\x01\x02".to_vec();
        data.push(unit);
        data.extend_from_slice(&x.to_be_bytes());
        data.extend_from_slice(&y.to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        marker(0xE0, &data)
    }

    fn icc(sequence: u8, of: u8, data: &[u8]) -> Vec<u8> {
        let mut body = b"ICC_PROFILE\0".to_vec();
        body.extend_from_slice(&[sequence, of]);
        body.extend_from_slice(data);
        marker(0xE2, &body)
    }

    #[test]
    fn a_progressive_cmyk_adobe_jpeg_reads_its_size_colour_and_inversion() {
        let bytes = jpeg(&[marker(0xEE, b"Adobe\0\x64\0\0\0\0\x02"), sof(640, 480, 4)]);
        let header = header(&bytes).expect("reads");
        assert_eq!(
            (header.width, header.height, header.components),
            (640, 480, 4)
        );
        assert!(header.adobe);
        let page = page(&bytes).expect("a page");
        assert_eq!(page.color, ImageColor::Cmyk);
        assert!(page.inverted_cmyk);
        assert_eq!(page.data, ImageData::Jpeg(bytes));
    }

    #[test]
    fn jfif_density_in_inches_and_centimetres_and_not_as_an_aspect_ratio() {
        let dpi = |segment| header(&jpeg(&[segment, sof(1, 1, 3)])).expect("reads").dpi;
        assert_eq!(dpi(jfif(1, 300, 150)), Some((300.0, 150.0)));
        assert_eq!(dpi(jfif(2, 100, 100)), Some((254.0, 254.0)));
        assert_eq!(dpi(jfif(0, 1, 1)), None);
    }

    #[test]
    fn icc_chunks_join_in_their_stated_order() {
        let bytes = jpeg(&[icc(2, 2, b"world"), icc(1, 2, b"hello "), sof(1, 1, 3)]);
        assert_eq!(
            header(&bytes).expect("reads").icc,
            Some(b"hello world".to_vec())
        );
    }

    #[test]
    fn what_a_pdf_cannot_carry_is_refused_with_why() {
        let two = jpeg(&[sof(1, 1, 2)]);
        assert!(page(&two)
            .expect_err("refused")
            .to_string()
            .contains("2 colour"));

        let mut twelve = sof(1, 1, 3);
        twelve[4] = 12;
        let error = page(&jpeg(&[twelve])).expect_err("refused").to_string();
        assert!(error.contains("12-bit"), "{error}");
    }

    #[test]
    fn a_broken_header_is_an_error_and_never_a_panic() {
        assert!(header(b"GIF89a").is_err());
        assert!(header(&[0xFF, 0xD8, 0xFF]).is_err());
        assert!(header(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]).is_err());
        assert!(header(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x40, 1, 2]).is_err());
        assert!(
            header(&[0xFF, 0xD8, 0xFF, 0xDA]).is_err(),
            "no frame before the scan"
        );
        assert!(header(&jpeg(&[sof(0, 0, 3)])).is_err(), "a DNL height");
        assert!(header(&jpeg(&[marker(0xC0, &[8, 0])])).is_err());
        assert!(header(&[0xFF, 0xD8, 0x00]).is_err());
    }

    #[test]
    fn fill_bytes_and_standalone_markers_are_stepped_over() {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xFF, 0xD0];
        bytes.extend_from_slice(&sof(7, 9, 1));
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        let header = header(&bytes).expect("reads");
        assert_eq!((header.width, header.height), (7, 9));
    }

    #[test]
    fn quality_is_clamped_to_what_the_encoder_takes() {
        assert_eq!(JpegCodec::new(0).quality(), 1);
        assert_eq!(JpegCodec::new(250).quality(), 100);
        assert_eq!(JpegCodec::default().quality(), JpegCodec::DEFAULT_QUALITY);
    }
}
