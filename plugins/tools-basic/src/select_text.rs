//! Text selection: a drag across glyphs, in document order.

use std::ops::RangeInclusive;

use onionskin_core::{
    Document, Glyph, Mapping, PagePoint, PageQuad, PageText, TextRun, TextSelection,
};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::marquee::is_drag;

/// Selects the glyphs between the point the drag started on and the point
/// it is over, in the order the page drew them. Reading order is document
/// order, the same order `content` extracts and searches in.
///
/// Selection is per page in M2: `content` extracts one page at a time, so a
/// drag that leaves the page it started on selects to that page's end
/// rather than reaching into the next one.
#[derive(Debug, Default)]
pub struct SelectTextTool {
    /// The fixed end of the selection. It outlives the drag so a later
    /// shift-click can extend from it.
    anchor: Option<PagePoint>,
    dragging: bool,
    quads: Vec<PageQuad>,
}

impl SelectTextTool {
    pub fn new() -> Self {
        Self::default()
    }

    fn extend_to(&mut self, ctx: &mut ToolCtx, at: PagePoint) {
        let Some(anchor) = self.anchor else {
            return;
        };
        if !is_drag(anchor, at, ctx.viewport) {
            self.clear(ctx);
            return;
        }
        let Ok(page) = ctx.doc.page_text(anchor.page) else {
            return;
        };
        let order = glyph_order(page);
        let Some(from) = nearest_glyph(page, &order, anchor) else {
            return;
        };
        let to = if at.page == anchor.page {
            match nearest_glyph(page, &order, at) {
                Some(index) => index,
                None => return,
            }
        } else if at.page > anchor.page {
            order.len() - 1
        } else {
            0
        };
        let selection = selection_for(page, &order, from.min(to)..=from.max(to));
        self.quads = selection.quads.clone();
        ctx.doc.selection_mut().set_text(selection);
    }

    fn clear(&mut self, ctx: &mut ToolCtx) {
        self.quads.clear();
        ctx.doc.selection_mut().clear();
    }
}

impl ToolPlugin for SelectTextTool {
    fn id(&self) -> &'static str {
        "select-text"
    }

    fn name(&self) -> &'static str {
        "Select Text"
    }

    fn icon(&self) -> &'static str {
        "select-text"
    }

    fn shortcut(&self) -> Option<&'static str> {
        Some("v")
    }

    fn group(&self) -> &'static str {
        "select"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Select]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.dragging = true;
        let extends = input.modifiers.shift
            && self
                .anchor
                .is_some_and(|anchor| anchor.page == input.at.page);
        if extends {
            self.extend_to(ctx, input.at);
            return;
        }
        self.anchor = Some(input.at);
        self.clear(ctx);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.dragging {
            self.extend_to(ctx, input.at);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.dragging {
            self.extend_to(ctx, input.at);
        }
        self.dragging = false;
    }

    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.dragging = false;
        self.anchor = None;
        self.clear(ctx);
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        if self.quads.is_empty() {
            Vec::new()
        } else {
            vec![Overlay::Quads(self.quads.clone())]
        }
    }
}

/// Every glyph on the page as `(run, glyph)`, in the order the page drew
/// them. Unmapped glyphs are included: they have a position and a code, so
/// they can be selected even though nothing knows what letter they are.
fn glyph_order(page: &PageText) -> Vec<(usize, usize)> {
    page.runs
        .iter()
        .enumerate()
        .flat_map(|(run, text)| (0..text.glyphs.len()).map(move |glyph| (run, glyph)))
        .collect()
}

fn nearest_glyph(page: &PageText, order: &[(usize, usize)], at: PagePoint) -> Option<usize> {
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
fn selection_for(
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
