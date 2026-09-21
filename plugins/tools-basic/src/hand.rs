//! The hand tool: drag the page under the cursor.

use onionskin_core::{PagePoint, ViewPoint};
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};

/// Keeps the page point it was grabbed by under the cursor, which is what
/// makes a pan feel like dragging paper rather than nudging a scrollbar.
#[derive(Debug, Default)]
pub struct HandTool {
    anchor: Option<PagePoint>,
}

impl HandTool {
    pub fn new() -> Self {
        Self::default()
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
        Some("Drag the page to scroll it.")
    }

    fn icon(&self) -> &'static str {
        "hand"
    }

    fn shortcut(&self) -> Option<&'static str> {
        Some("h")
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.anchor = Some(input.at);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.drag_to(ctx, input.at);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.drag_to(ctx, input.at);
        self.anchor = None;
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.anchor = None;
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.anchor = None;
    }
}
