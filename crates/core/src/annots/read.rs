//! Reading the annotations a document already carries.
//!
//! A comment on a file Acrobat produced has to appear in the Comments pane, so
//! this is not only about what Onionskin authored.
//!
//! **The seam with the edit session.** The plan's P6 text says this reader goes
//! through P3's `structure()` rather than through `&self.cos`, so that it can
//! see a comment the session just authored. P3 has not landed, so [`read`]
//! takes the base document **and** the session's pending edits, and prefers the
//! pending object whenever one exists for a number. That is the same guarantee
//! by the means available now: nothing reaches a reader that the session has
//! superseded. When P3 lands, this function's two arguments collapse into its
//! `structure()` and the behaviour does not change.

use std::collections::BTreeMap;

use onionskin_cos::{Dict, Document as CosDocument, ObjRef, Object, PendingEdit};

use super::model::{Color, Flags, Quad, Rect, Subtype};
use crate::Result;

/// One annotation as the Comments pane shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadAnnotation {
    pub objref: ObjRef,
    pub page: usize,
    pub subtype: Option<Subtype>,
    /// `/Subtype` as written, so an annotation of a kind M3 does not author
    /// still appears in the pane rather than vanishing from it.
    pub raw_subtype: String,
    pub rect: Rect,
    pub quads: Vec<Quad>,
    pub contents: Option<String>,
    pub author: Option<String>,
    pub modified: Option<String>,
    pub color: Option<Color>,
    pub flags: Flags,
    pub in_reply_to: Option<ObjRef>,
    pub has_appearance: bool,
    /// `/InkList`, one entry per stroke. Empty for anything but ink.
    pub ink: Vec<Vec<(f64, f64)>>,
    /// `/BS /W`, or 1 when the dictionary has none.
    pub border_width: f64,
    /// `/Subj`, the comment's kind as its author named it.
    pub subject: Option<String>,
    /// `/State` and `/StateModel` on a status annotation: what it sets and in
    /// which model. `None` for every other annotation.
    pub state: Option<(String, String)>,
    /// `/CA`, the opacity, when the dictionary gives one.
    pub opacity: Option<f64>,
}

/// Every annotation on every page, in page order and then in `/Annots` order.
pub(crate) fn read(
    doc: &CosDocument,
    page_count: usize,
    pending: &BTreeMap<u32, PendingEdit>,
) -> Result<Vec<ReadAnnotation>> {
    let mut out = Vec::new();
    for index in 0..page_count {
        let page = doc.page(index)?;
        let page_dict = current_dict(doc, pending, page.objref.number)?.unwrap_or(page.dict);
        let Some(annots) = page_dict.get(b"Annots") else {
            continue;
        };
        for item in array(doc, pending, annots)? {
            let Object::Ref(objref) = item else { continue };
            let Some(dict) = current_dict(doc, pending, objref.number)? else {
                continue;
            };
            out.push(one(objref, index, &dict));
        }
    }
    Ok(out)
}

/// The object as the session sees it: the pending value if the session has
/// superseded this number, else the base document's.
fn current_dict(
    doc: &CosDocument,
    pending: &BTreeMap<u32, PendingEdit>,
    number: u32,
) -> Result<Option<Dict>> {
    if let Some(PendingEdit::Set { object, .. }) = pending.get(&number) {
        return Ok(object.as_dict().cloned());
    }
    if let Some(PendingEdit::Delete { .. }) = pending.get(&number) {
        return Ok(None);
    }
    Ok(doc
        .get(number)
        .ok()
        .and_then(|p| p.object.as_dict().cloned()))
}

fn array(
    doc: &CosDocument,
    pending: &BTreeMap<u32, PendingEdit>,
    object: &Object,
) -> Result<Vec<Object>> {
    match object {
        Object::Array(items) => Ok(items.clone()),
        Object::Ref(objref) => {
            if let Some(PendingEdit::Set { object, .. }) = pending.get(&objref.number) {
                return Ok(match object {
                    Object::Array(items) => items.clone(),
                    _ => Vec::new(),
                });
            }
            Ok(match doc.resolve(object)? {
                Object::Array(items) => items,
                _ => Vec::new(),
            })
        }
        _ => Ok(Vec::new()),
    }
}

fn one(objref: ObjRef, page: usize, dict: &Dict) -> ReadAnnotation {
    let raw_subtype = dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
        .unwrap_or_default();
    ReadAnnotation {
        objref,
        page,
        subtype: dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .and_then(Subtype::from_name),
        raw_subtype,
        rect: rect(dict.get(b"Rect")).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
        quads: quads(dict.get(b"QuadPoints")),
        contents: text(dict.get(b"Contents")),
        author: text(dict.get(b"T")),
        modified: text(dict.get(b"M")),
        color: color(dict.get(b"C")),
        flags: Flags(dict.get(b"F").and_then(Object::as_integer).unwrap_or(0)),
        in_reply_to: match dict.get(b"IRT") {
            Some(Object::Ref(parent)) => Some(*parent),
            _ => None,
        },
        has_appearance: dict
            .get(b"AP")
            .and_then(Object::as_dict)
            .is_some_and(|ap| ap.get(b"N").is_some()),
        ink: ink_list(dict.get(b"InkList")),
        border_width: dict
            .get(b"BS")
            .and_then(Object::as_dict)
            .and_then(|border| border.get(b"W"))
            .and_then(as_number)
            .unwrap_or(1.0),
        subject: text(dict.get(b"Subj")),
        state: text(dict.get(b"State")).map(|state| {
            (
                state,
                text(dict.get(b"StateModel")).unwrap_or_else(|| "Marked".to_owned()),
            )
        }),
        opacity: dict.get(b"CA").and_then(as_number),
    }
}

/// `/InkList` as strokes of points. An odd trailing coordinate is dropped.
pub(crate) fn ink_list(object: Option<&Object>) -> Vec<Vec<(f64, f64)>> {
    let Some(Object::Array(strokes)) = object else {
        return Vec::new();
    };
    strokes
        .iter()
        .map(|stroke| {
            numbers(Some(stroke))
                .chunks_exact(2)
                .map(|pair| (pair[0], pair[1]))
                .collect()
        })
        .collect()
}

pub(crate) fn numbers(object: Option<&Object>) -> Vec<f64> {
    let Some(Object::Array(items)) = object else {
        return Vec::new();
    };
    items.iter().filter_map(as_number).collect()
}

pub(crate) fn as_number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

pub(crate) fn rect(object: Option<&Object>) -> Option<Rect> {
    let values = numbers(object);
    let [x0, y0, x1, y1] = values[..] else {
        return None;
    };
    Some(Rect::new(x0, y0, x1, y1))
}

/// Read back in the order [`Quad`] documents: upper-left, upper-right,
/// lower-left, lower-right.
pub(crate) fn quads(object: Option<&Object>) -> Vec<Quad> {
    numbers(object)
        .chunks(8)
        .filter(|chunk| chunk.len() == 8)
        .map(|c| Quad {
            upper_left: (c[0], c[1]),
            upper_right: (c[2], c[3]),
            lower_left: (c[4], c[5]),
            lower_right: (c[6], c[7]),
        })
        .collect()
}

pub(crate) fn color(object: Option<&Object>) -> Option<Color> {
    let values = numbers(object);
    match values[..] {
        [gray] => Some(Color::new(gray, gray, gray)),
        [red, green, blue] => Some(Color::new(red, green, blue)),
        _ => None,
    }
}

/// A PDF text string, decoded from UTF-16BE when it carries the byte-order
/// mark and from Latin-1 otherwise. PDFDocEncoding differs from Latin-1 in 24
/// positions; M3 reads the common case correctly and does not pretend the rest
/// is exact.
pub(crate) fn text(object: Option<&Object>) -> Option<String> {
    let Some(Object::String(bytes)) = object else {
        return None;
    };
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    Some(bytes.iter().map(|byte| *byte as char).collect())
}
