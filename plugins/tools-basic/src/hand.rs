//! The hand tool: drag the page under the cursor, click a form field to fill
//! it, or click a link to follow it.

use onionskin_core::forms::FieldKind;
use onionskin_core::{Document, FieldRequest, LinkRequest, PagePoint, ViewPoint};
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};

/// How far, in viewport pixels, the pointer may move between press and
/// release for the gesture still to be a click.
const CLICK_PIXELS: f32 = 3.0;

/// Keeps the page point it was grabbed by under the cursor, which is what
/// makes a pan feel like dragging paper rather than nudging a scrollbar.
#[derive(Debug, Default)]
pub struct HandTool {
    anchor: Option<PagePoint>,
    /// Where the press was on screen, to tell a click from a drag.
    pressed_at: Option<ViewPoint>,
    /// Whether the pointer has gone further than a click allows since.
    dragged: bool,
}

impl HandTool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Note whether the pointer, now over page point `at`, has gone further
    /// from the press than a click. Measured before the pan the move causes,
    /// when `at` is still under the pointer.
    fn track(&mut self, ctx: &ToolCtx, at: PagePoint) {
        let now = ctx.viewport.view_point_for(at).ok().flatten();
        if let (Some(from), Some(to)) = (self.pressed_at, now) {
            if (to.x - from.x).hypot(to.y - from.y) >= CLICK_PIXELS {
                self.dragged = true;
            }
        }
    }

    fn drag_to(&self, ctx: &mut ToolCtx, at: PagePoint) {
        let Some(anchor) = self.anchor else {
            return;
        };
        // Both ends are measured against the same offset, so their
        // difference is how far the grabbed point has slipped from the
        // cursor, and panning by it puts the point back.
        let (Ok(Some(from)), Ok(Some(to))) = (
            ctx.viewport.view_point_for(anchor),
            ctx.viewport.view_point_for(at),
        ) else {
            return;
        };
        let _ = ctx.viewport.pan_by(ViewPoint {
            x: to.x - from.x,
            y: to.y - from.y,
        });
    }
}

impl ToolPlugin for HandTool {
    fn id(&self) -> &'static str {
        "hand"
    }

    fn name(&self) -> &'static str {
        "Hand"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag the page to scroll it. Click a form field to fill it, or a link to follow it.")
    }

    fn icon(&self) -> &'static str {
        "hand"
    }

    fn shortcut(&self) -> Option<&'static str> {
        Some("h")
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.anchor = Some(input.at);
        self.pressed_at = ctx.viewport.view_point_for(input.at).ok().flatten();
        self.dragged = false;
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.track(ctx, input.at);
        self.drag_to(ctx, input.at);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.track(ctx, input.at);
        self.drag_to(ctx, input.at);
        let clicked = self.pressed_at.take().is_some() && !self.dragged;
        if clicked && self.anchor.is_some() {
            match field_at(ctx.doc, input.at) {
                Some(request) => ctx.doc.request_field(request),
                None => ctx.doc.request_link(LinkRequest::Follow(input.at)),
            }
        }
        self.anchor = None;
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.anchor = None;
        self.pressed_at = None;
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.anchor = None;
        self.pressed_at = None;
    }
}

/// The fillable form field under `at`: not read-only, and not a signature
/// field unless the shell is to say what signing needs.
fn field_at(doc: &mut Document, at: PagePoint) -> Option<FieldRequest> {
    let form = doc.form().ok()?;
    let (field, widget) = form.field_at(at.page, (at.x, at.y))?;
    let fillable = !field.flags.read_only || matches!(field.kind, FieldKind::Signature);
    fillable.then_some(FieldRequest {
        field: field.objref,
        widget: widget.objref,
        page: at.page,
        point: (at.x, at.y),
    })
}
