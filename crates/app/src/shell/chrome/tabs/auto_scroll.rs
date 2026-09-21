//! The frame's half of View > Page Display > Automatically Scroll: the menu
//! entry, and the keys Acrobat gives a running scroll. Up and Down change
//! the speed, minus reverses, and Escape stops.
//!
//! The keys are bound in [`AUTO_SCROLL_KEY_CONTEXT`], which the frame adds to
//! its own key context only while the tab in front is scrolling. Up and Down
//! otherwise move the focus ring, and a text field's own context still wins
//! over both.

use gpui::{Context, Window};

use super::ShellFrame;
use crate::shell::canvas::AutoScrollChange;
use crate::shell::chrome::accessible::SHELL_KEY_CONTEXT;

gpui::actions!(
    onionskin_auto_scroll,
    [
        /// Up while scrolling: one speed faster.
        AutoScrollFaster,
        /// Down while scrolling: one speed slower.
        AutoScrollSlower,
        /// Minus while scrolling: the other way.
        AutoScrollReverse,
    ]
);

/// The key context a running scroll adds to the frame's.
pub(in crate::shell) const AUTO_SCROLL_KEY_CONTEXT: &str = "OnionskinAutoScroll";

/// Bound after the shell's own keys, so at the frame's depth these win over
/// the focus ring's Up and Down while a scroll runs.
pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    let context = Some(AUTO_SCROLL_KEY_CONTEXT);
    cx.bind_keys([
        gpui::KeyBinding::new("up", AutoScrollFaster, context),
        gpui::KeyBinding::new("down", AutoScrollSlower, context),
        gpui::KeyBinding::new("-", AutoScrollReverse, context),
    ]);
}

impl ShellFrame {
    /// Whether the tab in front is automatically scrolling.
    pub(super) fn auto_scrolling(&self, cx: &gpui::App) -> bool {
        self.active_canvas()
            .is_some_and(|canvas| canvas.read(cx).model.auto_scrolling())
    }

    /// The frame's key context: the shell's, with the scroll's keys added
    /// while one runs.
    pub(super) fn key_context(&self, cx: &gpui::App) -> String {
        if self.auto_scrolling(cx) {
            format!("{SHELL_KEY_CONTEXT} {AUTO_SCROLL_KEY_CONTEXT}")
        } else {
            SHELL_KEY_CONTEXT.to_owned()
        }
    }

    /// The menu entry: start scrolling the tab in front, or stop.
    pub(super) fn toggle_auto_scroll(&mut self, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        if let Some(canvas) = self.active_canvas().cloned() {
            canvas.update(cx, |canvas, cx| {
                canvas.model.toggle_auto_scroll();
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Escape: stop a running scroll. Whether there was one.
    pub(super) fn stop_auto_scroll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(canvas) = self.active_canvas().cloned() else {
            return false;
        };
        let stopped = canvas.update(cx, |canvas, cx| {
            let stopped = canvas.model.stop_auto_scroll();
            cx.notify();
            stopped
        });
        cx.notify();
        stopped
    }

    /// Steer the running scroll, or hand the key on (to the focus ring)
    /// when nothing is scrolling.
    fn change_auto_scroll(&mut self, change: AutoScrollChange, cx: &mut Context<Self>) {
        let changed = self.active_canvas().cloned().is_some_and(|canvas| {
            canvas.update(cx, |canvas, _| canvas.model.change_auto_scroll(change))
        });
        if !changed {
            cx.propagate();
        }
    }

    pub(super) fn auto_scroll_faster(
        &mut self,
        _: &AutoScrollFaster,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_auto_scroll(AutoScrollChange::Faster, cx);
    }

    pub(super) fn auto_scroll_slower(
        &mut self,
        _: &AutoScrollSlower,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_auto_scroll(AutoScrollChange::Slower, cx);
    }

    pub(super) fn auto_scroll_reverse(
        &mut self,
        _: &AutoScrollReverse,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_auto_scroll(AutoScrollChange::Reverse, cx);
    }
}
