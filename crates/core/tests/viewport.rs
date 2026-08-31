use std::time::{Duration, Instant};

use onionskin_core::{
    Document, FitMode, LayoutError, PageAlignment, PageGeometry, PageLayoutMode, PagePoint,
    PageRenderRect, ViewPoint, ViewRect, ViewRotation, ViewSize, Viewport, ViewportError,
    ZoomPolicy,
};
use onionskin_render::raster_size;

const VIEWPORT: ViewSize = ViewSize {
    width: 1_100.0,
    height: 861.0,
};

fn viewport(page_count: usize) -> Viewport {
    let mut viewport = Viewport::new(page_count, VIEWPORT, 12.0).expect("viewport is valid");
    if page_count > 0 {
        viewport
            .measure_page(geometry(0, 850.0, 1_100.0))
            .expect("page 0 is valid");
    }
    viewport
}

fn page(viewport: &Viewport, index: usize) -> ViewRect {
    viewport
        .visible_pages()
        .expect("layout succeeds")
        .into_iter()
        .find(|placement| placement.page == index)
        .unwrap_or_else(|| panic!("page {index} is visible"))
        .rect
}

fn render_point(viewport: &Viewport, index: usize, at: ViewPoint) -> ViewPoint {
    let page = page(viewport, index);
    ViewPoint {
        x: (at.x - page.origin.x) / viewport.zoom(),
        y: (at.y - page.origin.y) / viewport.zoom(),
    }
}

#[test]
fn fit_to_window_shows_the_whole_page_centred() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Page).unwrap();
    let page = page(&viewport, 0);
    assert!(page.size.width <= VIEWPORT.width && page.size.height <= VIEWPORT.height);
    assert!((page.origin.x - (VIEWPORT.width - page.size.width) / 2.0).abs() < 0.01);
    assert!((page.origin.y - (VIEWPORT.height - page.size.height) / 2.0).abs() < 0.01);
}

#[test]
fn a_plain_scroll_pans_without_changing_zoom() {
    let mut viewport = viewport(1);
    viewport.zoom_to(2.0, center()).unwrap();
    let before = (viewport.zoom(), page(&viewport, 0));
    viewport
        .scroll(
            ViewPoint { x: -12.0, y: -30.0 },
            false,
            ViewPoint::default(),
        )
        .unwrap();
    let after = page(&viewport, 0);
    assert_eq!(viewport.zoom(), before.0);
    assert_eq!(after.origin.x, before.1.origin.x - 12.0);
    assert_eq!(after.origin.y, before.1.origin.y - 30.0);
}

/// A vertical navigation must not also pan the reader sideways.
/// `scroll_origin_for_page` returned a hardcoded `x: 0.0`, so every page jump
/// threw away the horizontal position of a reader zoomed in past the window.
#[test]
fn a_page_jump_keeps_the_horizontal_pan() {
    let mut viewport = viewport(10);
    viewport.zoom_to(2.0, center()).unwrap();
    viewport.pan_by(ViewPoint { x: -200.0, y: 0.0 }).unwrap();
    let panned = viewport.offset().x;
    assert!(panned > 0.0, "the test needs a page wider than the window");

    viewport.go_to_page(4, PageAlignment::Start).unwrap();
    assert_eq!(viewport.offset().x, panned);
}

#[test]
fn a_modified_scroll_zooms_and_holds_the_page_point_under_the_pointer() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Page).unwrap();
    let pointer = ViewPoint { x: 300.0, y: 220.0 };
    let before_zoom = viewport.zoom();
    let before = render_point(&viewport, 0, pointer);
    viewport
        .scroll(ViewPoint { x: 0.0, y: 240.0 }, true, pointer)
        .unwrap();
    assert!((viewport.zoom() - before_zoom * 2.0).abs() < 1e-4);
    assert_point_close(render_point(&viewport, 0, pointer), before);
}

#[test]
fn scrolling_back_up_returns_to_the_zoom_it_started_from() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Page).unwrap();
    let before = viewport.zoom();
    let pointer = ViewPoint { x: 400.0, y: 400.0 };
    viewport.dynamic_zoom(180.0, pointer).unwrap();
    viewport.dynamic_zoom(-180.0, pointer).unwrap();
    assert!((viewport.zoom() - before).abs() < 1e-4);
}

#[test]
fn a_pinch_zooms_by_its_factor_about_the_gesture_centre() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Page).unwrap();
    let centre = center();
    let before = render_point(&viewport, 0, centre);
    viewport.pinch(1.5, centre).unwrap();
    assert_point_close(render_point(&viewport, 0, centre), before);
}

/// `pinch` and `zoom_at` answer the same way to the same nonsense factor.
/// `pinch` used to swallow it as `Ok(false)`, so a caller that checked its
/// result saw a refusal from one and a no-op from the other.
#[test]
fn a_pinch_with_a_nonsense_factor_is_refused_the_way_zoom_at_refuses_it() {
    let mut viewport = viewport(1);
    let before = viewport.snapshot();
    for factor in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            viewport.pinch(factor, center()),
            Err(ViewportError::Layout(LayoutError::InvalidZoom(bad))) if bad.to_bits() == factor.to_bits()
        ));
        assert!(viewport.zoom_at(factor, center()).is_err());
    }
    assert_eq!(viewport.snapshot(), before);
}

#[test]
fn zoom_stays_within_its_limits() {
    let mut viewport = viewport(1);
    for _ in 0..40 {
        viewport.pinch(2.0, center()).unwrap();
    }
    assert_eq!(viewport.zoom(), 32.0);
    for _ in 0..80 {
        viewport.pinch(0.5, center()).unwrap();
    }
    assert_eq!(viewport.zoom(), 0.05);
}

#[test]
fn fit_width_is_re_applied_when_the_viewport_resizes() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Width).unwrap();
    let before = viewport.zoom();
    viewport
        .resize(ViewSize {
            width: 1_500.0,
            height: 861.0,
        })
        .unwrap();
    assert!(viewport.zoom() > before);
    assert_eq!(viewport.zoom_policy(), ZoomPolicy::Fit(FitMode::Width));
}

#[test]
fn layout_mode_and_cover_changes_stay_in_core() {
    let mut viewport = viewport(5);
    viewport
        .set_mode(PageLayoutMode::TwoPageContinuous)
        .unwrap();
    viewport.set_show_cover(true).unwrap();
    assert_eq!(viewport.mode(), PageLayoutMode::TwoPageContinuous);
    assert!(viewport.show_cover());
    let visible = viewport.visible_pages().unwrap();
    let cover_y = page(&viewport, 0).origin.y;
    assert!(visible.iter().any(|page| page.page == 0));
    assert!(!visible
        .iter()
        .any(|placement| placement.page == 1 && placement.rect.origin.y == cover_y));
}

#[test]
fn a_fixed_zoom_is_not_changed_by_resize() {
    let mut viewport = viewport(1);
    viewport.zoom_to(2.5, center()).unwrap();
    viewport
        .resize(ViewSize {
            width: 1_500.0,
            height: 1_000.0,
        })
        .unwrap();
    assert_eq!(viewport.zoom(), 2.5);
}

#[test]
fn actual_size_and_each_fit_mode_are_distinct() {
    let mut viewport = viewport(1);
    viewport.actual_size().unwrap();
    assert_eq!(viewport.zoom(), 1.0);
    viewport.fit(FitMode::Width).unwrap();
    let width = viewport.zoom();
    viewport.fit(FitMode::Height).unwrap();
    let height = viewport.zoom();
    viewport.fit(FitMode::Page).unwrap();
    assert_eq!(viewport.zoom(), width.min(height));

    let bounds = PageRenderRect::new(
        0,
        ViewPoint { x: 100.0, y: 200.0 },
        ViewSize {
            width: 200.0,
            height: 300.0,
        },
        ViewSize {
            width: 850.0,
            height: 1_100.0,
        },
    )
    .unwrap();
    viewport.fit(FitMode::Visible(bounds)).unwrap();
    assert!(viewport.zoom() > width.min(height));
    let page = page(&viewport, 0);
    assert_point_close(
        ViewPoint {
            x: page.origin.x + (bounds.origin().x + bounds.size().width / 2.0) * viewport.zoom(),
            y: page.origin.y + (bounds.origin().y + bounds.size().height / 2.0) * viewport.zoom(),
        },
        center(),
    );
}

#[test]
fn dynamic_zoom_uses_the_scroll_formula_and_rejects_non_finite_deltas() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Page).unwrap();
    let anchor = ViewPoint { x: 420.0, y: 330.0 };
    let before_zoom = viewport.zoom();
    let before_point = render_point(&viewport, 0, anchor);
    viewport.dynamic_zoom(240.0, anchor).unwrap();
    assert!((viewport.zoom() - before_zoom * 2.0).abs() < 1e-4);
    assert_point_close(render_point(&viewport, 0, anchor), before_point);
    assert!(viewport.dynamic_zoom(f32::NAN, anchor).is_err());

    viewport.dynamic_zoom(f32::MAX, anchor).unwrap();
    assert_eq!(viewport.zoom(), 32.0);
    viewport.dynamic_zoom(f32::MIN, anchor).unwrap();
    assert_eq!(viewport.zoom(), 0.05);
}

#[test]
fn background_anchored_zoom_keeps_the_current_page_centre_stable() {
    let mut viewport = viewport(1);
    viewport.fit(FitMode::Page).unwrap();
    let before = page(&viewport, 0);
    viewport.zoom_at(1.1, ViewPoint::default()).unwrap();
    let after = page(&viewport, 0);
    assert_point_close(rect_center(before), rect_center(after));
}

#[test]
fn page_navigation_covers_every_edge_and_page_900() {
    let mut viewport = viewport(1_000);
    for index in 1..12 {
        viewport
            .measure_page(geometry(index, 830.0, 1_080.0))
            .unwrap();
    }
    viewport.go_to_page(899, PageAlignment::Start).unwrap();
    assert_eq!(viewport.current_page(), 899);
    assert!(viewport
        .visible_pages()
        .unwrap()
        .iter()
        .any(|page| page.page == 899));
    viewport.first_page().unwrap();
    viewport.previous_page().unwrap();
    assert_eq!(viewport.current_page(), 0);
    viewport.last_page().unwrap();
    viewport.next_page().unwrap();
    assert_eq!(viewport.current_page(), 999);
}

#[test]
fn navigating_away_from_fit_visible_keeps_the_zoom_and_clears_the_fit() {
    let mut viewport = viewport(2);
    let bounds = PageRenderRect::new(
        0,
        ViewPoint { x: 100.0, y: 200.0 },
        ViewSize {
            width: 200.0,
            height: 300.0,
        },
        ViewSize {
            width: 850.0,
            height: 1_100.0,
        },
    )
    .unwrap();
    viewport.fit(FitMode::Visible(bounds)).unwrap();
    let zoom = viewport.zoom();

    viewport.next_page().unwrap();

    assert_eq!(viewport.current_page(), 1);
    assert_eq!(viewport.zoom(), zoom);
    assert_eq!(viewport.zoom_policy(), ZoomPolicy::Fixed);
}

#[test]
fn a_failed_page_jump_does_not_clear_fit_visible() {
    let mut viewport = viewport(2);
    let bounds = PageRenderRect::new(
        0,
        ViewPoint { x: 100.0, y: 200.0 },
        ViewSize {
            width: 200.0,
            height: 300.0,
        },
        ViewSize {
            width: 850.0,
            height: 1_100.0,
        },
    )
    .unwrap();
    viewport.fit(FitMode::Visible(bounds)).unwrap();
    let before = viewport.snapshot();

    assert!(viewport.go_to_page(2, PageAlignment::Start).is_err());

    assert_eq!(viewport.snapshot(), before);
}

#[test]
fn an_unmeasured_later_page_can_be_rotated_without_loading_it() {
    let mut viewport = viewport(1_000);
    viewport.go_to_page(899, PageAlignment::Start).unwrap();

    viewport.set_rotation(ViewRotation::Clockwise90).unwrap();

    assert_eq!(viewport.current_page(), 899);
    assert!(viewport
        .visible_pages()
        .unwrap()
        .iter()
        .any(|page| page.page == 899 && !page.measured));
}

#[test]
fn a_sparse_measurement_before_page_zero_is_retained() {
    let mut viewport = Viewport::new(2, VIEWPORT, 12.0).unwrap();

    viewport.measure_page(geometry(1, 700.0, 900.0)).unwrap();
    viewport.measure_page(geometry(0, 850.0, 1_100.0)).unwrap();
    viewport.go_to_page(1, PageAlignment::Start).unwrap();

    assert!(viewport
        .visible_pages()
        .unwrap()
        .iter()
        .any(|page| page.page == 1 && page.measured));
}

#[test]
fn continuous_scroll_updates_the_current_page() {
    let mut viewport = viewport(4);
    viewport.pan_by(ViewPoint { x: 0.0, y: -900.0 }).unwrap();
    assert!(viewport.current_page() > 0);
}

#[test]
fn measuring_a_page_above_the_view_keeps_the_current_page_anchor() {
    let mut viewport = viewport(20);
    viewport.go_to_page(10, PageAlignment::Start).unwrap();
    let before = page(&viewport, 10).origin;
    viewport.measure_page(geometry(5, 900.0, 1_500.0)).unwrap();
    assert_point_close(page(&viewport, 10).origin, before);
}

#[test]
fn an_estimated_page_cannot_be_hit_tested_as_if_it_were_measured() {
    let mut viewport = viewport(2);
    viewport.go_to_page(1, PageAlignment::Center).unwrap();
    let rect = page(&viewport, 1);
    let result = viewport.page_point_at(rect_center(rect));
    assert!(matches!(result, Err(ViewportError::UnmeasuredPage(1))));
    assert_eq!(
        viewport
            .page_point_at(ViewPoint {
                x: -100.0,
                y: -100.0
            })
            .unwrap(),
        None
    );
}

#[test]
fn view_rotation_and_page_hit_testing_use_the_core_geometry_mapping() {
    let bytes = geometry_pdf(90);
    let mut document = Document::open_bytes(bytes).unwrap();
    let geometry = document.page_geometry(0).unwrap().clone();
    let mut viewport = Viewport::new(1, VIEWPORT, 12.0).unwrap();
    viewport.measure_page(geometry.clone()).unwrap();

    for rotation in [
        ViewRotation::Clockwise90,
        ViewRotation::HalfTurn,
        ViewRotation::Clockwise270,
    ] {
        viewport.set_rotation(rotation).unwrap();
        viewport.fit(FitMode::Page).unwrap();
        let intrinsic = ViewPoint { x: 30.0, y: 45.0 };
        let rotated = rotate_point(rotation, intrinsic, geometry.render_size);
        let rect = page(&viewport, 0);
        let screen = ViewPoint {
            x: rect.origin.x + rotated.x * viewport.zoom(),
            y: rect.origin.y + rotated.y * viewport.zoom(),
        };
        let actual = viewport.page_point_at(screen).unwrap().unwrap();
        let expected = geometry
            .device_to_user(
                f64::from(intrinsic.x * viewport.zoom()),
                f64::from(intrinsic.y * viewport.zoom()),
                viewport.zoom(),
            )
            .unwrap();
        // The whole mapping runs in `f32` over device pixels of a page that is
        // ~4300 px on its long axis at these fit zooms, where one ulp is
        // already ~5e-4 px; dividing back to user space leaves ~1e-4 pt of
        // slack. Anything tighter tests the rounding, not the mapping.
        assert!((actual.x - expected.x).abs() < 1e-3);
        assert!((actual.y - expected.y).abs() < 1e-3);
    }
}

#[test]
fn a_page_point_maps_back_to_the_viewport_point_it_came_from() {
    let mut document = Document::open_bytes(geometry_pdf(90)).unwrap();
    let geometry = document.page_geometry(0).unwrap().clone();
    let mut viewport = Viewport::new(2, VIEWPORT, 12.0).unwrap();
    viewport.measure_page(geometry.clone()).unwrap();
    let mut second = geometry;
    second.index = 1;
    viewport.measure_page(second).unwrap();

    for rotation in [
        ViewRotation::None,
        ViewRotation::Clockwise90,
        ViewRotation::HalfTurn,
        ViewRotation::Clockwise270,
    ] {
        viewport.set_rotation(rotation).unwrap();
        viewport.zoom_to(1.7, center()).unwrap();
        for page_index in [0, 1] {
            let rect = page(&viewport, page_index);
            let screen = ViewPoint {
                x: rect.origin.x + rect.size.width * 0.37,
                y: rect.origin.y + rect.size.height * 0.61,
            };
            let at = viewport.page_point_at(screen).unwrap().unwrap();
            assert_eq!(at.page, page_index);
            let back = viewport.view_point_for(at).unwrap().unwrap();
            assert!((back.x - screen.x).abs() < 1e-3, "{back:?} != {screen:?}");
            assert!((back.y - screen.y).abs() < 1e-3, "{back:?} != {screen:?}");
        }
    }
}

#[test]
fn mapping_from_an_unmeasured_page_fails_loudly() {
    let viewport = viewport(2);

    assert!(matches!(
        viewport.view_point_for(PagePoint {
            page: 1,
            x: 0.0,
            y: 0.0,
        }),
        Err(ViewportError::UnmeasuredPage(1))
    ));
}

#[test]
fn a_page_outside_a_non_continuous_spread_has_no_viewport_point() {
    let mut viewport = viewport(2);
    viewport.measure_page(geometry(1, 850.0, 1_100.0)).unwrap();
    viewport.set_mode(PageLayoutMode::SinglePage).unwrap();

    assert_eq!(
        viewport
            .view_point_for(PagePoint {
                page: 1,
                x: 10.0,
                y: 10.0,
            })
            .unwrap(),
        None
    );
}

#[test]
fn invalid_viewport_inputs_fail_loudly() {
    assert!(Viewport::new(
        1,
        ViewSize {
            width: 0.0,
            height: 1.0,
        },
        12.0,
    )
    .is_err());
    let mut viewport = viewport(1);
    assert!(viewport.zoom_to(f32::NAN, center()).is_err());
    viewport.zoom_to(2.0, center()).unwrap();
    assert!(matches!(
        viewport.zoom_at(f32::MAX, center()),
        Err(ViewportError::Layout(LayoutError::InvalidZoom(zoom))) if zoom.is_infinite()
    ));
    assert!(viewport
        .pan_by(ViewPoint {
            x: f32::INFINITY,
            y: 0.0,
        })
        .is_err());

    let wrong_page = PageRenderRect::new(
        1,
        ViewPoint::default(),
        ViewSize {
            width: 100.0,
            height: 100.0,
        },
        ViewSize {
            width: 850.0,
            height: 1_100.0,
        },
    )
    .unwrap();
    assert!(matches!(
        viewport.fit(FitMode::Visible(wrong_page)),
        Err(ViewportError::FitVisiblePageMismatch { .. })
    ));

    let mut empty = Viewport::new(0, VIEWPORT, 12.0).unwrap();
    assert!(empty.visible_pages().unwrap().is_empty());
    assert!(matches!(empty.next_page(), Err(ViewportError::NoPages)));
}

#[test]
fn zoom_anchors_must_lie_inside_the_viewport() {
    for anchor in [
        ViewPoint { x: -1.0, y: 0.0 },
        ViewPoint {
            x: VIEWPORT.width + 1.0,
            y: 0.0,
        },
        ViewPoint {
            x: 0.0,
            y: VIEWPORT.height + 1.0,
        },
    ] {
        let mut view = viewport(1);
        assert!(matches!(
            view.zoom_to(2.0, anchor),
            Err(ViewportError::AnchorOutsideViewport(point)) if point == anchor
        ));

        let mut view = viewport(1);
        assert!(matches!(
            view.zoom_at(2.0, anchor),
            Err(ViewportError::AnchorOutsideViewport(point)) if point == anchor
        ));

        let mut view = viewport(1);
        assert!(matches!(
            view.scroll(ViewPoint { x: 0.0, y: 240.0 }, true, anchor),
            Err(ViewportError::AnchorOutsideViewport(point)) if point == anchor
        ));

        let mut view = viewport(1);
        assert!(matches!(
            view.pinch(2.0, anchor),
            Err(ViewportError::AnchorOutsideViewport(point)) if point == anchor
        ));

        let mut view = viewport(1);
        assert!(matches!(
            view.dynamic_zoom(240.0, anchor),
            Err(ViewportError::AnchorOutsideViewport(point)) if point == anchor
        ));
    }
}

#[test]
fn restoring_fit_visible_revalidates_the_target_document() {
    let source = viewport(1);
    let valid_bounds = PageRenderRect::new(
        0,
        ViewPoint { x: 500.0, y: 500.0 },
        ViewSize {
            width: 300.0,
            height: 300.0,
        },
        ViewSize {
            width: 850.0,
            height: 1_100.0,
        },
    )
    .unwrap();
    let mut state = source.snapshot();
    state.zoom_policy = ZoomPolicy::Fit(FitMode::Visible(valid_bounds));

    let mut empty = Viewport::new(0, VIEWPORT, 12.0).unwrap();
    assert!(matches!(empty.restore(state), Err(ViewportError::NoPages)));

    let page_one_bounds = PageRenderRect::new(
        1,
        ViewPoint::default(),
        ViewSize {
            width: 100.0,
            height: 100.0,
        },
        ViewSize {
            width: 850.0,
            height: 1_100.0,
        },
    )
    .unwrap();
    state.current_page = 1;
    state.zoom_policy = ZoomPolicy::Fit(FitMode::Visible(page_one_bounds));
    let mut unmeasured = viewport(2);
    assert!(matches!(
        unmeasured.restore(state),
        Err(ViewportError::UnmeasuredPage(1))
    ));

    state.current_page = 0;
    state.zoom_policy = ZoomPolicy::Fit(FitMode::Visible(valid_bounds));
    let mut smaller = Viewport::new(1, VIEWPORT, 12.0).unwrap();
    smaller.measure_page(geometry(0, 600.0, 600.0)).unwrap();
    assert!(matches!(
        smaller.restore(state),
        Err(ViewportError::Layout(_))
    ));
    assert!(matches!(
        PageRenderRect::new(
            0,
            ViewPoint { x: 590.0, y: 0.0 },
            ViewSize {
                width: 20.0,
                height: 20.0,
            },
            ViewSize {
                width: 600.0,
                height: 600.0,
            },
        ),
        Err(LayoutError::RectOutsidePage { page: 0 })
    ));
}

#[test]
fn restore_preserves_a_reachable_anchor_offset_outside_scroll_clamps() {
    let mut source = viewport(1);
    source.fit(FitMode::Page).unwrap();
    let anchor = page(&source, 0).origin;
    source.zoom_at(2.0, anchor).unwrap();
    let state = source.snapshot();
    assert!(state.offset.x < 0.0);

    let mut restored = viewport(1);
    restored.restore(state).unwrap();

    assert_eq!(restored.snapshot(), state);
    assert!(restored
        .visible_pages()
        .unwrap()
        .iter()
        .any(|page| page.page == state.current_page));
}

fn center() -> ViewPoint {
    ViewPoint {
        x: VIEWPORT.width / 2.0,
        y: VIEWPORT.height / 2.0,
    }
}

fn rect_center(rect: ViewRect) -> ViewPoint {
    ViewPoint {
        x: rect.origin.x + rect.size.width / 2.0,
        y: rect.origin.y + rect.size.height / 2.0,
    }
}

fn assert_point_close(actual: ViewPoint, expected: ViewPoint) {
    assert!(
        (actual.x - expected.x).abs() < 0.02,
        "{} != {}",
        actual.x,
        expected.x
    );
    assert!(
        (actual.y - expected.y).abs() < 0.02,
        "{} != {}",
        actual.y,
        expected.y
    );
}

fn rotate_point(rotation: ViewRotation, point: ViewPoint, size: (f64, f64)) -> ViewPoint {
    let width = size.0 as f32;
    let height = size.1 as f32;
    match rotation {
        ViewRotation::None => point,
        ViewRotation::Clockwise90 => ViewPoint {
            x: height - point.y,
            y: point.x,
        },
        ViewRotation::HalfTurn => ViewPoint {
            x: width - point.x,
            y: height - point.y,
        },
        ViewRotation::Clockwise270 => ViewPoint {
            x: point.y,
            y: width - point.x,
        },
    }
}

fn geometry(index: usize, width: f64, height: f64) -> PageGeometry {
    let mut document = Document::open_bytes(one_page_pdf()).expect("fixture opens");
    let mut geometry = document.page_geometry(0).expect("geometry loads").clone();
    geometry.index = index;
    geometry.render_size = (width, height);
    geometry
}

fn one_page_pdf() -> Vec<u8> {
    geometry_pdf(0)
}

fn geometry_pdf(rotation: i32) -> Vec<u8> {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [10 20 310 220] /CropBox [60 45 260 195] /Rotate {rotation} /Resources <<>> >>"
        )
        .into_bytes(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

#[test]
fn a_measurement_under_fit_does_not_recentre_a_scrolled_page() {
    let mut viewport = viewport(20);
    viewport.fit(FitMode::Page).unwrap();
    viewport.go_to_page(10, PageAlignment::Start).unwrap();
    viewport.pan_by(ViewPoint { x: 0.0, y: -300.0 }).unwrap();
    let before = viewport.offset();
    let zoom = viewport.zoom();
    viewport.measure_page(geometry(11, 850.0, 1_100.0)).unwrap();
    assert_eq!(viewport.zoom_policy(), ZoomPolicy::Fit(FitMode::Page));
    assert_eq!(viewport.zoom(), zoom);
    assert_point_close(viewport.offset(), before);
}

#[test]
fn the_largest_legal_page_stays_inside_what_the_renderer_can_rasterize() {
    // 14400 pt is the largest page a PDF can declare. hayro sizes pixmaps with
    // u16, so this page runs out of raster at about 4.5x, far below the 32x the
    // zoom menu offers; asking for 32x has to be capped, not passed through.
    let mut large = Viewport::new(1, VIEWPORT, 12.0).unwrap();
    large.measure_page(geometry(0, 14_400.0, 14_400.0)).unwrap();
    large.zoom_to(1_000.0, center()).unwrap();
    assert!(
        raster_size(14_400.0, 14_400.0, large.zoom()).is_ok(),
        "zoomed to {} which the renderer cannot rasterize",
        large.zoom()
    );

    // The floor comes off the same limit: under one pixel is unrenderable too.
    let mut small = Viewport::new(1, VIEWPORT, 12.0).unwrap();
    small.measure_page(geometry(0, 3.0, 4.0)).unwrap();
    small.zoom_to(0.001, center()).unwrap();
    assert!(
        raster_size(3.0, 4.0, small.zoom()).is_ok(),
        "zoomed to {} which the renderer cannot rasterize",
        small.zoom()
    );
}

#[test]
fn painting_a_frame_does_not_walk_every_measured_page() {
    const PAGES: usize = 1_000;
    const FRAMES: usize = 1_000;

    let mut viewport = Viewport::new(PAGES, VIEWPORT, 12.0).unwrap();
    let template = geometry(0, 850.0, 1_100.0);
    for index in 0..PAGES {
        let mut page = template.clone();
        page.index = index;
        // Vary the heights so every row carries a correction of its own.
        page.render_size = (850.0, 1_100.0 + (index % 7) as f64 * 10.0);
        viewport.measure_page(page).unwrap();
    }
    viewport
        .go_to_page(PAGES / 2, PageAlignment::Start)
        .unwrap();

    let start = Instant::now();
    for _ in 0..FRAMES {
        viewport.visible_pages().unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(150),
        "{FRAMES} frames over {PAGES} measured pages took {elapsed:?}: \
         the layout is still walking the measured rows per frame"
    );
}
