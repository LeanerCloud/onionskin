//! Circle and Line: drawn by a drag, or at a default size by a click.

use onionskin_core::{Annotation, Color, Document, PageRect, Rect, Subtype};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::gesture::{Gesture, Press};
use crate::{place, GROUP};

/// What a click gets: a circle round a word, a line under one.
const CIRCLE: (f64, f64) = (40.0, 20.0);
const LINE: f64 = 60.0;
/// Room round a line for its stroke, in points.
const PAD: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Circle,
    Line,
}

#[derive(Debug)]
pub struct ShapeTool {
    kind: Kind,
    press: Press,
}

impl ShapeTool {
    pub fn circle() -> Self {
        Self {
            kind: Kind::Circle,
            press: Press::default(),
        }
    }

    pub fn line() -> Self {
        Self {
            kind: Kind::Line,
            press: Press::default(),
        }
    }

    /// The annotation for a gesture.
    fn annotation(&self, gesture: Gesture) -> (usize, Annotation) {
        let (page, from, to) = match (gesture, self.kind) {
            (Gesture::Drag(from, to), _) => (from.page, (from.x, from.y), (to.x, to.y)),
            (Gesture::Click(at), Kind::Circle) => (
                at.page,
                (at.x - CIRCLE.0 / 2.0, at.y - CIRCLE.1 / 2.0),
                (at.x + CIRCLE.0 / 2.0, at.y + CIRCLE.1 / 2.0),
            ),
            (Gesture::Click(at), Kind::Line) => (at.page, (at.x, at.y), (at.x + LINE, at.y)),
        };
        let rect = Rect::new(
            from.0.min(to.0),
            from.1.min(to.1),
            from.0.max(to.0),
            from.1.max(to.1),
        );
        let mut annotation = match self.kind {
            Kind::Circle => Annotation::new(Subtype::Circle, rect),
            Kind::Line => {
                // A level line has no height: the box is grown round it so
                // the stroke has somewhere to be drawn.
                let grown = Rect::new(rect.x0 - PAD, rect.y0 - PAD, rect.x1 + PAD, rect.y1 + PAD);
                let mut line = Annotation::new(Subtype::Line, grown);
                line.line = Some((from, to));
                line
            }
        };
        annotation.color = Some(Color::BLACK);
        annotation.subject = Some("Fill & Sign".to_owned());
        (page, annotation)
    }
}

impl ToolPlugin for ShapeTool {
    fn id(&self) -> &'static str {
        match self.kind {
            Kind::Circle => "fill-sign.circle",
            Kind::Line => "fill-sign.line",
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Circle => "Circle",
            Kind::Line => "Line",
        }
    }

    fn icon(&self) -> &'static str {
        match self.kind {
            Kind::Circle => "fill-circle",
            Kind::Line => "fill-line",
        }
    }

    fn group(&self) -> &'static str {
        GROUP
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag to draw it, or click for one of the usual size.")
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::FillTextFields]
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.press.down(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.press.moved(input.at);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if let Some(gesture) = self.press.up(input.at, ctx.viewport) {
            let (page, annotation) = self.annotation(gesture);
            place(ctx.doc, page, self.name(), &annotation);
        }
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.press.cancel();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        let Some((from, to)) = self.press.span() else {
            return Vec::new();
        };
        if from == to {
            return Vec::new();
        }
        let bounds = PageRect {
            page: from.page,
            x0: from.x.min(to.x),
            y0: from.y.min(to.y),
            x1: from.x.max(to.x),
            y1: from.y.max(to.y),
        };
        vec![match self.kind {
            Kind::Circle => Overlay::Ellipse { bounds },
            Kind::Line => Overlay::Line { from, to },
        }]
    }
}
