//! Writing an annotation: the dictionary, its appearance stream, the page's
//! `/Annots`, and the structure element on a tagged document.
//!
//! One transaction, one undo entry. A tool that authors a comment writes three
//! or four objects and none of them is useful without the others, so a partial
//! undo would leave an appearance stream nothing references or a page naming an
//! annotation that is gone.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::appearance::{normal_appearance, numbers};
use super::model::{Annotation, BorderEffect, Color, Flags};
use crate::edit::Transaction;
use crate::structure::{attach_annotation, Structure};
use crate::{Error, Result};

/// Write `annotation` onto `page`, returning the new annotation's reference.
pub(crate) fn add(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
    annotation: &Annotation,
    now: i64,
) -> Result<ObjRef> {
    let appearance_number = tx.reserve();
    let annotation_number = tx.reserve();
    let annotation_ref = ObjRef::new(annotation_number, 0);

    tx.set_object(
        appearance_number,
        0,
        Object::Stream(normal_appearance(annotation)),
    )?;

    let (maintenance, parent_key) = attach_annotation(tx, structure, page, annotation_ref)?;
    let _ = maintenance;

    let dict = dictionary(
        annotation,
        page,
        ObjRef::new(appearance_number, 0),
        parent_key,
        now,
    );
    tx.set_object(annotation_number, 0, Object::Dict(dict))?;

    append_to_page_annots(tx, page, annotation_ref)?;
    Ok(annotation_ref)
}

/// Remove an annotation from its page.
///
/// M3 frees no object number, so the annotation object itself is left in the
/// file and the removal is a rewrite of the referrer: the page stops naming it.
/// That is what makes the removal undoable by restoring one array.
pub(crate) fn remove(tx: &mut Transaction<'_>, page: ObjRef, annotation: ObjRef) -> Result<bool> {
    let Some((holder, mut items)) = annots_array(tx, page)? else {
        return Ok(false);
    };
    let before = items.len();
    items.retain(|item| !matches!(item, Object::Ref(objref) if objref.number == annotation.number));
    if items.len() == before {
        return Ok(false);
    }
    write_annots(tx, page, holder, items)?;
    Ok(true)
}

/// The annotation dictionary.
fn dictionary(
    annotation: &Annotation,
    page: ObjRef,
    appearance: ObjRef,
    parent_key: Option<i64>,
    now: i64,
) -> Dict {
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("Annot"));
    dict.set(
        Name::new("Subtype"),
        Object::Name(annotation.subtype.as_name()),
    );
    dict.set(
        Name::new("Rect"),
        numbers(&[
            annotation.rect.x0,
            annotation.rect.y0,
            annotation.rect.x1,
            annotation.rect.y1,
        ]),
    );
    dict.set(Name::new("P"), Object::Ref(page));
    dict.set(Name::new("F"), Object::Integer(annotation.flags.0));

    let mut appearances = Dict::new();
    appearances.set(Name::new("N"), Object::Ref(appearance));
    dict.set(Name::new("AP"), Object::Dict(appearances));

    // Both dates, both with an explicit timezone, which is what the format
    // requires and what readers sort by. They are written in UTC with a `Z`
    // offset rather than local time: a local offset needs a timezone database
    // this crate does not carry, and an offset written wrong is worse than one
    // written honestly as UTC.
    let timestamp = Object::String(pdf_date(now).into_bytes());
    dict.set(Name::new("CreationDate"), timestamp.clone());
    dict.set(Name::new("M"), timestamp);

    if annotation.subtype.takes_quads() && !annotation.quads.is_empty() {
        let mut points = Vec::with_capacity(annotation.quads.len() * 8);
        for quad in &annotation.quads {
            points.extend_from_slice(&quad.as_array());
        }
        dict.set(Name::new("QuadPoints"), numbers(&points));
    }
    if !annotation.ink.is_empty() {
        let strokes = annotation
            .ink
            .iter()
            .map(|stroke| {
                let flat: Vec<f64> = stroke.iter().flat_map(|(x, y)| [*x, *y]).collect();
                numbers(&flat)
            })
            .collect();
        dict.set(Name::new("InkList"), Object::Array(strokes));
    }
    if let Some(((sx, sy), (ex, ey))) = annotation.line {
        dict.set(Name::new("L"), numbers(&[sx, sy, ex, ey]));
    }
    // The same `TextStyle` the appearance stream draws with, so the two cannot
    // disagree about font, size or colour.
    if let Some(style) = annotation.text_style {
        dict.set(
            Name::new("DA"),
            Object::String(style.default_appearance().into_bytes()),
        );
    }
    if let Some(intent) = annotation.intent {
        dict.set(Name::new("IT"), Object::name(intent.as_str()));
    }
    if !annotation.vertices.is_empty() {
        let flat: Vec<f64> = annotation
            .vertices
            .iter()
            .flat_map(|(x, y)| [*x, *y])
            .collect();
        dict.set(Name::new("Vertices"), numbers(&flat));
    }
    // An arrow is a `/Line` with `/LE`, never a subtype of its own: a
    // `/Polygon` drawn as one renders in Acrobat as a line with no head.
    if let Some((first, last)) = annotation.endings {
        dict.set(
            Name::new("LE"),
            Object::Array(vec![
                Object::name(first.as_str()),
                Object::name(last.as_str()),
            ]),
        );
    }
    if let Some(BorderEffect::Cloudy { intensity }) = annotation.border_effect {
        let mut effect = Dict::new();
        effect.set(Name::new("S"), Object::name("C"));
        effect.set(Name::new("I"), Object::Real(intensity));
        dict.set(Name::new("BE"), Object::Dict(effect));
    }
    if !annotation.callout.is_empty() {
        let flat: Vec<f64> = annotation
            .callout
            .iter()
            .flat_map(|(x, y)| [*x, *y])
            .collect();
        dict.set(Name::new("CL"), numbers(&flat));
    }
    if let Some(contents) = &annotation.contents {
        dict.set(Name::new("Contents"), text_string(contents));
    }
    if let Some(author) = &annotation.author {
        dict.set(Name::new("T"), text_string(author));
    }
    if let Some(subject) = &annotation.subject {
        dict.set(Name::new("Subj"), text_string(subject));
    }
    if let Some(color) = annotation.color {
        dict.set(Name::new("C"), color_array(color));
    }
    if let Some(color) = annotation.interior_color {
        dict.set(Name::new("IC"), color_array(color));
    }
    if let Some(opacity) = annotation.opacity {
        dict.set(Name::new("CA"), Object::Real(opacity));
    }
    if let Some(icon) = &annotation.icon {
        dict.set(Name::new("Name"), Object::Name(Name::new(icon)));
    }
    if let Some(parent) = annotation.in_reply_to {
        dict.set(Name::new("IRT"), Object::Ref(parent));
        dict.set(Name::new("RT"), Object::name("R"));
    }
    if let Some((state, model)) = &annotation.state {
        dict.set(Name::new("State"), text_string(state));
        dict.set(Name::new("StateModel"), text_string(model));
    }
    if let Some(key) = parent_key {
        dict.set(Name::new("StructParent"), Object::Integer(key));
    }

    let mut border = Dict::new();
    border.set(Name::new("W"), Object::Real(annotation.border_width));
    dict.set(Name::new("BS"), Object::Dict(border));
    dict
}

fn color_array(color: Color) -> Object {
    numbers(&[color.red, color.green, color.blue])
}

/// A PDF text string. Written as UTF-16BE with a byte-order mark whenever the
/// text is not pure ASCII, because PDFDocEncoding cannot carry most of what a
/// comment contains and a reader has no other way to tell which encoding it is
/// looking at.
fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        return Object::String(text.as_bytes().to_vec());
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes)
}

/// `D:YYYYMMDDHHmmSSZ00'00'`, which is ISO 32000-1 7.9.4's format with the
/// `Z` offset spelled out.
pub(crate) fn pdf_date(seconds_since_epoch: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_from_epoch(seconds_since_epoch);
    format!("D:{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z00'00'")
}

/// Days-to-civil, after Howard Hinnant's `civil_from_days`. Correct for the
/// proleptic Gregorian calendar over any range this will ever see, and 20 lines
/// against a dependency that would carry a timezone database for one format
/// string.
fn civil_from_epoch(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (
        year,
        month,
        day,
        (time / 3600) as u32,
        ((time % 3600) / 60) as u32,
        (time % 60) as u32,
    )
}

/// The page's `/Annots` array and where it lives: either inline in the page
/// dictionary, or in an object of its own that the page references.
fn annots_array(tx: &Transaction<'_>, page: ObjRef) -> Result<Option<(AnnotsHolder, Vec<Object>)>> {
    let Some(state) = tx.object(page.number)? else {
        return Err(Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: format!("page object {} is not in the document", page.number),
        }));
    };
    let Some(dict) = state.object.as_dict() else {
        return Err(Error::NotADictionary {
            number: page.number,
        });
    };
    match dict.get(b"Annots") {
        None | Some(Object::Null) => Ok(Some((AnnotsHolder::Inline, Vec::new()))),
        Some(Object::Array(items)) => Ok(Some((AnnotsHolder::Inline, items.clone()))),
        Some(Object::Ref(objref)) => {
            let Some(state) = tx.object(objref.number)? else {
                return Ok(Some((AnnotsHolder::Indirect(*objref), Vec::new())));
            };
            match state.object {
                Object::Array(items) => Ok(Some((AnnotsHolder::Indirect(*objref), items))),
                _ => Ok(Some((AnnotsHolder::Indirect(*objref), Vec::new()))),
            }
        }
        Some(_) => Ok(None),
    }
}

enum AnnotsHolder {
    /// The array is written in the page dictionary itself.
    Inline,
    /// The array is its own object, which is what a file with many annotations
    /// per page usually has. Rewriting that object rather than inlining the
    /// array keeps the page dictionary byte-identical, which matters because
    /// every other reference to it stays valid without a second rewrite.
    Indirect(ObjRef),
}

fn append_to_page_annots(tx: &mut Transaction<'_>, page: ObjRef, annotation: ObjRef) -> Result<()> {
    let Some((holder, mut items)) = annots_array(tx, page)? else {
        return Err(Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: format!("page object {}'s /Annots is not an array", page.number),
        }));
    };
    items.push(Object::Ref(annotation));
    write_annots(tx, page, holder, items)
}

fn write_annots(
    tx: &mut Transaction<'_>,
    page: ObjRef,
    holder: AnnotsHolder,
    items: Vec<Object>,
) -> Result<()> {
    match holder {
        AnnotsHolder::Indirect(objref) => {
            let generation = tx
                .object(objref.number)?
                .map_or(objref.generation, |state| state.generation);
            tx.set_object(objref.number, generation, Object::Array(items))
        }
        AnnotsHolder::Inline => {
            let Some(state) = tx.object(page.number)? else {
                return Ok(());
            };
            let Some(dict) = state.object.as_dict() else {
                return Ok(());
            };
            let mut dict = dict.clone();
            if items.is_empty() {
                // Removing the last annotation removes the key, rather than
                // leaving `/Annots []` behind. An empty array is legal, but a
                // page that carries one no longer equals the page the base
                // holds, so the overlay could not collapse and a save would
                // append a section for a document the user changed and changed
                // back.
                dict.remove(b"Annots");
            } else {
                dict.set(Name::new("Annots"), Object::Array(items));
            }
            tx.set_object(page.number, state.generation, Object::Dict(dict))
        }
    }
}

/// `/F` with the hidden bit set or cleared, which is the only thing the render
/// filter ever changes about an annotation.
pub(crate) fn with_hidden(dict: &Dict, hidden: bool) -> Dict {
    let current = dict.get(b"F").and_then(Object::as_integer).unwrap_or(0);
    let mut out = dict.clone();
    out.set(
        Name::new("F"),
        Object::Integer(Flags(current).with_hidden(hidden).0),
    );
    out
}
