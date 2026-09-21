//! A page as printed: the page itself, and over it the appearance of every
//! annotation that prints, as one Form XObject.
//!
//! A page imported as a form carries its content, not its annotations,
//! because a form has no `/Annots`. Print draws what a reader draws on
//! paper: the page and each annotation whose `/F` says it prints and does
//! not hide, in its `/AP` normal appearance, placed at its `/Rect` by the
//! algorithm in ISO 32000-1 12.5.5. So the printed form is a wrapper: one
//! `Do` of the page form, then one `Do` per annotation appearance.

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use super::import::{import_page_as_form, source_leaves, Copier};
use crate::edit::Transaction;
use crate::{Error, Result};

/// `/F` bit 2: hidden. Bit 3: print.
const HIDDEN: i64 = 1 << 1;
const PRINT: i64 = 1 << 2;

/// Page `index` of `source` with its printing annotations, as one Form
/// XObject in the transaction's document, and its `/BBox`. Refuses an
/// encrypted source, like every read-out.
pub fn import_page_for_print(
    tx: &mut Transaction<'_>,
    source: &CosDocument,
    index: usize,
) -> Result<(ObjRef, [f64; 4])> {
    let (page_form, bbox) = import_page_as_form(tx, source, index)?;
    let leaves = source_leaves(source)?;
    let leaf = leaves.get(index).ok_or(Error::NoSuchPage {
        page: index,
        count: leaves.len(),
    })?;
    let page = leaf.materialized(leaf.objref);

    let mut xobjects = Dict::new();
    xobjects.set(Name::new("Page"), Object::Ref(page_form));
    let mut content = String::from("/Page Do\n");
    let mut copier = Copier::new(source, &leaves, &[], None)?;
    for (number, appearance) in printed_appearances(source, &page).into_iter().enumerate() {
        let copied = copier.rewrite(tx, Object::Ref(appearance.stream))?;
        let name = format!("A{number}");
        let [a, b, c, d, e, f] = appearance.placement;
        content.push_str(&format!("q {a} {b} {c} {d} {e} {f} cm /{name} Do Q\n"));
        xobjects.set(Name::new(&name), copied);
    }
    copier.drain(tx)?;

    let mut resources = Dict::new();
    resources.set(Name::new("XObject"), Object::Dict(xobjects));
    let raw = onionskin_cos::flate_encode(content.as_bytes());
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("XObject"));
    dict.set(Name::new("Subtype"), Object::name("Form"));
    dict.set(
        Name::new("BBox"),
        Object::Array(bbox.iter().copied().map(Object::Real).collect()),
    );
    dict.set(Name::new("Resources"), Object::Dict(resources));
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    dict.set(Name::new("Length"), Object::Integer(raw.len() as i64));
    let number = tx.reserve();
    tx.put_object(number, 0, Object::Stream(Stream { dict, raw }))?;
    Ok((ObjRef::new(number, 0), bbox))
}

/// One annotation appearance to draw: its stream in the source, and the
/// matrix that puts its `/BBox` on its `/Rect`.
struct Printed {
    stream: ObjRef,
    placement: [f64; 6],
}

/// Every annotation on `page` that prints, in `/Annots` order.
fn printed_appearances(source: &CosDocument, page: &Dict) -> Vec<Printed> {
    let annots = match page.get(b"Annots").map(|annots| source.resolve(annots)) {
        Some(Ok(Object::Array(items))) => items,
        _ => return Vec::new(),
    };
    annots
        .iter()
        .filter_map(|item| {
            let Object::Dict(annot) = source.resolve(item).ok()? else {
                return None;
            };
            let flags = annot.get(b"F").and_then(Object::as_integer).unwrap_or(0);
            if flags & HIDDEN != 0 || flags & PRINT == 0 {
                return None;
            }
            let stream = normal_appearance(source, &annot)?;
            let Object::Stream(form) = source.get(stream.number).ok()?.object else {
                return None;
            };
            let rect = numbers4(source, annot.get(b"Rect")?)?;
            let bbox = numbers4(source, form.dict.get(b"BBox")?)?;
            let matrix = form
                .dict
                .get(b"Matrix")
                .and_then(|matrix| numbers(source, matrix))
                .and_then(|values| <[f64; 6]>::try_from(values).ok())
                .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            Some(Printed {
                stream,
                placement: placement(rect, bbox, matrix)?,
            })
        })
        .collect()
}

/// `/AP /N`: the stream itself, or the one `/AS` picks from a state
/// dictionary.
fn normal_appearance(source: &CosDocument, annot: &Dict) -> Option<ObjRef> {
    let appearances = source.resolve(annot.get(b"AP")?).ok()?;
    let normal = appearances.as_dict()?.get(b"N")?.clone();
    match normal {
        Object::Ref(objref) => match source.get(objref.number).ok()?.object {
            Object::Stream(_) => Some(objref),
            Object::Dict(states) => {
                let state = annot.get(b"AS")?.as_name()?.clone();
                states.get(state.as_bytes())?.as_reference()
            }
            _ => None,
        },
        Object::Dict(states) => {
            let state = annot.get(b"AS")?.as_name()?.clone();
            states.get(state.as_bytes())?.as_reference()
        }
        _ => None,
    }
}

fn numbers(source: &CosDocument, object: &Object) -> Option<Vec<f64>> {
    let Object::Array(items) = source.resolve(object).ok()? else {
        return None;
    };
    Some(
        items
            .iter()
            .filter_map(|item| match source.resolve(item).ok()? {
                Object::Integer(value) => Some(value as f64),
                Object::Real(value) => Some(value),
                _ => None,
            })
            .collect(),
    )
}

fn numbers4(source: &CosDocument, object: &Object) -> Option<[f64; 4]> {
    let [x0, y0, x1, y1] = <[f64; 4]>::try_from(numbers(source, object)?).ok()?;
    Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
}

/// ISO 32000-1 12.5.5: transform the appearance's `/BBox` by its
/// `/Matrix`, take the bounding box of the result, and map that onto the
/// annotation's `/Rect`. The form's own `/Matrix` is applied by `Do`, so
/// this is the matrix that goes before it. `None` for a degenerate box.
pub(crate) fn placement(rect: [f64; 4], bbox: [f64; 4], matrix: [f64; 6]) -> Option<[f64; 6]> {
    let [a, b, c, d, e, f] = matrix;
    let corners = [
        (bbox[0], bbox[1]),
        (bbox[2], bbox[1]),
        (bbox[0], bbox[3]),
        (bbox[2], bbox[3]),
    ]
    .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
    let min_x = corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = corners
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let (width, height) = (max_x - min_x, max_y - min_y);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let sx = (rect[2] - rect[0]) / width;
    let sy = (rect[3] - rect[1]) / height;
    Some([sx, 0.0, 0.0, sy, rect[0] - sx * min_x, rect[1] - sy * min_y])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(pairs: Vec<(&str, Object)>) -> Object {
        let mut dict = Dict::new();
        for (key, value) in pairs {
            dict.set(Name::new(key), value);
        }
        Object::Dict(dict)
    }

    fn reference(number: u32) -> Object {
        Object::Ref(ObjRef::new(number, 0))
    }

    fn annot(flags: i64, normal: Object, state: Option<&str>) -> Object {
        let mut pairs = vec![
            ("Type", Object::name("Annot")),
            ("Subtype", Object::name("Square")),
            ("F", Object::Integer(flags)),
            (
                "Rect",
                Object::Array([10, 10, 30, 20].map(Object::Integer).to_vec()),
            ),
            ("AP", entries(vec![("N", normal)])),
        ];
        if let Some(state) = state {
            pairs.push(("AS", Object::name(state)));
        }
        entries(pairs)
    }

    fn appearance() -> Object {
        let dict = match entries(vec![
            ("Subtype", Object::name("Form")),
            (
                "BBox",
                Object::Array([0, 0, 20, 10].map(Object::Integer).to_vec()),
            ),
        ]) {
            Object::Dict(dict) => dict,
            _ => unreachable!(),
        };
        Object::Stream(Stream {
            dict,
            raw: b"0 0 20 10 re f".to_vec(),
        })
    }

    /// Only an annotation whose `/F` says print and not hidden is drawn,
    /// and a state dictionary is resolved through `/AS`.
    #[test]
    fn only_printing_visible_annotations_are_drawn_in_their_chosen_state() {
        let states = entries(vec![("On", reference(9)), ("Off", reference(10))]);
        let objects = vec![
            (
                ObjRef::new(1, 0),
                entries(vec![
                    ("Type", Object::name("Catalog")),
                    ("Pages", reference(2)),
                ]),
            ),
            (
                ObjRef::new(2, 0),
                entries(vec![
                    ("Type", Object::name("Pages")),
                    ("Count", Object::Integer(1)),
                    ("Kids", Object::Array(vec![reference(3)])),
                ]),
            ),
            (
                ObjRef::new(3, 0),
                entries(vec![
                    ("Type", Object::name("Page")),
                    ("Parent", reference(2)),
                    ("Annots", Object::Array((5..=8).map(reference).collect())),
                ]),
            ),
            (ObjRef::new(5, 0), annot(PRINT, reference(9), None)),
            (ObjRef::new(6, 0), annot(0, reference(9), None)),
            (ObjRef::new(7, 0), annot(PRINT | HIDDEN, reference(9), None)),
            (ObjRef::new(8, 0), annot(PRINT, states, Some("Off"))),
            (ObjRef::new(9, 0), appearance()),
            (ObjRef::new(10, 0), appearance()),
        ];
        let trailer = match entries(vec![("Root", reference(1))]) {
            Object::Dict(dict) => dict,
            _ => unreachable!(),
        };
        let bytes = CosDocument::write_new(&objects, trailer).expect("writes");
        let doc =
            CosDocument::open(Box::new(onionskin_cos::BytesSource::new(bytes))).expect("opens");
        let page = doc.page(0).expect("page").dict;
        let printed = printed_appearances(&doc, &page);
        assert_eq!(
            printed.iter().map(|p| p.stream.number).collect::<Vec<_>>(),
            [9, 10],
            "the printing one, then the stateful one in its /AS state"
        );
        assert_eq!(printed[0].placement, [1.0, 0.0, 0.0, 1.0, 10.0, 10.0]);
    }

    #[test]
    fn an_appearance_box_is_mapped_onto_its_rect() {
        // A 20 x 10 box drawn at (100, 200) to (140, 210): twice as wide.
        let m = placement(
            [100.0, 200.0, 140.0, 210.0],
            [0.0, 0.0, 20.0, 10.0],
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        )
        .unwrap();
        assert_eq!(m, [2.0, 0.0, 0.0, 1.0, 100.0, 200.0]);
        // A box that does not start at the origin is moved to the rect.
        let m = placement(
            [0.0, 0.0, 10.0, 10.0],
            [5.0, 5.0, 15.0, 15.0],
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        )
        .unwrap();
        assert_eq!(m, [1.0, 0.0, 0.0, 1.0, -5.0, -5.0]);
        assert!(placement(
            [0.0; 4],
            [0.0, 0.0, 0.0, 5.0],
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        )
        .is_none());
    }
}
