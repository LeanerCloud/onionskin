//! Press and release: a click, or a drag from one point to another on the
//! same page.

use onionskin_core::{PagePoint, Viewport};
use onionskin_plugin_api::marquee::is_drag;

#[derive(Debug, Default)]
pub(crate) struct Press {
    anchor: Option<PagePoint>,
    at: Option<PagePoint>,
}

/// What a finished gesture was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Gesture {
    Click(PagePoint),
    Drag(PagePoint, PagePoint),
}

impl Press {
    pub(crate) fn down(&mut self, at: PagePoint) {
        self.anchor = Some(at);
        self.at = Some(at);
    }

    pub(crate) fn moved(&mut self, at: PagePoint) {
        if self.anchor.is_some_and(|anchor| anchor.page == at.page) {
            self.at = Some(at);
        }
    }

    /// The two ends while a drag is under way, for a preview.
    pub(crate) fn span(&self) -> Option<(PagePoint, PagePoint)> {
        Some((self.anchor?, self.at?))
    }

    /// End the gesture at `at`.
    pub(crate) fn up(&mut self, at: PagePoint, viewport: &Viewport) -> Option<Gesture> {
        self.moved(at);
        let (anchor, at) = (self.anchor.take()?, self.at.take()?);
        Some(if is_drag(anchor, at, viewport) {
            Gesture::Drag(anchor, at)
        } else {
            Gesture::Click(anchor)
        })
    }

    pub(crate) fn cancel(&mut self) {
        self.anchor = None;
        self.at = None;
    }
}
