//! The seven shape tools: line, arrow, rectangle, oval, polygon, connected
//! lines and cloud.
//!
//! One tool type, one defaults struct, one commit path and one call into
//! `core`'s appearance generator. Seven small renderers would be seven places
//! for a default to drift and seven chances to write a shape a reader draws
//! differently, and the differences between these seven are entirely in the
//! geometry a gesture produces and the subtype it is written as.
//!
//! **An arrow is a `/Line` with `/LE`.** Acrobat has no arrow subtype; a
//! `/Polygon` shaped like one renders in Acrobat as a line with no head. A
//! cloud is likewise a `/Polygon` with a `/BE` border effect rather than a
//! subtype of its own.
//!
//! **The preview is the committed shape.** An oval is an ellipse inscribed in
//! the dragged rectangle, so its preview is `Overlay::Ellipse` over the same
//! rectangle; a polygon's preview is closed because the commit closes it. That
//! is what `Overlay`'s `Ellipse` and `closed` exist for.

use onionskin_core::{
    add_annotation, Annotation, BorderEffect, Color, Document, LineEnding, PagePoint, PageRect,
    Rect, Subtype, Viewport,
};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::place::{now, page_object};

/// Below this, in view pixels, a drag is a click: it makes no shape.
const MIN_DRAG_PIXELS: f64 = 3.0;

/// Within this, in view pixels, a click on a polygon's first vertex closes it
/// rather than adding a vertex on top of it.
const CLOSE_PIXELS: f64 = 8.0;

/// What a gesture means, and what it is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// Two points: the drag's ends.
    Line { arrow: bool },
    /// The dragged rectangle, as `/Square` or `/Circle`.
    Boxed(Subtype),
    /// Vertices collected by clicking, committed by Enter. `closed` picks
    /// `/Polygon` over `/PolyLine`, and `cloudy` adds the `/BE`.
    Vertices { closed: bool, cloudy: bool },
}

/// What every shape is created with, in one place so seven tools cannot
/// disagree about what a default looks like.
#[derive(Clone, Copy, Debug)]
struct Defaults {
    color: Color,
    interior: Option<Color>,
    border_width: f64,
    /// Acrobat's cloud intensity: 0, 1 or 2.
    cloud_intensity: f64,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            color: Color::new(0.85, 0.16, 0.16),
            interior: None,
            border_width: 2.0,
            cloud_intensity: 1.0,
        }
    }
}

pub struct ShapeTool {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    shape: Shape,
    defaults: Defaults,
    /// A drag's fixed end, or the page a vertex run belongs to.
    anchor: Option<PagePoint>,
    at: Option<PagePoint>,
    dragging: bool,
    /// Collected vertices, for the three that are built by clicking.
    vertices: Vec<PagePoint>,
}

impl ShapeTool {
    pub fn line() -> Self {
        Self::new("line", "Line", "line", Shape::Line { arrow: false })
    }

    pub fn arrow() -> Self {
        Self::new("arrow", "Arrow", "arrow", Shape::Line { arrow: true })
    }

    pub fn rectangle() -> Self {
        Self::new(
            "rectangle",
            "Rectangle",
            "rectangle",
            Shape::Boxed(Subtype::Square),
        )
    }

    pub fn oval() -> Self {
        Self::new("oval", "Oval", "oval", Shape::Boxed(Subtype::Circle))
    }

    pub fn polygon() -> Self {
        Self::new(
            "polygon",
            "Polygon",
            "polygon",
            Shape::Vertices {
                closed: true,
                cloudy: false,
            },
        )
    }

    pub fn connected_lines() -> Self {
        Self::new(
            "polyline",
            "Connected Lines",
            "polyline",
            Shape::Vertices {
                closed: false,
                cloudy: false,
            },
        )
    }

    pub fn cloud() -> Self {
        Self::new(
            "cloud",
            "Cloud",
            "cloud",
            Shape::Vertices {
                closed: true,
                cloudy: true,
            },
        )
    }

    fn new(id: &'static str, name: &'static str, icon: &'static str, shape: Shape) -> Self {
        ShapeTool {
            id,
            name,
            icon,
            shape,
            defaults: Defaults::default(),
            anchor: None,
            at: None,
            dragging: false,
            vertices: Vec::new(),
        }
    }

    fn builds_by_clicking(&self) -> bool {
        matches!(self.shape, Shape::Vertices { .. })
    }

    /// The rectangle a drag encloses, in page coordinates.
    fn dragged_rect(&self) -> Option<Rect> {
        let (anchor, at) = (self.anchor?, self.at?);
        Some(Rect::new(anchor.x, anchor.y, at.x, at.y))
    }

    /// The annotation this gesture produces, or `None` when it produces none.
    fn annotation(&self, viewport: &Viewport) -> Option<Annotation> {
        let mut annotation = match self.shape {
            Shape::Line { arrow } => {
                let (anchor, at) = (self.anchor?, self.at?);
                if !is_drag(anchor, at, viewport) {
                    return None;
                }
                let mut annotation =
                    Annotation::new(Subtype::Line, Rect::new(anchor.x, anchor.y, at.x, at.y));
                annotation.line = Some(((anchor.x, anchor.y), (at.x, at.y)));
                if arrow {
                    // The head is on the end the drag finished at, which is
                    // where the user was pointing.
                    annotation.endings = Some((LineEnding::None, LineEnding::ClosedArrow));
                }
                annotation
            }
            Shape::Boxed(subtype) => {
                let (anchor, at) = (self.anchor?, self.at?);
                if !is_drag(anchor, at, viewport) {
                    return None;
                }
                Annotation::new(subtype, self.dragged_rect()?)
            }
            Shape::Vertices { closed, cloudy } => {
                let points: Vec<(f64, f64)> = self
                    .vertices
                    .iter()
                    .map(|point| (point.x, point.y))
                    .collect();
                let subtype = if closed {
                    Subtype::Polygon
                } else {
                    Subtype::PolyLine
                };
                let mut annotation = Annotation::vertices(subtype, points)?;
                if cloudy {
                    annotation.border_effect = Some(BorderEffect::Cloudy {
                        intensity: self.defaults.cloud_intensity,
                    });
                }
                annotation
            }
        };
        annotation.color = Some(self.defaults.color);
        annotation.interior_color = self.defaults.interior;
        annotation.border_width = self.defaults.border_width;
        Some(annotation)
    }

    /// The one commit path. Every shape goes through it, so the transaction,
    /// the label and the appearance call are written once.
    fn commit(&mut self, ctx: &mut ToolCtx) {
        let Some(annotation) = self.annotation(ctx.viewport) else {
            self.reset();
            return;
        };
        let page = self
            .anchor
            .or_else(|| self.vertices.first().copied())
            .map(|point| point.page);
        self.reset();
        let Some(page) = page else {
            return;
        };
        let Some(page) = page_object(ctx.doc, page) else {
            return;
        };
        let label = self.name;
        let _ = ctx.doc.edit_annotations(label, |tx, structure| {
            add_annotation(tx, structure, page, &annotation, now()).map(|_| ())
        });
    }

    fn reset(&mut self) {
        self.anchor = None;
        self.at = None;
        self.dragging = false;
        self.vertices.clear();
    }

    fn preview_points(&self) -> Vec<PagePoint> {
        let mut points = self.vertices.clone();
        if let Some(at) = self.at {
            // The vertex the pointer is over is part of the shape the user is
            // looking at, even though it has not been clicked yet.
            points.push(at);
        }
        points
    }
}

fn is_drag(from: PagePoint, to: PagePoint, viewport: &Viewport) -> bool {
    view_distance(from, to, viewport).is_some_and(|distance| distance >= MIN_DRAG_PIXELS)
}

fn view_distance(from: PagePoint, to: PagePoint, viewport: &Viewport) -> Option<f64> {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return None;
    };
    Some(f64::from((to.x - from.x).hypot(to.y - from.y)))
}

impl ToolPlugin for ShapeTool {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn icon(&self) -> &'static str {
        self.icon
    }

    fn group(&self) -> &'static str {
        "shape"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Comment]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if !self.builds_by_clicking() {
            self.anchor = Some(input.at);
            self.at = Some(input.at);
            self.dragging = true;
            return;
        }
        // A click on the first vertex closes the shape, which is how a polygon
        // ends without a keyboard.
        if let Some(first) = self.vertices.first().copied() {
            if first.page == input.at.page
                && view_distance(first, input.at, ctx.viewport)
                    .is_some_and(|distance| distance <= CLOSE_PIXELS)
            {
                self.at = None;
                self.commit(ctx);
                return;
            }
        }
        if self
            .vertices
            .first()
            .is_some_and(|first| first.page != input.at.page)
        {
            return;
        }
        self.vertices.push(input.at);
        self.at = Some(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        if self.builds_by_clicking() {
            if self
                .vertices
                .first()
                .is_none_or(|first| first.page == input.at.page)
            {
                self.at = Some(input.at);
            }
            return;
        }
        if self.dragging && self.anchor.is_some_and(|a| a.page == input.at.page) {
            self.at = Some(input.at);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.builds_by_clicking() || !self.dragging {
            return;
        }
        if self.anchor.is_some_and(|a| a.page == input.at.page) {
            self.at = Some(input.at);
        }
        self.dragging = false;
        self.commit(ctx);
    }

    /// Enter finishes a shape built by clicking. For the others the gesture
    /// has already committed, and there is nothing pending.
    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        if !self.builds_by_clicking() {
            return;
        }
        self.at = None;
        self.commit(ctx);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.reset();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    /// The shape as it will be written. An oval previews as an ellipse over
    /// the dragged rectangle and a polygon previews closed, because that is
    /// what each commits.
    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        match self.shape {
            Shape::Line { .. } => {
                let (Some(from), Some(to)) = (self.anchor, self.at) else {
                    return Vec::new();
                };
                if !self.dragging {
                    return Vec::new();
                }
                vec![Overlay::Line { from, to }]
            }
            Shape::Boxed(subtype) => {
                if !self.dragging {
                    return Vec::new();
                }
                let (Some(anchor), Some(rect)) = (self.anchor, self.dragged_rect()) else {
                    return Vec::new();
                };
                let bounds = PageRect {
                    page: anchor.page,
                    x0: rect.x0,
                    y0: rect.y0,
                    x1: rect.x1,
                    y1: rect.y1,
                };
                vec![match subtype {
                    Subtype::Circle => Overlay::Ellipse { bounds },
                    _ => Overlay::Rect(bounds),
                }]
            }
            Shape::Vertices { closed, .. } => {
                let points = self.preview_points();
                if points.len() < 2 {
                    return Vec::new();
                }
                vec![Overlay::Polyline { points, closed }]
            }
        }
    }
}
