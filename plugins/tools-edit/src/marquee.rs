//! The rectangle a drag sweeps on one page: what the Crop Pages and Link
//! tools draw before they act on it.

use onionskin_core::{PagePoint, PageRect, Viewport};

/// A drag shorter than this many viewport pixels is a click, as it is for
/// every marquee tool.
const MIN_DRAG_PIXELS: f32 = 3.0;

#[derive(Debug, Default)]
pub(crate) struct Marquee {
    anchor: Option<PagePoint>,
    rect: Option<PageRect>,
}

impl Marquee {
    pub(crate) fn begin(&mut self, at: PagePoint) {
        self.anchor = Some(at);
        self.rect = None;
    }

    /// Grow the rectangle to `at`. A point on another page, or one too close
    /// to the anchor to be a drag, changes nothing.
    pub(crate) fn extend(&mut self, at: PagePoint, viewport: &Viewport) {
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

    /// End the gesture, keeping the rectangle.
    pub(crate) fn release(&mut self) {
        self.anchor = None;
    }

    pub(crate) fn rect(&self) -> Option<PageRect> {
        self.rect
    }

    /// Hand back the rectangle and forget it.
    pub(crate) fn take(&mut self) -> Option<PageRect> {
        self.anchor = None;
        self.rect.take()
    }

    pub(crate) fn clear(&mut self) {
        self.anchor = None;
        self.rect = None;
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
