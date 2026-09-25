//! A page's text as lines: what text editing works on.
//!
//! A line is the runs drawn one after another along one baseline, close
//! enough to read as one piece of text. Runs join a line under the rule
//! `search::flatten` uses for its separators (the same baseline, and a gap
//! forward of less than a column's width), with one addition: a jump of more
//! than three line heights forward starts a new line. Editing also splits
//! between ordinary and ActualText-protected runs, so protected replacement
//! text and ordinary text are never rewritten as one editable target.

use std::ops::Range;

use crate::run::{Mapping, PageText, TextRun};
use crate::{PageIndex, PageQuad};

/// A gap forward of more than this many line heights starts a new line.
const COLUMN_GAP: f64 = 3.0;

/// One glyph of a line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineGlyph {
    /// `(run, glyph)` in the page's extraction order.
    pub at: (usize, usize),
    pub quad: PageQuad,
    /// What it spelled in [`TextLine::text`], or `None` when unmapped or
    /// ActualText-protected.
    pub range: Option<Range<usize>>,
}

/// A line of text on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub page: PageIndex,
    pub text: String,
    pub glyphs: Vec<LineGlyph>,
}

impl TextLine {
    /// Whether every glyph maps to editable decoded text. ActualText-protected
    /// and unmapped glyphs return false.
    pub fn is_mapped(&self) -> bool {
        self.glyphs.iter().all(|glyph| glyph.range.is_some())
    }

    /// The smallest page rectangle holding every glyph.
    pub fn bounds(&self) -> [f64; 4] {
        let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for (x, y) in self.glyphs.iter().flat_map(|glyph| glyph.quad.corners) {
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
        bounds
    }

    /// Whether `(x, y)` falls on the line.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        let [x0, y0, x1, y1] = self.bounds();
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }

    /// The glyphs whose text overlaps `range` of [`TextLine::text`].
    pub fn quads_for(&self, range: &Range<usize>) -> Vec<PageQuad> {
        self.glyphs
            .iter()
            .filter(|glyph| {
                glyph
                    .range
                    .as_ref()
                    .is_some_and(|r| r.start < range.end && range.start < r.end)
            })
            .map(|glyph| glyph.quad)
            .collect()
    }
}

/// `page`'s lines, in the order the page drew them.
pub fn text_lines(page: &PageText) -> Vec<TextLine> {
    let mut lines: Vec<TextLine> = Vec::new();
    let mut previous: Option<&TextRun> = None;
    for (index, run) in page.runs.iter().enumerate() {
        if run.glyphs.is_empty() {
            continue;
        }
        match previous.map(|previous| join(previous, run)) {
            Some(Some(separator)) => {
                let line = lines.last_mut().expect("a line is open");
                line.text.push_str(separator);
                append(line, index, run);
            }
            _ => {
                let mut line = TextLine {
                    page: page.page,
                    text: String::new(),
                    glyphs: Vec::new(),
                };
                append(&mut line, index, run);
                lines.push(line);
            }
        }
        previous = Some(run);
    }
    lines
}

fn append(line: &mut TextLine, index: usize, run: &TextRun) {
    let start = line.text.len();
    line.text.push_str(&run.decoded_text);
    for (glyph_index, glyph) in run.glyphs.iter().enumerate() {
        line.glyphs.push(LineGlyph {
            at: (index, glyph_index),
            quad: glyph.quad,
            range: match (&run.actual_text, &glyph.mapping) {
                (Some(_), _) => None,
                (None, Mapping::Text(range)) => Some(start + range.start..start + range.end),
                (None, Mapping::Unmapped) => None,
            },
        });
    }
}

/// What joins `next` to the line `previous` ended, or `None` when `next`
/// starts a new line.
fn join(previous: &TextRun, next: &TextRun) -> Option<&'static str> {
    if previous.actual_text.is_some() != next.actual_text.is_some() {
        return None;
    }
    let (last, first) = (previous.glyphs.last()?, next.glyphs.first()?);
    let from = last.quad.corners[3];
    let to = first.quad.corners[2];
    let direction = (
        last.quad.corners[3].0 - last.quad.corners[2].0,
        last.quad.corners[3].1 - last.quad.corners[2].1,
    );
    let height = (last.quad.corners[0].0 - last.quad.corners[2].0)
        .hypot(last.quad.corners[0].1 - last.quad.corners[2].1);
    if height <= 0.0 || !height.is_finite() {
        return None;
    }
    let length = direction.0.hypot(direction.1);
    let unit = if length > 0.0 {
        (direction.0 / length, direction.1 / length)
    } else {
        (1.0, 0.0)
    };
    let delta = (to.0 - from.0, to.1 - from.1);
    let along = delta.0 * unit.0 + delta.1 * unit.1;
    let across = -delta.0 * unit.1 + delta.1 * unit.0;
    if across.abs() > 0.5 * height || along > COLUMN_GAP * height || along < -0.5 * height {
        return None;
    }
    let spaced = previous.decoded_text.ends_with(char::is_whitespace)
        || next.decoded_text.starts_with(char::is_whitespace);
    Some(if along > 0.2 * height && !spaced {
        " "
    } else {
        ""
    })
}
