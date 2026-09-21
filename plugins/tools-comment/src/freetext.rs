//! The three `/FreeText` tools: typewriter, text box and callout.
//!
//! All three write a `/FreeText` and differ in `/IT` and in what the gesture
//! means, so they share one implementation for the same reason the five markup
//! tools do.
//!
//! **The font is named, never embedded.** A `/DA` string and the appearance
//! stream both come from one [`TextStyle`], which is what stops the two
//! disagreeing - the disagreement that makes a text box render one way in
//! Acrobat and another in a reader that trusts `/AP`. No font file is written
//! anywhere: `/Helv` is a standard name every reader substitutes for, and
//! shipping Adobe's font files is what `PLAN.md`'s legal posture rule 6
//! forbids.

use onionskin_core::{
    add_annotation, Annotation, Color, Intent, PagePoint, PageRect, Rect, TextStyle, Viewport,
};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::place::{now, page_object};

/// Below this, in view pixels, a drag is a click. Expressed in view pixels so
/// it means the same thing at every zoom.
const MIN_DRAG_PIXELS: f32 = 3.0;

/// The box a click gets, in page units, when the user did not drag one out.
/// Wide enough for a few words at the default size, which is what a typewriter
/// click is for.
const DEFAULT_BOX: (f64, f64) = (180.0, 24.0);

/// How far the text box sits from the point a callout points at, in page
/// units, when the user clicks rather than dragging the box out.
const CALLOUT_OFFSET: (f64, f64) = (60.0, 40.0);

/// What the gesture means for this tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// Text on the page. A click places it; a drag sizes it.
    Typewriter,
    /// A bordered box. A drag sizes it; a click gets the default size.
    Box,
    /// A bordered box with a leader. The drag **starts at what the callout
    /// points at** and ends where the box goes, which is the order Acrobat
    /// asks for and the only one that makes a single drag enough.
    Callout,
}

pub struct FreeTextTool {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    shortcut: Option<&'static str>,
    shape: Shape,
    style: TextStyle,
    color: Color,
    anchor: Option<PagePoint>,
    at: Option<PagePoint>,
    dragging: bool,
}

impl FreeTextTool {
    pub fn typewriter() -> Self {
        Self::new(
            "typewriter",
            "Add Text Comment",
            "typewriter",
            Some("t"),
            Shape::Typewriter,
        )
    }

    pub fn text_box() -> Self {
        Self::new("text-box", "Text Box", "text-box", None, Shape::Box)
    }

    pub fn callout() -> Self {
        Self::new("callout", "Callout", "callout", None, Shape::Callout)
    }

    fn new(
        id: &'static str,
        name: &'static str,
        icon: &'static str,
        shortcut: Option<&'static str>,
        shape: Shape,
    ) -> Self {
        FreeTextTool {
            id,
            name,
            icon,
            shortcut,
            shape,
            style: TextStyle::default(),
            color: Color::BLACK,
            anchor: None,
            at: None,
            dragging: false,
        }
    }

    /// The text box this gesture produces, in page coordinates.
    ///
    /// A gesture that stayed a click gets the default box: for a typewriter and
    /// a text box it hangs from the click, and for a callout it sits off to one
    /// side of the point being called out, because a box on top of what it
    /// points at is a box the leader cannot reach.
    fn text_rect(&self, viewport: &Viewport) -> Option<Rect> {
        let anchor = self.anchor?;
        let at = self.at?;
        let (width, height) = DEFAULT_BOX;
        if !is_drag(anchor, at, viewport) {
            let corner = match self.shape {
                Shape::Callout => PagePoint {
                    page: anchor.page,
                    x: anchor.x + CALLOUT_OFFSET.0,
                    y: anchor.y + CALLOUT_OFFSET.1,
                },
                _ => anchor,
            };
            return Some(Rect::new(
                corner.x,
                corner.y - height,
                corner.x + width,
                corner.y,
            ));
        }
        match self.shape {
            // The drag's start is what the callout points at, not a corner of
            // the box, so the box hangs off the drag's end.
            Shape::Callout => Some(Rect::new(at.x, at.y - height, at.x + width, at.y)),
            _ => Some(Rect::new(
                anchor.x.min(at.x),
                anchor.y.min(at.y),
                anchor.x.max(at.x),
                anchor.y.max(at.y),
            )),
        }
    }

    /// `/CL`: the tail at what the callout points at, a knee, and the end on
    /// the edge of the box nearest the tail.
    ///
    /// Three points rather than two because that is what Acrobat writes and
    /// what a reader draws an elbow from; a two-point leader is legal and looks
    /// like a stray line.
    fn leader(tail: PagePoint, rect: Rect) -> Vec<(f64, f64)> {
        let landing = if tail.x <= rect.x0 {
            (rect.x0, rect.y0 + rect.height() / 2.0)
        } else if tail.x >= rect.x1 {
            (rect.x1, rect.y0 + rect.height() / 2.0)
        } else if tail.y <= rect.y0 {
            (rect.x0 + rect.width() / 2.0, rect.y0)
        } else {
            (rect.x0 + rect.width() / 2.0, rect.y1)
        };
        let knee = (
            (tail.x + landing.0) / 2.0,
            // The elbow turns level with the landing, which is what makes the
            // last segment horizontal into the side of a box.
            landing.1,
        );
        vec![(tail.x, tail.y), knee, landing]
    }

    fn commit(&mut self, ctx: &mut ToolCtx) {
        let Some(rect) = self.text_rect(ctx.viewport) else {
            return;
        };
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let Some(anchor) = self.anchor.take() else {
            return;
        };
        self.at = None;
        let Some(page) = page_object(ctx.doc, anchor.page) else {
            return;
        };
        let intent = match self.shape {
            Shape::Typewriter => Some(Intent::FreeTextTypewriter),
            Shape::Box => None,
            Shape::Callout => Some(Intent::FreeTextCallout),
        };
        let mut annotation = Annotation::free_text(rect, self.style, intent);
        annotation.color = Some(self.color);
        if self.shape == Shape::Callout {
            annotation.callout = FreeTextTool::leader(anchor, rect);
        }
        // A typewriter has no box, so it has no border either: a reader that
        // draws its own frame from `/BS` would give it one.
        if self.shape == Shape::Typewriter {
            annotation.border_width = 0.0;
        }

        let label = self.name;
        let _ = ctx.doc.edit_annotations(label, |tx, structure| {
            add_annotation(tx, structure, page, &annotation, now()).map(|_| ())
        });
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

impl ToolPlugin for FreeTextTool {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn icon(&self) -> &'static str {
        self.icon
    }

    fn shortcut(&self) -> Option<&'static str> {
        self.shortcut
    }

    fn group(&self) -> &'static str {
        "freetext"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Comment]
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.anchor = Some(input.at);
        self.at = Some(input.at);
        self.dragging = true;
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        if self.dragging && self.anchor.is_some_and(|a| a.page == input.at.page) {
            self.at = Some(input.at);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if !self.dragging {
            return;
        }
        if self.anchor.is_some_and(|a| a.page == input.at.page) {
            self.at = Some(input.at);
        }
        self.dragging = false;
        self.commit(ctx);
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.commit(ctx);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.dragging = false;
        self.anchor = None;
        self.at = None;
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    /// The box as it will be written, and a callout's leader with it, so the
    /// user sees where the text will land before it is there.
    fn overlays(&self, _doc: &onionskin_core::Document) -> Vec<Overlay> {
        if !self.dragging {
            return Vec::new();
        }
        // The preview needs the same viewport the commit will use, and
        // `overlays` is not given one, so a gesture that has not yet passed the
        // drag threshold previews its dragged-out box rather than the default
        // one. That is the shape under the pointer either way.
        let (Some(anchor), Some(at)) = (self.anchor, self.at) else {
            return Vec::new();
        };
        let rect = match self.shape {
            Shape::Callout => {
                let (width, height) = DEFAULT_BOX;
                Rect::new(at.x, at.y - height, at.x + width, at.y)
            }
            _ => Rect::new(
                anchor.x.min(at.x),
                anchor.y.min(at.y),
                anchor.x.max(at.x),
                anchor.y.max(at.y),
            ),
        };
        let bounds = PageRect {
            page: anchor.page,
            x0: rect.x0,
            y0: rect.y0,
            x1: rect.x1,
            y1: rect.y1,
        };
        let mut overlays = vec![Overlay::Rect(bounds)];
        if self.shape == Shape::Callout {
            overlays.push(Overlay::Polyline {
                points: FreeTextTool::leader(anchor, rect)
                    .into_iter()
                    .map(|(x, y)| PagePoint {
                        page: anchor.page,
                        x,
                        y,
                    })
                    .collect(),
                closed: false,
            });
        }
        overlays
    }
}
