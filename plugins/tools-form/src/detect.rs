//! Finding where a form is filled in on pages that have no fields: what
//! Acrobat's Prepare Form does when it opens a document.
//!
//! Acrobat's detection is trained; this is rules, and says so. It finds:
//!
//! - a run of three or more underscores, which becomes a text field over it;
//! - a horizontal rule at least half an inch long with nothing written on
//!   it, which becomes a text field standing on it;
//! - a small square box, or a `☐` or `□` character, which becomes a check
//!   box;
//! - an empty box of a text line's height, which becomes a text field
//!   inside it.
//!
//! Each is named after the words just before it on its line, the way a
//! label reads ("Name:" names the field `Name`), or numbered when there are
//! none. Nothing is placed over a field the page already has, or over
//! another place found.

use std::collections::BTreeSet;

use onionskin_content::shapes::Shape;
use onionskin_content::{Mapping, PageText};
use onionskin_core::forms::{add_named_field, Form, NewField};
use onionskin_core::{Document, PageIndex};
use onionskin_plugin_api::CommandError;

use crate::fill::failed;

const LABEL: &str = "Detect Form Fields";

/// Points: a rule shorter than this is not a line to write on.
const MIN_RULE: f64 = 36.0;
/// How far a rule may lean or be thick and still be a rule.
const RULE_SLACK: f64 = 2.0;
/// How tall a field standing on a rule is.
const LINE_HEIGHT: f64 = 16.0;
/// A check box is a square this big, give or take.
const BOX_SIZE: std::ops::RangeInclusive<f64> = 6.0..=20.0;
/// How far left of a place its label may be, on the same line.
const LABEL_REACH: f64 = 200.0;

/// A place found to fill in.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub page: PageIndex,
    pub kind: NewField,
    pub rect: [f64; 4],
    /// The words before it on its line, which name it.
    pub label: Option<String>,
}

/// One glyph as detection reads it: its box and its text.
#[derive(Debug, Clone)]
struct Letter {
    rect: [f64; 4],
    text: String,
}

fn quad_rect(corners: &[(f64, f64); 4]) -> [f64; 4] {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &(x, y) in corners {
        out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
    }
    out
}

fn letters(text: &PageText) -> Vec<Letter> {
    text.runs
        .iter()
        .flat_map(|run| {
            run.glyphs
                .iter()
                .filter_map(move |glyph| match &glyph.mapping {
                    Mapping::Text(range) => Some(Letter {
                        rect: quad_rect(&glyph.quad.corners),
                        text: run.text.get(range.clone()).unwrap_or_default().to_owned(),
                    }),
                    Mapping::Unmapped => None,
                })
        })
        .collect()
}

fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// How much of the smaller of `a` and `b` their overlap covers.
fn covered(a: [f64; 4], b: [f64; 4]) -> f64 {
    let width = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let height = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let area = |r: [f64; 4]| ((r[2] - r[0]) * (r[3] - r[1])).max(f64::EPSILON);
    width * height / area(a).min(area(b))
}

/// Whether anything but underscores and spaces is written inside `rect`.
fn written_in(letters: &[Letter], rect: [f64; 4]) -> bool {
    letters.iter().any(|letter| {
        overlaps(letter.rect, rect) && letter.text.chars().any(|c| !matches!(c, '_' | ' '))
    })
}

/// The words just before `rect` on its line: the letters left of it whose
/// middle is within its height, read right to left up to a gap or a colon.
fn label(letters: &[Letter], rect: [f64; 4]) -> Option<String> {
    let middle = |r: [f64; 4]| (r[1] + r[3]) / 2.0;
    let height = (rect[3] - rect[1]).max(8.0);
    let mut before: Vec<&Letter> = letters
        .iter()
        .filter(|letter| {
            letter.rect[2] <= rect[0] + 1.0
                && rect[0] - letter.rect[2] <= LABEL_REACH
                && (middle(letter.rect) - middle(rect)).abs() <= height
                && !letter.text.chars().all(|c| c == '_')
        })
        .collect();
    before.sort_by(|a, b| b.rect[0].total_cmp(&a.rect[0]));
    let mut words = String::new();
    let mut edge = rect[0];
    for letter in before {
        let gap = edge - letter.rect[2];
        let size = letter.rect[3] - letter.rect[1];
        if gap > size.max(4.0) * 2.0 && !words.trim().is_empty() {
            break;
        }
        words.insert_str(0, &letter.text);
        edge = letter.rect[0];
    }
    let words: String = words
        .trim()
        .trim_end_matches(':')
        .trim()
        .replace('.', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!words.is_empty() && words.chars().any(char::is_alphanumeric)).then_some(words)
}

/// Runs of three or more underscores, and the box characters.
fn from_text(page: PageIndex, letters: &[Letter]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut run: Vec<&Letter> = Vec::new();
    let flush = |run: &mut Vec<&Letter>, found: &mut Vec<Found>| {
        if run.len() >= 3 {
            let x0 = run.iter().map(|l| l.rect[0]).fold(f64::INFINITY, f64::min);
            let x1 = run
                .iter()
                .map(|l| l.rect[2])
                .fold(f64::NEG_INFINITY, f64::max);
            let y0 = run.iter().map(|l| l.rect[1]).fold(f64::INFINITY, f64::min);
            let size = run
                .iter()
                .map(|l| l.rect[3] - l.rect[1])
                .fold(0.0, f64::max);
            let rect = [x0, y0, x1, y0 + (size * 1.2).clamp(12.0, 24.0)];
            found.push(Found {
                page,
                kind: NewField::Text,
                rect,
                label: None,
            });
        }
        run.clear();
    };
    for letter in letters {
        match letter.text.as_str() {
            "_" => {
                let joins = run
                    .last()
                    .is_none_or(|last| (letter.rect[0] - last.rect[2]).abs() < 2.0);
                if !joins {
                    flush(&mut run, &mut found);
                }
                run.push(letter);
            }
            "☐" | "□" => {
                flush(&mut run, &mut found);
                found.push(Found {
                    page,
                    kind: NewField::CheckBox,
                    rect: letter.rect,
                    label: None,
                });
            }
            _ => flush(&mut run, &mut found),
        }
    }
    flush(&mut run, &mut found);
    found
}

/// Rules to write on, and boxes to tick or to write in.
fn from_shapes(page: PageIndex, shapes: &[Shape], letters: &[Letter], width: f64) -> Vec<Found> {
    let mut found = Vec::new();
    for shape in shapes {
        for rect in &shape.rects {
            let [x0, y0, x1, y1] = *rect;
            let (w, h) = (x1 - x0, y1 - y0);
            if shape.stroked
                && BOX_SIZE.contains(&w)
                && BOX_SIZE.contains(&h)
                && (w / h - 1.0).abs() < 0.35
            {
                found.push(Found {
                    page,
                    kind: NewField::CheckBox,
                    rect: *rect,
                    label: None,
                });
            } else if shape.stroked && w >= MIN_RULE * 1.5 && (14.0..=40.0).contains(&h) {
                let inside = [x0 + 1.0, y0 + 1.0, x1 - 1.0, y1 - 1.0];
                if !written_in(letters, inside) {
                    found.push(Found {
                        page,
                        kind: NewField::Text,
                        rect: inside,
                        label: None,
                    });
                }
            } else if w >= MIN_RULE && h <= RULE_SLACK && shape.filled {
                found.extend(rule(page, *rect, letters, width));
            }
        }
        if shape.rects.is_empty() && shape.stroked {
            let [x0, y0, x1, y1] = shape.bounds();
            if x1 - x0 >= MIN_RULE && y1 - y0 <= RULE_SLACK {
                found.extend(rule(page, [x0, y0, x1, y1], letters, width));
            }
        }
    }
    found
}

/// A field standing on the rule at `rect`, unless something is written on
/// it or it runs the width of the page with no label: a separator.
fn rule(
    page: PageIndex,
    [x0, y0, x1, y1]: [f64; 4],
    letters: &[Letter],
    width: f64,
) -> Option<Found> {
    let line = (y0 + y1) / 2.0;
    let rect = [x0, line, x1, line + LINE_HEIGHT];
    if written_in(letters, [x0, line + 1.0, x1, line + LINE_HEIGHT]) {
        return None;
    }
    let spans_page = x1 - x0 > width * 0.8;
    if spans_page && label(letters, rect).is_none() {
        return None;
    }
    Some(Found {
        page,
        kind: NewField::Text,
        rect,
        label: None,
    })
}

/// Every place to fill in on `page`, labelled, with the ones that overlap a
/// field the page has, or one found before them, left out.
pub fn detect_page(doc: &mut Document, page: PageIndex) -> Result<Vec<Found>, CommandError> {
    let text = doc.page_text(page).map_err(failed(LABEL))?.clone();
    let letters = letters(&text);
    let shapes = {
        let structure = doc.structure().map_err(failed(LABEL))?;
        onionskin_content::page_shapes(structure, page).map_err(|error| CommandError::Failed {
            label: LABEL,
            reason: error.to_string(),
        })?
    };
    let width = doc
        .page_geometry(page)
        .map(|geometry| geometry.media_box[2] - geometry.media_box[0])
        .unwrap_or(612.0);
    let taken: Vec<[f64; 4]> = doc
        .form()
        .map_err(failed(LABEL))?
        .fields
        .iter()
        .flat_map(|field| &field.widgets)
        .filter(|widget| widget.page == Some(page))
        .map(|widget| widget.rect)
        .collect();
    let mut kept: Vec<Found> = Vec::new();
    let candidates = from_text(page, &letters)
        .into_iter()
        .chain(from_shapes(page, &shapes, &letters, width));
    for mut candidate in candidates {
        let clashes = taken
            .iter()
            .chain(kept.iter().map(|found| &found.rect))
            .any(|other| covered(*other, candidate.rect) > 0.3);
        if !clashes {
            candidate.label = label(&letters, candidate.rect);
            kept.push(candidate);
        }
    }
    Ok(kept)
}

/// Detect fields on every page and add them as one undo step. How many
/// were added.
pub fn detect_fields(doc: &mut Document) -> Result<usize, CommandError> {
    let mut found = Vec::new();
    for page in 0..doc.page_count() {
        found.extend(detect_page(doc, page)?);
    }
    if found.is_empty() {
        return Ok(0);
    }
    let form = doc.form().map_err(failed(LABEL))?;
    let names = names(&form, &found);
    doc.edit_annotations(LABEL, |tx, structure| {
        for (place, name) in found.iter().zip(&names) {
            add_named_field(
                tx,
                structure,
                &form,
                &place.kind,
                place.page,
                place.rect,
                Some(name),
            )?;
        }
        Ok(found.len())
    })
    .map_err(failed(LABEL))
}

/// A name for each place: its label, or its kind's numbered name, never
/// one the form or an earlier place has.
fn names(form: &Form, found: &[Found]) -> Vec<String> {
    let mut used: BTreeSet<String> = form.fields.iter().map(|field| field.name.clone()).collect();
    found
        .iter()
        .map(|place| {
            let base = place
                .label
                .clone()
                .unwrap_or_else(|| place.kind.base_name().to_owned());
            let name = if place.label.is_some() && !used.contains(&base) {
                base
            } else {
                (1..)
                    .map(|number| format!("{base}{number}"))
                    .find(|candidate| !used.contains(candidate))
                    .expect("a name is free")
            };
            used.insert(name.clone());
            name
        })
        .collect()
}
