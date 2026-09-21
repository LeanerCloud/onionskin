//! A text selection carries its text cut into spans of one face and size,
//! for Export Selection As; joined, the spans are the selection's text.

mod common;

use onionskin_core::textselect::{select_between, styled_text};
use onionskin_core::{Document, PagePoint};

/// "Plain" in 12-point Helvetica, then "Bold" in 18-point Times-Bold on the
/// same line, then "Next" in Helvetica on the line below.
fn two_faces() -> Vec<u8> {
    common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
        common::stream(
            "BT /F1 12 Tf 72 700 Td (Plain) Tj /F2 18 Tf 60 0 Td (Bold) Tj ET \
             BT /F1 12 Tf 72 670 Td (Next) Tj ET",
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold >>".to_vec(),
    ])
}

#[test]
fn the_spans_join_to_the_text_and_change_where_the_face_does() {
    let mut doc = Document::open_bytes(two_faces()).expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let (text, spans) = styled_text(&page);
    assert_eq!(text, "Plain Bold\nNext");
    let joined: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(joined, text);
    let faces: Vec<(&str, &str, f64)> = spans
        .iter()
        .map(|span| (span.text.as_str(), span.font.as_str(), span.size))
        .collect();
    assert_eq!(
        faces,
        [
            ("Plain ", "Helvetica", 12.0),
            ("Bold\n", "Times-Bold", 18.0),
            ("Next", "Helvetica", 12.0),
        ]
    );
}

#[test]
fn a_selection_made_by_dragging_carries_its_spans() {
    let mut doc = Document::open_bytes(two_faces()).expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let selection = select_between(
        &page,
        PagePoint {
            page: 0,
            x: 132.0,
            y: 705.0,
        },
        PagePoint {
            page: 0,
            x: 100.0,
            y: 672.0,
        },
    )
    .expect("selects");
    let joined: String = selection
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    assert_eq!(joined, selection.text);
    assert_eq!(selection.spans[0].font, "Times-Bold");
}
