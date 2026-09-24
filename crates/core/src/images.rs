//! Images in and out of a document: a page made from an image, and every
//! image a document's pages draw.
//!
//! Format-agnostic on purpose. Decoding a PNG or reading a JPEG's header is a
//! codec's job (`codecs-common`); this module takes samples or a JPEG stream
//! and writes the PDF objects, and reads image XObjects back out as the same.
//!
//! # A page from an image
//!
//! One page whose `/MediaBox` is the image's **physical size at its own
//! resolution** - a 3000-pixel-wide scan at 300 DPI is ten inches wide - and
//! whose content draws the image across all of it. A JPEG is embedded as it
//! is, `/DCTDecode`, never decoded and re-encoded: re-encoding loses quality
//! and turns a CMYK file destined for print into RGB. Samples are
//! `/FlateDecode`d in their own colour space, and an alpha channel becomes an
//! `/SMask`.
//!
//! # Images out of a document
//!
//! Every image XObject a page draws, directly or through form XObjects, once
//! each however many pages share it. A JPEG comes out as the bytes in the
//! file; samples come out decoded. What this cannot hand back - JPEG 2000,
//! JBIG2 and CCITT streams, indexed colour, anything above 8 bits - is listed
//! with the reason, not dropped. Inline images (`BI` ... `EI`) live inside
//! content streams rather than as objects and are out of scope.

use std::collections::BTreeSet;

use onionskin_cos::{flate_encode, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use crate::{Error, Result};

/// The colour spaces an image page can be written in, and read back as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageColor {
    Gray,
    Rgb,
    Cmyk,
}

impl ImageColor {
    pub fn components(self) -> usize {
        match self {
            ImageColor::Gray => 1,
            ImageColor::Rgb => 3,
            ImageColor::Cmyk => 4,
        }
    }

    fn device_name(self) -> &'static str {
        match self {
            ImageColor::Gray => "DeviceGray",
            ImageColor::Rgb => "DeviceRGB",
            ImageColor::Cmyk => "DeviceCMYK",
        }
    }
}

/// An image's pixels, as they go into or come out of a PDF.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageData {
    /// A complete JPEG file, embedded or extracted without re-encoding.
    Jpeg(Vec<u8>),
    /// Uncompressed samples, 8 bits per component, rows top to bottom.
    Samples(Vec<u8>),
}

/// What a page is made from.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePage {
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, horizontally and vertically. A file that states none
    /// is taken at 72, which makes one pixel one point.
    pub dpi: (f64, f64),
    pub color: ImageColor,
    pub data: ImageData,
    /// 8-bit coverage, one byte per pixel, for an image with transparency.
    pub alpha: Option<Vec<u8>>,
    /// A CMYK JPEG written by Adobe software stores its channels inverted,
    /// which its APP14 marker says; the image's `/Decode` undoes it.
    pub inverted_cmyk: bool,
    /// The source's embedded ICC profile, carried as the image's
    /// `/ICCBased` colour space so its colours mean what they meant.
    pub icc: Option<Vec<u8>>,
}

impl ImagePage {
    /// The page size in points: pixels at the image's own resolution.
    pub fn page_size(&self) -> (f64, f64) {
        (
            f64::from(self.width) * 72.0 / self.dpi.0,
            f64::from(self.height) * 72.0 / self.dpi.1,
        )
    }

    fn validate(&self) -> Result<()> {
        let invalid = |detail: &str| Err(Error::InvalidImage(detail.to_owned()));
        if self.width == 0 || self.height == 0 {
            return invalid("an image needs at least one pixel");
        }
        if !(self.dpi.0.is_finite()
            && self.dpi.1.is_finite()
            && self.dpi.0 > 0.0
            && self.dpi.1 > 0.0)
        {
            return invalid("an image's resolution has to be positive");
        }
        let pixels = self.width as usize * self.height as usize;
        if let ImageData::Samples(samples) = &self.data {
            if samples.len() != pixels * self.color.components() {
                return invalid("the samples do not fill the image");
            }
        }
        if self
            .alpha
            .as_ref()
            .is_some_and(|alpha| alpha.len() != pixels)
        {
            return invalid("the alpha channel does not fill the image");
        }
        Ok(())
    }
}

/// A one-page document drawing `image` at its own physical size, as bytes.
/// Several are joined into one document with `pages::Assembly`.
pub fn image_document(image: &ImagePage) -> Result<Vec<u8>> {
    image.validate()?;
    let (width, height) = image.page_size();
    let (catalog, pages, page, content, xobject, smask, profile) = (1, 2, 3, 4, 5, 6, 7);

    let mut objects = vec![
        (
            ObjRef::new(catalog, 0),
            dict(&[
                ("Type", Object::name("Catalog")),
                ("Pages", reference(pages)),
            ]),
        ),
        (
            ObjRef::new(pages, 0),
            dict(&[
                ("Type", Object::name("Pages")),
                ("Kids", Object::Array(vec![reference(page)])),
                ("Count", Object::Integer(1)),
            ]),
        ),
        (
            ObjRef::new(page, 0),
            dict(&[
                ("Type", Object::name("Page")),
                ("Parent", reference(pages)),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Real(width),
                        Object::Real(height),
                    ]),
                ),
                ("Contents", reference(content)),
                (
                    "Resources",
                    dict(&[("XObject", dict(&[("Im0", reference(xobject))]))]),
                ),
            ]),
        ),
        (
            ObjRef::new(content, 0),
            Object::Stream(stream(
                Dict::new(),
                format!("q {width} 0 0 {height} 0 0 cm /Im0 Do Q").into_bytes(),
            )),
        ),
        (
            ObjRef::new(xobject, 0),
            Object::Stream(image_xobject(
                image,
                image.alpha.as_ref().map(|_| smask),
                image.icc.as_ref().map(|_| profile),
            )),
        ),
    ];
    if let Some(icc) = &image.icc {
        objects.push((
            ObjRef::new(profile, 0),
            Object::Stream(icc_stream(image.color, icc)),
        ));
    }
    if let Some(alpha) = &image.alpha {
        objects.push((
            ObjRef::new(smask, 0),
            Object::Stream(samples_xobject(
                image.width,
                image.height,
                ImageColor::Gray,
                alpha,
            )),
        ));
    }

    let mut trailer = Dict::new();
    trailer.set(Name::new("Root"), reference(catalog));
    Ok(CosDocument::write_new(&objects, trailer)?)
}

fn image_xobject(image: &ImagePage, smask: Option<u32>, profile: Option<u32>) -> Stream {
    let mut xobject = match &image.data {
        ImageData::Jpeg(bytes) => {
            let mut dict = image_dict(image.width, image.height, image.color);
            dict.set(Name::new("Filter"), Object::name("DCTDecode"));
            if image.inverted_cmyk && image.color == ImageColor::Cmyk {
                dict.set(
                    Name::new("Decode"),
                    Object::Array([1, 0, 1, 0, 1, 0, 1, 0].map(Object::Integer).to_vec()),
                );
            }
            stream(dict, bytes.clone())
        }
        ImageData::Samples(samples) => {
            samples_xobject(image.width, image.height, image.color, samples)
        }
    };
    if let Some(smask) = smask {
        xobject.dict.set(Name::new("SMask"), reference(smask));
    }
    if let Some(profile) = profile {
        xobject.dict.set(
            Name::new("ColorSpace"),
            Object::Array(vec![Object::name("ICCBased"), reference(profile)]),
        );
    }
    xobject
}

/// An ICC profile stream, with the device space a reader that cannot use the
/// profile falls back to.
fn icc_stream(color: ImageColor, icc: &[u8]) -> Stream {
    let Object::Dict(dict) = dict(&[
        ("N", Object::Integer(color.components() as i64)),
        ("Alternate", Object::name(color.device_name())),
        ("Filter", Object::name("FlateDecode")),
    ]) else {
        unreachable!("dict builds a dictionary");
    };
    stream(dict, flate_encode(icc))
}

fn samples_xobject(width: u32, height: u32, color: ImageColor, samples: &[u8]) -> Stream {
    let mut dict = image_dict(width, height, color);
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    stream(dict, flate_encode(samples))
}

fn image_dict(width: u32, height: u32, color: ImageColor) -> Dict {
    let Object::Dict(dict) = dict(&[
        ("Type", Object::name("XObject")),
        ("Subtype", Object::name("Image")),
        ("Width", Object::Integer(i64::from(width))),
        ("Height", Object::Integer(i64::from(height))),
        ("ColorSpace", Object::name(color.device_name())),
        ("BitsPerComponent", Object::Integer(8)),
    ]) else {
        unreachable!("dict builds a dictionary");
    };
    dict
}

fn stream(mut dict: Dict, raw: Vec<u8>) -> Stream {
    dict.set(Name::new("Length"), Object::Integer(raw.len() as i64));
    Stream { dict, raw }
}

fn dict(entries: &[(&str, Object)]) -> Object {
    let mut dict = Dict::new();
    for (key, value) in entries {
        dict.set(Name::new(key), value.clone());
    }
    Object::Dict(dict)
}

fn reference(number: u32) -> Object {
    Object::Ref(ObjRef::new(number, 0))
}

/// An image a document's pages draw.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentImage {
    /// The first page that draws it.
    pub page: usize,
    pub object: ObjRef,
    pub width: u32,
    pub height: u32,
    /// What can be handed back, or why not.
    pub content: std::result::Result<(ImageColor, ImageData), String>,
}

/// Every image XObject the document's pages draw, in page order, once each.
pub fn document_images(doc: &CosDocument) -> Result<Vec<DocumentImage>> {
    let count = doc.page_count()? as usize;
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for index in 0..count {
        let page = doc.page(index)?;
        let resources = resolve_dict(doc, page.dict.get(b"Resources"));
        collect(doc, index, resources.as_ref(), &mut seen, &mut found, 0);
    }
    Ok(found)
}

/// How deep form XObjects are followed: far past any real document, short of
/// a stack overflow on a hostile one.
const MAX_FORM_DEPTH: usize = 32;

fn collect(
    doc: &CosDocument,
    page: usize,
    resources: Option<&Dict>,
    seen: &mut BTreeSet<u32>,
    found: &mut Vec<DocumentImage>,
    depth: usize,
) {
    if depth > MAX_FORM_DEPTH {
        return;
    }
    let Some(xobjects) =
        resources.and_then(|resources| resolve_dict(doc, resources.get(b"XObject")))
    else {
        return;
    };
    for (_, value) in xobjects.iter() {
        let Object::Ref(objref) = value else { continue };
        if !seen.insert(objref.number) {
            continue;
        }
        let Ok(parsed) = doc.get(objref.number) else {
            continue;
        };
        let Object::Stream(stream) = parsed.object else {
            continue;
        };
        match stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(Name::as_bytes)
        {
            Some(b"Image") => found.push(read_image(doc, page, *objref, &stream)),
            Some(b"Form") => {
                let inner = resolve_dict(doc, stream.dict.get(b"Resources"));
                collect(doc, page, inner.as_ref(), seen, found, depth + 1);
            }
            _ => {}
        }
    }
}

/// An image XObject's size and pixels, or why they cannot be handed back:
/// what redaction paints an area of out.
pub fn decode_image(
    doc: &CosDocument,
    stream: &Stream,
) -> std::result::Result<(u32, u32, ImageColor, ImageData), String> {
    let image = read_image(doc, 0, ObjRef::new(0, 0), stream);
    let (color, data) = image.content?;
    Ok((image.width, image.height, color, data))
}

fn read_image(doc: &CosDocument, page: usize, object: ObjRef, stream: &Stream) -> DocumentImage {
    let integer = |key: &[u8]| {
        stream
            .dict
            .get(key)
            .and_then(Object::as_integer)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0)
    };
    let (width, height) = (integer(b"Width"), integer(b"Height"));
    DocumentImage {
        page,
        object,
        width,
        height,
        content: image_content(doc, stream, width, height),
    }
}

fn image_content(
    doc: &CosDocument,
    stream: &Stream,
    width: u32,
    height: u32,
) -> std::result::Result<(ImageColor, ImageData), String> {
    let color = color_space(doc, stream.dict.get(b"ColorSpace"))?;
    let filters = filter_names(doc, stream.dict.get(b"Filter"));
    if let Some(unsupported) = filters.iter().find(|name| {
        matches!(
            name.as_str(),
            "JPXDecode" | "JBIG2Decode" | "CCITTFaxDecode"
        )
    }) {
        return Err(format!("{unsupported} images are not extracted"));
    }
    if filters.last().is_some_and(|name| name == "DCTDecode") {
        if filters.len() > 1 {
            return Err("a JPEG wrapped in another filter is not extracted".to_owned());
        }
        return Ok((color, ImageData::Jpeg(stream.raw.clone())));
    }
    let bits = stream
        .dict
        .get(b"BitsPerComponent")
        .and_then(Object::as_integer)
        .unwrap_or(8);
    let samples = doc
        .decode_stream(stream)
        .map_err(|error| format!("the image does not decode: {error}"))?;
    let expected = width as usize * height as usize * color.components();
    match bits {
        8 if samples.len() >= expected => {
            Ok((color, ImageData::Samples(samples[..expected].to_vec())))
        }
        1 if color == ImageColor::Gray => Ok((
            color,
            ImageData::Samples(unpack_bits(&samples, width, height)),
        )),
        8 => Err("the image has fewer samples than its size".to_owned()),
        other => Err(format!("{other}-bit images are not extracted")),
    }
}

/// 1-bit rows, each padded to a byte, as 8-bit gray.
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
            out.push(if bit == 1 { 255 } else { 0 });
        }
    }
    out
}

fn color_space(
    doc: &CosDocument,
    value: Option<&Object>,
) -> std::result::Result<ImageColor, String> {
    let resolved = match value {
        Some(Object::Ref(objref)) => doc.get(objref.number).ok().map(|parsed| parsed.object),
        other => other.cloned(),
    };
    match resolved {
        Some(Object::Name(name)) => match name.as_bytes() {
            b"DeviceGray" | b"G" | b"CalGray" => Ok(ImageColor::Gray),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Ok(ImageColor::Rgb),
            b"DeviceCMYK" | b"CMYK" => Ok(ImageColor::Cmyk),
            other => Err(format!(
                "{} colour is not extracted",
                String::from_utf8_lossy(other)
            )),
        },
        Some(Object::Array(items)) => {
            match items.first().and_then(Object::as_name).map(Name::as_bytes) {
                Some(b"ICCBased") => icc_components(doc, items.get(1)),
                Some(b"CalGray") => Ok(ImageColor::Gray),
                Some(b"CalRGB") => Ok(ImageColor::Rgb),
                Some(other) => Err(format!(
                    "{} colour is not extracted",
                    String::from_utf8_lossy(other)
                )),
                None => Err("the image names no colour space".to_owned()),
            }
        }
        _ => Err("the image names no colour space".to_owned()),
    }
}

fn icc_components(
    doc: &CosDocument,
    profile: Option<&Object>,
) -> std::result::Result<ImageColor, String> {
    let Some(Object::Ref(objref)) = profile else {
        return Err("an ICC profile that is not a stream".to_owned());
    };
    let n = doc.get(objref.number).ok().and_then(|parsed| {
        parsed
            .object
            .as_stream()
            .and_then(|s| s.dict.get(b"N").and_then(Object::as_integer))
    });
    match n {
        Some(1) => Ok(ImageColor::Gray),
        Some(3) => Ok(ImageColor::Rgb),
        Some(4) => Ok(ImageColor::Cmyk),
        _ => Err("an ICC profile with an unusual number of components".to_owned()),
    }
}

fn filter_names(doc: &CosDocument, value: Option<&Object>) -> Vec<String> {
    let resolved = match value {
        Some(Object::Ref(objref)) => doc.get(objref.number).ok().map(|parsed| parsed.object),
        other => other.cloned(),
    };
    let name = |object: &Object| {
        object
            .as_name()
            .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
    };
    match resolved {
        Some(Object::Name(_)) => resolved.as_ref().and_then(name).into_iter().collect(),
        Some(Object::Array(items)) => items.iter().filter_map(name).collect(),
        _ => Vec::new(),
    }
}

fn resolve_dict(doc: &CosDocument, value: Option<&Object>) -> Option<Dict> {
    match value? {
        Object::Dict(dict) => Some(dict.clone()),
        Object::Ref(objref) => match doc.get(objref.number).ok()?.object {
            Object::Dict(dict) => Some(dict),
            Object::Stream(stream) => Some(stream.dict),
            _ => None,
        },
        _ => None,
    }
}
