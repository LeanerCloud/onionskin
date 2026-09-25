//! Turning two points on a page into a text selection.
//!
//! This lives in `core` rather than in the tool that first needed it because
//! two tools need it: `tools-basic`'s Select Text, and every text-markup tool
//! in `tools-comment`, which has to build the same selection from its own drag
//! rather than waiting for someone else's. A second copy would be a second
//! place for the glyph-ordering rules to drift.

use std::ops::RangeInclusive;

use onionskin_content::{Glyph, Mapping, PageText, SelectedRun, TextRun};

use crate::{PagePoint, PageQuad, TextSelection, TextSpan};

/// The selection between two points on one page, in the order the page drew
/// its glyphs.
///
/// `None` when the page has no glyphs to select, which is a blank page rather
/// than an error.
pub fn select_between(page: &PageText, from: PagePoint, to: PagePoint) -> Option<TextSelection> {
    let order = glyph_order(page);
    if order.is_empty() {
        return None;
    }
    let first = nearest_glyph(page, &order, from)?;
    let last = nearest_glyph(page, &order, to)?;
    Some(selection_for(
        page,
        &order,
        first.min(last)..=first.max(last),
    ))
}

/// Every glyph on the page as `(run, glyph)`, in the order the page drew
/// them. Unmapped glyphs are included: they have a position and a code, so
/// they can be selected even though nothing knows what letter they are.
pub fn glyph_order(page: &PageText) -> Vec<(usize, usize)> {
    page.runs
        .iter()
        .enumerate()
        .flat_map(|(run, text)| (0..text.glyphs.len()).map(move |glyph| (run, glyph)))
        .collect()
}

pub fn nearest_glyph(page: &PageText, order: &[(usize, usize)], at: PagePoint) -> Option<usize> {
    order
        .iter()
        .map(|&(run, glyph)| distance_squared(&page.runs[run].glyphs[glyph].quad, at))
        .enumerate()
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
}

/// Distance from a point to a glyph's bounding box, zero inside it. Keeping
/// the box rather than its centre is what makes a point past the end of a
/// line land on that line rather than on the nearest line above.
fn distance_squared(quad: &PageQuad, at: PagePoint) -> f64 {
    let (x0, x1) = min_max(quad.corners.map(|(x, _)| x));
    let (y0, y1) = min_max(quad.corners.map(|(_, y)| y));
    let dx = (x0 - at.x).max(0.0).max(at.x - x1);
    let dy = (y0 - at.y).max(0.0).max(at.y - y1);
    dx * dx + dy * dy
}

fn min_max(values: [f64; 4]) -> (f64, f64) {
    values
        .iter()
        .fold((f64::MAX, f64::MIN), |(min, max), value| {
            (min.min(*value), max.max(*value))
        })
}

/// The quads and the text for a run of glyphs. Touching an ActualText glyph
/// expands the whole occurrence; geometry and copied text use those members.
/// `order` must be `glyph_order(page)` for the same unchanged page, and
/// `range` must be valid for that order.
///
/// The text comes from flattening a copy of the page clipped to the
/// selection, so the separator rules that decide where a space or a line
/// break goes stay in `content` rather than being guessed at again here.
pub fn selection_for(
    page: &PageText,
    order: &[(usize, usize)],
    range: RangeInclusive<usize>,
) -> TextSelection {
    let selected = &order[range];
    let members = page.selection_members(selected);
    let quads = members
        .iter()
        .flat_map(|member| page.runs[member.run].glyphs[member.glyphs.clone()].iter())
        .map(|glyph| glyph.quad)
        .collect();
    let runs: Vec<TextRun> = members
        .into_iter()
        .map(|member: SelectedRun| {
            let run = &page.runs[member.run];
            if run.actual_text.is_some() {
                run.clone()
            } else {
                clip_run(run, member.glyphs.start, member.glyphs.end - 1)
            }
        })
        .collect();

    let clipped = PageText {
        page: page.page,
        runs,
        warnings: Vec::new(),
    };
    let (text, spans) = styled_text(&clipped);
    TextSelection {
        page: page.page,
        quads,
        text,
        spans,
    }
}

/// A page's flattened text, and the same text cut into spans of one face
/// and size. A separator the join added belongs to the span before it, so
/// the spans joined are exactly the text.
pub fn styled_text(page: &PageText) -> (String, Vec<TextSpan>) {
    let flattened = page.flatten();
    let mut spans: Vec<TextSpan> = Vec::new();
    let mut at = 0;
    for piece in flattened.pieces() {
        let range = &piece.range;
        let index = piece.style_run;
        let run = &page.runs[index];
        if range.start > at {
            if let Some(last) = spans.last_mut() {
                last.text.push_str(&flattened.text[at..range.start]);
            }
        }
        let piece = &flattened.text[range.clone()];
        match spans.last_mut() {
            Some(last) if last.font == run.font_name && last.size == run.size => {
                last.text.push_str(piece);
            }
            _ => spans.push(TextSpan {
                text: piece.to_owned(),
                font: run.font_name.clone(),
                size: run.size,
            }),
        }
        at = range.end;
    }
    (flattened.text, spans)
}

/// A copy of `run` holding only glyphs `first..=last` and the text they
/// produced, with the glyph mappings rebased onto that text.
fn clip_run(run: &TextRun, first: usize, last: usize) -> TextRun {
    let glyphs = &run.glyphs[first..=last];
    let start = glyphs.iter().filter_map(mapped_start).min();
    let end = glyphs.iter().filter_map(mapped_end).max();
    let (text, base) = match (start, end) {
        // Every glyph unmapped, or a mapping that does not name a slice of
        // this run's text: keep the quads, claim no characters.
        (Some(start), Some(end)) => match run.decoded_text.get(start..end) {
            Some(text) => (text.to_string(), start),
            None => (String::new(), 0),
        },
        _ => (String::new(), 0),
    };

    TextRun {
        decoded_text: text,
        glyphs: glyphs
            .iter()
            .map(|glyph| Glyph {
                mapping: match &glyph.mapping {
                    Mapping::Text(range) => Mapping::Text(
                        range.start.saturating_sub(base)..range.end.saturating_sub(base),
                    ),
                    Mapping::Unmapped => Mapping::Unmapped,
                },
                ..glyph.clone()
            })
            .collect(),
        ..run.clone()
    }
}

fn mapped_start(glyph: &Glyph) -> Option<usize> {
    match &glyph.mapping {
        Mapping::Text(range) => Some(range.start),
        Mapping::Unmapped => None,
    }
}

fn mapped_end(glyph: &Glyph) -> Option<usize> {
    match &glyph.mapping {
        Mapping::Text(range) => Some(range.end),
        Mapping::Unmapped => None,
    }
}
