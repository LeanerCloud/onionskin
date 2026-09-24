//! View > Show/Hide > Line Weights, and Page Display's "Use line weights":
//! one preference, which every window takes at once.
//!
//! Off draws every stroke one pixel wide, for reading drawings whose heavy
//! borders hide the detail. It is a way of looking at the page: printing,
//! export and the saved file keep the page's own widths, which the render
//! worker enforces for every render it answers on a caller's own channel.
//!
//! It is application-wide, as Acrobat's is, and it has to be: two windows
//! on one document draw from one session, so a per-window setting would
//! have one window's menu say something the other window's pages contradict.

use gpui::{App, Context};

use super::ShellFrame;
use crate::shell::preferences_dialog::PreferenceChange;

impl ShellFrame {
    /// The View menu's Line Weights: turn the preference over. Saved, and
    /// shown by the Preferences dialog, because it is the same setting.
    pub(super) fn toggle_line_weights(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        let on = !self.settings.preferences.line_weights;
        self.change_preference(PreferenceChange::LineWeights(on), cx);
    }

    /// Take `on` as this window's Line Weights: the setting it would save,
    /// the menu's check mark, and the pixels of every tab and thumbnail.
    pub(in crate::shell) fn adopt_line_weights(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.preferences.line_weights = on;
        self.shell_view_state.set_line_weights(on);
        let mut redrawn = false;
        for tab in self.tabs.tabs() {
            redrawn |= tab.canvas.update(cx, |canvas, cx| {
                // The route every view change takes: it repaints, re-requests
                // the pages the new options invalidated, and reports a failure.
                let result = canvas.model.set_hairline_strokes(!on);
                let changed = matches!(result, Ok(true));
                canvas.handle_change(result, cx);
                changed
            });
        }
        if redrawn {
            self.navigation.invalidate_thumbnails();
        }
        cx.notify();
    }
}

/// Every other window takes `on` as well. Deferred, because it runs from
/// inside the frame that changed it, which cannot be updated from within
/// its own update.
pub(super) fn tell_other_windows(origin: gpui::EntityId, on: bool, cx: &mut Context<ShellFrame>) {
    cx.defer(move |cx| apply_elsewhere(origin, on, cx));
}

fn apply_elsewhere(origin: gpui::EntityId, on: bool, cx: &mut App) {
    for handle in cx.windows() {
        let Some(frame) = handle.downcast::<ShellFrame>() else {
            continue;
        };
        let _ = frame.update(cx, |frame, _window, cx| {
            if cx.entity_id() != origin {
                frame.adopt_line_weights(on, cx);
            }
        });
    }
}
