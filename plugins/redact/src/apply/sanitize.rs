//! Remove Hidden Information, Acrobat's sanitize: what a document carries
//! besides what its pages show.
//!
//! - **Metadata**: the document information dictionary and every XMP
//!   `/Metadata` stream.
//! - **Scripts and actions**: document JavaScript, every additional action
//!   (`/AA`), an XFA form's scripts, and every action that runs something
//!   or fetches something rather than going to a page or a web address.
//! - **Attachments**: embedded files, file attachment annotations, and
//!   associated files.
//! - **Comments**: every markup annotation. Links and form fields stay.
//! - **Hidden layers**: everything drawn in a layer that is off, and then
//!   the layers themselves.
//! - **Cropped content**: everything drawn outside a page's crop box.
//! - **Private data and thumbnails**: `/PieceInfo` and `/Thumb`.
//!
//! Deleted objects and earlier revisions need nothing here: the new file
//! holds only what the document reaches.

use onionskin_content::redact::Area;
use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::objects::Objects;

/// What removing hidden information took out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sanitized {
    pub metadata: usize,
    pub scripts: usize,
    pub actions: usize,
    pub attachments: usize,
    pub comments: usize,
    /// Glyphs and painting operators drawn in hidden layers.
    pub hidden_layers: usize,
    /// Pages whose content outside the crop box was removed.
    pub cropped_pages: usize,
    pub private_data: usize,
}

/// The actions that stay: going to a page, and opening a web address.
const KEPT_ACTIONS: [&[u8]; 2] = [b"GoTo", b"URI"];

/// The annotations that stay: links and form fields.
const KEPT_ANNOTATIONS: [&[u8]; 2] = [b"Link", b"Widget"];

/// The layers that are off by default: `/D /OFF`, or every layer not on
/// when the base state is off.
pub(crate) fn hidden_layers(objects: &Objects) -> Vec<ObjRef> {
    let root = objects.root();
    let properties = objects.dict(root.get(b"OCProperties"));
    let config = objects.dict(properties.get(b"D"));
    let refs = |value: Option<&Object>| -> Vec<ObjRef> {
        match value.map(|value| objects.resolve(value)) {
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_reference).collect(),
            _ => Vec::new(),
        }
    };
    let base_off = config
        .get(b"BaseState")
        .and_then(Object::as_name)
        .is_some_and(|name| name.as_bytes() == b"OFF");
    if base_off {
        let on = refs(config.get(b"ON"));
        return refs(properties.get(b"OCGs"))
            .into_iter()
            .filter(|layer| !on.contains(layer))
            .collect();
    }
    refs(config.get(b"OFF"))
}

/// What of the media box `crop` leaves out, as up to four rectangles.
pub(crate) fn outside_crop(media: [f64; 4], crop: Option<[f64; 4]>) -> Vec<Area> {
    let Some(crop) = crop else {
        return Vec::new();
    };
    let [mx0, my0, mx1, my1] = normal(media);
    let [cx0, cy0, cx1, cy1] = normal(crop);
    let (cx0, cy0, cx1, cy1) = (cx0.max(mx0), cy0.max(my0), cx1.min(mx1), cy1.min(my1));
    [
        (mx0, my0, cx0, my1),
        (cx1, my0, mx1, my1),
        (cx0, my0, cx1, cy0),
        (cx0, cy1, cx1, my1),
    ]
    .into_iter()
    .filter(|(x0, y0, x1, y1)| x1 - x0 > 1e-6 && y1 - y0 > 1e-6)
    .map(|(x0, y0, x1, y1)| Area::rect(x0, y0, x1, y1))
    .collect()
}

fn normal([x0, y0, x1, y1]: [f64; 4]) -> [f64; 4] {
    [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
}

/// Everything besides the pages' content, taken out of `objects`.
pub(crate) fn sweep(objects: &mut Objects, hidden: &[ObjRef], report: &mut Sanitized) {
    if objects.trailer_mut().remove(b"Info").is_some() {
        report.metadata += 1;
    }
    catalog(objects, report);
    comments(objects, hidden, report);
    for number in objects.reachable() {
        let Some(object) = objects.get(number).cloned() else {
            continue;
        };
        let cleaned = match object {
            Object::Dict(dict) => clean(objects, dict, report).map(Object::Dict),
            Object::Stream(mut stream) => clean(objects, stream.dict.clone(), report).map(|dict| {
                stream.dict = dict;
                Object::Stream(stream)
            }),
            _ => None,
        };
        if let Some(cleaned) = cleaned {
            objects.set(number, cleaned);
        }
    }
}

/// The catalog's own keys: document scripts, attachments and layers.
fn catalog(objects: &mut Objects, report: &mut Sanitized) {
    let Some(root) = objects
        .trailer_mut()
        .get(b"Root")
        .and_then(Object::as_reference)
    else {
        return;
    };
    let mut dict = objects.dict(Some(&Object::Ref(root)));
    let mut names = objects.dict(dict.get(b"Names"));
    if names.remove(b"JavaScript").is_some() {
        report.scripts += 1;
    }
    if names.remove(b"EmbeddedFiles").is_some() {
        report.attachments += 1;
    }
    if dict.contains(b"Names") {
        dict.set(Name::new("Names"), Object::Dict(names));
    }
    for key in [b"AF".as_slice(), b"Collection"] {
        if dict.remove(key).is_some() {
            report.attachments += 1;
        }
    }
    dict.remove(b"OCProperties");
    let mut form = objects.dict(dict.get(b"AcroForm"));
    if form.remove(b"XFA").is_some() {
        report.scripts += 1;
        dict.set(Name::new("AcroForm"), Object::Dict(form));
    }
    objects.set(root.number, Object::Dict(dict));
}

/// One dictionary without its metadata, private data, additional actions,
/// layer membership and the actions that do not stay. `None` when nothing
/// changed.
fn clean(objects: &Objects, mut dict: Dict, report: &mut Sanitized) -> Option<Dict> {
    let before = dict.clone();
    if dict.remove(b"Metadata").is_some() {
        report.metadata += 1;
    }
    for key in [b"PieceInfo".as_slice(), b"Thumb"] {
        if dict.remove(key).is_some() {
            report.private_data += 1;
        }
    }
    if dict.remove(b"AA").is_some() {
        report.scripts += 1;
    }
    dict.remove(b"OC");
    for key in [b"A".as_slice(), b"OpenAction", b"Next"] {
        let removed = dict
            .get(key)
            .map(|action| objects.dict(Some(action)))
            .and_then(|action| action.get(b"S").and_then(Object::as_name).cloned())
            .is_some_and(|kind| !KEPT_ACTIONS.contains(&kind.as_bytes()));
        if removed {
            dict.remove(key);
            report.actions += 1;
        }
    }
    (dict != before).then_some(dict)
}

/// Every annotation other than a link or a form field, and any in a hidden
/// layer, off every page.
fn comments(objects: &mut Objects, hidden: &[ObjRef], report: &mut Sanitized) {
    let root = objects.root();
    let mut pages = vec![objects.resolve(root.get(b"Pages").unwrap_or(&Object::Null))];
    let mut seen = 0usize;
    while let Some(node) = pages.pop() {
        seen += 1;
        if seen > 1_000_000 {
            break;
        }
        let Object::Dict(node) = node else { continue };
        if let Some(Object::Array(kids)) = node.get(b"Kids").map(|kids| objects.resolve(kids)) {
            for kid in kids {
                if let Some(objref) = kid.as_reference() {
                    strip_page(objects, objref, hidden, report);
                    pages.push(objects.resolve(&kid));
                }
            }
        }
    }
}

fn strip_page(objects: &mut Objects, page: ObjRef, hidden: &[ObjRef], report: &mut Sanitized) {
    let mut dict = objects.dict(Some(&Object::Ref(page)));
    let Some(Object::Array(items)) = dict.get(b"Annots").map(|annots| objects.resolve(annots))
    else {
        return;
    };
    let mut kept = Vec::new();
    for item in items {
        let annotation = objects.dict(Some(&item));
        let subtype = annotation
            .get(b"Subtype")
            .and_then(Object::as_name)
            .cloned();
        let in_hidden_layer = annotation
            .get(b"OC")
            .and_then(Object::as_reference)
            .is_some_and(|layer| hidden.contains(&layer));
        let kept_kind =
            subtype.is_some_and(|subtype| KEPT_ANNOTATIONS.contains(&subtype.as_bytes()));
        if kept_kind && !in_hidden_layer {
            kept.push(item);
            continue;
        }
        let attachment = annotation
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|name| name.as_bytes() == b"FileAttachment");
        if attachment {
            report.attachments += 1;
        } else if !annotation.is_empty() {
            report.comments += 1;
        }
        if let Some(objref) = item.as_reference() {
            objects.set(objref.number, Object::Null);
        }
    }
    dict.set(Name::new("Annots"), Object::Array(kept));
    objects.set(page.number, Object::Dict(dict));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_crop_box_leaves_out_is_up_to_four_strips() {
        let media = [0.0, 0.0, 100.0, 200.0];
        assert!(outside_crop(media, None).is_empty());
        assert!(outside_crop(media, Some(media)).is_empty());
        let strips = outside_crop(media, Some([10.0, 20.0, 90.0, 180.0]));
        let bounds: Vec<_> = strips.iter().map(Area::bounds).collect();
        assert_eq!(
            bounds,
            [
                (0.0, 0.0, 10.0, 200.0),
                (90.0, 0.0, 100.0, 200.0),
                (10.0, 0.0, 90.0, 20.0),
                (10.0, 180.0, 90.0, 200.0),
            ]
        );
        let left_only = outside_crop(media, Some([50.0, 0.0, 100.0, 200.0]));
        assert_eq!(left_only.len(), 1);
    }
}
