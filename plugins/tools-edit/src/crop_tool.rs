//! The Crop Pages tool: drag a rectangle on a page, then double-click in it
//! or press Enter, and the page's crop box becomes that rectangle.
//!
//! Acrobat's tool opens its dialog on the double-click. The dialog here is
//! the shell's, reached from Edit > Crop Pages, so the tool crops directly
//! and the dialog is where margins, other boxes and more pages are set.

use onionskin_core::pages::MIN_BOX_SIZE;
use onionskin_core::{Document, PagePoint, PageRect};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::crop::crop_to_rect;
use onionskin_plugin_api::marquee::Marquee;

#[derive(Debug, Default)]
pub struct CropTool {
    marquee: Marquee,
}

impl CropTool {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rectangle drawn so far, if any.
    pub fn rect(&self) -> Option<PageRect> {
        self.marquee.rect()
    }

    /// Crop to the rectangle, if there is one big enough to be a page.
    fn crop(&mut self, doc: &mut Document) {
        let Some(rect) = self.marquee.finish() else {
            return;
        };
        if rect.x1 - rect.x0 >= MIN_BOX_SIZE && rect.y1 - rect.y0 >= MIN_BOX_SIZE {
            // A refusal leaves the page as it was, which is all a canvas
            // gesture can say; the dialog is where a crop explains itself.
            let _ = crop_to_rect(doc, rect);
        }
    }
}

fn inside(rect: PageRect, at: PagePoint) -> bool {
    at.page == rect.page
        && (rect.x0..=rect.x1).contains(&at.x)
        && (rect.y0..=rect.y1).contains(&at.y)
}

impl ToolPlugin for CropTool {
    fn id(&self) -> &'static str {
        "crop-pages"
    }

    fn name(&self) -> &'static str {
        "Crop Pages"
    }

    fn icon(&self) -> &'static str {
        "crop"
    }

    fn hint(&self) -> Option<&'static str> {
        Some(
            "Drag a rectangle on a page, then double-click in it or press Enter to crop the page \
             to it. Edit > Crop Pages sets margins, other boxes and more pages.",
        )
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::EditPages]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        if input.clicks >= 2 && self.rect().is_some_and(|rect| inside(rect, input.at)) {
            self.crop(ctx.doc);
            return;
        }
        self.marquee.begin(input.at);
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.marquee.extend(input.at, ctx.viewport);
        self.marquee.release();
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.crop(ctx.doc);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.marquee.cancel();
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        self.rect().map(Overlay::AntsRect).into_iter().collect()
    }
}
