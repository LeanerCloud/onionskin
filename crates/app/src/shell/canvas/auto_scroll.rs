//! View > Page Display > Automatically Scroll on the canvas (P22).
//!
//! The timing is `core`'s [`AutoScroll`]; this applies it to the viewport a
//! frame at a time. The shell calls [`CanvasModel::advance_auto_scroll`] when
//! it draws the canvas and asks for another frame while the scroll runs, so
//! nothing runs while the window is not drawn: GPUI only runs a window's
//! display link while the platform reports it visible.

use std::time::Instant;

use onionskin_core::{AutoScroll, ViewPoint};

use super::{CanvasError, CanvasModel};

/// A change to a running scroll, from the keys Acrobat gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum AutoScrollChange {
    Faster,
    Slower,
    Reverse,
}

impl CanvasModel {
    pub fn auto_scrolling(&self) -> bool {
        self.auto_scroll.is_some()
    }

    #[cfg(test)]
    pub(in crate::shell) fn auto_scroll(&self) -> Option<&AutoScroll> {
        self.auto_scroll.as_ref()
    }

    /// Start scrolling at the default speed, or stop.
    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = match self.auto_scroll {
            Some(_) => None,
            None => Some(AutoScroll::default()),
        };
    }

    /// Stop scrolling. Whether it was running.
    pub fn stop_auto_scroll(&mut self) -> bool {
        self.auto_scroll.take().is_some()
    }

    /// Change a running scroll. Whether one was running to change.
    pub(in crate::shell) fn change_auto_scroll(&mut self, change: AutoScrollChange) -> bool {
        let Some(scroll) = self.auto_scroll.as_mut() else {
            return false;
        };
        match change {
            AutoScrollChange::Faster => {
                scroll.faster();
            }
            AutoScrollChange::Slower => {
                scroll.slower();
            }
            AutoScrollChange::Reverse => scroll.reverse(),
        }
        true
    }

    /// The user touched the document: hold the scroll for a moment.
    pub(super) fn pause_auto_scroll(&mut self, now: Instant) {
        if let Some(scroll) = self.auto_scroll.as_mut() {
            scroll.pause(now);
        }
    }

    /// Move for a frame drawn at `now`. Whether the view moved. Reaching the
    /// end (or, reversed, the start) stops the scroll, as Acrobat's does.
    pub fn advance_auto_scroll(&mut self, now: Instant) -> Result<bool, CanvasError> {
        let Some(scroll) = self.auto_scroll.as_mut() else {
            return Ok(false);
        };
        let distance = scroll.tick(now);
        if distance == 0.0 {
            return Ok(false);
        }
        let before = self.viewport.offset();
        self.viewport.pan_by(ViewPoint {
            x: 0.0,
            y: -distance,
        })?;
        let moved = self.viewport.offset() != before;
        if !moved {
            self.auto_scroll = None;
        }
        Ok(moved)
    }
}
