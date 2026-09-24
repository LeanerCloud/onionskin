//! Redact Text & Images: drag across text to mark it, drag anywhere else to
//! mark a region, click a mark to open its properties.
//!
//! A drag that starts on a glyph selects text as the highlighter does; one
//! that starts on no text sweeps a rectangle. Every mark takes the look the
//! shell last set as the default, so Redaction Properties' "use as default"
//! carries to the next one.

use onionskin_core::redactions::RedactionLook;
use onionskin_core::textselect::select_between;
use onionskin_core::{Document, PagePoint, PageQuad, PageRect, Viewport};
use onionskin_plugin_api::{
    Overlay, PointerInput, ToolCapability, ToolCtx, ToolEnvironment, ToolPlugin,
};

use crate::mark::{mark_region, mark_text};

/// A drag shorter than this many viewport pixels is a click.
const MIN_DRAG_PIXELS: f32 = 3.0;

#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    /// Selecting text from a glyph: the quads so far.
    Text {
        anchor: PagePoint,
        quads: Vec<PageQuad>,
    },
    /// Sweeping a region from a point with no text under it.
    Region {
        anchor: PagePoint,
        rect: Option<PageRect>,
    },
}

#[derive(Debug, Default)]
pub struct RedactTool {
    gesture: Option<Gesture>,
    look: RedactionLook,
}

impl RedactTool {
    pub fn new() -> Self {
        Self::default()
    }

    fn follow(&mut self, doc: &mut Document, viewport: &Viewport, at: PagePoint) {
        match self.gesture.as_mut() {
            Some(Gesture::Text { anchor, quads }) => {
                *quads = match doc.page_text(anchor.page) {
                    Ok(page) if at.page == anchor.page && is_drag(*anchor, at, viewport) => {
                        select_between(page, *anchor, at)
                            .map(|selection| selection.quads)
                            .unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
            }
            Some(Gesture::Region { anchor, rect })
                if at.page == anchor.page && is_drag(*anchor, at, viewport) =>
            {
                *rect = Some(PageRect {
                    page: anchor.page,
                    x0: anchor.x.min(at.x),
                    y0: anchor.y.min(at.y),
                    x1: anchor.x.max(at.x),
                    y1: anchor.y.max(at.y),
                });
            }
            Some(Gesture::Region { .. }) | None => {}
        }
    }
}

fn is_drag(from: PagePoint, to: PagePoint, viewport: &Viewport) -> bool {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return false;
    };
    (to.x - from.x).hypot(to.y - from.y) >= MIN_DRAG_PIXELS
}

/// Whether a glyph on the page is under `at`.
fn over_text(doc: &mut Document, at: PagePoint) -> bool {
    let Ok(page) = doc.page_text(at.page) else {
        return false;
    };
    page.runs.iter().flat_map(|run| &run.glyphs).any(|glyph| {
        let xs = glyph.quad.corners.map(|(x, _)| x);
        let ys = glyph.quad.corners.map(|(_, y)| y);
        let (x0, x1) = (
            xs.iter().copied().fold(f64::INFINITY, f64::min),
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let (y0, y1) = (
            ys.iter().copied().fold(f64::INFINITY, f64::min),
            ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        (x0..=x1).contains(&at.x) && (y0..=y1).contains(&at.y)
    })
}

impl ToolPlugin for RedactTool {
    fn id(&self) -> &'static str {
        "redact.mark"
    }

    fn name(&self) -> &'static str {
        "Redact Text & Images"
    }

    fn icon(&self) -> &'static str {
        "redact"
    }

    fn group(&self) -> &'static str {
        "redact"
    }

    fn hint(&self) -> Option<&'static str> {
        Some(
            "Drag across text, or around a region, to mark it for redaction. Nothing is removed \
             until Apply Redactions.",
        )
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Redact]
    }

    fn configure(&mut self, environment: &ToolEnvironment) {
        self.look = environment
            .redaction
            .as_ref()
            .map(crate::look::look_of)
            .unwrap_or_default();
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let anchor = input.at;
        self.gesture = Some(if over_text(ctx.doc, anchor) {
            Gesture::Text {
                anchor,
                quads: Vec::new(),
            }
        } else {
            Gesture::Region { anchor, rect: None }
        });
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.follow(ctx.doc, ctx.viewport, input.at);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.follow(ctx.doc, ctx.viewport, input.at);
        match self.gesture.take() {
            Some(Gesture::Text { anchor, quads }) if !quads.is_empty() => {
                let _ = mark_text(ctx.doc, anchor.page, &quads, &self.look);
            }
            Some(Gesture::Region {
                rect: Some(rect), ..
            }) => {
                let _ = mark_region(
                    ctx.doc,
                    rect.page,
                    [rect.x0, rect.y0, rect.x1, rect.y1],
                    &self.look,
                );
            }
            _ => {
                let at = input.at;
                let hit = ctx.doc.redactions().ok().and_then(|marks| {
                    marks
                        .iter()
                        .rev()
                        .find(|mark| mark.page == at.page && mark.contains((at.x, at.y)))
                        .map(|mark| mark.objref)
                });
                if let Some(mark) = hit {
                    ctx.doc.request_redaction_properties(mark);
                }
            }
        }
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.gesture = None;
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        match &self.gesture {
            Some(Gesture::Text { quads, .. }) => quads
                .iter()
                .map(|quad| Overlay::Quads(vec![*quad]))
                .collect(),
            Some(Gesture::Region {
                rect: Some(rect), ..
            }) => vec![Overlay::AntsRect(*rect)],
            _ => Vec::new(),
        }
    }
}
