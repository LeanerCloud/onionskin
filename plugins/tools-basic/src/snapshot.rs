//! Take A Snapshot: marquee a region and copy it as an image.

use onionskin_core::Document;
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use onionskin_plugin_api::marquee::Marquee;

/// Selects a region and raises a `SnapshotRequest`. Producing the pixels is
/// the shell's job: a plugin holding a render handle would be a second way
/// into the renderer, and the clipboard is not the kernel's business.
#[derive(Debug, Default)]
pub struct SnapshotTool {
    marquee: Marquee,
}

impl SnapshotTool {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolPlugin for SnapshotTool {
    fn id(&self) -> &'static str {
        "snapshot"
    }

    fn name(&self) -> &'static str {
        "Take A Snapshot"
    }

    fn hint(&self) -> Option<&'static str> {
        Some("Drag a rectangle. That part of the page is copied to the clipboard as an image.")
    }

    fn icon(&self) -> &'static str {
        "snapshot"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Snapshot]
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
        if let Some(region) = self.marquee.finish() {
            ctx.doc.request_snapshot(region);
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
