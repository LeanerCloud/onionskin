//! Draw and Erase Ink, through the real gesture lifecycle, on a real document.
//!
//! Pressure is asserted by what reaches the page - covered pixels - not by
//! reading widths out of the content stream: a tool that accepted pressure and
//! drew every segment at one width would pass a stream-text check that looked
//! for the right operator.

use onionskin_core::{
    read_annotations, Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport,
};
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::{EraseInkTool, InkTool};

const VIEWPORT: ViewSize = ViewSize {
    width: 800.0,
    height: 600.0,
};

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn blank() -> Self {
        let mut doc = Document::open_bytes(blank_page()).expect("document opens");
        let mut viewport = Viewport::new(1, VIEWPORT, 12.0).expect("viewport is valid");
        let geometry = doc.page_geometry(0).expect("page measures").clone();
        viewport.measure_page(geometry).expect("page is measurable");
        viewport.fit(FitMode::Page).expect("the page fits");
        Fixture { doc, viewport }
    }

    /// Drag through `points` at `pressure`, one pointer event per point.
    fn stroke(&mut self, tool: &mut dyn ToolPlugin, points: &[(f64, f64)], pressure: f32) {
        let mut ctx = ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        };
        let (first, rest) = points.split_first().expect("a stroke has a point");
        tool.on_pointer_down(&mut ctx, at(*first, pressure));
        for point in rest {
            tool.on_pointer_move(&mut ctx, at(*point, pressure));
        }
        let last = points.last().expect("a point");
        tool.on_pointer_up(&mut ctx, at(*last, pressure));
    }

    fn undo_entries(&self) -> usize {
        self.doc.edit().history().reach()
    }

    fn inks(&mut self) -> Vec<onionskin_core::ReadAnnotation> {
        let count = self.doc.page_count();
        let current = self.doc.structure().expect("the current document");
        read_annotations(current, count, &Default::default())
            .expect("annotations read")
            .into_iter()
            .filter(|annotation| annotation.raw_subtype == "Ink")
            .collect()
    }

    fn ink_pixels(&mut self) -> usize {
        self.doc
            .render_page_now(0, 2.0)
            .expect("the page renders")
            .raster
            .rgba()
            .chunks_exact(4)
            .filter(|pixel| i32::from(pixel[2]) > i32::from(pixel[0]) + 40)
            .count()
    }
}

fn at((x, y): (f64, f64), pressure: f32) -> PointerInput {
    PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure,
        modifiers: Modifiers::default(),
        clicks: 1,
    }
}

/// A straight horizontal stroke of `events` pointer events.
fn horizontal(from: f64, to: f64, y: f64, events: usize) -> Vec<(f64, f64)> {
    (0..events)
        .map(|index| (from + (to - from) * index as f64 / (events - 1) as f64, y))
        .collect()
}

/// The mutation this must catch: pressure accepted and ignored.
#[test]
fn a_harder_stroke_covers_more_of_the_page() {
    let path = horizontal(100.0, 500.0, 400.0, 40);
    let mut coverage = Vec::new();
    for pressure in [0.2, 1.0] {
        let mut fixture = Fixture::blank();
        fixture.stroke(&mut InkTool::new(), &path, pressure);
        coverage.push(fixture.ink_pixels());
    }
    let (light, hard) = (coverage[0], coverage[1]);
    assert!(light > 0, "the light stroke draws something");
    assert!(
        hard as f64 > light as f64 * 1.8,
        "full pressure is several times the width of a light touch: {light} vs {hard}"
    );
}

/// Several hundred pointer events are one gesture, one transaction, one
/// undo entry.
#[test]
fn a_long_stroke_is_one_undo_entry() {
    let mut fixture = Fixture::blank();
    let path: Vec<(f64, f64)> = (0..400)
        .map(|index| {
            let t = index as f64 / 10.0;
            (100.0 + index as f64, 400.0 + 50.0 * t.sin())
        })
        .collect();
    fixture.stroke(&mut InkTool::new(), &path, 0.7);

    assert_eq!(fixture.undo_entries(), 1);
    let inks = fixture.inks();
    assert_eq!(inks.len(), 1);
    assert_eq!(inks[0].ink.len(), 1, "one stroke");
    assert!(inks[0].ink[0].len() > 100, "and it kept its points");
}

#[test]
fn erasing_the_middle_splits_the_stroke_and_fits_the_rect_to_what_is_left() {
    let mut fixture = Fixture::blank();
    fixture.stroke(
        &mut InkTool::new(),
        &horizontal(100.0, 500.0, 400.0, 30),
        1.0,
    );
    fixture.stroke(
        &mut EraseInkTool::new(),
        &[(300.0, 440.0), (300.0, 360.0)],
        1.0,
    );

    assert_eq!(fixture.undo_entries(), 2, "the draw and the erase");
    let inks = fixture.inks();
    assert_eq!(inks.len(), 1, "the same annotation, rewritten");
    let strokes = &inks[0].ink;
    assert_eq!(strokes.len(), 2, "split in two: {strokes:?}");
    assert!(strokes.iter().all(|stroke| !stroke.is_empty()));
    assert!(strokes[0].iter().all(|(x, _)| *x < 300.0));
    assert!(strokes[1].iter().all(|(x, _)| *x > 300.0));

    let rect = inks[0].rect;
    let half = inks[0].border_width / 2.0;
    let xs = strokes.iter().flatten().map(|(x, _)| *x);
    let (min, max) = xs.fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
    assert!((rect.x0 - (min - half)).abs() < 1e-6, "{rect:?}");
    assert!((rect.x1 - (max + half)).abs() < 1e-6, "{rect:?}");
}

#[test]
fn erasing_the_end_of_a_stroke_shrinks_its_rect() {
    let mut fixture = Fixture::blank();
    fixture.stroke(
        &mut InkTool::new(),
        &horizontal(100.0, 500.0, 400.0, 30),
        1.0,
    );
    let before = fixture.inks()[0].rect;
    fixture.stroke(
        &mut EraseInkTool::new(),
        &horizontal(420.0, 520.0, 400.0, 30),
        1.0,
    );
    let after = fixture.inks()[0].rect;
    assert!(after.x1 < before.x1 - 50.0, "{before:?} -> {after:?}");
    assert_eq!(after.x0, before.x0);
}

#[test]
fn erasing_a_whole_stroke_removes_the_annotation() {
    let mut fixture = Fixture::blank();
    fixture.stroke(
        &mut InkTool::new(),
        &horizontal(100.0, 140.0, 400.0, 10),
        1.0,
    );
    assert_eq!(fixture.inks().len(), 1);
    fixture.stroke(
        &mut EraseInkTool::new(),
        &horizontal(90.0, 150.0, 400.0, 40),
        1.0,
    );
    assert!(fixture.inks().is_empty(), "the page no longer names it");
    assert_eq!(fixture.undo_entries(), 2);
}

#[test]
fn erasing_nothing_is_no_edit_and_no_undo_entry() {
    let mut fixture = Fixture::blank();
    fixture.stroke(
        &mut InkTool::new(),
        &horizontal(100.0, 500.0, 400.0, 30),
        1.0,
    );
    fixture.stroke(
        &mut EraseInkTool::new(),
        &horizontal(100.0, 500.0, 100.0, 30),
        1.0,
    );
    assert_eq!(fixture.undo_entries(), 1);
    assert_eq!(fixture.inks()[0].ink.len(), 1);
}

/// A click is a dot: one point, never zero.
#[test]
fn a_click_draws_a_dot_and_no_empty_stroke() {
    let mut fixture = Fixture::blank();
    fixture.stroke(&mut InkTool::new(), &[(200.0, 200.0)], 1.0);
    let inks = fixture.inks();
    assert_eq!(inks.len(), 1);
    assert!(inks[0].ink.iter().all(|stroke| !stroke.is_empty()));
    assert!(fixture.ink_pixels() > 0, "the dot is visible");
}

fn blank_page() -> Vec<u8> {
    let objects: &[&[u8]] = &[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
    ];
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}
