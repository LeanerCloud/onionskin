//! Rendering a real document: the shared buffer it is opened over, and the
//! three knobs on [`RenderOptions`] that decide what ends up on the page.
//!
//! The fixtures are written here rather than taken from the corpus because
//! each one has to isolate a single switch: two independently toggleable
//! optional content groups, one annotation and nothing else, or one thick
//! stroke beside one fill.

use std::path::PathBuf;
use std::sync::Arc;

use onionskin_render::{Document, ObjectIdentifier, PageRender, RenderOptions};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// Assemble numbered objects into a PDF with a correct cross-reference table.
/// Object `n` is `objects[n - 1]`, and object 1 is the catalog.
fn pdf(objects: &[String]) -> Vec<u8> {
    let mut pdf = Vec::from(&b"%PDF-1.7\n"[..]);
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }

    let startxref = pdf.len();
    let size = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n")
            .as_bytes(),
    );
    pdf
}

fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}endstream",
        content.len()
    )
}

fn render(doc: &Document, options: &RenderOptions) -> PageRender {
    let page = doc
        .render_page(0, 1.0, options)
        .expect("the fixture renders");
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    page
}

/// The page is 200x100 at 1x and the y axis is flipped, so a square drawn at
/// PDF `(10, 10)-(90, 90)` covers device `(10, 10)-(90, 90)` too.
fn pixel(page: &PageRender, x: u32, y: u32) -> [u8; 4] {
    let i = (y * page.raster.width() + x) as usize * 4;
    page.raster.rgba()[i..i + 4].try_into().expect("4 bytes")
}

#[test]
fn from_shared_opens_the_buffer_the_session_already_holds() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/two-page.pdf");
    let bytes =
        Arc::new(std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())));

    let doc = Document::from_shared(Arc::clone(&bytes)).expect("a seed opens");
    assert_eq!(doc.page_count(), 2);
    assert!(
        Arc::strong_count(&bytes) > 1,
        "hayro copied the bytes instead of sharing the allocation"
    );

    let page = render(&doc, &RenderOptions::default());
    assert_eq!(
        (page.raster.width(), page.raster.height()),
        (200, 100),
        "the seed's media box at 1x"
    );
}

/// A 200x100 page with a red square in the left half and a blue one in the
/// right, each inside its own optional content group, both on by default.
fn two_layers() -> Vec<u8> {
    const CONTENT: &str = "/OC /L1 BDC\n1 0 0 rg\n10 10 80 80 re f\nEMC\n\
                           /OC /L2 BDC\n0 0 1 rg\n110 10 80 80 re f\nEMC\n";

    pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties \
         << /OCGs [5 0 R 6 0 R] /D << /ON [5 0 R 6 0 R] >> >> >>"
            .to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R \
         /Resources << /Properties << /L1 5 0 R /L2 6 0 R >> >> >>"
            .to_string(),
        stream("", CONTENT),
        "<< /Type /OCG /Name (Red) >>".to_string(),
        "<< /Type /OCG /Name (Blue) >>".to_string(),
    ])
}

const RED_LAYER: ObjectIdentifier = ObjectIdentifier {
    obj_number: 5,
    gen_number: 0,
};
const BLUE_LAYER: ObjectIdentifier = ObjectIdentifier {
    obj_number: 6,
    gen_number: 0,
};

/// hayro builds its OCG state from the file's own default configuration inside
/// `Context::new_with`, so without the fork's `ocg_overrides` a layers pane
/// cannot toggle anything at all.
#[test]
fn a_layer_override_shows_and_hides_one_group() {
    let doc = Document::open(two_layers()).expect("the fixture opens");

    let both = render(&doc, &RenderOptions::default());
    assert_eq!(pixel(&both, 50, 50), RED, "the red group is on by default");
    assert_eq!(pixel(&both, 150, 50), BLUE, "so is the blue one");

    let mut options = RenderOptions::default();
    options.layer_visibility.insert(RED_LAYER, false);
    let red_off = render(&doc, &options);
    assert_eq!(pixel(&red_off, 50, 50), WHITE, "the red group is hidden");
    assert_eq!(pixel(&red_off, 150, 50), BLUE, "the blue one is untouched");
    assert_ne!(red_off.raster.rgba(), both.raster.rgba());

    options.layer_visibility.insert(RED_LAYER, true);
    let red_on = render(&doc, &options);
    assert_eq!(
        red_on.raster.rgba(),
        both.raster.rgba(),
        "toggling a layer back on must restore the page exactly"
    );
}

#[test]
fn an_override_hides_a_group_the_file_turns_on() {
    let doc = Document::open(two_layers()).expect("the fixture opens");

    let mut options = RenderOptions::default();
    options.layer_visibility.insert(RED_LAYER, false);
    options.layer_visibility.insert(BLUE_LAYER, false);

    let neither = render(&doc, &options);
    assert_eq!(pixel(&neither, 50, 50), WHITE);
    assert_eq!(pixel(&neither, 150, 50), WHITE);
}

/// A 200x100 page whose only mark is a square annotation's appearance stream.
fn one_annotation() -> Vec<u8> {
    pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R \
         /Annots [5 0 R] /Resources << >> >>"
            .to_string(),
        stream("", ""),
        "<< /Type /Annot /Subtype /Square /Rect [10 10 90 90] /AP << /N 6 0 R >> >>".to_string(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 80 80]",
            "1 0 0 rg\n0 0 80 80 re f\n",
        ),
    ])
}

#[test]
fn annotations_are_drawn_unless_they_are_switched_off() {
    let doc = Document::open(one_annotation()).expect("the fixture opens");

    let drawn = render(&doc, &RenderOptions::default());
    assert_eq!(pixel(&drawn, 50, 50), RED, "annotations render by default");

    let options = RenderOptions {
        render_annotations: false,
        ..Default::default()
    };
    let skipped = render(&doc, &options);
    assert_eq!(pixel(&skipped, 50, 50), WHITE, "and can be switched off");
}

/// A 200x100 page with a horizontal line 20 points wide across its middle,
/// at y 50, and a filled black square in the top-left corner.
fn thick_line() -> Vec<u8> {
    const CONTENT: &str = "0 G 20 w 20 50 m 180 50 l S\n0 g 0 80 10 10 re f\n";
    pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
        stream("", CONTENT),
    ])
}

/// How many pixels of column `x` are marked, top to bottom.
fn marked_in_column(page: &PageRender, x: u32) -> usize {
    (0..page.raster.height())
        .filter(|&y| pixel(page, x, y) != WHITE)
        .count()
}

fn hairlines() -> RenderOptions {
    RenderOptions {
        hairline_strokes: true,
        ..RenderOptions::default()
    }
}

/// Line Weights off draws the 20-point line one device pixel wide. The PDF
/// specification defines width 0 as the thinnest line the device can draw,
/// and hayro widens it to one pixel; antialiasing may touch a second.
#[test]
fn hairline_strokes_draw_a_thick_line_one_pixel_wide() {
    let doc = Document::open(thick_line()).expect("the fixture opens");

    let weighted = render(&doc, &RenderOptions::default());
    assert_eq!(marked_in_column(&weighted, 100), 20, "20 points at 1x");

    let hairline = render(&doc, &hairlines());
    let width = marked_in_column(&hairline, 100);
    assert!((1..=2).contains(&width), "{width} pixels");
    assert_ne!(
        pixel(&hairline, 100, 50),
        WHITE,
        "the line is still drawn, where it was"
    );
}

/// The width is one pixel at any zoom, where the weighted line grows with it.
#[test]
fn a_hairline_stays_one_pixel_wide_when_zoomed() {
    let doc = Document::open(thick_line()).expect("the fixture opens");
    let at = |options: &RenderOptions| {
        let page = doc.render_page(0, 4.0, options).expect("renders at 4x");
        marked_in_column(&page, 400)
    };
    assert_eq!(at(&RenderOptions::default()), 80, "20 points at 4x");
    assert!((1..=2).contains(&at(&hairlines())));
}

/// Only strokes change: the filled square is the same pixels either way.
#[test]
fn hairline_strokes_leave_fills_alone() {
    let doc = Document::open(thick_line()).expect("the fixture opens");
    let (weighted, hairline) = (
        render(&doc, &RenderOptions::default()),
        render(&doc, &hairlines()),
    );
    for (x, y) in [(0, 10), (5, 15), (9, 19)] {
        assert_eq!(pixel(&hairline, x, y), pixel(&weighted, x, y), "({x}, {y})");
        assert_ne!(pixel(&hairline, x, y), WHITE);
    }
}

/// Line Weights is a way of looking at a page, not a change to it: an SVG
/// export is the same with it on or off.
#[test]
fn an_svg_export_keeps_its_line_weights() {
    let doc = Document::open(thick_line()).expect("the fixture opens");
    let svg = |options: &RenderOptions| doc.render_page_svg(0, options).expect("exports").svg;
    assert_eq!(svg(&hairlines()), svg(&RenderOptions::default()));
}
