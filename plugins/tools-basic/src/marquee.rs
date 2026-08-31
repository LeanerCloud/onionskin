//! The rubber-band gesture behind region selection, snapshot and marquee
//! zoom: three tools that differ only in what they do with the rectangle.

use onionskin_core::{PagePoint, PageRect, Viewport};
use onionskin_plugin_api::Overlay;

/// A drag shorter than this many viewport pixels is a click. Acrobat treats
/// a click with a marquee tool as a click, not a rectangle with no area.
pub(crate) const MIN_DRAG_PIXELS: f32 = 3.0;

/// Whether two page points are far enough apart on screen to be a drag.
///
/// The threshold is in viewport pixels, so it stays the same gesture at
/// every zoom. An unmappable point means the page left the layout mid-drag;
/// treating that as "not a drag" ends the gesture without inventing a
/// rectangle from coordinates we cannot place.
pub(crate) fn is_drag(from: PagePoint, to: PagePoint, viewport: &Viewport) -> bool {
    let (Ok(Some(from)), Ok(Some(to))) =
        (viewport.view_point_for(from), viewport.view_point_for(to))
    else {
        return false;
    };
    (to.x - from.x).hypot(to.y - from.y) >= MIN_DRAG_PIXELS
}

/// The rectangle a drag has swept so far, in the page it started on.
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

    /// Grow the rectangle to `at`. A point on another page is ignored: a
    /// `PageRect` names one page, and a marquee that spans two of them has
    /// no representation to fall back on.
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

    /// End the gesture, handing back the rectangle it swept, if any.
    pub(crate) fn finish(&mut self) -> Option<PageRect> {
        self.anchor = None;
        self.rect.take()
    }

    pub(crate) fn cancel(&mut self) {
        self.anchor = None;
        self.rect = None;
    }

    pub(crate) fn overlays(&self) -> Vec<Overlay> {
        self.rect.map(Overlay::AntsRect).into_iter().collect()
    }
}
