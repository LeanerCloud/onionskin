//! Draw (freehand ink) and Erase Ink.
//!
//! **Pressure is drawn, not stored.** Each point of a stroke keeps the pen
//! width its pressure gave it, and the appearance stream draws each segment at
//! its ends' mean width. PDF has no key for per-point width, which is why
//! Acrobat bakes pressure into `/AP` as well; `/InkList` carries the geometry.
//!
//! **One gesture, one undo entry.** A stroke is hundreds of pointer events and
//! one transaction, committed when the pointer lifts. So is an erase, however
//! many strokes and annotations it touches - and an erase that touches nothing
//! is no transaction at all.
//!
//! **Erasing splits.** The eraser removes the parts of a stroke under it and
//! keeps the rest as separate strokes of the same annotation, whose `/Rect` is
//! recomputed for what is left. An annotation with nothing left is removed
//! from its page. No stroke is ever written with no points.

use onionskin_core::{
    add_annotation, read_annotations, remove_annotation, set_ink_strokes, Annotation, Color,
    Document, PageIndex, PagePoint, PageRect, Subtype, Viewport,
};
use onionskin_plugin_api::{
    Overlay, PointerInput, ToolCapability, ToolCtx, ToolEnvironment, ToolPlugin,
};

use crate::place::{now, page_object, Signer};

/// Below this, in view pixels, a pointer move adds no point: a stylus reports
/// many more events than a stroke needs.
const MIN_STEP_PIXELS: f64 = 0.75;

/// The eraser's radius, in view pixels.
const ERASER_PIXELS: f64 = 8.0;

/// The pen width pressure `p` gives a pen whose full-pressure width is `base`.
/// A mouse reports 1.0 and draws at `base`; a light stylus touch draws at a
/// quarter of it, never at nothing.
pub(crate) fn pen_width(base: f64, pressure: f32) -> f64 {
    base * (0.25 + 0.75 * f64::from(pressure.clamp(0.0, 1.0)))
}

/// A stroke in progress.
#[derive(Default)]
struct Stroke {
    page: Option<PageIndex>,
    points: Vec<PagePoint>,
    widths: Vec<f64>,
}

impl Stroke {
    fn clear(&mut self) {
        *self = Stroke::default();
    }

    /// Add a point, unless it is on another page or too close to the last to
    /// matter at this zoom.
    fn push(&mut self, at: PagePoint, width: f64, viewport: &Viewport) {
        if self.page.is_some_and(|page| page != at.page) {
            return;
        }
        if let Some(last) = self.points.last() {
            if view_distance(*last, at, viewport).is_some_and(|distance| distance < MIN_STEP_PIXELS)
            {
                return;
            }
        }
        self.page = Some(at.page);
        self.points.push(at);
        self.widths.push(width);
    }
}

pub struct InkTool {
    color: Color,
    width: f64,
    stroke: Stroke,
    signer: Signer,
}

impl InkTool {
    pub fn new() -> Self {
        InkTool {
            color: Color::new(0.16, 0.34, 0.85),
            width: 3.0,
            stroke: Stroke::default(),
            signer: Signer::default(),
        }
    }

    fn commit(&mut self, doc: &mut Document) {
        let stroke = std::mem::take(&mut self.stroke);
        let Some(page) = stroke.page.and_then(|page| page_object(doc, page)) else {
            return;
        };
        let points = stroke
            .points
            .iter()
            .map(|point| (point.x, point.y))
            .collect();
        let Some(mut annotation) = Annotation::ink(vec![points], vec![stroke.widths], self.width)
        else {
            return;
        };
        annotation.color = Some(self.color);
        self.signer.sign(&mut annotation);
        let _ = doc.edit_annotations("Draw", |tx, structure| {
            add_annotation(tx, structure, page, &annotation, now()).map(|_| ())
        });
    }
}

impl Default for InkTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolPlugin for InkTool {
    fn configure(&mut self, environment: &ToolEnvironment) {
        self.signer.configure(environment);
    }

    fn id(&self) -> &'static str {
        "ink"
    }

    fn name(&self) -> &'static str {
        "Draw"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag on the page to draw freehand. Each stroke is one ink comment.")
    }

    fn icon(&self) -> &'static str {
        "ink"
    }

    fn group(&self) -> &'static str {
        "draw"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Draw]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.stroke.clear();
        self.stroke.push(
            input.at,
            pen_width(self.width, input.pressure),
            ctx.viewport,
        );
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.stroke.page.is_some() {
            self.stroke.push(
                input.at,
                pen_width(self.width, input.pressure),
                ctx.viewport,
            );
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.stroke.page.is_none() {
            return;
        }
        self.stroke.push(
            input.at,
            pen_width(self.width, input.pressure),
            ctx.viewport,
        );
        self.commit(ctx.doc);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.stroke.clear();
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.stroke.clear();
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        if self.stroke.points.is_empty() {
            return Vec::new();
        }
        vec![Overlay::Polyline {
            points: self.stroke.points.clone(),
            closed: false,
        }]
    }
}

pub struct EraseInkTool {
    path: Vec<PagePoint>,
    /// The eraser's radius in page units, fixed when the gesture starts so a
    /// zoom mid-gesture cannot change what one stroke of the eraser removes.
    radius: f64,
}

impl EraseInkTool {
    pub fn new() -> Self {
        EraseInkTool {
            path: Vec::new(),
            radius: 0.0,
        }
    }

    fn commit(&mut self, doc: &mut Document) {
        let path = std::mem::take(&mut self.path);
        let Some(page) = path.first().map(|point| point.page) else {
            return;
        };
        // Densified, like the strokes: a fast swipe reports few events, and the
        // eraser covers the path between them too.
        let points: Vec<(f64, f64)> = path.iter().map(|point| (point.x, point.y)).collect();
        let eraser = densify(&points, self.radius / 2.0);
        let changes = erase_on_page(doc, page, &eraser, self.radius);
        if changes.is_empty() {
            // Nothing under the eraser: no edit, and so no undo entry.
            return;
        }
        let Some(page) = page_object(doc, page) else {
            return;
        };
        let _ = doc.edit_annotations("Erase Ink", |tx, _| {
            for (annotation, strokes) in changes {
                if strokes.is_empty() {
                    remove_annotation(tx, page, annotation)?;
                } else {
                    set_ink_strokes(tx, annotation, strokes, now())?;
                }
            }
            Ok(())
        });
    }
}

impl Default for EraseInkTool {
    fn default() -> Self {
        Self::new()
    }
}

/// An ink annotation's strokes, each a list of points.
type Strokes = Vec<Vec<(f64, f64)>>;

/// What erasing along `eraser` does to each ink annotation on `page`: its new
/// strokes, empty for one with nothing left. Only annotations it touches.
fn erase_on_page(
    doc: &mut Document,
    page: PageIndex,
    eraser: &[(f64, f64)],
    radius: f64,
) -> Vec<(onionskin_core::ObjRef, Strokes)> {
    let count = doc.page_count();
    let Ok(current) = doc.structure() else {
        return Vec::new();
    };
    let Ok(annotations) = read_annotations(current, count, &Default::default()) else {
        return Vec::new();
    };
    annotations
        .into_iter()
        .filter(|annotation| annotation.page == page && annotation.subtype == Some(Subtype::Ink))
        .filter_map(|annotation| {
            erase_strokes(&annotation.ink, eraser, radius)
                .map(|strokes| (annotation.objref, strokes))
        })
        .collect()
}

/// The strokes left after erasing along `eraser`, or `None` if it touched
/// none of them. A stroke is densified first, so a long straight segment
/// crossed by the eraser between its ends is cut too.
pub(crate) fn erase_strokes(
    strokes: &[Vec<(f64, f64)>],
    eraser: &[(f64, f64)],
    radius: f64,
) -> Option<Strokes> {
    let mut touched = false;
    let mut left = Vec::new();
    for stroke in strokes {
        match erase_stroke(stroke, eraser, radius) {
            Some(pieces) => {
                touched = true;
                left.extend(pieces);
            }
            None => left.push(stroke.clone()),
        }
    }
    touched.then_some(left)
}

fn erase_stroke(stroke: &[(f64, f64)], eraser: &[(f64, f64)], radius: f64) -> Option<Strokes> {
    let under = |point: &(f64, f64)| {
        eraser
            .iter()
            .any(|centre| (point.0 - centre.0).hypot(point.1 - centre.1) <= radius)
    };
    let dense = densify(stroke, radius / 2.0);
    if !dense.iter().any(under) {
        return None;
    }
    let mut pieces: Strokes = Vec::new();
    let mut current = Vec::new();
    for point in dense {
        if under(&point) {
            if !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
            }
        } else {
            current.push(point);
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    Some(pieces)
}

/// `stroke` with points added so no two consecutive are further apart than
/// `step`.
fn densify(stroke: &[(f64, f64)], step: f64) -> Vec<(f64, f64)> {
    let Some(first) = stroke.first() else {
        return Vec::new();
    };
    let mut out = vec![*first];
    for pair in stroke.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        let steps = ((x1 - x0).hypot(y1 - y0) / step.max(f64::EPSILON))
            .ceil()
            .max(1.0) as usize;
        for index in 1..=steps {
            let t = index as f64 / steps as f64;
            out.push((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t));
        }
    }
    out
}

impl ToolPlugin for EraseInkTool {
    fn id(&self) -> &'static str {
        "erase-ink"
    }

    fn name(&self) -> &'static str {
        "Erase Ink"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag across a drawing to erase the part you cross.")
    }

    fn icon(&self) -> &'static str {
        "eraser"
    }

    fn group(&self) -> &'static str {
        "draw"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Draw]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.radius = ERASER_PIXELS / f64::from(ctx.viewport.zoom());
        self.path = vec![input.at];
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        if self
            .path
            .first()
            .is_some_and(|first| first.page == input.at.page)
        {
            self.path.push(input.at);
        }
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.path.is_empty() {
            return;
        }
        self.on_pointer_move(ctx, input);
        self.commit(ctx.doc);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.path.clear();
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        let Some(at) = self.path.last() else {
            return Vec::new();
        };
        let r = self.radius;
        vec![Overlay::Ellipse {
            bounds: PageRect {
                page: at.page,
                x0: at.x - r,
                y0: at.y - r,
                x1: at.x + r,
                y1: at.y + r,
            },
        }]
    }
}

fn view_distance(from: PagePoint, to: PagePoint, viewport: &Viewport) -> Option<f64> {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return None;
    };
    Some(f64::from((to.x - from.x).hypot(to.y - from.y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_scales_the_pen_and_never_to_nothing() {
        assert_eq!(pen_width(4.0, 1.0), 4.0);
        assert_eq!(pen_width(4.0, 0.0), 1.0);
        assert!(pen_width(4.0, 0.2) < pen_width(4.0, 0.8));
        assert_eq!(pen_width(4.0, 7.0), 4.0, "out-of-range pressure is clamped");
    }

    fn line(from: f64, to: f64) -> Vec<(f64, f64)> {
        vec![(from, 100.0), (to, 100.0)]
    }

    /// The middle of a two-point stroke is crossed by the eraser between its
    /// ends: densifying is what lets it cut there.
    #[test]
    fn erasing_the_middle_splits_the_stroke_in_two() {
        let left = erase_strokes(&[line(0.0, 100.0)], &[(50.0, 100.0)], 5.0).expect("touched");
        assert_eq!(left.len(), 2);
        assert!(left[0].iter().all(|(x, _)| *x < 45.1));
        assert!(left[1].iter().all(|(x, _)| *x > 54.9));
        assert!(
            left.iter().all(|stroke| !stroke.is_empty()),
            "no empty stroke"
        );
    }

    #[test]
    fn erasing_a_whole_stroke_leaves_the_others() {
        let strokes = [line(0.0, 4.0), line(200.0, 300.0)];
        let left = erase_strokes(&strokes, &[(2.0, 100.0)], 5.0).expect("touched");
        assert_eq!(left, [line(200.0, 300.0)]);
    }

    #[test]
    fn erasing_nothing_touches_nothing() {
        assert_eq!(
            erase_strokes(&[line(0.0, 100.0)], &[(50.0, 300.0)], 5.0),
            None
        );
    }

    #[test]
    fn erasing_everything_leaves_no_stroke_at_all() {
        let left = erase_strokes(&[line(0.0, 4.0)], &[(2.0, 100.0)], 5.0).expect("touched");
        assert!(left.is_empty());
    }
}
