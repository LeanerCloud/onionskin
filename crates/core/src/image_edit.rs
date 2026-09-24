//! Editing the images a page draws, one placement at a time: moving,
//! resizing, turning, flipping, replacing, taking away, and adding a new
//! one.
//!
//! Every edit rewrites the `Do` that drew the image, in the stream it is
//! in, a page's content or a form's, found through the placement's
//! provenance:
//!
//! - **Move, resize, turn, flip:** `q M cm /Im Do Q`, where `M` is the
//!   page-space change carried into the image's own space, so nothing else
//!   the stream draws moves.
//! - **Replace:** the new image, as a form XObject, drawn fitted and
//!   centred in the old one's frame, keeping its proportions.
//! - **Take away:** nothing where the `Do` was.
//!
//! The image XObject itself is never changed, so another placement of the
//! same image, on this page or another, stays as it was. An added image
//! goes after the page's content, as its own content stream, guarded so the
//! page's leftover graphics state does not reach it.

use onionskin_content::placements::ImagePlacement;
use onionskin_content::Matrix;
use onionskin_cos::{flate_encode, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use crate::edit::Transaction;
use crate::pages::{dict_at, import_page_as_form, page_ref, resolve};
use crate::{Error, PageIndex, Result};

/// What becomes of a placement.
#[derive(Debug, Clone, PartialEq)]
pub enum PlacementEdit {
    /// Carry the image through this page-space transform.
    Transform(Matrix),
    /// Draw this form XObject, with its box, fitted in the image's frame.
    Replace { form: ObjRef, bbox: [f64; 4] },
    /// Draw nothing.
    Remove,
}

/// Why an image cannot be edited.
fn stale(what: &str) -> Error {
    Error::ImageEdit(format!(
        "the image {what}; the page changed since it was found"
    ))
}

/// The topmost image drawn at `(x, y)`: the last one drawn there.
pub fn image_at(placements: &[ImagePlacement], x: f64, y: f64) -> Option<&ImagePlacement> {
    placements.iter().rev().find(|placed| placed.contains(x, y))
}

/// `translate`, `scale`, `rotate` and `flip` about a point, as page-space
/// transforms for [`PlacementEdit::Transform`].
pub mod transforms {
    use super::Matrix;

    pub fn translate(dx: f64, dy: f64) -> Matrix {
        Matrix::translate(dx, dy)
    }

    /// Scale by `(sx, sy)` keeping `(cx, cy)` where it is.
    pub fn scale_about(sx: f64, sy: f64, (cx, cy): (f64, f64)) -> Matrix {
        Matrix::translate(-cx, -cy)
            .then(&Matrix::scale(sx, sy))
            .then(&Matrix::translate(cx, cy))
    }

    /// Turn a quarter clockwise, `quarters` times, about `(cx, cy)`.
    pub fn rotate_about(quarters: i32, (cx, cy): (f64, f64)) -> Matrix {
        let turn = match quarters.rem_euclid(4) {
            0 => Matrix::IDENTITY,
            1 => Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0),
            2 => Matrix::new(-1.0, 0.0, 0.0, -1.0, 0.0, 0.0),
            _ => Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0),
        };
        Matrix::translate(-cx, -cy)
            .then(&turn)
            .then(&Matrix::translate(cx, cy))
    }

    /// Mirror left to right (`horizontal`) or top to bottom about
    /// `(cx, cy)`.
    pub fn flip_about(horizontal: bool, (cx, cy): (f64, f64)) -> Matrix {
        let (sx, sy) = if horizontal { (-1.0, 1.0) } else { (1.0, -1.0) };
        scale_about(sx, sy, (cx, cy))
    }
}

/// Make `edit` to `placement` on `page`.
pub fn edit_placement(
    tx: &mut Transaction<'_>,
    page: PageIndex,
    placement: &ImagePlacement,
    edit: &PlacementEdit,
) -> Result<()> {
    let found = placement
        .provenance
        .ok_or_else(|| stale("could not be located in its stream"))?;
    let state = tx
        .object(found.stream.number)?
        .ok_or_else(|| stale("stream is gone"))?;
    let Object::Stream(stream) = state.object else {
        return Err(stale("stream is gone"));
    };
    let decoded = tx
        .base()
        .decode_stream(&stream)
        .map_err(|error| Error::ImageEdit(format!("the stream does not decode: {error}")))?;
    let (start, end) = (found.decoded.start as usize, found.decoded.end as usize);
    let written = decoded.get(start..end).unwrap_or_default();
    let expected = format!("/{} Do", placement.name);
    if !written.starts_with(b"/") || !written.ends_with(b"Do") || written.len() < expected.len() {
        return Err(stale("is not where it was"));
    }
    let replacement = match edit {
        PlacementEdit::Remove => Vec::new(),
        PlacementEdit::Transform(change) => {
            let inverse = placement
                .ctm
                .inverse()
                .ok_or_else(|| stale("is drawn flat"))?;
            let inside = placement.ctm.then(change).then(&inverse);
            format!(
                "q {} cm {} Q",
                operands(&inside),
                String::from_utf8_lossy(written)
            )
            .into_bytes()
        }
        PlacementEdit::Replace { form, bbox } => {
            let name = name_form(tx, page, found.stream, &stream.dict, *form)?;
            let fit = fitted(&placement.ctm, *bbox);
            format!("q {} cm /{name} Do Q", operands(&fit)).into_bytes()
        }
    };
    let mut rewritten = decoded[..start].to_vec();
    rewritten.extend_from_slice(&replacement);
    rewritten.extend_from_slice(&decoded[end..]);
    // The stream may have been renamed into by `name_form`: read it again.
    let current = match tx.object(found.stream.number)?.map(|state| state.object) {
        Some(Object::Stream(current)) => current,
        _ => stream,
    };
    tx.put_object(
        found.stream.number,
        state.generation,
        Object::Stream(recompressed(current.dict, &rewritten)),
    )
}

/// `stream`'s dictionary with `content` as its data, deflated.
fn recompressed(mut dict: Dict, content: &[u8]) -> Stream {
    let raw = flate_encode(content);
    dict.remove(b"DecodeParms");
    dict.remove(b"DL");
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    dict.set(Name::new("Length"), Object::Integer(raw.len() as i64));
    Stream { dict, raw }
}

fn operands(m: &Matrix) -> String {
    [m.a, m.b, m.c, m.d, m.e, m.f]
        .iter()
        .map(|value| {
            let text = format!("{value:.6}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text == "-0" {
                "0".to_owned()
            } else {
                text.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The matrix drawing a form whose box is `bbox` inside the image's unit
/// square, fitted and centred as the image is shown on the page, keeping
/// its proportions.
fn fitted(ctm: &Matrix, [x0, y0, x1, y1]: [f64; 4]) -> Matrix {
    let (w, h) = ((x1 - x0).max(f64::EPSILON), (y1 - y0).max(f64::EPSILON));
    let (page_w, page_h) = (
        (ctm.a * ctm.a + ctm.b * ctm.b).sqrt().max(f64::EPSILON),
        (ctm.c * ctm.c + ctm.d * ctm.d).sqrt().max(f64::EPSILON),
    );
    let scale = (page_w / w).min(page_h / h);
    let (sx, sy) = (scale * w / page_w, scale * h / page_h);
    Matrix::new(
        sx / w,
        0.0,
        0.0,
        sy / h,
        (1.0 - sx) / 2.0 - x0 * sx / w,
        (1.0 - sy) / 2.0 - y0 * sy / h,
    )
}

/// Name `form` in the resources of the stream the `Do` is in: a form's own,
/// or the page's. The name.
fn name_form(
    tx: &mut Transaction<'_>,
    page: PageIndex,
    stream: ObjRef,
    stream_dict: &Dict,
    form: ObjRef,
) -> Result<String> {
    let is_form = stream_dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|kind| kind.as_bytes() == b"Form");
    if is_form {
        let mut resources = resources_of(tx, stream_dict.get(b"Resources"))?;
        let name = add_xobject(tx, &mut resources, form)?;
        let mut dict = stream_dict.clone();
        dict.set(Name::new("Resources"), Object::Dict(resources));
        let raw = match tx.object(stream.number)?.map(|state| state.object) {
            Some(Object::Stream(current)) => current.raw,
            _ => Vec::new(),
        };
        let generation = tx
            .object(stream.number)?
            .map_or(0, |state| state.generation);
        tx.put_object(
            stream.number,
            generation,
            Object::Stream(Stream { dict, raw }),
        )?;
        return Ok(name);
    }
    let page_object = page_ref(tx, page)?;
    let mut page_dict = dict_at(tx, page_object)?;
    let inherited = crate::pages::inherited_resources(tx, page_object)?;
    let mut resources = resources_of(tx, inherited.as_ref())?;
    let name = add_xobject(tx, &mut resources, form)?;
    page_dict.set(Name::new("Resources"), Object::Dict(resources));
    tx.put_object(
        page_object.number,
        page_object.generation,
        Object::Dict(page_dict),
    )?;
    Ok(name)
}

fn resources_of(tx: &Transaction<'_>, value: Option<&Object>) -> Result<Dict> {
    Ok(match resolve(tx, value)? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    })
}

/// `form` named in `resources`' `/XObject`, under the first `OSIm` name
/// free. The name.
fn add_xobject(tx: &Transaction<'_>, resources: &mut Dict, form: ObjRef) -> Result<String> {
    let mut xobjects = match resolve(tx, resources.get(b"XObject"))? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    };
    let name = (0..)
        .map(|index| format!("OSIm{index}"))
        .find(|name| xobjects.get(name.as_bytes()).is_none())
        .expect("a free name");
    xobjects.set(Name::new(&name), Object::Ref(form));
    resources.set(Name::new("XObject"), Object::Dict(xobjects));
    Ok(name)
}

/// Page 1 of `source` as a form XObject in the document, for
/// [`PlacementEdit::Replace`] or [`add_image`].
pub fn import_image(tx: &mut Transaction<'_>, source: &CosDocument) -> Result<(ObjRef, [f64; 4])> {
    import_page_as_form(tx, source, 0)
}

/// Draw page 1 of `source` over `rect` on `page`, fitted and centred,
/// after everything the page draws.
pub fn add_image(
    tx: &mut Transaction<'_>,
    page: PageIndex,
    source: &CosDocument,
    rect: [f64; 4],
) -> Result<()> {
    let (form, bbox) = import_page_as_form(tx, source, 0)?;
    let [x0, y0, x1, y1] = rect;
    let frame = Matrix::new(x1 - x0, 0.0, 0.0, y1 - y0, x0, y0);
    let placed = fitted(&frame, bbox).then(&frame);
    let page_object = page_ref(tx, page)?;
    let mut page_dict = dict_at(tx, page_object)?;
    let inherited = crate::pages::inherited_resources(tx, page_object)?;
    let mut resources = resources_of(tx, inherited.as_ref())?;
    let name = add_xobject(tx, &mut resources, form)?;
    let content = format!("q {} cm /{name} Do Q\n", operands(&placed));
    crate::pages::append_content(tx, &mut page_dict, content.into_bytes())?;
    page_dict.set(Name::new("Resources"), Object::Dict(resources));
    tx.put_object(
        page_object.number,
        page_object.generation,
        Object::Dict(page_dict),
    )
}
