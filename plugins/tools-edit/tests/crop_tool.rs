//! The Crop Pages tool, driven headlessly: page-space pointer input and a
//! viewport, as the canvas hands them to it.

use onionskin_core::{Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_plugin_api::{Overlay, PointerInput, ToolCapability, ToolCtx, ToolPlugin};
use onionskin_tools_edit::{crop_to_rect, CropTool};

/// A two-page Letter document, the second turned a quarter clockwise.
fn document() -> Document {
    let objects: [&[u8]; 4] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>",
        b"<< /Type /Page /Parent 2 0 R >>",
        b"<< /Type /Page /Parent 2 0 R /Rotate 90 >>",
    ];
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\n");
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    Document::open_bytes(out).expect("opens")
}

struct Fixture {
    doc: Document,
    viewport: Viewport,
    tool: CropTool,
}

impl Fixture {
    fn new() -> Self {
        let mut doc = document();
        let size = ViewSize {
            width: 800.0,
            height: 600.0,
        };
        let mut viewport = Viewport::new(doc.page_count(), size, 12.0).expect("viewport");
        for page in 0..doc.page_count() {
            let geometry = doc.page_geometry(page).expect("measures").clone();
            viewport.measure_page(geometry).expect("measurable");
        }
        viewport.fit(FitMode::Page).expect("fits");
        Self {
            doc,
            viewport,
            tool: CropTool::new(),
        }
    }

    fn input(page: usize, x: f64, y: f64, clicks: u8) -> PointerInput {
        PointerInput {
            at: PagePoint { page, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks,
        }
    }

    fn drag(&mut self, page: usize, from: (f64, f64), to: (f64, f64)) {
        let mut ctx = ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        };
        self.tool
            .on_pointer_down(&mut ctx, Self::input(page, from.0, from.1, 1));
        self.tool
            .on_pointer_move(&mut ctx, Self::input(page, to.0, to.1, 1));
        self.tool
            .on_pointer_up(&mut ctx, Self::input(page, to.0, to.1, 1));
    }

    fn double_click(&mut self, page: usize, at: (f64, f64)) {
        let mut ctx = ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        };
        self.tool
            .on_pointer_down(&mut ctx, Self::input(page, at.0, at.1, 2));
    }

    fn commit(&mut self) {
        self.tool.on_commit(&mut ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        });
    }

    fn crop_box(&mut self, page: usize) -> Option<[f64; 4]> {
        self.doc.page_geometry(page).expect("geometry").crop_box
    }
}

#[test]
fn the_tool_edits_pages_and_says_how_it_is_used() {
    let tool = CropTool::new();
    assert_eq!(tool.id(), "crop-pages");
    assert_eq!(tool.name(), "Crop Pages");
    assert_eq!(tool.icon(), "crop");
    assert!(tool.hint().expect("a hint").contains("double-click"));
    assert_eq!(tool.capabilities(), [ToolCapability::EditPages]);
    assert!(ToolCapability::EditPages.edits_document());
}

#[test]
fn a_drawn_rectangle_crops_the_page_on_enter() {
    let mut fixture = Fixture::new();
    fixture.drag(0, (500.0, 700.0), (100.0, 200.0));
    let drawn = fixture.tool.rect().expect("a rectangle");
    assert_eq!(
        fixture.tool.overlays(&fixture.doc),
        [Overlay::AntsRect(drawn)]
    );
    assert_eq!(fixture.crop_box(0), None, "nothing is cropped until asked");

    fixture.commit();
    assert_eq!(fixture.crop_box(0), Some([100.0, 200.0, 500.0, 700.0]));
    assert!(fixture.tool.overlays(&fixture.doc).is_empty());
    assert!(fixture.doc.undo().expect("undoes"), "one undo step");
    assert_eq!(fixture.crop_box(0), None);
}

#[test]
fn a_double_click_inside_crops_and_outside_starts_over() {
    let mut fixture = Fixture::new();
    fixture.drag(0, (100.0, 100.0), (300.0, 300.0));
    fixture.double_click(0, (400.0, 400.0));
    assert_eq!(fixture.crop_box(0), None, "outside the rectangle");
    assert_eq!(fixture.tool.rect(), None, "a new gesture began");

    fixture.drag(0, (100.0, 100.0), (300.0, 300.0));
    fixture.double_click(0, (200.0, 200.0));
    assert_eq!(fixture.crop_box(0), Some([100.0, 100.0, 300.0, 300.0]));
}

/// The rectangle is in the page's own coordinates whatever its rotation,
/// and what lies past the media box is not the page's.
#[test]
fn a_turned_page_and_a_rectangle_past_the_edge_crop_to_the_page() {
    let mut fixture = Fixture::new();
    fixture.drag(1, (-50.0, 100.0), (300.0, 900.0));
    fixture.commit();
    assert_eq!(fixture.crop_box(1), Some([0.0, 100.0, 300.0, 792.0]));
}

#[test]
fn a_click_escape_or_a_switch_away_crops_nothing() {
    let mut fixture = Fixture::new();
    fixture.drag(0, (100.0, 100.0), (100.1, 100.1));
    assert_eq!(fixture.tool.rect(), None, "a click is not a rectangle");
    fixture.commit();

    fixture.drag(0, (100.0, 100.0), (300.0, 300.0));
    fixture.tool.on_cancel(&mut ToolCtx {
        doc: &mut fixture.doc,
        viewport: &mut fixture.viewport,
    });
    fixture.commit();

    fixture.drag(0, (100.0, 100.0), (300.0, 300.0));
    fixture.tool.on_deactivate(&mut ToolCtx {
        doc: &mut fixture.doc,
        viewport: &mut fixture.viewport,
    });
    fixture.commit();

    // A drag that ends on another page draws nothing there.
    fixture.drag(0, (100.0, 100.0), (0.0, 0.0));
    let mut ctx = ToolCtx {
        doc: &mut fixture.doc,
        viewport: &mut fixture.viewport,
    };
    fixture
        .tool
        .on_pointer_down(&mut ctx, Fixture::input(0, 10.0, 10.0, 1));
    fixture
        .tool
        .on_pointer_move(&mut ctx, Fixture::input(1, 300.0, 300.0, 1));
    assert_eq!(fixture.tool.rect(), None);
    assert_eq!(fixture.crop_box(0), None);
    assert!(!fixture.doc.undo().expect("nothing to undo"));
}

#[test]
fn a_rectangle_on_no_page_is_refused_by_name() {
    let mut doc = document();
    let error = crop_to_rect(
        &mut doc,
        onionskin_core::PageRect {
            page: 9,
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        },
    )
    .expect_err("no such page");
    assert!(error.to_string().starts_with("page 10"), "{error}");
}
