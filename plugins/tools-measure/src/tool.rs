//! The Distance, Perimeter and Area tools: one tool type, told apart by what
//! it measures.
//!
//! **The gestures are Acrobat's.** A distance is a click at each end, or a
//! drag from one to the other. A perimeter is a click at each point, ended
//! by a double click or Enter. An area is the same, and a click back on its
//! first point also ends it. Escape abandons what is being measured.
//!
//! **What is read, and what is kept.** While a measurement is being made
//! the tool reads it off the page for the side panel, snapped to the page's
//! line art. Once made, it stays in the panel, and with measurement markup
//! on it is also kept on the page as a comment, as Acrobat keeps one.

use onionskin_core::measure::{Kind, Measure};
use onionskin_core::{add_annotation, Color, Document, PagePoint, PageRect, Viewport};
use onionskin_plugin_api::{
    Overlay, PointerInput, Reading, ToolCapability, ToolChoice, ToolCtx, ToolEnvironment,
    ToolPlugin,
};

use crate::settings::Shared;
use crate::snap::{SnapKind, Snapper};

/// Below this, in view pixels, a drag is a click.
const MIN_DRAG_PIXELS: f64 = 3.0;

/// Within this, in view pixels, a point is snapped to the page's line art,
/// and a click on an area's first point closes it.
const SNAP_PIXELS: f64 = 8.0;

/// What a measurement comment is drawn in.
const MARKUP_COLOR: Color = Color {
    red: 0.0,
    green: 0.35,
    blue: 0.8,
};

pub struct MeasureTool {
    kind: Kind,
    settings: Shared,
    snapper: Snapper,
    /// The points placed so far, all on one page.
    points: Vec<PagePoint>,
    /// Where the pointer is, snapped.
    at: Option<PagePoint>,
    /// What `at` was snapped to, and the box drawn round it.
    snapped: Option<(SnapKind, PageRect)>,
    /// A distance's first press, while it may yet be a drag.
    dragging: bool,
    /// The last measurement made, which the panel shows until the next.
    last: Vec<(f64, f64)>,
    author: Option<String>,
}

impl MeasureTool {
    pub fn distance(settings: Shared) -> Self {
        Self::new(Kind::Distance, settings)
    }

    pub fn perimeter(settings: Shared) -> Self {
        Self::new(Kind::Perimeter, settings)
    }

    pub fn area(settings: Shared) -> Self {
        Self::new(Kind::Area, settings)
    }

    fn new(kind: Kind, settings: Shared) -> Self {
        MeasureTool {
            kind,
            settings,
            snapper: Snapper::default(),
            points: Vec::new(),
            at: None,
            snapped: None,
            dragging: false,
            last: Vec::new(),
            author: None,
        }
    }

    fn measure(&self) -> Measure {
        Measure::new(self.kind, self.settings.get().scale)
    }

    /// `input`'s point, snapped to what is near it on its page.
    fn locate(&mut self, ctx: &mut ToolCtx, input: PointerInput) -> PagePoint {
        let at = input.at;
        self.snapped = None;
        let Some(per_pixel) = points_per_pixel(ctx.viewport, at) else {
            return at;
        };
        let radius = SNAP_PIXELS * per_pixel;
        self.snapper.load(ctx.doc, at.page);
        let options = self.settings.get().snap;
        let Some(((x, y), kind)) = self.snapper.snap((at.x, at.y), radius, options) else {
            return at;
        };
        let half = radius / 2.0;
        let marker = PageRect {
            page: at.page,
            x0: x - half,
            y0: y - half,
            x1: x + half,
            y1: y + half,
        };
        self.snapped = Some((kind, marker));
        PagePoint {
            page: at.page,
            x,
            y,
        }
    }

    /// The points being measured, the pointer's among them.
    fn live(&self) -> Vec<(f64, f64)> {
        let mut points: Vec<(f64, f64)> = self.points.iter().map(|p| (p.x, p.y)).collect();
        if let Some(at) = self.at.filter(|_| !self.points.is_empty()) {
            points.push((at.x, at.y));
        }
        points
    }

    fn on_same_page(&self, at: PagePoint) -> bool {
        self.points
            .first()
            .is_none_or(|first| first.page == at.page)
    }

    /// Finish the measurement: keep it for the panel and, with markup on,
    /// on the page. Too few points and it is dropped.
    fn commit(&mut self, ctx: &mut ToolCtx) {
        let points: Vec<(f64, f64)> = self.points.iter().map(|p| (p.x, p.y)).collect();
        let page = self.points.first().map(|point| point.page);
        self.reset();
        let (Some(page), true) = (page, points.len() >= self.kind.least_points()) else {
            return;
        };
        self.last = points;
        if self.settings.get().markup {
            self.keep(ctx.doc, page);
        }
    }

    /// The last measurement, kept on `page` as a comment.
    fn keep(&self, doc: &mut Document, page: usize) {
        let Some(mut annotation) = self.measure().annotation(&self.last) else {
            return;
        };
        annotation.color = Some(MARKUP_COLOR);
        annotation.author.clone_from(&self.author);
        let Some(page) = doc
            .structure()
            .ok()
            .and_then(|structure| structure.page(page).ok())
            .map(|page| page.objref)
        else {
            return;
        };
        // Refused on a document that may not be commented, which leaves the
        // measurement in the panel and nothing on the page.
        let _ = doc.edit_annotations(self.name(), |tx, structure| {
            add_annotation(tx, structure, page, &annotation, now()).map(|_| ())
        });
    }

    fn reset(&mut self) {
        self.points.clear();
        self.at = None;
        self.snapped = None;
        self.dragging = false;
    }

    fn press_distance(&mut self, ctx: &mut ToolCtx, at: PagePoint) {
        if self.points.is_empty() {
            self.points.push(at);
            self.at = Some(at);
            self.dragging = true;
            return;
        }
        self.points.push(at);
        self.commit(ctx);
    }

    fn press_vertex(&mut self, ctx: &mut ToolCtx, at: PagePoint, clicks: u8) {
        if clicks >= 2 && !self.points.is_empty() {
            self.commit(ctx);
            return;
        }
        let closes = self.kind == Kind::Area
            && self.points.len() >= self.kind.least_points()
            && self.points.first().is_some_and(|first| {
                view_distance(*first, at, ctx.viewport)
                    .is_some_and(|distance| distance <= SNAP_PIXELS)
            });
        if closes {
            self.commit(ctx);
            return;
        }
        self.points.push(at);
        self.at = Some(at);
    }
}

/// How many page points one view pixel is, where `at` is.
fn points_per_pixel(viewport: &Viewport, at: PagePoint) -> Option<f64> {
    let across = PagePoint {
        x: at.x + 1.0,
        ..at
    };
    view_distance(at, across, viewport)
        .filter(|pixels| *pixels > 0.0)
        .map(|pixels| 1.0 / pixels)
}

fn view_distance(from: PagePoint, to: PagePoint, viewport: &Viewport) -> Option<f64> {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return None;
    };
    Some(f64::from((to.x - from.x).hypot(to.y - from.y)))
}

/// Seconds since the Unix epoch, or zero when the clock is before it.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

impl ToolPlugin for MeasureTool {
    fn id(&self) -> &'static str {
        match self.kind {
            Kind::Distance => "measure-distance",
            Kind::Perimeter => "measure-perimeter",
            Kind::Area => "measure-area",
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Distance => "Distance",
            Kind::Perimeter => "Perimeter",
            Kind::Area => "Area",
        }
    }

    fn icon(&self) -> &'static str {
        self.id()
    }

    fn group(&self) -> &'static str {
        "measure"
    }

    fn hint(&self) -> Option<&'static str> {
        Some(match self.kind {
            Kind::Distance => {
                "Click where the distance starts and again where it ends, or drag between them."
            }
            Kind::Perimeter => {
                "Click each point in turn. Double-click the last one, or press Enter, to finish."
            }
            Kind::Area => {
                "Click each corner in turn. Click the first again, double-click the last, or press Enter, to finish."
            }
        })
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Measure]
    }

    fn configure(&mut self, environment: &ToolEnvironment) {
        self.author.clone_from(&environment.author);
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let at = self.locate(ctx, input);
        if !self.on_same_page(at) {
            return;
        }
        match self.kind {
            Kind::Distance => self.press_distance(ctx, at),
            Kind::Perimeter | Kind::Area => self.press_vertex(ctx, at, input.clicks),
        }
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let at = self.locate(ctx, input);
        if self.on_same_page(at) {
            self.at = Some(at);
        }
    }

    /// A distance dragged from end to end is made on release. A press and
    /// release in one place is its first click.
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if self.kind != Kind::Distance || !self.dragging {
            return;
        }
        self.dragging = false;
        let at = self.locate(ctx, input);
        let Some(first) = self.points.first().copied() else {
            return;
        };
        let dragged = view_distance(first, at, ctx.viewport)
            .is_some_and(|distance| distance >= MIN_DRAG_PIXELS);
        if dragged && first.page == at.page {
            self.points.push(at);
            self.commit(ctx);
        }
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.commit(ctx);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.reset();
    }

    fn on_activate(&mut self, _ctx: &mut ToolCtx) {
        // The page may have changed since the tool was last used.
        self.snapper.clear();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
        self.snapper.clear();
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        let mut overlays = Vec::new();
        let page = self.points.first().or(self.at.as_ref()).map(|p| p.page);
        let points: Vec<PagePoint> = match page {
            Some(page) => self
                .live()
                .into_iter()
                .map(|(x, y)| PagePoint { page, x, y })
                .collect(),
            None => Vec::new(),
        };
        match (self.kind, &points[..]) {
            (Kind::Distance, [from, to, ..]) => overlays.push(Overlay::Line {
                from: *from,
                to: *to,
            }),
            (Kind::Perimeter | Kind::Area, [_, _, ..]) => overlays.push(Overlay::Polyline {
                points,
                closed: self.kind == Kind::Area,
            }),
            _ => {}
        }
        if let Some((_, marker)) = self.snapped {
            overlays.push(Overlay::Rect(marker));
        }
        overlays
    }

    fn settings(&self) -> Vec<ToolChoice> {
        self.settings.get().choices()
    }

    fn choose(&mut self, id: &str) -> bool {
        self.settings.lock().choose(id)
    }

    fn chosen(&self) -> Option<String> {
        Some(self.settings.get().chosen())
    }

    fn picked(&self, id: &str) -> bool {
        self.settings.get().picked(id)
    }

    /// The Measurement Info panel: the scale, the measurement being made or
    /// the last one made, and for a distance how far across and up it goes
    /// and at what angle.
    fn readings(&self) -> Vec<Reading> {
        let measure = self.measure();
        let mut readings = vec![Reading::new("Scale", measure.scale.label())];
        let live = self.live();
        let points = if self.points.is_empty() {
            &self.last
        } else {
            &live
        };
        if points.len() >= self.kind.least_points() {
            readings.push(Reading::new(self.name(), measure.label(points)));
        }
        if let (Kind::Distance, [from, to, ..]) = (self.kind, &points[..]) {
            let per_point = measure.scale.per_point();
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let unit = measure.unit_label();
            let across = |value: f64| format!("{:.2} {unit}", value * per_point);
            readings.push(Reading::new("ΔX", across(dx)));
            readings.push(Reading::new("ΔY", across(dy)));
            readings.push(Reading::new(
                "Angle",
                format!("{:.1}°", dy.atan2(dx).to_degrees()),
            ));
        }
        if let Some((kind, _)) = self.snapped {
            readings.push(Reading::new("Snapped to", kind.label()));
        }
        readings
    }
}
