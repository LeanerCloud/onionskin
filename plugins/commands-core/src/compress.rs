//! Compress a PDF (Acrobat's Reduce File Size): the one M3 save path that
//! rewrites the file rather than appending to it, and so the one that
//! discards its history.
//!
//! **What it does, and only this.** Images drawn at more than the target
//! resolution are downsampled and written as JPEG, objects nothing refers
//! to are dropped, and the result is written as a new file with a classic
//! cross-reference table. It writes no object streams and no
//! cross-reference stream: `cos` has no writer for either, and adding one is
//! its own deliverable, not a clause of this one. A document whose bulk is
//! already in object streams will shrink by little, and the row says so.
//!
//! **What an image's resolution is taken to be.** Its pixels over the page
//! it is first drawn on. An image drawn smaller than the page has a higher
//! real resolution than that, so an image downsampled to the target by this
//! measure still meets the target wherever it is drawn: the rule can keep
//! more pixels than needed, never fewer.
//!
//! **Encrypted documents are refused** under the encrypted-source rule:
//! compressing one would either keep `/Encrypt` and write a file no reader
//! opens, or drop it and write a decrypted copy.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use onionskin_core::images::{document_images, DocumentImage, ImageColor, ImageData};
use onionskin_core::protection::Refusal;
use onionskin_core::Document;
use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object, Stream, XrefEntry};

/// What a compress that found nothing to do says.
pub const NOTHING_TO_COMPRESS: &str =
    "Nothing to compress: no image is above the target resolution, and a rewrite would not be smaller";

/// Where the copy goes by default: beside the document, named after it.
pub fn reduced_path(document: &std::path::Path) -> std::path::PathBuf {
    let stem = document
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    document.with_file_name(format!("{stem} (reduced).pdf"))
}

/// How hard to compress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressOptions {
    /// Images above this many pixels per inch of their page are downsampled
    /// to it.
    pub target_dpi: f64,
    /// The JPEG quality a downsampled image is written at, 1 to 100.
    pub jpeg_quality: u8,
}

impl Default for CompressOptions {
    /// Acrobat's "Reduce File Size" defaults for colour and greyscale
    /// images: 150 ppi, medium JPEG quality.
    fn default() -> Self {
        CompressOptions {
            target_dpi: 150.0,
            jpeg_quality: 75,
        }
    }
}

/// What compressing produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Compressed {
    /// Nothing to compress: no image above the target and nothing
    /// unreferenced, or a rewrite that would not be smaller. No file is
    /// written, rather than a same-size rewrite that would throw the
    /// history away for nothing.
    Nothing,
    /// A new, smaller file.
    Smaller {
        bytes: Vec<u8>,
        /// The document's size before, as it stands on disk.
        before: usize,
        images: usize,
        dropped: usize,
    },
}

#[derive(Debug)]
pub enum CompressError {
    /// The encrypted-source rule.
    Refused(Refusal),
    Core(onionskin_core::Error),
    Cos(onionskin_cos::Error),
}

impl std::fmt::Display for CompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "{refusal}"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Cos(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CompressError {}

impl From<onionskin_core::Error> for CompressError {
    fn from(error: onionskin_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<onionskin_cos::Error> for CompressError {
    fn from(error: onionskin_cos::Error) -> Self {
        Self::Cos(error)
    }
}

/// Compress `doc` as it stands, edits included, into a new file's bytes.
pub fn compress(
    doc: &mut Document,
    options: &CompressOptions,
) -> Result<Compressed, CompressError> {
    if let Some(refusal) = doc.read_out_refusal() {
        return Err(CompressError::Refused(refusal));
    }
    let before = doc.bytes().len();
    let source = doc.structure()?;
    let reached = source.reachable_from_trailer();
    let in_use = source
        .xref()
        .iter()
        .filter(|(number, entry)| *number != 0 && !matches!(entry, XrefEntry::Free { .. }))
        .count();
    let dropped = in_use.saturating_sub(reached.len());

    let mut objects: Vec<(ObjRef, Object)> = Vec::with_capacity(reached.len());
    for number in &reached {
        let parsed = source.get(*number)?;
        objects.push((parsed.objref, parsed.object));
    }
    let mut images = 0;
    for image in document_images(source)? {
        let Some(limit) = pixel_limit(source, &image, options.target_dpi) else {
            continue;
        };
        let Some(slot) = objects
            .iter_mut()
            .find(|(objref, _)| objref.number == image.object.number)
        else {
            continue;
        };
        if let Some(smaller) = downsample(&slot.1, &image, limit, options.jpeg_quality) {
            slot.1 = Object::Stream(smaller);
            images += 1;
        }
    }
    if images == 0 && dropped == 0 {
        return Ok(Compressed::Nothing);
    }
    let bytes = CosDocument::write_new(&objects, new_trailer(source.trailer()))?;
    // Dropping an object stream's container and writing its objects out
    // plainly can make a file bigger; a "compressed" file that is not
    // smaller is not written.
    if bytes.len() >= before {
        return Ok(Compressed::Nothing);
    }
    Ok(Compressed::Smaller {
        bytes,
        before,
        images,
        dropped,
    })
}

/// The trailer a rewritten file carries: what names its catalog and its
/// information, and nothing that described the old file's layout.
fn new_trailer(old: &Dict) -> Dict {
    let mut trailer = Dict::new();
    for key in ["Root", "Info", "ID"] {
        if let Some(value) = old.get(key.as_bytes()) {
            trailer.set(Name::new(key), value.clone());
        }
    }
    trailer
}

/// The most pixels along its longer side `image` should keep, or `None`
/// when it is already at or under the target.
fn pixel_limit(doc: &CosDocument, image: &DocumentImage, target_dpi: f64) -> Option<u32> {
    let page = doc.page(image.page).ok()?;
    let longest_inches = media_box(doc, &page.dict)
        .map(|(width, height)| width.max(height) / 72.0)
        .unwrap_or(11.0);
    let limit = (target_dpi * longest_inches).round().max(1.0) as u32;
    // A few percent of slack: re-encoding an image that is barely over
    // costs quality and saves almost nothing.
    let longest = image.width.max(image.height);
    (f64::from(longest) > f64::from(limit) * 1.05).then_some(limit)
}

fn media_box(doc: &CosDocument, page: &Dict) -> Option<(f64, f64)> {
    let value = doc.resolve(page.get(b"MediaBox")?).ok()?;
    let Object::Array(items) = value else {
        return None;
    };
    let number = |object: &Object| match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    };
    let values: Vec<f64> = items.iter().filter_map(number).collect();
    match values[..] {
        [x0, y0, x1, y1] => Some(((x1 - x0).abs(), (y1 - y0).abs())),
        _ => None,
    }
}

/// `object` as a smaller JPEG no longer than `limit` pixels on its longer
/// side, or `None` when it cannot be, or would not be smaller.
fn downsample(object: &Object, image: &DocumentImage, limit: u32, quality: u8) -> Option<Stream> {
    let Object::Stream(stream) = object else {
        return None;
    };
    // An image mask, a /Decode array or an explicit /Mask all change what
    // the samples mean; re-encoding them as plain JPEG would change the
    // picture, so they are left as they are.
    if ["ImageMask", "Decode", "Mask"]
        .iter()
        .any(|key| stream.dict.get(key.as_bytes()).is_some())
    {
        return None;
    }
    let (color, data) = image.content.as_ref().ok()?;
    let decoded = decode(*color, data, image.width, image.height)?;
    let scale = f64::from(limit) / f64::from(image.width.max(image.height));
    let width = ((f64::from(image.width) * scale).round() as u32).max(1);
    let height = ((f64::from(image.height) * scale).round() as u32).max(1);
    let resized = decoded.resize_exact(width, height, FilterType::Triangle);
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, quality.clamp(1, 100))
        .encode_image(&resized)
        .ok()?;
    if jpeg.len() >= stream.raw.len() {
        return None;
    }
    let mut dict = stream.dict.clone();
    dict.remove(b"DecodeParms");
    dict.set(Name::new("Width"), Object::Integer(i64::from(width)));
    dict.set(Name::new("Height"), Object::Integer(i64::from(height)));
    dict.set(Name::new("BitsPerComponent"), Object::Integer(8));
    dict.set(Name::new("Filter"), Object::name("DCTDecode"));
    Some(Stream { dict, raw: jpeg })
}

/// The image's pixels, for Gray and RGB only: a CMYK image is left alone,
/// because a JPEG round trip through RGB would change the colours a print
/// file was made with.
fn decode(color: ImageColor, data: &ImageData, width: u32, height: u32) -> Option<DynamicImage> {
    match (color, data) {
        (ImageColor::Cmyk, _) => None,
        (_, ImageData::Jpeg(bytes)) => {
            let image = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg).ok()?;
            match color {
                ImageColor::Gray => Some(DynamicImage::ImageLuma8(image.to_luma8())),
                _ => Some(DynamicImage::ImageRgb8(image.to_rgb8())),
            }
        }
        (ImageColor::Gray, ImageData::Samples(samples)) => {
            GrayImage::from_raw(width, height, samples.clone()).map(DynamicImage::ImageLuma8)
        }
        (ImageColor::Rgb, ImageData::Samples(samples)) => {
            RgbImage::from_raw(width, height, samples.clone()).map(DynamicImage::ImageRgb8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_acrobats_reduce_file_size() {
        let options = CompressOptions::default();
        assert_eq!(options.target_dpi, 150.0);
        assert_eq!(options.jpeg_quality, 75);
    }

    #[test]
    fn a_cmyk_image_is_never_re_encoded() {
        assert!(decode(ImageColor::Cmyk, &ImageData::Samples(vec![0; 4]), 1, 1).is_none());
        assert!(decode(ImageColor::Rgb, &ImageData::Samples(vec![0; 3]), 1, 1).is_some());
        assert!(
            decode(ImageColor::Rgb, &ImageData::Samples(vec![0; 2]), 1, 1).is_none(),
            "too few samples"
        );
    }

    #[test]
    fn the_new_trailer_keeps_only_what_names_the_document() {
        let mut old = Dict::new();
        old.set(Name::new("Root"), Object::Ref(ObjRef::new(1, 0)));
        old.set(Name::new("Prev"), Object::Integer(9));
        old.set(Name::new("XRefStm"), Object::Integer(9));
        let trailer = new_trailer(&old);
        assert!(trailer.get(b"Root").is_some());
        assert!(trailer.get(b"Prev").is_none());
        assert!(trailer.get(b"XRefStm").is_none());
    }
}
