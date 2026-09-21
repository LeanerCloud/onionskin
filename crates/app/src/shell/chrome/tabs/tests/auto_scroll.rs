//! P22 on a real window: View > Page Display > Automatically Scroll starts
//! from its keystroke, moves the view frame by frame, takes Acrobat's keys
//! while it runs, pauses when the document is touched, stops at the end and
//! on Escape, and the menu's check follows it.

use std::time::{Duration, Instant};

use super::*;
use crate::shell::canvas::AutoScrollChange;
use crate::shell::chrome::accessible::SHELL_KEY_CONTEXT;

fn window(
    cx: &mut TestAppContext,
) -> (gpui::WindowHandle<ShellFrame>, Vec<crate::keymap::Binding>) {
    bound_window_in(&["two-page.pdf"], crate::config::ConfigPaths::default(), cx)
}

fn canvas(frame: &ShellFrame) -> Entity<Canvas> {
    frame.active_canvas().expect("a tab").clone()
}

/// The View menu's check on Automatically Scroll.
fn checked(frame: &ShellFrame, cx: &App) -> bool {
    main_menu_schema(frame.menu_state(cx))
        .into_iter()
        .flat_map(|section| section.entries)
        .find(|entry| entry.command == MenuCommand::AutoScroll)
        .expect("in the View menu")
        .selected
}

/// Drive `frames` frames 16 ms apart from `start`, as drawing would.
fn run_frames(frame: &ShellFrame, start: Instant, frames: u32, cx: &mut App) {
    canvas(frame).update(cx, |canvas, _| {
        for index in 0..=frames {
            canvas
                .model
                .advance_auto_scroll(start + Duration::from_millis(16 * u64::from(index)))
                .expect("scrolls");
        }
    });
}

fn offset(frame: &ShellFrame, cx: &App) -> f32 {
    canvas(frame).read(cx).model.viewport().offset().y
}

fn level(frame: &ShellFrame, cx: &App) -> Option<(usize, bool)> {
    canvas(frame)
        .read(cx)
        .model
        .auto_scroll()
        .map(|scroll| (scroll.level(), scroll.reversed()))
}

#[gpui::test]
fn the_keystroke_starts_it_frames_move_it_and_escape_stops_it(cx: &mut TestAppContext) {
    let (window, bindings) = window(cx);
    cx.simulate_keystrokes(
        window.into(),
        &keystroke_for(&bindings, "view.automatically-scroll"),
    );
    window
        .update(cx, |frame, _window, cx| {
            assert!(frame.auto_scrolling(cx));
            assert!(checked(frame, cx), "the menu shows it running");
            let before = offset(frame, cx);
            run_frames(frame, Instant::now(), 30, cx);
            assert!(
                offset(frame, cx) > before,
                "the view moved down the document: {before} to {}",
                offset(frame, cx)
            );
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "escape");
    window
        .update(cx, |frame, _window, cx| {
            assert!(!frame.auto_scrolling(cx));
            assert!(!checked(frame, cx));
        })
        .unwrap();
}

/// Up and Down change the speed only while it runs, and minus reverses.
/// Without a scroll the same keys are the focus ring's again.
#[gpui::test]
fn the_arrow_keys_and_minus_steer_a_running_scroll(cx: &mut TestAppContext) {
    let (window, _bindings) = window(cx);
    window
        .update(cx, |frame, _window, cx| frame.toggle_auto_scroll(cx))
        .unwrap();
    cx.run_until_parked();
    let start = window
        .update(cx, |frame, _window, cx| level(frame, cx))
        .unwrap()
        .expect("running");
    cx.simulate_keystrokes(window.into(), "up up");
    cx.simulate_keystrokes(window.into(), "down");
    cx.simulate_keystrokes(window.into(), "-");
    window
        .update(cx, |frame, _window, cx| {
            assert_eq!(level(frame, cx), Some((start.0 + 1, true)));
        })
        .unwrap();

    window
        .update(cx, |frame, _window, cx| frame.toggle_auto_scroll(cx))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            assert_eq!(frame.key_context(cx), SHELL_KEY_CONTEXT);
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "tab");
    let ring = |cx: &mut TestAppContext| {
        window
            .update(cx, |frame, _window, _cx| frame.a11y.focused().cloned())
            .unwrap()
    };
    let before = ring(cx);
    cx.simulate_keystrokes(window.into(), "down");
    assert_ne!(ring(cx), before, "Down moves the focus ring again");
    window
        .update(cx, |frame, _window, cx| {
            assert_eq!(level(frame, cx), None, "no scroll was started by a key");
        })
        .unwrap();
}

/// A wheel scroll or a press on the page holds the scroll, so reading a
/// passage with the pointer on it is not a fight.
#[gpui::test]
fn touching_the_document_pauses_the_scroll(cx: &mut TestAppContext) {
    let (window, _bindings) = window(cx);
    window
        .update(cx, |frame, _window, cx| {
            frame.toggle_auto_scroll(cx);
            canvas(frame).update(cx, |canvas, _| {
                canvas
                    .model
                    .scroll(
                        onionskin_core::ViewPoint { x: 0.0, y: -1.0 },
                        false,
                        gpui::point(px(10.0), px(10.0)),
                    )
                    .expect("scrolls");
                let scroll = canvas.model.auto_scroll().expect("running");
                assert!(scroll.is_paused(Instant::now()));
                // A fresh scroll, then a press on the page.
                canvas.model.toggle_auto_scroll();
                canvas.model.toggle_auto_scroll();
                assert!(!canvas
                    .model
                    .auto_scroll()
                    .unwrap()
                    .is_paused(Instant::now()));
                canvas
                    .model
                    .pointer_down(
                        gpui::point(px(400.0), px(300.0)),
                        1.0,
                        gpui::Modifiers::default(),
                    )
                    .expect("presses");
                assert!(canvas
                    .model
                    .auto_scroll()
                    .unwrap()
                    .is_paused(Instant::now()));
                let scroll = canvas.model.auto_scroll().expect("running");
                assert!(
                    !scroll.is_paused(Instant::now() + onionskin_core::AUTO_SCROLL_RESUME_AFTER)
                );
            });
        })
        .unwrap();
}

/// At the end of the document there is nowhere to go, and the scroll stops
/// itself as Acrobat's does, taking the menu's check with it.
#[gpui::test]
fn reaching_the_end_stops_it(cx: &mut TestAppContext) {
    let (window, _bindings) = window(cx);
    window
        .update(cx, |frame, _window, cx| {
            frame.toggle_auto_scroll(cx);
            canvas(frame).update(cx, |canvas, _| {
                for _ in 0..8 {
                    canvas.model.change_auto_scroll(AutoScrollChange::Faster);
                }
            });
            // 350 px/s for ten minutes of frames is far past two pages.
            let start = Instant::now();
            canvas(frame).update(cx, |canvas, _| {
                let mut index = 0u64;
                while canvas.model.auto_scrolling() && index < 40_000 {
                    canvas
                        .model
                        .advance_auto_scroll(start + Duration::from_millis(16 * index))
                        .expect("scrolls");
                    index += 1;
                }
            });
            assert!(!frame.auto_scrolling(cx), "it stopped at the end");
            assert!(!checked(frame, cx));
        })
        .unwrap();
}
