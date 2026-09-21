//! Acrobat's two zoom tools, which share a rail slot the way its Zoom
//! flyout does: marquee zoom, with its click semantics, and dynamic zoom,
//! which zooms continuously while the pointer is dragged.

use onionskin_core::{
    Document, FitMode, PageAlignment, PageGeometry, PagePoint, PageRect, PageRenderRect, ViewPoint,
    ViewSize, Viewport,
};
use onionskin_plugin_api::{Modifiers, Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::marquee::Marquee;

/// Drag a rectangle and the viewport fits it; click and the viewport zooms
/// one step at the point clicked. Acrobat zooms out instead when the click
/// carries the platform's inverting modifier: option on macOS, control on
/// Windows and Linux.
#[derive(Debug, Default)]
pub struct ZoomTool {
    marquee: Marquee,
}

impl ZoomTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for ZoomTool {
    fn id(&self) -> &'static str {
        "zoom"
    }

    fn name(&self) -> &'static str {
        "Marquee Zoom"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag a rectangle to zoom into it. Click to zoom in a step; Option-click zooms out.")
    }

    fn icon(&self) -> &'static str {
        "zoom"
    }

    fn shortcut(&self) -> Option<&'static str> {
        Some("z")
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.begin(input.at);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
        match self.marquee.finish() {
            Some(region) => fit_region(ctx.viewport, region),
            None => step_zoom(ctx.viewport, input),
        }
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        self.marquee.overlays()
    }
}

/// Zoom so the dragged rectangle fills the viewport.
fn fit_region(viewport: &mut Viewport, region: PageRect) {
    let Some(geometry) = viewport.page_geometry(region.page).cloned() else {
        return;
    };
    let Some(bounds) = render_rect(&geometry, region) else {
        return;
    };
    // `FitMode::Visible` is anchored on the current page, so a rectangle
    // dragged on another page has to become the current one first.
    if viewport.current_page() != region.page
        && viewport
            .go_to_page(region.page, PageAlignment::Center)
            .is_err()
    {
        return;
    }
    let _ = viewport.fit(FitMode::Visible(bounds));
}

/// The region as a rectangle of the page's unrotated render space, which is
/// what `FitMode::Visible` is expressed in.
fn render_rect(geometry: &PageGeometry, region: PageRect) -> Option<PageRenderRect> {
    let quad = geometry.user_to_device(region.into(), 1.0).ok()?;
    let page = ViewSize {
        width: geometry.render_size.0 as f32,
        height: geometry.render_size.1 as f32,
    };
    let (x0, x1) = span(quad.corners.map(|(x, _)| x), page.width);
    let (y0, y1) = span(quad.corners.map(|(_, y)| y), page.height);
    PageRenderRect::new(
        region.page,
        ViewPoint { x: x0, y: y0 },
        ViewSize {
            width: x1 - x0,
            height: y1 - y0,
        },
        page,
    )
    .ok()
}

/// The extent of four device coordinates on one axis, clipped to the page:
/// the transform is exact but the drag is not, so a corner may land a
/// fraction outside the page a `PageRenderRect` has to stay inside.
fn span(values: [f64; 4], limit: f32) -> (f32, f32) {
    let (min, max) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(min, max), value| {
            (min.min(*value), max.max(*value))
        });
    (
        (min as f32).clamp(0.0, limit),
        (max as f32).clamp(0.0, limit),
    )
}

fn step_zoom(viewport: &mut Viewport, input: PointerInput) {
    let Some(at) = anchor_point(viewport, input.at) else {
        return;
    };
    let _ = if zooms_out(input.modifiers) {
        viewport.zoom_out(at)
    } else {
        viewport.zoom_in(at)
    };
}

fn zooms_out(modifiers: Modifiers) -> bool {
    modifiers.alt || modifiers.ctrl_or_cmd
}

/// Where a page point sits on screen. `None` means the point is on no page
/// the layout currently places.
fn view_point(viewport: &Viewport, at: PagePoint) -> Option<ViewPoint> {
    viewport.view_point_for(at).ok().flatten()
}

/// A screen point pulled inside the viewport.
///
/// Only for a zoom anchor: `Viewport` refuses one outside the view, and
/// answers `AnchorOutsideViewport` rather than zooming, which for a gesture
/// that discards the error is a silent no-op for the whole drag. Measuring a
/// drag never clamps, because clamping there would stop counting the moment
/// the pointer left the window while the button was still down.
fn clamped_to_view(point: ViewPoint, size: ViewSize) -> ViewPoint {
    ViewPoint {
        x: point.x.clamp(0.0, size.width),
        y: point.y.clamp(0.0, size.height),
    }
}

/// Where a page point sits on screen, pulled inside the viewport so it can
/// be a zoom anchor. See [`clamped_to_view`].
fn anchor_point(viewport: &Viewport, at: PagePoint) -> Option<ViewPoint> {
    view_point(viewport, at).map(|at| clamped_to_view(at, viewport.size()))
}

/// Press and drag up to zoom in, down to zoom out, continuously, about the
/// point the drag started from.
///
/// The anchor is the press point and stays there for the whole gesture: it
/// is the page position the user put the pointer on, and re-deriving it from
/// each move would let the page creep out from under the cursor.
///
/// This is a tool rather than a menu command because it is a drag, and
/// Acrobat's View > Zoom > Dynamic Zoom entry selects exactly this tool
/// rather than changing the view itself.
#[derive(Debug, Default)]
pub struct DynamicZoomTool {
    /// The press point on screen, and the pointer's last screen height. Both
    /// are `None` between gestures, which is what makes a stray move a no-op.
    drag: Option<Drag>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    anchor: ViewPoint,
    last_y: f32,
}

impl DynamicZoomTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for DynamicZoomTool {
    fn id(&self) -> &'static str {
        "dynamic-zoom"
    }

    fn name(&self) -> &'static str {
        "Dynamic Zoom"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag up to zoom in and down to zoom out.")
    }

    fn icon(&self) -> &'static str {
        "dynamic-zoom"
    }

    /// One rail slot with marquee zoom, which is where Acrobat's Zoom
    /// flyout keeps them.
    fn group(&self) -> &'static str {
        "zoom"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::DynamicZoom]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        // The two differ on a press the window edge cuts off: the anchor has
        // to be inside the view or every later zoom is refused, while the
        // measurement starts where the pointer really is or the first move
        // is short by however far outside it was.
        self.drag = view_point(ctx.viewport, input.at).map(|at| Drag {
            anchor: clamped_to_view(at, ctx.viewport.size()),
            last_y: at.y,
        });
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        // Read the pointer's screen height before zooming: `input.at` is the
        // page point under the pointer as of this event, so it maps back to
        // the true screen position only against the viewport that produced
        // it. Unclamped, because a drag goes on past the window edge.
        let Some(at) = view_point(ctx.viewport, input.at) else {
            return;
        };
        // Up the screen is a smaller y and is Acrobat's zoom-in direction.
        let delta = drag.last_y - at.y;
        drag.last_y = at.y;
        let _ = ctx.viewport.dynamic_zoom(delta, drag.anchor);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.on_pointer_move(ctx, input);
        self.drag = None;
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.drag = None;
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.drag = None;
    }
}
