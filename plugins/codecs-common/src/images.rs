//! Export All Images: every image a document's pages draw, as files.
//!
//! `core::images` finds them; this chooses each one's format. A JPEG comes
//! out as the bytes in the PDF, never re-encoded. Gray and RGB samples become
//! PNG, which is lossless. CMYK samples become a CMYK TIFF, because PNG has
//! no CMYK and converting would change the colours a print file was made
//! with. What cannot be extracted is listed with its reason.
//!
//! Inline images (`BI` ... `EI`) live inside content streams rather than as
//! objects, and are out of scope; so is an image's `/SMask`, which is written
//! as the mask it is rather than merged into the image.

use image::{ExtendedColorType, ImageEncoder};
use onionskin_core::images::{decode_image, document_images, DocumentImage, ImageColor, ImageData};
use onionskin_cos::{Document as CosDocument, ObjRef, Object};

use crate::tiff::encode;

/// An image, ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedImage {
    /// A file name naming the page it was first drawn on and its object, so
    /// two images never share one: `page-3-image-41.png`.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// An image that was found and could not be written, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedImage {
    pub page: usize,
    pub object: u32,
    pub reason: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Extraction {
    pub images: Vec<ExtractedImage>,
    pub skipped: Vec<SkippedImage>,
}

/// Every image `doc`'s pages draw, once each, in page order.
pub fn extract_images(doc: &CosDocument) -> onionskin_core::Result<Extraction> {
    let mut extraction = Extraction::default();
    for image in document_images(doc)? {
        match file(&image) {
            Ok((extension, bytes)) => extraction.images.push(ExtractedImage {
                name: name_for(&image, extension),
                bytes,
            }),
            Err(reason) => extraction.skipped.push(SkippedImage {
                page: image.page,
                object: image.object.number,
                reason,
            }),
        }
    }
    Ok(extraction)
}

/// The image XObject `object`, drawn on `page`, as a file: what Save Image
/// As writes, in the format Export All Images would give it.
pub fn extract_image(
    doc: &CosDocument,
    object: ObjRef,
    page: usize,
) -> Result<ExtractedImage, String> {
    let parsed = doc.get(object.number).map_err(|error| error.to_string())?;
    let Object::Stream(stream) = parsed.object else {
        return Err("the object is not an image".to_owned());
    };
    let (width, height, color, data) = decode_image(doc, &stream)?;
    let image = DocumentImage {
        page,
        object,
        width,
        height,
        content: Ok((color, data)),
    };
    let (extension, bytes) = file(&image)?;
    Ok(ExtractedImage {
        name: name_for(&image, extension),
        bytes,
    })
}

fn name_for(image: &DocumentImage, extension: &str) -> String {
    format!(
        "page-{}-image-{}.{extension}",
        image.page + 1,
        image.object.number
    )
}

fn file(image: &DocumentImage) -> Result<(&'static str, Vec<u8>), String> {
    let (color, data) = image.content.as_ref().map_err(String::clone)?;
    match (color, data) {
        (_, ImageData::Jpeg(bytes)) => Ok(("jpg", bytes.clone())),
        (ImageColor::Cmyk, ImageData::Samples(samples)) => {
            encode::<tiff::encoder::colortype::CMYK8>(image.width, image.height, samples, 72)
                .map(|bytes| ("tif", bytes))
                .map_err(|error| error.to_string())
        }
        (ImageColor::Gray | ImageColor::Rgb, ImageData::Samples(samples)) => {
            let kind = if *color == ImageColor::Gray {
                ExtendedColorType::L8
            } else {
                ExtendedColorType::Rgb8
            };
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(samples, image.width, image.height, kind)
                .map_err(|error| error.to_string())?;
            Ok(("png", bytes))
        }
    }
}
