//! One page's redaction applied to the new file's objects: its content
//! rewritten, the images and forms under its marks replaced, its marks
//! filled, and the annotations over them removed.

use onionskin_content::redact::{Area, Counts, NewResource, Removed};
use onionskin_content::Warning;
use onionskin_core::redactions::{redaction_areas, RedactionMark};
use onionskin_cos::{flate_encode, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use super::objects::Objects;
use super::overlay::{drawing, needs_font, FONT_NAME};
use super::scrub::{blank, scrubbed};
use crate::RedactError;

/// What applying one page's marks did.
pub(crate) struct PageApplied {
    pub(crate) page: usize,
    pub(crate) areas: Vec<Area>,
    pub(crate) counts: Counts,
    pub(crate) removed: Vec<Removed>,
    pub(crate) annotations: usize,
    /// The stream that draws the fills and overlay text: the one place text
    /// may sit inside an area afterwards.
    pub(crate) overlay: ObjRef,
    /// The page's content streams before, and the images and forms it drew
    /// that were replaced, which the new file must not keep unless another
    /// page draws them.
    pub(crate) originals: Vec<ObjRef>,
}

/// What else goes from a page besides its marks: content outside its crop
/// box, and hidden layers, when hidden information is removed.
#[derive(Debug, Clone, Default)]
pub(crate) struct Beyond {
    pub(crate) areas: Vec<Area>,
    pub(crate) hidden: Vec<ObjRef>,
}

/// Applies `marks` and removes what `beyond` names on `page`. `None` when
/// there are no marks and nothing else on the page had to go.
pub(crate) fn apply_page(
    source: &CosDocument,
    objects: &mut Objects,
    page: usize,
    marks: &[&RedactionMark],
    beyond: &Beyond,
) -> Result<Option<PageApplied>, RedactError> {
    let node = source.page(page)?;
    let mut areas: Vec<Area> = marks
        .iter()
        .flat_map(|mark| redaction_areas(mark))
        .collect();
    areas.extend(beyond.areas.iter().cloned());
    let redaction =
        onionskin_content::redact_page_with_hidden(source, page, &areas, &beyond.hidden)?;
    if marks.is_empty() && !redaction.content.changed {
        return Ok(None);
    }
    if redaction
        .warnings
        .iter()
        .any(|warning| matches!(warning, Warning::GlyphLimit { .. }))
    {
        return Err(RedactError::TooMuchText { page });
    }
    let fill = marks.first().and_then(|mark| mark.look.fill);
    let base = node.resources.clone().unwrap_or_default();
    let context = Context {
        source,
        areas: &areas,
        fill,
    };
    let mut resources = context.materialize(objects, &base, &redaction.content.resources)?;
    prune(objects, &mut resources, &redaction.content.bytes);
    if needs_font(marks) {
        let mut font = Dict::new();
        font.set(Name::new("Type"), Object::name("Font"));
        font.set(Name::new("Subtype"), Object::name("Type1"));
        font.set(Name::new("BaseFont"), Object::name("Helvetica"));
        font.set(Name::new("Encoding"), Object::name("WinAnsiEncoding"));
        add_to(
            objects,
            &mut resources,
            b"Font",
            FONT_NAME,
            Object::Dict(font),
        );
    }

    let mut content = b"q\n".to_vec();
    content.extend_from_slice(&redaction.content.bytes);
    content.extend_from_slice(b"\nQ\n");
    let content = objects.add(Object::Stream(flate_stream(Dict::new(), &content)));
    let overlay = objects.add(Object::Stream(flate_stream(Dict::new(), &drawing(marks))));

    let mut dict = objects.dict(Some(&Object::Ref(node.objref)));
    let mut originals = content_refs(dict.get(b"Contents"));
    replaced(&redaction.content.resources, &mut originals);
    dict.set(
        Name::new("Contents"),
        Object::Array(vec![Object::Ref(content), Object::Ref(overlay)]),
    );
    dict.set(Name::new("Resources"), Object::Dict(resources));
    // A thumbnail is a picture of the page as it was; private application
    // data may hold its content too.
    dict.remove(b"Thumb");
    dict.remove(b"PieceInfo");
    let annotations = remove_annotations(objects, &mut dict, &areas);
    objects.set(node.objref.number, Object::Dict(dict));

    Ok(Some(PageApplied {
        page,
        areas,
        counts: redaction.counts,
        removed: redaction.removed,
        annotations,
        overlay,
        originals,
    }))
}

struct Context<'a> {
    source: &'a CosDocument,
    areas: &'a [Area],
    fill: Option<[f64; 3]>,
}

impl Context<'_> {
    /// `base` with the resources a rewritten stream names added: scrubbed
    /// images, rewritten forms, graphics states with a rewritten mask.
    fn materialize(
        &self,
        objects: &mut Objects,
        base: &Dict,
        uses: &[NewResource],
    ) -> Result<Dict, RedactError> {
        let mut resources = objects.dict(Some(&Object::Dict(base.clone())));
        for resource in uses {
            match resource {
                NewResource::Image {
                    name,
                    original,
                    placement,
                } => {
                    let image = self.stream(*original)?;
                    let copy = match scrubbed(self.source, &image, placement, self.areas, self.fill)
                    {
                        Some((mut copy, mask)) => {
                            if let Some(mask) = mask {
                                let mask = objects.add(Object::Stream(mask));
                                copy.dict.set(Name::new("SMask"), Object::Ref(mask));
                            }
                            copy
                        }
                        None => blank(),
                    };
                    let copy = objects.add(Object::Stream(copy));
                    add_to(
                        objects,
                        &mut resources,
                        b"XObject",
                        name.clone(),
                        Object::Ref(copy),
                    );
                }
                NewResource::Form {
                    name,
                    original,
                    content,
                } => {
                    let form = self.form(objects, *original, &resources, content)?;
                    add_to(
                        objects,
                        &mut resources,
                        b"XObject",
                        name.clone(),
                        Object::Ref(form),
                    );
                }
                NewResource::GState {
                    name,
                    original,
                    group,
                    content,
                } => {
                    let group = self.form(objects, *group, &resources, content)?;
                    let mut state = original.clone();
                    let mut mask = objects.dict(state.get(b"SMask"));
                    mask.set(Name::new("G"), Object::Ref(group));
                    state.set(Name::new("SMask"), Object::Dict(mask));
                    let state = objects.add(Object::Dict(state));
                    add_to(
                        objects,
                        &mut resources,
                        b"ExtGState",
                        name.clone(),
                        Object::Ref(state),
                    );
                }
            }
        }
        Ok(resources)
    }

    /// A copy of form `original` drawing `content`, with its own resources,
    /// or the caller's when it had none, plus what `content` names.
    fn form(
        &self,
        objects: &mut Objects,
        original: ObjRef,
        caller: &Dict,
        content: &onionskin_content::redact::Rewritten,
    ) -> Result<ObjRef, RedactError> {
        let form = self.stream(original)?;
        let own = form
            .dict
            .get(b"Resources")
            .map(|resources| objects.dict(Some(resources)));
        let base = own.unwrap_or_else(|| caller.clone());
        let mut resources = self.materialize(objects, &base, &content.resources)?;
        prune(objects, &mut resources, &content.bytes);
        let mut dict = form.dict.clone();
        for key in ["Length", "Filter", "DecodeParms", "PieceInfo"] {
            dict.remove(key.as_bytes());
        }
        dict.set(Name::new("Resources"), Object::Dict(resources));
        Ok(objects.add(Object::Stream(flate_stream(dict, &content.bytes))))
    }

    fn stream(&self, objref: ObjRef) -> Result<Stream, RedactError> {
        self.source
            .get(objref.number)?
            .object
            .as_stream()
            .cloned()
            .ok_or(RedactError::NotAStream(objref.number))
    }
}

/// `value` named `name` in `resources`' `category`, the category made a
/// dictionary of its own so a shared one is not changed.
fn add_to(
    objects: &Objects,
    resources: &mut Dict,
    category: &[u8],
    name: impl Into<Name>,
    value: Object,
) {
    let mut entries = objects.dict(resources.get(category));
    entries.set(name.into(), value);
    resources.set(Name(category.to_vec()), Object::Dict(entries));
}

/// The images, forms and mask groups a rewrite replaced, at any depth.
fn replaced(resources: &[NewResource], out: &mut Vec<ObjRef>) {
    for resource in resources {
        match resource {
            NewResource::Image { original, .. } => out.push(*original),
            NewResource::Form {
                original, content, ..
            } => {
                out.push(*original);
                replaced(&content.resources, out);
            }
            NewResource::GState { group, content, .. } => {
                out.push(*group);
                replaced(&content.resources, out);
            }
        }
    }
}

/// Keeps only the XObjects, graphics states and marked-content properties
/// `content` names. A name the rewrite stopped using points at what it
/// replaced, or at a hidden layer it removed, and a file that still refers
/// to that keeps it.
fn prune(objects: &Objects, resources: &mut Dict, content: &[u8]) {
    let used = used_names(content);
    for (category, operator) in [
        (b"XObject".as_slice(), b"Do".as_slice()),
        (b"ExtGState", b"gs"),
        (b"Properties", b"BDC"),
    ] {
        if resources.get(category).is_none() {
            continue;
        }
        let mut entries = objects.dict(resources.get(category));
        let names: Vec<Name> = entries.iter().map(|(name, _)| name.clone()).collect();
        for name in names {
            if !used.contains(&(operator.to_vec(), name.as_bytes().to_vec())) {
                entries.remove(name.as_bytes());
            }
        }
        resources.set(Name(category.to_vec()), Object::Dict(entries));
    }
}

/// `(operator, name)` for every `Do`, `gs` and `BDC` in `content`.
fn used_names(content: &[u8]) -> std::collections::BTreeSet<(Vec<u8>, Vec<u8>)> {
    let mut lexer = onionskin_content::Tokenizer::new(content);
    let mut used = std::collections::BTreeSet::new();
    while let Some(op) = lexer.next_operation() {
        let operator = op.operator.as_bytes();
        if matches!(operator, b"Do" | b"gs" | b"BDC") {
            if let Some(Object::Name(name)) = op.operands.last() {
                used.insert((operator.to_vec(), name.as_bytes().to_vec()));
            }
        }
    }
    used
}

fn flate_stream(mut dict: Dict, bytes: &[u8]) -> Stream {
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    Stream {
        dict,
        raw: flate_encode(bytes),
    }
}

fn content_refs(contents: Option<&Object>) -> Vec<ObjRef> {
    match contents {
        Some(Object::Ref(objref)) => vec![*objref],
        Some(Object::Array(items)) => items.iter().filter_map(Object::as_reference).collect(),
        _ => Vec::new(),
    }
}

/// Takes the page's redaction marks off it, and every annotation whose
/// rectangle reaches an area, with its pop-up. They are written as `null`,
/// so whatever else pointed at them (a form field, the structure tree)
/// points at nothing. How many annotations other than marks went.
fn remove_annotations(objects: &mut Objects, page: &mut Dict, areas: &[Area]) -> usize {
    let items = match page.get(b"Annots").map(|annots| objects.resolve(annots)) {
        Some(Object::Array(items)) => items,
        _ => return 0,
    };
    let mut kept = Vec::new();
    let mut removed = 0;
    for item in items {
        let dict = objects.dict(Some(&item));
        let is_mark = dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|name| name.as_bytes() == b"Redact");
        if !is_mark && !reaches(&dict, areas) {
            kept.push(item);
            continue;
        }
        if !is_mark {
            removed += 1;
        }
        for objref in [
            item.as_reference(),
            dict.get(b"Popup").and_then(Object::as_reference),
        ]
        .into_iter()
        .flatten()
        {
            objects.set(objref.number, Object::Null);
        }
    }
    page.set(Name::new("Annots"), Object::Array(kept));
    removed
}

fn reaches(annotation: &Dict, areas: &[Area]) -> bool {
    let numbers: Vec<f64> = annotation
        .get(b"Rect")
        .and_then(Object::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|value| match value {
            Object::Integer(value) => Some(*value as f64),
            Object::Real(value) => Some(*value),
            _ => None,
        })
        .collect();
    let [x0, y0, x1, y1] = numbers.as_slice() else {
        return false;
    };
    let (x0, x1, y0, y1) = (x0.min(*x1), x0.max(*x1), y0.min(*y1), y0.max(*y1));
    areas.iter().any(|area| {
        let (ax0, ay0, ax1, ay1) = area.bounds();
        ax0 < x1 && x0 < ax1 && ay0 < y1 && y0 < ay1
    })
}
