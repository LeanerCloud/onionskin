//! The Crop Pages tool: drag a rectangle on a page, then double-click in it
//! or press Enter, and the page's crop box becomes that rectangle.
//!
//! Acrobat's tool opens its dialog on the double-click. The dialog here is
//! the shell's, reached from Edit > Crop Pages, so the tool crops directly
//! and the dialog is where margins, other boxes and more pages are set.

use onionskin_core::pages::MIN_BOX_SIZE;
use onionskin_core::{Document, PagePoint, PageRect, Viewport};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};

use crate::crop::crop_to_rect;

/// A drag shorter than this many viewport pixels is a click, as it is for
/// every marquee tool.
const MIN_DRAG_PIXELS: f32 = 3.0;

#[derive(Debug, Default)]
pub struct CropTool {
    anchor: Option<PagePoint>,
    rect: Option<PageRect>,
}

impl CropTool {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rectangle drawn so far, if any.
    pub fn rect(&self) -> Option<PageRect> {
        self.rect
    }

    fn extend(&mut self, at: PagePoint, viewport: &Viewport) {
        let Some(anchor) = self.anchor else {
            return;
        };
        if at.page != anchor.page || !is_drag(anchor, at, viewport) {
            return;
        }
        self.rect = Some(PageRect {
            page: anchor.page,
            x0: anchor.x.min(at.x),
            y0: anchor.y.min(at.y),
            x1: anchor.x.max(at.x),
            y1: anchor.y.max(at.y),
        });
    }

    /// Crop to the rectangle, if there is one big enough to be a page.
    fn crop(&mut self, doc: &mut Document) {
        self.anchor = None;
        let Some(rect) = self.rect.take() else {
            return;
        };
        if rect.x1 - rect.x0 >= MIN_BOX_SIZE && rect.y1 - rect.y0 >= MIN_BOX_SIZE {
            // A refusal leaves the page as it was, which is all a canvas
            // gesture can say; the dialog is where a crop explains itself.
            let _ = crop_to_rect(doc, rect);
        }
    }
}

/// Whether two page points are far enough apart on screen to be a drag, at
/// any zoom. A point that cannot be placed is not one.
fn is_drag(from: PagePoint, to: PagePoint, viewport: &Viewport) -> bool {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return false;
    };
    (to.x - from.x).hypot(to.y - from.y) >= MIN_DRAG_PIXELS
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
        if input.clicks >= 2 && self.rect.is_some_and(|rect| inside(rect, input.at)) {
            self.crop(ctx.doc);
            return;
        }
        self.anchor = Some(input.at);
        self.rect = None;
    }

    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.extend(input.at, ctx.viewport);
    }

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        self.extend(input.at, ctx.viewport);
        self.anchor = None;
    }

    fn on_commit(&mut self, ctx: &mut ToolCtx) {
        self.crop(ctx.doc);
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.anchor = None;
        self.rect = None;
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }

    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        self.rect.map(Overlay::AntsRect).into_iter().collect()
    }
}
