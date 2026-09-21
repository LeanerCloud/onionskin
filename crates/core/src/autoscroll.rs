//! View > Page Display > Automatically Scroll: the timing, without a clock.
//!
//! The shell asks how far to move each time it draws a frame and passes the
//! time in, so this is testable without waiting and a window that is not
//! drawn is not scrolled: no frames, no ticks. A long gap between frames (the
//! window came back from being hidden) moves at most
//! [`AUTO_SCROLL_MAX_STEP`]'s worth rather than jumping by everything it
//! missed.
//!
//! Acrobat's controls, kept: the arrow keys change the speed, minus reverses,
//! Escape stops, and touching the document pauses it. Here the pause ends by
//! itself [`AUTO_SCROLL_RESUME_AFTER`] after the last touch, so reading a
//! passage with the pointer on it does not need the menu to start again.

use std::time::{Duration, Instant};

/// Speed levels in view pixels per second, slowest first.
pub const AUTO_SCROLL_SPEEDS: [f32; 9] = [15.0, 25.0, 40.0, 60.0, 90.0, 130.0, 180.0, 250.0, 350.0];

/// The level a new scroll starts at: slow enough to read at.
pub const DEFAULT_AUTO_SCROLL_LEVEL: usize = 3;

/// How long after the last touch the scroll carries on.
pub const AUTO_SCROLL_RESUME_AFTER: Duration = Duration::from_secs(2);

/// The most time one frame is allowed to account for.
pub const AUTO_SCROLL_MAX_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoScroll {
    /// 1 to `AUTO_SCROLL_SPEEDS.len()`.
    level: usize,
    reversed: bool,
    last: Option<Instant>,
    paused_until: Option<Instant>,
}

impl Default for AutoScroll {
    fn default() -> Self {
        Self {
            level: DEFAULT_AUTO_SCROLL_LEVEL,
            reversed: false,
            last: None,
            paused_until: None,
        }
    }
}

impl AutoScroll {
    pub fn level(&self) -> usize {
        self.level
    }

    pub fn reversed(&self) -> bool {
        self.reversed
    }

    /// One level faster. Whether it changed, so the fastest level says so.
    pub fn faster(&mut self) -> bool {
        self.set_level(self.level + 1)
    }

    /// One level slower. Whether it changed.
    pub fn slower(&mut self) -> bool {
        self.set_level(self.level.saturating_sub(1))
    }

    fn set_level(&mut self, level: usize) -> bool {
        let level = level.clamp(1, AUTO_SCROLL_SPEEDS.len());
        let changed = level != self.level;
        self.level = level;
        changed
    }

    /// Scroll the other way.
    pub fn reverse(&mut self) {
        self.reversed = !self.reversed;
    }

    /// The user touched the document: stop moving until
    /// [`AUTO_SCROLL_RESUME_AFTER`] from now.
    pub fn pause(&mut self, now: Instant) {
        self.paused_until = Some(now + AUTO_SCROLL_RESUME_AFTER);
        self.last = None;
    }

    pub fn is_paused(&self, now: Instant) -> bool {
        self.paused_until.is_some_and(|until| now < until)
    }

    /// How far to scroll for a frame drawn at `now`, in view pixels:
    /// positive toward the end of the document. The first frame after a
    /// start or a pause only sets the clock and moves nothing.
    pub fn tick(&mut self, now: Instant) -> f32 {
        if self.is_paused(now) {
            return 0.0;
        }
        self.paused_until = None;
        let Some(last) = self.last.replace(now) else {
            return 0.0;
        };
        let elapsed = now
            .saturating_duration_since(last)
            .min(AUTO_SCROLL_MAX_STEP);
        let distance = AUTO_SCROLL_SPEEDS[self.level - 1] * elapsed.as_secs_f32();
        if self.reversed {
            -distance
        } else {
            distance
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn the_first_frame_sets_the_clock_and_later_ones_move_at_the_speed() {
        let start = Instant::now();
        let mut scroll = AutoScroll::default();

        assert_eq!(scroll.tick(start), 0.0);
        let speed = AUTO_SCROLL_SPEEDS[DEFAULT_AUTO_SCROLL_LEVEL - 1];
        assert!(close(
            scroll.tick(start + Duration::from_millis(50)),
            speed * 0.05
        ));
        assert!(close(
            scroll.tick(start + Duration::from_millis(66)),
            speed * 0.016
        ));
    }

    /// A window that was hidden draws no frames; when it comes back the
    /// scroll carries on from where it was rather than jumping.
    #[test]
    fn a_long_gap_moves_at_most_one_step() {
        let start = Instant::now();
        let mut scroll = AutoScroll::default();
        scroll.tick(start);

        let moved = scroll.tick(start + Duration::from_secs(60));

        let speed = AUTO_SCROLL_SPEEDS[DEFAULT_AUTO_SCROLL_LEVEL - 1];
        assert!(
            close(moved, speed * AUTO_SCROLL_MAX_STEP.as_secs_f32()),
            "{moved}"
        );
    }

    #[test]
    fn speed_steps_are_clamped_and_say_when_they_did_nothing() {
        let mut scroll = AutoScroll::default();
        while scroll.faster() {}
        assert_eq!(scroll.level(), AUTO_SCROLL_SPEEDS.len());
        assert!(!scroll.faster());
        while scroll.slower() {}
        assert_eq!(scroll.level(), 1);
        assert!(!scroll.slower());

        let start = Instant::now();
        scroll.tick(start);
        assert!(close(
            scroll.tick(start + Duration::from_millis(100)),
            AUTO_SCROLL_SPEEDS[0] * 0.1
        ));
    }

    #[test]
    fn reversing_scrolls_toward_the_start() {
        let start = Instant::now();
        let mut scroll = AutoScroll::default();
        scroll.reverse();
        assert!(scroll.reversed());
        scroll.tick(start);

        assert!(scroll.tick(start + Duration::from_millis(50)) < 0.0);
    }

    /// Touching the document holds the scroll for `AUTO_SCROLL_RESUME_AFTER`,
    /// and the first frame after it restarts the clock rather than paying out
    /// the paused time.
    #[test]
    fn a_pause_holds_then_resumes_without_a_jump() {
        let start = Instant::now();
        let mut scroll = AutoScroll::default();
        scroll.tick(start);
        scroll.pause(start);

        assert!(scroll.is_paused(start + Duration::from_secs(1)));
        assert_eq!(scroll.tick(start + Duration::from_secs(1)), 0.0);
        let resumed = start + AUTO_SCROLL_RESUME_AFTER;
        assert!(!scroll.is_paused(resumed));
        assert_eq!(scroll.tick(resumed), 0.0, "the clock restarts");
        assert!(scroll.tick(resumed + Duration::from_millis(20)) > 0.0);
    }
}
