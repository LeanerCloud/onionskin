//! Reading redaction marks, as this crate writes them and as Acrobat does.

use onionskin_content::redact::Area;
use onionskin_cos::{Dict, Document as CosDocument, Object};

use super::{Align, Overlay, RedactionLook, RedactionMark, BLACK, RED};
use crate::{PageQuad, Result};

/// Every redaction mark on the document's pages, in page order.
pub fn read_redactions(doc: &CosDocument, page_count: usize) -> Result<Vec<RedactionMark>> {
    let mut out = Vec::new();
    for index in 0..page_count {
        let page = doc.page(index)?;
        let Some(annots) = page.dict.get(b"Annots") else {
            continue;
        };
        let Some(annots) = doc.resolve(annots)?.as_array().map(<[Object]>::to_vec) else {
            continue;
        };
        for item in annots {
            let Object::Ref(objref) = item else { continue };
            let Some(dict) = doc.get(objref.number)?.object.as_dict().cloned() else {
                continue;
            };
            if !is_redaction(&dict) {
                continue;
            }
            out.push(RedactionMark {
                objref,
                page: index,
                rect: rect(doc, &dict),
                quads: quads(doc, &dict, index),
                look: look(doc, &dict),
            });
        }
    }
    Ok(out)
}

/// What a mark removes when it is applied: its quads, or its rectangle.
pub fn redaction_areas(mark: &RedactionMark) -> Vec<Area> {
    if mark.quads.is_empty() {
        let [x0, y0, x1, y1] = mark.rect;
        return vec![Area::rect(x0, y0, x1, y1)];
    }
    mark.quads.iter().map(Area::quad).collect()
}

pub(super) fn is_redaction(dict: &Dict) -> bool {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|name| name.as_bytes() == b"Redact")
}

fn numbers(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Vec<f64> {
    dict.get(key)
        .and_then(|value| doc.resolve(value).ok())
        .and_then(|value| value.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default()
        .iter()
        .filter_map(|value| match value {
            Object::Integer(value) => Some(*value as f64),
            Object::Real(value) => Some(*value),
            _ => None,
        })
        .collect()
}

fn rect(doc: &CosDocument, dict: &Dict) -> [f64; 4] {
    match numbers(doc, dict, b"Rect").as_slice() {
        [x0, y0, x1, y1] => [x0.min(*x1), y0.min(*y1), x0.max(*x1), y0.max(*y1)],
        _ => [0.0; 4],
    }
}

fn quads(doc: &CosDocument, dict: &Dict, page: usize) -> Vec<PageQuad> {
    numbers(doc, dict, b"QuadPoints")
        .as_chunks::<8>()
        .0
        .iter()
        .map(|q| PageQuad {
            page,
            corners: [(q[0], q[1]), (q[2], q[3]), (q[4], q[5]), (q[6], q[7])],
        })
        .collect()
}

fn color(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<[f64; 3]> {
    match numbers(doc, dict, key).as_slice() {
        [r, g, b] => Some([*r, *g, *b]),
        [gray] => Some([*gray; 3]),
        _ => None,
    }
}

fn look(doc: &CosDocument, dict: &Dict) -> RedactionLook {
    let fill = if dict.contains(b"IC") {
        color(doc, dict, b"IC")
    } else {
        // No /IC is Acrobat's black.
        Some(BLACK)
    };
    RedactionLook {
        fill,
        outline: color(doc, dict, b"OC")
            .or_else(|| color(doc, dict, b"C"))
            .unwrap_or(RED),
        overlay: overlay(doc, dict),
    }
}

fn overlay(doc: &CosDocument, dict: &Dict) -> Option<Overlay> {
    let Object::String(bytes) = doc.resolve(dict.get(b"OverlayText")?).ok()? else {
        return None;
    };
    let text = onionskin_content::pdf_text_string(&bytes);
    if text.is_empty() {
        return None;
    }
    let (size, color) = appearance_string(doc, dict);
    Some(Overlay {
        text,
        size,
        color,
        align: Align::from_quadding(dict.get(b"Q").and_then(Object::as_integer).unwrap_or(0)),
        repeat: matches!(dict.get(b"Repeat"), Some(Object::Bool(true))),
    })
}

/// The size and colour `/DA` gives, as `/Helv 12 Tf 1 0 0 rg` writes them.
fn appearance_string(doc: &CosDocument, dict: &Dict) -> (f64, [f64; 3]) {
    let text = match dict.get(b"DA").and_then(|value| doc.resolve(value).ok()) {
        Some(Object::String(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        _ => String::new(),
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    let number = |at: usize| words.get(at).and_then(|word| word.parse::<f64>().ok());
    let mut size = 0.0;
    let mut color = [0.0; 3];
    for (at, word) in words.iter().enumerate() {
        match *word {
            "Tf" if at >= 1 => size = number(at - 1).unwrap_or(0.0),
            "g" if at >= 1 => color = [number(at - 1).unwrap_or(0.0); 3],
            "rg" if at >= 3 => {
                color = [
                    number(at - 3).unwrap_or(0.0),
                    number(at - 2).unwrap_or(0.0),
                    number(at - 1).unwrap_or(0.0),
                ]
            }
            _ => {}
        }
    }
    (size, color)
}
