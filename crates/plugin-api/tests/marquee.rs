//! The shared drag gesture: a click is not a drag at any zoom, a drag
//! sweeps a rectangle on its own page, and the gesture ends three ways.

use onionskin_core::{Document, FitMode, PagePoint, PageRect, ViewSize, Viewport};
use onionskin_plugin_api::marquee::{is_drag, Marquee};
use onionskin_plugin_api::Overlay;

/// Two 200-point pages, fitted into the view.
fn viewport() -> Viewport {
    let bytes = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 200] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n";
    let mut doc = Document::open_bytes(bytes.to_vec()).expect("opens, repaired");
    let mut viewport = Viewport::new(
        doc.page_count(),
        ViewSize {
            width: 400.0,
            height: 400.0,
        },
        12.0,
    )
    .expect("a viewport");
    for page in 0..doc.page_count() {
        let geometry = doc.page_geometry(page).expect("measures").clone();
        viewport.measure_page(geometry).expect("measurable");
    }
    viewport.fit(FitMode::Page).expect("fits");
    viewport
}

fn at(page: usize, x: f64, y: f64) -> PagePoint {
    PagePoint { page, x, y }
}

#[test]
fn a_press_that_barely_moves_is_a_click_and_a_longer_one_a_drag() {
    let viewport = viewport();
    assert!(!is_drag(at(0, 10.0, 10.0), at(0, 10.5, 10.0), &viewport));
    assert!(is_drag(at(0, 10.0, 10.0), at(0, 50.0, 60.0), &viewport));
    assert!(
        !is_drag(at(0, 10.0, 10.0), at(9, 50.0, 60.0), &viewport),
        "a point off the layout is not a drag"
    );
}

#[test]
fn a_drag_sweeps_a_rectangle_on_its_own_page() {
    let viewport = viewport();
    let mut marquee = Marquee::default();
    marquee.extend(at(0, 1.0, 1.0), &viewport);
    assert_eq!(marquee.rect(), None, "nothing before a press");
    marquee.begin(at(0, 50.0, 60.0));
    assert_eq!(marquee.anchor(), Some(at(0, 50.0, 60.0)));
    marquee.extend(at(0, 50.2, 60.0), &viewport);
    assert_eq!(marquee.rect(), None, "a click so far");
    marquee.extend(at(0, 10.0, 100.0), &viewport);
    let swept = PageRect {
        page: 0,
        x0: 10.0,
        y0: 60.0,
        x1: 50.0,
        y1: 100.0,
    };
    assert_eq!(marquee.rect(), Some(swept));
    marquee.extend(at(1, 150.0, 150.0), &viewport);
    assert_eq!(marquee.rect(), Some(swept), "another page is ignored");
    assert_eq!(marquee.overlays(), [Overlay::AntsRect(swept)]);

    marquee.release();
    assert_eq!(marquee.anchor(), None);
    assert_eq!(marquee.rect(), Some(swept), "kept after the release");
    assert_eq!(marquee.finish(), Some(swept));
    assert_eq!(marquee.rect(), None);

    marquee.begin(at(0, 0.0, 0.0));
    marquee.extend(at(0, 100.0, 100.0), &viewport);
    marquee.cancel();
    assert_eq!((marquee.anchor(), marquee.rect()), (None, None));
    assert!(marquee.overlays().is_empty());
}
