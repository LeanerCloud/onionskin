//! Turning two points on a page into a text selection.
//!
//! This lives in `core` rather than in the tool that first needed it because
//! two tools need it: `tools-basic`'s Select Text, and every text-markup tool
//! in `tools-comment`, which has to build the same selection from its own drag
//! rather than waiting for someone else's. A second copy would be a second
//! place for the glyph-ordering rules to drift.

use std::ops::RangeInclusive;

use onionskin_content::{Glyph, Mapping, PageText, TextRun};

use crate::{PagePoint, PageQuad, TextSelection};

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

/// The quads and the text for a run of glyphs.
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
    let mut quads = Vec::with_capacity(selected.len());
    let mut runs: Vec<TextRun> = Vec::new();
    let mut group: Option<(usize, usize, usize)> = None;

    for &(run, glyph) in selected {
        quads.push(page.runs[run].glyphs[glyph].quad);
        group = match group {
            Some((current, first, _)) if current == run => Some((run, first, glyph)),
            Some((current, first, last)) => {
                runs.push(clip_run(&page.runs[current], first, last));
                Some((run, glyph, glyph))
            }
            None => Some((run, glyph, glyph)),
        };
    }
    if let Some((run, first, last)) = group {
        runs.push(clip_run(&page.runs[run], first, last));
    }

    TextSelection {
        page: page.page,
        quads,
        text: PageText {
            page: page.page,
            runs,
            warnings: Vec::new(),
        }
        .flatten()
        .text,
    }
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
        (Some(start), Some(end)) => match run.text.get(start..end) {
            Some(text) => (text.to_string(), start),
            None => (String::new(), 0),
        },
        _ => (String::new(), 0),
    };

    TextRun {
        text,
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
