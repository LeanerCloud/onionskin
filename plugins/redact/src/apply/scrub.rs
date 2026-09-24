//! Images under a redaction: decoded, the covered pixels painted out, and
//! written again as a new image.
//!
//! An image that cannot be decoded (JPEG 2000, JBIG2, CCITT, an indexed or
//! separation colour space) is not kept at all where it was drawn: a blank
//! form takes its place, since what it shows cannot be looked at.

use onionskin_content::redact::Area;
use onionskin_content::Matrix;
use onionskin_core::images::{decode_image, ImageColor, ImageData};
use onionskin_cos::{flate_encode, Dict, Document as CosDocument, Name, Object, Stream};

/// The copy that replaces `image` where it is drawn with `placement`, the
/// pixels whose centres fall in `areas` painted `fill` (white when the mark
/// has none). `None` when the image cannot be decoded.
pub(crate) fn scrubbed(
    doc: &CosDocument,
    image: &Stream,
    placement: &Matrix,
    areas: &[Area],
    fill: Option<[f64; 3]>,
) -> Option<(Stream, Option<Stream>)> {
    let (width, height, color, data) = decode_image(doc, image).ok()?;
    let mut samples = match data {
        ImageData::Samples(samples) => samples,
        ImageData::Jpeg(bytes) => jpeg_samples(&bytes, color, width, height)?,
    };
    let paint = fill_samples(color, fill.unwrap_or([1.0; 3]), decode_array(&image.dict));
    let covered = covered_pixels(placement, areas, width, height);
    let components = color.components();
    for &(column, row) in &covered {
        let at = (row * width as usize + column) * components;
        samples[at..at + components].copy_from_slice(&paint);
    }
    let mask = soft_mask(doc, image, &covered);
    Some((
        image_stream(&image.dict, color, width, height, &samples),
        mask,
    ))
}

/// An empty form, drawn where an image that could not be scrubbed was.
pub(crate) fn blank() -> Stream {
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("XObject"));
    dict.set(Name::new("Subtype"), Object::name("Form"));
    dict.set(
        Name::new("BBox"),
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1),
            Object::Integer(1),
        ]),
    );
    Stream {
        dict,
        raw: Vec::new(),
    }
}

/// The pixels, as `(column, row)` with row 0 at the top, whose centres the
/// image's placement puts inside an area.
fn covered_pixels(
    placement: &Matrix,
    areas: &[Area],
    width: u32,
    height: u32,
) -> Vec<(usize, usize)> {
    let (width, height) = (width as usize, height as usize);
    let mut out = Vec::new();
    for row in 0..height {
        let v = 1.0 - (row as f64 + 0.5) / height as f64;
        for column in 0..width {
            let u = (column as f64 + 0.5) / width as f64;
            let point = placement.apply(u, v);
            if areas.iter().any(|area| area_contains(area, point)) {
                out.push((column, row));
            }
        }
    }
    out
}

fn area_contains(area: &Area, (x, y): (f64, f64)) -> bool {
    let corners = area.corners();
    let sign = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
    let sides: Vec<f64> = (0..4)
        .map(|i| sign(corners[i], corners[(i + 1) % 4]))
        .collect();
    sides.iter().all(|side| *side >= -1e-9) || sides.iter().all(|side| *side <= 1e-9)
}

/// The image's `/Decode`, when it has one.
fn decode_array(dict: &Dict) -> Option<Vec<f64>> {
    let items = dict.get(b"Decode")?.as_array()?;
    let values: Vec<f64> = items
        .iter()
        .filter_map(|value| match value {
            Object::Integer(value) => Some(*value as f64),
            Object::Real(value) => Some(*value),
            _ => None,
        })
        .collect();
    (!values.is_empty()).then_some(values)
}

/// `fill` as the image's samples, through its `/Decode` when it has one, so
/// the painted area shows the fill however the image maps its samples.
fn fill_samples(color: ImageColor, [r, g, b]: [f64; 3], decode: Option<Vec<f64>>) -> Vec<u8> {
    let wanted: Vec<f64> = match color {
        ImageColor::Gray => vec![0.299 * r + 0.587 * g + 0.114 * b],
        ImageColor::Rgb => vec![r, g, b],
        ImageColor::Cmyk => {
            let k = 1.0 - r.max(g).max(b);
            let ink = |c: f64| {
                if k >= 1.0 {
                    0.0
                } else {
                    (1.0 - c - k) / (1.0 - k)
                }
            };
            vec![ink(r), ink(g), ink(b), k]
        }
    };
    wanted
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let (low, high) = decode
                .as_ref()
                .and_then(|decode| Some((*decode.get(index * 2)?, *decode.get(index * 2 + 1)?)))
                .unwrap_or((0.0, 1.0));
            let unit = if (high - low).abs() < 1e-12 {
                0.0
            } else {
                (value - low) / (high - low)
            };
            (unit.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
}

fn jpeg_samples(bytes: &[u8], color: ImageColor, width: u32, height: u32) -> Option<Vec<u8>> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).ok()?;
    if decoded.width() != width || decoded.height() != height {
        return None;
    }
    match color {
        ImageColor::Gray => Some(decoded.into_luma8().into_raw()),
        ImageColor::Rgb => Some(decoded.into_rgb8().into_raw()),
        // A CMYK JPEG decodes to RGB, not to its inks.
        ImageColor::Cmyk => None,
    }
}

/// The image's soft mask with the covered pixels made opaque, so the fill
/// shows through; `None` when it has none or it cannot be read, in which case
/// the copy is written without one.
fn soft_mask(doc: &CosDocument, image: &Stream, covered: &[(usize, usize)]) -> Option<Stream> {
    let mask = doc.resolve(image.dict.get(b"SMask")?).ok()?;
    let mask = mask.as_stream()?;
    let (width, height, color, ImageData::Samples(mut samples)) = decode_image(doc, mask).ok()?
    else {
        return None;
    };
    if color != ImageColor::Gray {
        return None;
    }
    let (image_width, image_height) = image_size(&image.dict);
    for &(column, row) in covered {
        // The mask may have its own resolution: take the pixel it covers.
        let x = column * width as usize / image_width.max(1);
        let y = row * height as usize / image_height.max(1);
        if let Some(sample) = samples.get_mut(y * width as usize + x) {
            *sample = 255;
        }
    }
    Some(image_stream(&mask.dict, color, width, height, &samples))
}

fn image_size(dict: &Dict) -> (usize, usize) {
    let number = |key: &[u8]| {
        dict.get(key)
            .and_then(Object::as_integer)
            .map_or(0, |value| value.max(0) as usize)
    };
    (number(b"Width"), number(b"Height"))
}

/// An 8-bit image of `samples`, Flate compressed, keeping what the original
/// said about how it is drawn.
fn image_stream(
    original: &Dict,
    color: ImageColor,
    width: u32,
    height: u32,
    samples: &[u8],
) -> Stream {
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("XObject"));
    dict.set(Name::new("Subtype"), Object::name("Image"));
    dict.set(Name::new("Width"), Object::Integer(i64::from(width)));
    dict.set(Name::new("Height"), Object::Integer(i64::from(height)));
    dict.set(
        Name::new("ColorSpace"),
        Object::name(match color {
            ImageColor::Gray => "DeviceGray",
            ImageColor::Rgb => "DeviceRGB",
            ImageColor::Cmyk => "DeviceCMYK",
        }),
    );
    dict.set(Name::new("BitsPerComponent"), Object::Integer(8));
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    for key in ["Decode", "Interpolate", "Intent"] {
        if let Some(value) = original.get(key.as_bytes()) {
            dict.set(Name::new(key), value.clone());
        }
    }
    Stream {
        dict,
        raw: flate_encode(samples),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fill_passes_through_the_decode_array() {
        assert_eq!(
            fill_samples(ImageColor::Rgb, [1.0, 0.0, 0.0], None),
            [255, 0, 0]
        );
        assert_eq!(
            fill_samples(ImageColor::Gray, [0.0; 3], Some(vec![1.0, 0.0])),
            [255]
        );
        assert_eq!(
            fill_samples(ImageColor::Cmyk, [0.0; 3], None),
            [0, 0, 0, 255]
        );
        assert_eq!(fill_samples(ImageColor::Cmyk, [1.0; 3], None), [0, 0, 0, 0]);
        assert_eq!(
            fill_samples(ImageColor::Gray, [1.0; 3], Some(vec![0.5, 0.5])),
            [0]
        );
    }

    #[test]
    fn covered_pixels_are_those_whose_centres_are_inside() {
        // A 4x2 image drawn over [0 0 40 20]; the area covers its left half.
        let placement = Matrix::new(40.0, 0.0, 0.0, 20.0, 0.0, 0.0);
        let covered = covered_pixels(&placement, &[Area::rect(0.0, 0.0, 20.0, 20.0)], 4, 2);
        assert_eq!(covered, [(0, 0), (1, 0), (0, 1), (1, 1)]);
        let top = covered_pixels(&placement, &[Area::rect(0.0, 10.0, 40.0, 20.0)], 4, 2);
        assert_eq!(top, [(0, 0), (1, 0), (2, 0), (3, 0)], "row 0 is the top");
    }
}
