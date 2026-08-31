//! Marquee zoom, with Acrobat's click semantics.

use onionskin_core::{
    Document, FitMode, PageAlignment, PageGeometry, PageRect, PageRenderRect, ViewPoint, ViewSize,
    Viewport,
};
use onionskin_plugin_api::{Modifiers, Overlay, PointerInput, ToolCtx, ToolPlugin};

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
    let Ok(Some(at)) = viewport.view_point_for(input.at) else {
        return;
    };
    let size = viewport.size();
    let at = ViewPoint {
        x: at.x.clamp(0.0, size.width),
        y: at.y.clamp(0.0, size.height),
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
