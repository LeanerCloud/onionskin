//! Embedded page thumbnails: each page's `/Thumb`, a small image other
//! readers show in their page panes instead of rendering the page.
//!
//! Acrobat's Embed All Page Thumbnails and Remove All Page Thumbnails. Both
//! are one edit, so one Undo takes either back whole. Onionskin's own pane
//! never needs them: it renders what it shows.

use onionskin_cos::{Dict, Name, Object, Stream};

use super::ops::leaves;
use super::rewrite::dict_at;
use crate::edit::Transaction;
use crate::{Document, Result};

/// The longer side of an embedded thumbnail, in pixels. Acrobat's are about
/// this size, and a larger one only makes the file bigger.
pub const THUMBNAIL_SIDE: f32 = 96.0;

/// Remove every page's `/Thumb`, as one edit. How many pages had one.
pub fn remove_thumbnails(doc: &mut Document) -> Result<usize> {
    doc.edit_document("Remove All Page Thumbnails", strip)
}

fn strip(tx: &mut Transaction<'_>) -> Result<usize> {
    let mut removed = 0;
    for leaf in leaves(tx)? {
        let mut dict = dict_at(tx, leaf.objref)?;
        if dict.remove(b"Thumb").is_some() {
            removed += 1;
            tx.put_object(
                leaf.objref.number,
                leaf.objref.generation,
                Object::Dict(dict),
            )?;
        }
    }
    Ok(removed)
}

/// Render every page small and store the picture as its `/Thumb`, as one
/// edit, replacing any thumbnail it had. How many pages got one.
pub fn embed_thumbnails(doc: &mut Document) -> Result<usize> {
    let mut images = Vec::with_capacity(doc.page_count());
    for page in 0..doc.page_count() {
        let (width, height) = doc.page_geometry(page)?.render_size;
        let zoom = THUMBNAIL_SIDE / width.max(height).max(1.0) as f32;
        let render = doc.render_page_now(page, zoom)?;
        images.push(thumbnail_image(
            render.raster.width(),
            render.raster.height(),
            render.raster.rgba(),
        ));
    }
    doc.edit_document("Embed All Page Thumbnails", move |tx| {
        let leaves = leaves(tx)?;
        for (leaf, image) in leaves.iter().zip(images) {
            let number = tx.reserve();
            tx.put_object(number, 0, image)?;
            let mut dict = dict_at(tx, leaf.objref)?;
            dict.set(
                Name::new("Thumb"),
                Object::Ref(onionskin_cos::ObjRef::new(number, 0)),
            );
            tx.put_object(
                leaf.objref.number,
                leaf.objref.generation,
                Object::Dict(dict),
            )?;
        }
        Ok(leaves.len())
    })
}

/// A thumbnail image stream: RGB, on white where the page was transparent,
/// deflated. ISO 32000-1 12.3.4.
fn thumbnail_image(width: u32, height: u32, rgba: &[u8]) -> Object {
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            let alpha = u16::from(px[3]);
            [0, 1, 2].map(|i| (u16::from(px[i]) * alpha / 255 + (255 - alpha)) as u8)
        })
        .collect();
    let mut dict = Dict::new();
    dict.set(Name::new("Width"), Object::Integer(i64::from(width)));
    dict.set(Name::new("Height"), Object::Integer(i64::from(height)));
    dict.set(Name::new("ColorSpace"), Object::name("DeviceRGB"));
    dict.set(Name::new("BitsPerComponent"), Object::Integer(8));
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    Object::Stream(Stream {
        dict,
        raw: onionskin_cos::flate_encode(&rgb),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_is_an_rgb_image_of_the_rendered_size() {
        let Object::Stream(stream) = thumbnail_image(2, 1, &[0, 0, 0, 0, 10, 20, 30, 255]) else {
            panic!("a stream");
        };
        assert_eq!(stream.dict.get(b"Width"), Some(&Object::Integer(2)));
        assert_eq!(stream.dict.get(b"Height"), Some(&Object::Integer(1)));
        assert_eq!(
            stream.dict.get(b"ColorSpace"),
            Some(&Object::name("DeviceRGB"))
        );
        assert_eq!(
            stream.raw,
            onionskin_cos::flate_encode(&[255, 255, 255, 10, 20, 30])
        );
    }

    #[test]
    fn a_thumbnail_composites_alpha_and_drops_trailing_rgba_bytes() {
        let rgba = [10, 20, 30, 128, 1, 2, 3];
        for trailing in 0..=3 {
            let Object::Stream(stream) = thumbnail_image(1, 1, &rgba[..4 + trailing]) else {
                panic!("a stream");
            };
            assert_eq!(stream.raw, onionskin_cos::flate_encode(&[132, 137, 142]));
        }
    }
}
