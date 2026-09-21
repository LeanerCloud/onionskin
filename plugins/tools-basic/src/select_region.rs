//! Region selection: a marquee over part of a page.

use onionskin_core::Document;
use onionskin_plugin_api::{Overlay, PointerInput, ToolCtx, ToolPlugin};

use crate::marquee::Marquee;

/// Selects a rectangle of the page rather than its text, which is what a
/// snapshot, a crop or an export of an area starts from.
#[derive(Debug, Default)]
pub struct SelectRegionTool {
    marquee: Marquee,
}

impl SelectRegionTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for SelectRegionTool {
    fn id(&self) -> &'static str {
        "select-region"
    }

    fn name(&self) -> &'static str {
        "Select Region"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag a rectangle to select part of the page.")
    }

    fn icon(&self) -> &'static str {
        "select-region"
    }

    fn group(&self) -> &'static str {
        "select"
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.begin(input.at);
        ctx.doc.selection_mut().clear();
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
        // A click is a click: it leaves the selection cleared instead of
        // selecting a rectangle with no area.
        if let Some(region) = self.marquee.finish() {
            ctx.doc.selection_mut().set_region(region);
        }
    }

    fn on_cancel(&mut self, ctx: &mut ToolCtx) {
        self.marquee.cancel();
        ctx.doc.selection_mut().clear();
    }

    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        self.marquee.overlays()
    }
}
