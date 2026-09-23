//! Text markup, driven headlessly through the real gesture lifecycle.
//!
//! The tools only ever see page-space pointer input and a viewport, so
//! everything they do can be asserted without a window. Same shape as
//! `tools-basic`'s gesture tests.
//!
//! The test that bites is
//! `a_drag_over_two_columns_produces_two_quads_not_one_bounding_box`. An
//! implementation that writes the selection's bounding rectangle passes every
//! other test here and paints the gutter and every intervening line on a real
//! two-column page.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, PageQuad, ViewSize, Viewport};
use onionskin_corpus_testing::seed;
use onionskin_cos::{Object, PendingEdit};
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::MarkupTool;

const VIEWPORT: ViewSize = ViewSize {
    width: 800.0,
    height: 600.0,
};

/// The zoom the render assertions work at. Above 1.0 so a quad's interior is
/// several pixels wider than the antialiased ring around it.
const ZOOM: f32 = 2.0;

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn seed(name: &str) -> Self {
        Self::from_document(Document::open_path(&seed(name)).expect("seed opens"))
    }

    fn bytes(bytes: Vec<u8>) -> Self {
        Self::from_document(Document::open_bytes(bytes).expect("document opens"))
    }

    fn from_document(mut doc: Document) -> Self {
        let mut viewport =
            Viewport::new(doc.page_count(), VIEWPORT, 12.0).expect("viewport is valid");
        for page in 0..doc.page_count() {
            let geometry = doc.page_geometry(page).expect("page measures").clone();
            viewport.measure_page(geometry).expect("page is measurable");
        }
        viewport.fit(FitMode::Page).expect("the page fits");
        Fixture { doc, viewport }
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }

    /// Drag from one glyph to another, through the lifecycle the canvas uses.
    fn drag(&mut self, tool: &mut MarkupTool, from: PagePoint, to: PagePoint, shift: bool) {
        let input = |at: PagePoint| PointerInput {
            at,
            pressure: 1.0,
            modifiers: Modifiers {
                shift,
                ..Modifiers::default()
            },
            clicks: 1,
        };
        let mut ctx = self.ctx();
        tool.on_pointer_down(&mut ctx, input(from));
        tool.on_pointer_move(&mut ctx, input(to));
        tool.on_pointer_up(&mut ctx, input(to));
    }

    fn glyph_centre(&mut self, page: usize, run: usize, glyph: usize) -> PagePoint {
        let quad = self.glyph_quad(page, run, glyph);
        PagePoint {
            page,
            x: quad.corners.iter().map(|(x, _)| x).sum::<f64>() / 4.0,
            y: quad.corners.iter().map(|(_, y)| y).sum::<f64>() / 4.0,
        }
    }

    fn glyph_quad(&mut self, page: usize, run: usize, glyph: usize) -> PageQuad {
        self.doc.page_text(page).expect("page text extracts").runs[run].glyphs[glyph].quad
    }

    fn last_run(&mut self, page: usize) -> usize {
        self.doc.page_text(page).expect("text").runs.len() - 1
    }

    fn glyphs_in(&mut self, page: usize, run: usize) -> usize {
        self.doc.page_text(page).expect("text").runs[run]
            .glyphs
            .len()
    }

    /// Every annotation the session has written, as its dictionary.
    fn annotations(&self) -> Vec<onionskin_cos::Dict> {
        self.doc
            .edit()
            .pending_edits()
            .values()
            .filter_map(|edit| match edit {
                PendingEdit::Set { object, .. } => object.as_dict().cloned(),
                PendingEdit::Delete { .. } => None,
            })
            .filter(|dict| {
                dict.get(b"Type")
                    .and_then(Object::as_name)
                    .is_some_and(|name| name.as_bytes() == b"Annot")
            })
            .collect()
    }

    fn quad_points(&self) -> Vec<f64> {
        let annotations = self.annotations();
        let markup = annotations
            .iter()
            .find(|dict| dict.get(b"QuadPoints").is_some())
            .expect("a markup annotation was written");
        numbers(markup.get(b"QuadPoints"))
    }

    fn edits(&self) -> usize {
        self.doc.edit().history().reach()
    }

    /// Page 0 as the viewer would draw it now, pending edits and all.
    fn pixels(&mut self) -> Vec<[u8; 4]> {
        self.doc
            .render_page_now(0, ZOOM)
            .expect("the page renders")
            .raster
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect()
    }

    fn raster_width(&mut self) -> usize {
        self.doc
            .render_page_now(0, ZOOM)
            .expect("the page renders")
            .raster
            .width() as usize
    }
}

fn numbers(object: Option<&Object>) -> Vec<f64> {
    let Some(Object::Array(items)) = object else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Object::Integer(v) => Some(*v as f64),
            Object::Real(v) => Some(*v),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Gestures
// ---------------------------------------------------------------------------

#[test]
fn a_highlight_drag_over_glyphs_creates_one_annotation_with_their_quads() {
    let mut fixture = Fixture::seed("hello.pdf");
    let mut tool = MarkupTool::highlight();
    let from = fixture.glyph_centre(0, 0, 0);
    let last = fixture.glyphs_in(0, 0) - 1;
    let to = fixture.glyph_centre(0, 0, last);

    fixture.drag(&mut tool, from, to, false);

    assert_eq!(fixture.edits(), 1, "one gesture, one undo step");
    let annotations = fixture.annotations();
    assert_eq!(annotations.len(), 1, "one annotation");
    assert_eq!(
        annotations[0]
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes().to_vec()),
        Some(b"Highlight".to_vec())
    );

    let points = fixture.quad_points();
    assert_eq!(points.len() % 8, 0, "eight numbers per quad");
    assert!(!points.is_empty(), "the annotation covers the glyphs");

    // The quads have to cover the glyphs that were dragged over.
    let first = fixture.glyph_quad(0, 0, 0);
    let (left, _) = horizontal(&first);
    assert!(
        points[0] <= left + 0.5,
        "the markup starts at the first glyph, not after it"
    );
}

#[test]
fn a_tiny_drag_creates_nothing() {
    let mut fixture = Fixture::seed("hello.pdf");
    let mut tool = MarkupTool::highlight();
    let at = fixture.glyph_centre(0, 0, 0);
    let barely = PagePoint {
        x: at.x + 0.01,
        ..at
    };

    fixture.drag(&mut tool, at, barely, false);

    assert_eq!(fixture.edits(), 0, "a click is not a markup");
    assert!(fixture.annotations().is_empty());
}

#[test]
fn shift_extends_the_markup_to_the_new_selection() {
    let mut fixture = Fixture::seed("hello.pdf");
    let mut tool = MarkupTool::highlight();
    let last = fixture.glyphs_in(0, 0) - 1;
    let from = fixture.glyph_centre(0, 0, 0);
    let middle = fixture.glyph_centre(0, 0, last / 2);
    let end = fixture.glyph_centre(0, 0, last);

    fixture.drag(&mut tool, from, middle, false);
    let short = width_of(&fixture.quad_points());

    fixture.drag(&mut tool, end, end, true);
    let extended = width_of(&fixture.quad_points());

    assert!(
        extended > short,
        "shift extends from the anchor: {extended} should exceed {short}"
    );
    assert_eq!(
        fixture.annotations().len(),
        1,
        "and extends the markup rather than laying a second one over it"
    );
    assert_eq!(fixture.edits(), 1, "one markup, one undo step");
}

/// **The one that bites.** Two columns on one line: a bounding rectangle would
/// be one quad spanning the gutter.
#[test]
fn a_drag_over_two_columns_produces_two_quads_not_one_bounding_box() {
    let mut fixture = Fixture::bytes(two_columns());
    let mut tool = MarkupTool::highlight();
    let from = fixture.glyph_centre(0, 0, 0);
    let right_run = fixture.last_run(0);
    let last = fixture.glyphs_in(0, right_run) - 1;
    let to = fixture.glyph_centre(0, right_run, last);

    fixture.drag(&mut tool, from, to, false);

    let points = fixture.quad_points();
    let quads = points.len() / 8;
    assert!(
        quads >= 2,
        "a selection across a gutter is at least two quads, got {quads}"
    );

    // And none of them spans the gutter, which is what a bounding box would do.
    for quad in points.chunks(8) {
        let left = quad[0];
        let right = quad[2];
        assert!(
            right - left < 250.0,
            "quad from {left} to {right} spans the gutter; this is a bounding box"
        );
    }
}

// ---------------------------------------------------------------------------
// What gets written
// ---------------------------------------------------------------------------

#[test]
fn quad_points_are_written_in_the_vertex_order_readers_expect() {
    let mut fixture = Fixture::seed("hello.pdf");
    let mut tool = MarkupTool::highlight();
    let from = fixture.glyph_centre(0, 0, 0);
    let last = fixture.glyphs_in(0, 0) - 1;
    let to = fixture.glyph_centre(0, 0, last);
    fixture.drag(&mut tool, from, to, false);

    for quad in fixture.quad_points().chunks(8) {
        let [ulx, uly, urx, ury, llx, lly, lrx, lry] = quad else {
            panic!("eight numbers per quad");
        };
        assert!(ulx < urx, "upper-left is left of upper-right");
        assert!(llx < lrx, "lower-left is left of lower-right");
        assert!(uly > lly, "upper edge is above lower edge");
        assert_eq!(uly, ury, "the upper edge is level");
        assert_eq!(lly, lry, "the lower edge is level");
        assert_eq!(ulx, llx, "the left edge is vertical");
        assert_eq!(urx, lrx, "the right edge is vertical");
    }
}

// ---------------------------------------------------------------------------
// Rendered result
// ---------------------------------------------------------------------------

/// The quads being right in the file is half the claim; the other half is that
/// a reader draws ink over the glyphs the user dragged over and over nothing
/// else. Asserted over the whole region rather than at a sample point, because
/// an appearance stream placed one quad-height too low still covers a pixel.
#[test]
fn the_highlight_covers_the_glyphs_it_was_dragged_over() {
    assert_the_highlight_lands_on_its_quads(Fixture::bytes(one_line(0)));
}

/// The trap M2's P3 documented, at the markup layer: on a `/Rotate 90` page the
/// page-space quads a tool writes and the device space a reader paints in are
/// a quarter turn apart, and an implementation that conflates them puts the
/// highlight somewhere else on the page entirely. The file is unrotated page
/// space either way - `/QuadPoints` never carries the rotation - so only a
/// rendered result can tell the two apart.
#[test]
fn a_highlight_on_a_rotated_page_still_lands_on_its_glyphs() {
    assert_the_highlight_lands_on_its_quads(Fixture::bytes(one_line(90)));
}

fn assert_the_highlight_lands_on_its_quads(mut fixture: Fixture) {
    let last = fixture.glyphs_in(0, 0) - 1;
    let from = fixture.glyph_centre(0, 0, 0);
    let to = fixture.glyph_centre(0, 0, last);

    let before = fixture.pixels();
    fixture.drag(&mut MarkupTool::highlight(), from, to, false);
    let after = fixture.pixels();
    assert_eq!(before.len(), after.len(), "the raster changed size");

    let geometry = fixture.doc.page_geometry(0).expect("page measures").clone();
    let quads: Vec<[(f64, f64); 4]> = fixture
        .quad_points()
        .chunks(8)
        .map(|quad| {
            let device = geometry
                .user_to_device(
                    PageQuad {
                        page: 0,
                        corners: [
                            (quad[0], quad[1]),
                            (quad[2], quad[3]),
                            (quad[4], quad[5]),
                            (quad[6], quad[7]),
                        ],
                    },
                    ZOOM,
                )
                .expect("the quad maps to the raster");
            device.corners
        })
        .collect();
    assert!(!quads.is_empty(), "the drag wrote no quads");

    let width = fixture.raster_width();
    let (mut inside, mut outside) = (0usize, 0usize);
    for (index, pixel) in after.iter().enumerate() {
        let x = (index % width) as f64 + 0.5;
        let y = (index / width) as f64 + 0.5;
        match classify(&quads, x, y) {
            Region::Inside => {
                inside += 1;
                assert!(
                    !is_background(*pixel),
                    "({x}, {y}) is inside a quad and still page background: the highlight does not cover the glyphs it was dragged over"
                );
            }
            Region::Outside => {
                outside += 1;
                assert_eq!(
                    *pixel, before[index],
                    "({x}, {y}) is outside every quad and changed: the highlight paints beyond the selection"
                );
            }
            // The antialiased ring, where neither claim is meaningful.
            Region::Edge => {}
        }
    }
    assert!(
        inside > 500 && outside > 500,
        "the sample has to cover both regions to mean anything, got {inside} inside and {outside} outside"
    );
}

#[test]
fn each_tool_writes_its_own_subtype() {
    for (mut tool, expected) in [
        (MarkupTool::highlight(), "Highlight"),
        (MarkupTool::underline(), "Underline"),
        (MarkupTool::strikethrough(), "StrikeOut"),
    ] {
        let mut fixture = Fixture::seed("hello.pdf");
        let from = fixture.glyph_centre(0, 0, 0);
        let last = fixture.glyphs_in(0, 0) - 1;
        let to = fixture.glyph_centre(0, 0, last);
        fixture.drag(&mut tool, from, to, false);

        let annotations = fixture.annotations();
        assert_eq!(annotations.len(), 1, "{expected}: one annotation");
        assert_eq!(
            annotations[0]
                .get(b"Subtype")
                .and_then(Object::as_name)
                .map(|name| name.as_bytes().to_vec()),
            Some(expected.as_bytes().to_vec())
        );
    }
}

/// Acrobat models a replacement as a strike-out with a reply linked by `/IRT`,
/// not as two unrelated annotations that happen to overlap.
#[test]
fn replace_text_writes_a_strikeout_and_a_reply_linked_to_it() {
    let mut fixture = Fixture::seed("hello.pdf");
    let mut tool = MarkupTool::replace_text();
    let from = fixture.glyph_centre(0, 0, 0);
    let last = fixture.glyphs_in(0, 0) - 1;
    let to = fixture.glyph_centre(0, 0, last);

    fixture.drag(&mut tool, from, to, false);

    assert_eq!(fixture.edits(), 1, "a replacement is one undo step");
    let annotations = fixture.annotations();
    assert_eq!(annotations.len(), 2, "the strike-out and its reply");

    let reply = annotations
        .iter()
        .find(|dict| dict.get(b"IRT").is_some())
        .expect("one of them replies to the other");
    let Some(Object::Ref(parent)) = reply.get(b"IRT") else {
        panic!("/IRT names an object");
    };
    let struck = annotations
        .iter()
        .find(|dict| {
            dict.get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|name| name.as_bytes() == b"StrikeOut")
        })
        .expect("a strike-out was written");
    assert!(
        struck.get(b"QuadPoints").is_some(),
        "the strike-out covers the text"
    );
    assert!(parent.number > 0, "the reply points at a real object");
}

#[test]
fn undo_removes_the_markup_and_redo_puts_it_back() {
    let mut fixture = Fixture::seed("hello.pdf");
    let mut tool = MarkupTool::highlight();
    let from = fixture.glyph_centre(0, 0, 0);
    let last = fixture.glyphs_in(0, 0) - 1;
    let to = fixture.glyph_centre(0, 0, last);
    fixture.drag(&mut tool, from, to, false);
    let after = fixture.doc.edit().pending_edits();

    let (edit, base) = fixture.doc.edit_mut();
    assert!(edit.undo(base).expect("undo runs"));
    assert!(
        fixture.doc.edit().pending_edits().is_empty(),
        "undo restores the page's /Annots by value, so nothing is left to write"
    );

    let (edit, base) = fixture.doc.edit_mut();
    assert!(edit.redo(base).expect("redo runs"));
    assert_eq!(
        fixture.doc.edit().pending_edits(),
        after,
        "redo re-adds the same objects"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Where a pixel sits relative to the quads a markup wrote.
#[derive(Debug, PartialEq, Eq)]
enum Region {
    Inside,
    Edge,
    Outside,
}

/// The ring, in device pixels, where a rasterizer's antialiasing makes a pixel
/// neither reliably covered nor reliably untouched.
const EDGE: f64 = 2.0;

fn classify(quads: &[[(f64, f64); 4]], x: f64, y: f64) -> Region {
    let mut region = Region::Outside;
    for quad in quads {
        let xs = quad.map(|(x, _)| x);
        let ys = quad.map(|(_, y)| y);
        let left = xs.iter().copied().fold(f64::MAX, f64::min);
        let right = xs.iter().copied().fold(f64::MIN, f64::max);
        let top = ys.iter().copied().fold(f64::MAX, f64::min);
        let bottom = ys.iter().copied().fold(f64::MIN, f64::max);
        if x >= left + EDGE && x <= right - EDGE && y >= top + EDGE && y <= bottom - EDGE {
            return Region::Inside;
        }
        if x >= left - EDGE && x <= right + EDGE && y >= top - EDGE && y <= bottom + EDGE {
            region = Region::Edge;
        }
    }
    region
}

/// The opaque white a page starts from.
fn is_background(pixel: [u8; 4]) -> bool {
    pixel == [255, 255, 255, 255]
}

fn horizontal(quad: &PageQuad) -> (f64, f64) {
    let xs = quad.corners.map(|(x, _)| x);
    (
        xs.iter().copied().fold(f64::MAX, f64::min),
        xs.iter().copied().fold(f64::MIN, f64::max),
    )
}

/// Total horizontal extent of every quad, which grows as a selection extends.
fn width_of(points: &[f64]) -> f64 {
    points.chunks(8).map(|quad| (quad[2] - quad[0]).abs()).sum()
}

/// A two-column page.
///
/// Hand-built rather than taken from the corpus: the assertion is about a
/// gutter, and a fixture whose column positions are written here cannot stop
/// being two columns when the corpus is refetched. The text is drawn with a
/// standard font, so extraction needs no embedded font programme.
fn two_columns() -> Vec<u8> {
    let content = "BT /F1 12 Tf 40 700 Td (Left column text here) Tj ET\n\
                   BT /F1 12 Tf 360 700 Td (Right column text here) Tj ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

/// One line of text on an otherwise blank page, optionally rotated.
///
/// Hand-built for the same reason `two_columns` is: the render assertions are
/// about where ink lands relative to one known run of glyphs, and a corpus file
/// can grow a second one on refetch.
fn one_line(rotate: i32) -> Vec<u8> {
    let content = "BT /F1 18 Tf 0 0 0 rg 40 400 Td (Highlight this line) Tj ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {rotate} \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream(content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

fn stream(data: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data.as_bytes());
    out.extend_from_slice(b"\nendstream");
    out
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
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
