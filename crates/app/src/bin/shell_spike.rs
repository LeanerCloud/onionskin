//! M1 spike (c): a GPUI window showing a rendered page with pan/zoom/pinch.
//!
//! Throwaway-permitted spike code. It stands in for the real canvas: the page
//! raster is generated here rather than coming from `crates/render`, so this
//! builds and runs on its own.
//!
//! `ONIONSKIN_SPIKE_ZOOM=<factor>` applies one `zoom_at` toward the viewport
//! centre right after fit-to-window, which is how the zoom evidence capture
//! drives it without synthetic input events.

use std::sync::Arc;

use gpui::{
    canvas, div, point, px, size, App, AppContext as _, Application, Bounds, Context,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, PinchEvent, Pixels, Point, Render, RenderImage,
    ScrollWheelEvent, Styled as _, TitlebarOptions, TouchPhase, Window, WindowBounds,
    WindowOptions,
};
use smallvec::smallvec;

const PAGE_W: u32 = 850;
const PAGE_H: u32 = 1100;
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;

/// A US-Letter-ish page: white sheet, black text-like bars, one yellow
/// highlight, a red header rule. Stands in for a rendered PDF page.
fn placeholder_page() -> Arc<RenderImage> {
    // `RenderImage` takes BGRA despite the `RgbaImage` type.
    let mut buf = vec![0u8; (PAGE_W * PAGE_H * 4) as usize];
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32, b: u8, g: u8, r: u8| {
        for y in y0..(y0 + h).min(PAGE_H) {
            for x in x0..(x0 + w).min(PAGE_W) {
                let i = ((y * PAGE_W + x) * 4) as usize;
                buf[i] = b;
                buf[i + 1] = g;
                buf[i + 2] = r;
                buf[i + 3] = 255;
            }
        }
    };

    fill(0, 0, PAGE_W, PAGE_H, 255, 255, 255);
    fill(0, 0, PAGE_W, 4, 60, 60, 60);
    fill(0, PAGE_H - 4, PAGE_W, 4, 60, 60, 60);
    fill(0, 0, 4, PAGE_H, 60, 60, 60);
    fill(PAGE_W - 4, 0, 4, PAGE_H, 60, 60, 60);

    // Title bar and rule.
    fill(70, 90, 420, 26, 20, 20, 20);
    fill(70, 132, 710, 3, 40, 40, 200);

    // Paragraphs of text-like bars, with one line highlighted.
    let mut y = 180;
    let mut seed: u32 = 0x5eed;
    for para in 0..7 {
        for line in 0..5 {
            let mut rand = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 16) & 0xff
            };
            let short = line == 4;
            let w = if short {
                240 + rand() * 2
            } else {
                640 + rand() / 4
            };
            if para == 2 && line == 1 {
                fill(64, y - 4, w + 12, 22, 90, 235, 255);
            }
            fill(70, y, w, 13, 25, 25, 25);
            y += 26;
        }
        y += 22;
    }

    let img =
        image::RgbaImage::from_raw(PAGE_W, PAGE_H, buf).expect("page buffer is PAGE_W*PAGE_H*4");
    Arc::new(RenderImage::new(smallvec![image::Frame::new(img)]))
}

struct Spike {
    page: Arc<RenderImage>,
    /// Top-left of the page in window coordinates.
    origin: Point<Pixels>,
    zoom: f32,
    /// Fit-to-window runs on the first paint, once the viewport size is known.
    fitted: bool,
    drag_from: Option<Point<Pixels>>,
}

impl Spike {
    fn new() -> Self {
        Self {
            page: placeholder_page(),
            origin: point(px(0.0), px(0.0)),
            zoom: 1.0,
            fitted: false,
            drag_from: None,
        }
    }

    /// The spike has no status bar, so view state goes to stdout instead;
    /// that is what the evidence runs read back.
    fn log(&self, what: &str) {
        println!(
            "{what}: zoom={:.4} origin=({:.1}, {:.1})",
            self.zoom,
            f32::from(self.origin.x),
            f32::from(self.origin.y)
        );
    }

    fn fit(&mut self, viewport: Bounds<Pixels>) {
        let zoom = (f32::from(viewport.size.width) / PAGE_W as f32)
            .min(f32::from(viewport.size.height) / PAGE_H as f32)
            * 0.96;
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.origin = point(
            viewport.origin.x + (viewport.size.width - px(PAGE_W as f32 * self.zoom)) / 2.0,
            viewport.origin.y + (viewport.size.height - px(PAGE_H as f32 * self.zoom)) / 2.0,
        );
    }

    /// Scale by `factor`, keeping the page point currently under `anchor`
    /// under `anchor` afterwards.
    fn zoom_at(&mut self, factor: f32, anchor: Point<Pixels>) {
        let next = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = next / self.zoom;
        self.origin = point(
            anchor.x + (self.origin.x - anchor.x) * applied,
            anchor.y + (self.origin.y - anchor.y) * applied,
        );
        self.zoom = next;
    }

    fn pan_by(&mut self, delta: Point<Pixels>) {
        self.origin = point(self.origin.x + delta.x, self.origin.y + delta.y);
    }

    /// One scroll-wheel or touchpad scroll, already reduced to a pixel delta.
    /// Exponential zoom so scrolling back up returns to the zoom you started
    /// from, and a two-pixel touchpad delta still moves it a little.
    fn scroll(&mut self, delta: Point<Pixels>, zooming: bool, at: Point<Pixels>) {
        if !zooming {
            self.pan_by(delta);
            return;
        }
        let steps = f32::from(delta.y) / 240.0;
        if steps.abs() > f32::EPSILON {
            self.zoom_at(2f32.powf(steps), at);
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        self.drag_from = Some(ev.position);
        cx.notify();
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _w: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.drag_from else { return };
        self.pan_by(point(ev.position.x - from.x, ev.position.y - from.y));
        self.drag_from = Some(ev.position);
        self.log("drag-pan");
        cx.notify();
    }

    fn on_mouse_up(&mut self, _ev: &MouseUpEvent, _w: &mut Window, cx: &mut Context<Self>) {
        self.drag_from = None;
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _w: &mut Window, cx: &mut Context<Self>) {
        // Touchpads send many small pixel deltas, wheels a few line-sized
        // ones; 30px per line puts them on a comparable footing.
        let delta = ev.delta.pixel_delta(px(30.0));
        let zooming = ev.modifiers.control || ev.modifiers.platform;
        self.scroll(delta, zooming, ev.position);
        self.log(if zooming { "scroll-zoom" } else { "scroll-pan" });
        cx.notify();
    }

    /// `delta` is the multiplicative change since the previous event of the
    /// gesture, so it composes straight into `zoom_at`.
    fn pinch(&mut self, delta: f32, at: Point<Pixels>) -> bool {
        if !(delta.is_finite() && delta > 0.0) {
            return false;
        }
        self.zoom_at(delta, at);
        true
    }

    fn on_pinch(&mut self, ev: &PinchEvent, _w: &mut Window, cx: &mut Context<Self>) {
        if ev.phase != TouchPhase::Moved || !self.pinch(ev.delta, ev.position) {
            return;
        }
        self.log("pinch-zoom");
        cx.notify();
    }
}

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .id("canvas")
            .size_full()
            .overflow_hidden()
            .bg(gpui::rgb(0x2c2c30))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, ev, w, cx| s.on_mouse_down(ev, w, cx)),
            )
            .on_mouse_move(cx.listener(|s, ev, w, cx| s.on_mouse_move(ev, w, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|s, ev, w, cx| s.on_mouse_up(ev, w, cx)),
            )
            .on_scroll_wheel(cx.listener(|s, ev, w, cx| s.on_scroll(ev, w, cx)))
            .on_pinch(cx.listener(|s, ev, w, cx| s.on_pinch(ev, w, cx)))
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        entity.update(cx, |s, _cx| {
                            if !s.fitted {
                                s.fit(bounds);
                                s.fitted = true;
                                s.log("fit");
                                if let Some(factor) = std::env::var("ONIONSKIN_SPIKE_ZOOM")
                                    .ok()
                                    .and_then(|v| v.parse::<f32>().ok())
                                {
                                    s.zoom_at(factor, bounds.center());
                                    s.log("env-zoom");
                                }
                            }
                            (
                                Bounds {
                                    origin: s.origin,
                                    size: size(
                                        px(PAGE_W as f32 * s.zoom),
                                        px(PAGE_H as f32 * s.zoom),
                                    ),
                                },
                                s.page.clone(),
                            )
                        })
                    },
                    move |_bounds,
                          (page_bounds, page): (Bounds<Pixels>, Arc<RenderImage>),
                          window,
                          _cx| {
                        window
                            .paint_image(page_bounds, gpui::Corners::default(), page, 0, false)
                            .expect("paint page raster");
                    },
                )
                .size_full(),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.0), px(860.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Onionskin".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| Spike::new()),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}

/// Live event injection needs an accessibility grant this machine does not
/// have, so the view transitions the GPUI handlers delegate to are pinned
/// here instead. Run with
/// `cargo test --features shell --bin shell_spike`.
#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Bounds<Pixels> = Bounds {
        origin: Point {
            x: px(0.0),
            y: px(0.0),
        },
        size: gpui::Size {
            width: px(1100.0),
            height: px(861.0),
        },
    };

    fn fitted() -> Spike {
        let mut spike = Spike::new();
        spike.fit(VIEWPORT);
        spike
    }

    /// Which point of the page currently sits under a window coordinate.
    /// Anchored zoom means this does not move.
    fn page_point(spike: &Spike, at: Point<Pixels>) -> (f32, f32) {
        (
            (f32::from(at.x) - f32::from(spike.origin.x)) / spike.zoom,
            (f32::from(at.y) - f32::from(spike.origin.y)) / spike.zoom,
        )
    }

    #[test]
    fn fit_to_window_shows_the_whole_page_centred() {
        let spike = fitted();
        let w = PAGE_W as f32 * spike.zoom;
        let h = PAGE_H as f32 * spike.zoom;
        assert!(
            w <= 1100.0 && h <= 861.0,
            "page {w}x{h} exceeds the viewport"
        );
        // Centred means equal margins on both axes.
        assert!((f32::from(spike.origin.x) - (1100.0 - w) / 2.0).abs() < 0.01);
        assert!((f32::from(spike.origin.y) - (861.0 - h) / 2.0).abs() < 0.01);
    }

    #[test]
    fn a_plain_scroll_pans_without_changing_zoom() {
        let mut spike = fitted();
        let before = (spike.zoom, spike.origin);
        spike.scroll(point(px(-12.0), px(-30.0)), false, point(px(0.0), px(0.0)));
        assert_eq!(spike.zoom, before.0);
        assert_eq!(spike.origin.x, before.1.x - px(12.0));
        assert_eq!(spike.origin.y, before.1.y - px(30.0));
    }

    #[test]
    fn a_modified_scroll_zooms_and_holds_the_page_point_under_the_pointer() {
        let mut spike = fitted();
        let before = spike.zoom;
        let pointer = point(px(300.0), px(220.0));
        let under_pointer = page_point(&spike, pointer);

        spike.scroll(point(px(0.0), px(240.0)), true, pointer);

        assert!(
            (spike.zoom - before * 2.0).abs() < 1e-4,
            "240px of scroll should double zoom, got {}",
            spike.zoom
        );
        let after = page_point(&spike, pointer);
        assert!(
            (under_pointer.0 - after.0).abs() < 0.01,
            "{under_pointer:?} vs {after:?}"
        );
        assert!(
            (under_pointer.1 - after.1).abs() < 0.01,
            "{under_pointer:?} vs {after:?}"
        );
    }

    #[test]
    fn scrolling_back_up_returns_to_the_zoom_it_started_from() {
        let mut spike = fitted();
        let before = spike.zoom;
        let pointer = point(px(400.0), px(400.0));
        spike.scroll(point(px(0.0), px(180.0)), true, pointer);
        spike.scroll(point(px(0.0), px(-180.0)), true, pointer);
        assert!(
            (spike.zoom - before).abs() < 1e-4,
            "{} != {before}",
            spike.zoom
        );
    }

    #[test]
    fn a_pinch_zooms_by_its_factor_about_the_gesture_centre() {
        let mut spike = fitted();
        let before = spike.zoom;
        let centre = point(px(550.0), px(430.0));
        let under_centre = page_point(&spike, centre);

        assert!(spike.pinch(1.5, centre));

        assert!((spike.zoom - before * 1.5).abs() < 1e-4);
        // The gesture centre is a fixed point of the transform.
        let after = page_point(&spike, centre);
        assert!(
            (under_centre.0 - after.0).abs() < 0.01,
            "{under_centre:?} vs {after:?}"
        );
        assert!(
            (under_centre.1 - after.1).abs() < 0.01,
            "{under_centre:?} vs {after:?}"
        );
    }

    #[test]
    fn a_pinch_with_a_nonsense_delta_is_ignored() {
        let mut spike = fitted();
        let before = (spike.zoom, spike.origin);
        for delta in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(!spike.pinch(delta, point(px(10.0), px(10.0))));
        }
        assert_eq!(spike.zoom, before.0);
        assert_eq!(spike.origin, before.1);
    }

    #[test]
    fn zoom_stays_within_its_limits() {
        let mut spike = fitted();
        let corner = point(px(0.0), px(0.0));
        for _ in 0..40 {
            spike.pinch(2.0, corner);
        }
        assert_eq!(spike.zoom, MAX_ZOOM);
        for _ in 0..80 {
            spike.pinch(0.5, corner);
        }
        assert_eq!(spike.zoom, MIN_ZOOM);
    }
}
