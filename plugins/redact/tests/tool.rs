//! The Redact tool, driven headlessly: page-space pointer input and a
//! viewport, as the canvas hands them over, read back as the marks the
//! document holds.

use onionskin_core::redactions::{Align, Overlay as MarkOverlay, RedactionLook};
use onionskin_core::{Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport};
use onionskin_plugin_api::{
    Overlay, PluginManifest, PluginRegistry, PointerInput, ToolCapability, ToolCtx, ToolPlugin,
};
use onionskin_redact::{RedactPlugin, RedactTool};

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

fn input(x: f64, y: f64) -> PointerInput {
    PointerInput {
        at: PagePoint { page: 0, x, y },
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    }
}

fn pdf() -> Vec<u8> {
    let content = "BT /F1 12 Tf 20 200 Td (Hello Secret World) Tj ET";
    pdf_with_content(content)
}

fn pdf_with_content(content: &str) -> Vec<u8> {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
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
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n");
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

impl Fixture {
    fn new() -> Self {
        let mut doc = Document::open_bytes(pdf()).expect("opens");
        Self::from_document(doc)
    }

    fn from_document(mut doc: Document) -> Self {
        let mut viewport = Viewport::new(
            doc.page_count(),
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("viewport");
        let geometry = doc.page_geometry(0).expect("measures").clone();
        viewport.measure_page(geometry).expect("measurable");
        viewport.fit(FitMode::Page).expect("fits");
        Self { doc, viewport }
    }

    fn ctx(&mut self) -> ToolCtx<'_> {
        ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        }
    }
}

#[test]
fn a_drag_over_an_actual_text_span_marks_all_member_quads() {
    let content = "BT /F1 12 Tf 20 200 Td (left ) Tj \
        /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC ( right) Tj ET";
    let mut fixture =
        Fixture::from_document(Document::open_bytes(pdf_with_content(content)).expect("opens"));
    let source = fixture.doc.page_text(0).expect("extracts").clone();
    let first = source.runs[1].glyphs[0].quad.corners;
    let last = source.runs[2].glyphs[1].quad.corners;
    let centre = |corners: &[(f64, f64); 4]| {
        (
            corners.iter().map(|(x, _)| *x).sum::<f64>() / 4.0,
            corners.iter().map(|(_, y)| *y).sum::<f64>() / 4.0,
        )
    };
    let mut tool = RedactTool::new();
    let mut ctx = fixture.ctx();
    let from = centre(&first);
    let to = centre(&last);
    tool.on_pointer_down(&mut ctx, input(from.0, from.1));
    tool.on_pointer_move(&mut ctx, input(to.0, to.1));
    let overlays = tool.overlays(ctx.doc);
    let overlay_quads = overlays
        .into_iter()
        .map(|overlay| match overlay {
            Overlay::Quads(quads) => {
                assert_eq!(quads.len(), 1);
                quads[0]
            }
            other => panic!("unexpected overlay: {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        overlay_quads,
        [
            source.runs[1].glyphs[0].quad,
            source.runs[1].glyphs[1].quad,
            source.runs[2].glyphs[0].quad,
            source.runs[2].glyphs[1].quad,
        ]
    );
    tool.on_pointer_up(&mut ctx, input(to.0, to.1));
    let marks = fixture.doc.redactions().expect("reads");
    assert_eq!(marks.len(), 1);
    let expected = [
        source.runs[1].glyphs[0].quad,
        source.runs[1].glyphs[1].quad,
        source.runs[2].glyphs[0].quad,
        source.runs[2].glyphs[1].quad,
    ];
    assert_eq!(marks[0].quads.len(), expected.len());
    // Persisted PDF coordinates are rounded to six decimal places.
    for (actual, expected) in marks[0].quads.iter().zip(expected) {
        assert_eq!(actual.page, expected.page);
        for ((actual_x, actual_y), (expected_x, expected_y)) in
            actual.corners.iter().zip(expected.corners)
        {
            assert!((actual_x - expected_x).abs() <= 1e-6);
            assert!((actual_y - expected_y).abs() <= 1e-6);
        }
    }
}

/// Where "Secret" is, from the page's own glyphs.
fn secret_span(doc: &mut Document) -> ((f64, f64), (f64, f64)) {
    let page = doc.page_text(0).expect("text");
    let run = &page.runs[0];
    let at = run.decoded_text.find("Secret").expect("found");
    let quads = run.quads_for_decoded(at..at + 6);
    let first = quads[0].corners[2];
    let last = quads[5].corners[1];
    ((first.0 + 1.0, first.1 + 3.0), (last.0 - 1.0, last.1 - 3.0))
}

#[test]
fn a_drag_across_text_marks_that_text() {
    let mut fixture = Fixture::new();
    let (from, to) = secret_span(&mut fixture.doc);
    let mut tool = RedactTool::new();
    let look = RedactionLook {
        overlay: Some(MarkOverlay {
            text: "(b)(6)".to_owned(),
            align: Align::Left,
            ..MarkOverlay::default()
        }),
        ..RedactionLook::default()
    };
    tool.configure(&onionskin_plugin_api::ToolEnvironment {
        redaction: Some(onionskin_redact::look::default_of(&look)),
        ..Default::default()
    });
    let mut ctx = fixture.ctx();
    tool.on_pointer_down(&mut ctx, input(from.0, from.1));
    tool.on_pointer_move(&mut ctx, input(to.0, to.1));
    let previewed = tool.overlays(ctx.doc);
    assert_eq!(previewed.len(), 6, "a quad a glyph");
    assert!(matches!(previewed[0], Overlay::Quads(_)));
    tool.on_pointer_up(&mut ctx, input(to.0, to.1));

    let marks = fixture.doc.redactions().expect("reads");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].quads.len(), 6);
    assert_eq!(marks[0].look, look);
    assert!(tool.overlays(&fixture.doc).is_empty());
    assert_eq!(
        fixture.doc.edit().history().undo_label(),
        Some("Mark for Redaction")
    );
}

#[test]
fn a_drag_on_no_text_marks_a_region_and_a_click_opens_a_mark() {
    let mut fixture = Fixture::new();
    let mut tool = RedactTool::new();
    let mut ctx = fixture.ctx();
    tool.on_pointer_down(&mut ctx, input(50.0, 20.0));
    tool.on_pointer_move(&mut ctx, input(150.0, 80.0));
    assert!(matches!(
        tool.overlays(ctx.doc).as_slice(),
        [Overlay::AntsRect(_)]
    ));
    tool.on_pointer_up(&mut ctx, input(150.0, 80.0));
    let marks = fixture.doc.redactions().expect("reads");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].rect, [50.0, 20.0, 150.0, 80.0]);
    assert!(marks[0].quads.is_empty());

    // A click inside the mark asks for its properties; one outside does not.
    let mut ctx = fixture.ctx();
    tool.on_pointer_down(&mut ctx, input(100.0, 50.0));
    tool.on_pointer_up(&mut ctx, input(100.0, 50.0));
    assert_eq!(ctx.doc.take_redaction_request(), Some(marks[0].objref));
    tool.on_pointer_down(&mut ctx, input(250.0, 250.0));
    tool.on_pointer_up(&mut ctx, input(250.0, 250.0));
    assert_eq!(ctx.doc.take_redaction_request(), None);
    assert_eq!(
        fixture.doc.redactions().expect("reads").len(),
        1,
        "a click marks nothing"
    );
}

#[test]
fn a_cancelled_gesture_marks_nothing() {
    let mut fixture = Fixture::new();
    let mut tool = RedactTool::new();
    let mut ctx = fixture.ctx();
    tool.on_pointer_down(&mut ctx, input(50.0, 20.0));
    tool.on_pointer_move(&mut ctx, input(150.0, 80.0));
    tool.on_deactivate(&mut ctx);
    tool.on_pointer_up(&mut ctx, input(150.0, 80.0));
    assert!(fixture.doc.redactions().expect("reads").is_empty());
}

#[test]
fn the_plugin_registers_the_tool() {
    let mut registry = PluginRegistry::default();
    RedactPlugin.register(&mut registry);
    let tool = registry.tools().next().expect("a tool");
    assert_eq!(tool.id(), "redact.mark");
    assert_eq!(tool.icon(), "redact");
    assert_eq!(tool.group(), "redact");
    assert!(tool.hint().is_some());
    assert_eq!(tool.capabilities(), [ToolCapability::Redact]);
    assert!(ToolCapability::Redact.edits_document());
    assert_eq!(
        (RedactPlugin.id(), RedactPlugin.name()),
        ("onionskin.redact", "Redact")
    );
}
