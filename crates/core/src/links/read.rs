//! Reading the links a document carries, from the session's current view of
//! it.

use onionskin_cos::{Dict, Document as CosDocument, Object};

use super::{Highlight, LineStyle, Link, LinkLook, LinkTarget};
use crate::outline::Reader;
use crate::{PageIndex, Result};

/// Every link on every page, in page order and then `/Annots` order.
pub fn read_links(doc: &CosDocument, page_count: usize) -> Result<Vec<Link>> {
    let mut destinations = Reader::new(doc, page_count);
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
            if !is_link(&dict) {
                continue;
            }
            out.push(Link {
                objref,
                page: index,
                rect: rect(&dict),
                target: target(doc, &mut destinations, &dict)?,
                look: look(&dict),
            });
        }
    }
    Ok(out)
}

/// The topmost link on `page` under `point`: the last one drawn.
pub fn link_at(links: &[Link], page: PageIndex, point: (f64, f64)) -> Option<&Link> {
    links
        .iter()
        .rev()
        .find(|link| link.page == page && link.contains(point))
}

pub(super) fn is_link(dict: &Dict) -> bool {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|name| name.as_bytes() == b"Link")
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn rect(dict: &Dict) -> [f64; 4] {
    let values: Vec<f64> = dict
        .get(b"Rect")
        .and_then(Object::as_array)
        .map(|items| items.iter().filter_map(number).collect())
        .unwrap_or_default();
    match values[..] {
        [x0, y0, x1, y1] => [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)],
        _ => [0.0; 4],
    }
}

fn text(object: Option<&Object>) -> Option<String> {
    match object? {
        Object::String(bytes) => Some(onionskin_content::pdf_text_string(bytes)),
        Object::Name(name) => Some(String::from_utf8_lossy(name.as_bytes()).into_owned()),
        Object::Dict(spec) => text(spec.get(b"UF").or_else(|| spec.get(b"F"))),
        _ => None,
    }
}

fn target(doc: &CosDocument, destinations: &mut Reader<'_>, dict: &Dict) -> Result<LinkTarget> {
    let action = match dict.get(b"A") {
        Some(action) => doc.resolve(action)?.as_dict().cloned(),
        None => None,
    };
    let Some(action) = action else {
        // A `/Dest` of its own, or nothing at all.
        return Ok(match destinations.destination(dict)? {
            Some(page) => LinkTarget::Page(page),
            None => LinkTarget::Other("Dest".to_owned()),
        });
    };
    let kind = action
        .get(b"S")
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
        .unwrap_or_default();
    Ok(match kind.as_str() {
        "GoTo" => match destinations.destination(dict)? {
            Some(page) => LinkTarget::Page(page),
            None => LinkTarget::Other(kind),
        },
        "URI" => match action.get(b"URI") {
            Some(Object::String(bytes)) => {
                LinkTarget::Web(String::from_utf8_lossy(bytes).into_owned())
            }
            _ => LinkTarget::Other(kind),
        },
        "Launch" | "GoToR" => match text(action.get(b"F")) {
            Some(file) => LinkTarget::File(file),
            None => LinkTarget::Other(kind),
        },
        _ => LinkTarget::Other(kind),
    })
}

fn look(dict: &Dict) -> LinkLook {
    let defaults = LinkLook::default();
    let border = dict.get(b"BS").and_then(Object::as_dict);
    let width = border
        .and_then(|border| border.get(b"W"))
        .and_then(number)
        .or_else(|| {
            dict.get(b"Border")
                .and_then(Object::as_array)
                .and_then(|items| items.get(2))
                .and_then(number)
        })
        // A link with neither is drawn with a one-point border.
        .unwrap_or(1.0);
    let style = border
        .and_then(|border| border.get(b"S"))
        .and_then(Object::as_name)
        .and_then(|name| {
            LineStyle::ALL
                .into_iter()
                .find(|style| style.key().as_bytes() == name.as_bytes())
        })
        .unwrap_or(LineStyle::Solid);
    let highlight = dict
        .get(b"H")
        .and_then(Object::as_name)
        .and_then(|name| {
            Highlight::ALL
                .into_iter()
                .find(|highlight| highlight.key().as_bytes() == name.as_bytes())
        })
        .unwrap_or(Highlight::Invert);
    let color = match dict.get(b"C").and_then(Object::as_array) {
        Some(items) => {
            let values: Vec<f64> = items.iter().filter_map(number).collect();
            match values[..] {
                [r, g, b] => [r, g, b],
                [gray] => [gray; 3],
                _ => defaults.color,
            }
        }
        None => defaults.color,
    };
    LinkLook {
        visible: width > 0.0,
        width: if width > 0.0 { width } else { defaults.width },
        color,
        style,
        highlight,
    }
}
