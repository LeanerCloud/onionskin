//! Writing redaction marks: a new one, a new look for one, and removing.

use onionskin_cos::{Dict, Name, ObjRef, Object, Stream};

use super::read::is_redaction;
use super::{Overlay, RedactionLook};
use crate::annots::author::{append_to_page_annots, text_string};
use crate::edit::Transaction;
use crate::pages::page_ref;
use crate::{Error, PageIndex, PageQuad, Result};

/// Mark `quads` of text on `page` for redaction, or the rectangle `rect`
/// when there are none. The new mark's reference.
pub fn add_redaction(
    tx: &mut Transaction<'_>,
    page: PageIndex,
    rect: [f64; 4],
    quads: &[PageQuad],
    look: &RedactionLook,
) -> Result<ObjRef> {
    let page_object = page_ref(tx, page)?;
    let rect = if quads.is_empty() {
        let [x0, y0, x1, y1] = rect;
        [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
    } else {
        bounds(quads)
    };
    let mark = ObjRef::new(tx.reserve(), 0);
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("Annot"));
    dict.set(Name::new("Subtype"), Object::name("Redact"));
    dict.set(Name::new("Rect"), numbers(&rect));
    dict.set(Name::new("P"), Object::Ref(page_object));
    dict.set(Name::new("F"), Object::Integer(4));
    if !quads.is_empty() {
        let points: Vec<f64> = quads
            .iter()
            .flat_map(|quad| quad.corners.iter().flat_map(|(x, y)| [*x, *y]))
            .collect();
        dict.set(Name::new("QuadPoints"), numbers(&points));
    }
    set_look(tx, &mut dict, look)?;
    tx.put_object(mark.number, 0, Object::Dict(dict))?;
    append_to_page_annots(tx, page_object, mark)?;
    Ok(mark)
}

/// Give mark `mark` a new look.
pub fn set_redaction(tx: &mut Transaction<'_>, mark: ObjRef, look: &RedactionLook) -> Result<()> {
    let not_a_mark = || Error::NotADictionary {
        number: mark.number,
    };
    let state = tx.object(mark.number)?.ok_or_else(not_a_mark)?;
    let mut dict = state
        .object
        .as_dict()
        .filter(|dict| is_redaction(dict))
        .ok_or_else(not_a_mark)?
        .clone();
    set_look(tx, &mut dict, look)?;
    tx.put_object(mark.number, state.generation, Object::Dict(dict))
}

/// Take mark `mark` off `page`. Whether it was there.
pub fn remove_redaction(tx: &mut Transaction<'_>, page: PageIndex, mark: ObjRef) -> Result<bool> {
    let page_object = page_ref(tx, page)?;
    crate::annots::author::remove(tx, page_object, mark)
}

fn set_look(tx: &mut Transaction<'_>, dict: &mut Dict, look: &RedactionLook) -> Result<()> {
    for key in ["IC", "OC", "OverlayText", "DA", "Q", "Repeat"] {
        dict.remove(key.as_bytes());
    }
    // No /IC would read as Acrobat's black, so "no fill" is an empty array.
    dict.set(
        Name::new("IC"),
        look.fill
            .map_or(Object::Array(Vec::new()), |fill| numbers(&fill)),
    );
    dict.set(Name::new("OC"), numbers(&look.outline));
    if let Some(overlay) = &look.overlay {
        set_overlay(dict, overlay);
    }
    let appearance = ObjRef::new(tx.reserve(), 0);
    tx.put_object(
        appearance.number,
        0,
        Object::Stream(outline_appearance(dict, look.outline)),
    )?;
    let mut ap = Dict::new();
    ap.set(Name::new("N"), Object::Ref(appearance));
    dict.set(Name::new("AP"), Object::Dict(ap));
    Ok(())
}

fn set_overlay(dict: &mut Dict, overlay: &Overlay) {
    let [r, g, b] = overlay.color;
    dict.set(Name::new("OverlayText"), text_string(&overlay.text));
    dict.set(
        Name::new("DA"),
        Object::String(format!("/Helv {} Tf {r} {g} {b} rg", overlay.size).into_bytes()),
    );
    dict.set(Name::new("Q"), Object::Integer(overlay.align.quadding()));
    dict.set(Name::new("Repeat"), Object::Bool(overlay.repeat));
}

/// What a mark looks like before it is applied: its quads, or its
/// rectangle, outlined.
fn outline_appearance(dict: &Dict, [r, g, b]: [f64; 3]) -> Stream {
    let values = |key: &[u8]| -> Vec<f64> {
        dict.get(key)
            .and_then(Object::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|value| match value {
                Object::Integer(value) => Some(*value as f64),
                Object::Real(value) => Some(*value),
                _ => None,
            })
            .collect()
    };
    let rect = values(b"Rect");
    let mut content = format!("{r} {g} {b} RG 1 w\n");
    let quads = values(b"QuadPoints");
    if quads.len() >= 8 {
        for q in quads.as_chunks::<8>().0 {
            // Upper-left, upper-right, lower-left, lower-right: round the
            // edge is 0, 1, 3, 2.
            content.push_str(&format!(
                "{} {} m {} {} l {} {} l {} {} l h S\n",
                q[0], q[1], q[2], q[3], q[6], q[7], q[4], q[5]
            ));
        }
    } else if let [x0, y0, x1, y1] = rect.as_slice() {
        content.push_str(&format!("{x0} {y0} {} {} re S\n", x1 - x0, y1 - y0));
    }
    let mut stream_dict = Dict::new();
    stream_dict.set(Name::new("Type"), Object::name("XObject"));
    stream_dict.set(Name::new("Subtype"), Object::name("Form"));
    stream_dict.set(Name::new("BBox"), numbers(&rect));
    Stream {
        dict: stream_dict,
        raw: content.into_bytes(),
    }
}

fn bounds(quads: &[PageQuad]) -> [f64; 4] {
    let points = quads.iter().flat_map(|quad| quad.corners);
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in points {
        out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
    }
    out
}

fn numbers(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|value| Object::Real(*value)).collect())
}
