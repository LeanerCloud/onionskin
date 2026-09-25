//! A text selection carries its text cut into spans of one face and size,
//! for Export Selection As; joined, the spans are the selection's text.

mod common;

use onionskin_core::textselect::{glyph_order, select_between, selection_for, styled_text};
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
#[test]
fn actual_text_continuation_selection_copies_the_replacement() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(page.runs[1].decoded_text, "CD");
    assert_eq!(
        page.runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        vec![65, 66, 67, 68]
    );
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Courier");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 10.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);

    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(22.0, 107.5), (28.0, 107.5), (22.0, 97.5), (28.0, 97.5)],
        [(28.0, 107.5), (34.0, 107.5), (28.0, 97.5), (34.0, 97.5)],
    ];
    let source_corners: Vec<_> = page
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad.corners))
        .collect();
    let source_geometry_matches =
        source_corners
            .iter()
            .zip(expected_corners)
            .all(|(actual, expected)| {
                actual.iter().zip(expected).all(
                    |(&(actual_x, actual_y), (expected_x, expected_y))| {
                        (actual_x - expected_x).abs() <= 1e-9
                            && (actual_y - expected_y).abs() <= 1e-9
                    },
                )
            });
    assert!(
        source_geometry_matches,
        "source corners: {source_corners:?}"
    );

    let indexed = selection_for(&page, &order, 2..=2);
    let point = PagePoint {
        page: 0,
        x: 25.0,
        y: 102.0,
    };
    let dragged = select_between(&page, point, point).expect("selects");
    let selections = [indexed, dragged];

    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "XY");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "XY");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }

    let expected_source_quads: Vec<_> = page
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect();
    let selection_quads: Vec<Vec<_>> = selections
        .iter()
        .map(|selection| selection.quads.clone())
        .collect();
    assert_eq!(
        selection_quads,
        vec![expected_source_quads.clone(), expected_source_quads]
    );
}
fn actual_text_page(content: &str) -> Vec<u8> {
    common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
        common::stream(content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold >>".to_vec(),
    ])
}

fn verified_source_quads(
    page: &onionskin_content::PageText,
    expected_corners: &[[(f64, f64); 4]],
) -> Vec<onionskin_core::PageQuad> {
    let source_corners: Vec<_> = page
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad.corners))
        .collect();
    assert_eq!(source_corners.len(), expected_corners.len());
    for (actual, expected) in source_corners.iter().zip(expected_corners) {
        for (&(actual_x, actual_y), &(expected_x, expected_y)) in actual.iter().zip(expected) {
            assert!((actual_x - expected_x).abs() <= 1e-9);
            assert!((actual_y - expected_y).abs() <= 1e-9);
        }
    }
    page.runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect()
}

#[test]
fn actual_text_first_selection_copies_the_replacement() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(page.runs[1].decoded_text, "CD");
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Courier");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 10.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(22.0, 107.5), (28.0, 107.5), (22.0, 97.5), (28.0, 97.5)],
        [(28.0, 107.5), (34.0, 107.5), (28.0, 97.5), (34.0, 97.5)],
    ];
    let expected_quads = verified_source_quads(&page, &expected_corners);
    let indexed = selection_for(&page, &order, 0..=0);
    let point = PagePoint {
        page: 0,
        x: 13.0,
        y: 102.0,
    };
    let dragged = select_between(&page, point, point).expect("selects");
    let selections = [indexed, dragged];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![expected_quads.clone(), expected_quads]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "XY");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "XY");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}

#[test]
fn actual_text_both_and_reverse_selection_deduplicate_members() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(page.runs[1].decoded_text, "CD");
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Courier");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 10.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(22.0, 107.5), (28.0, 107.5), (22.0, 97.5), (28.0, 97.5)],
        [(28.0, 107.5), (34.0, 107.5), (28.0, 97.5), (34.0, 97.5)],
    ];
    let expected_quads = verified_source_quads(&page, &expected_corners);
    let point = PagePoint {
        page: 0,
        x: 13.0,
        y: 102.0,
    };
    let reverse_point = PagePoint {
        page: 0,
        x: 31.0,
        y: 102.0,
    };
    let selections = [
        selection_for(&page, &order, 0..=3),
        select_between(&page, point, reverse_point).expect("selects"),
        select_between(&page, reverse_point, point).expect("selects"),
    ];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![
            expected_quads.clone(),
            expected_quads.clone(),
            expected_quads.clone(),
        ]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "XY");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "XY");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}

#[test]
fn actual_text_mixed_fonts_keep_one_representative_copy_style() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj /F2 18 Tf 1 0 0 1 60 100 Tm (B) Tj EMC ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order, vec![(0, 0), (1, 0)]);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 1);
    assert_eq!(page.runs[1].glyphs.len(), 1);
    assert_eq!(page.runs[0].decoded_text, "A");
    assert_eq!(page.runs[1].decoded_text, "B");
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Times-Bold");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 18.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(60.0, 113.5), (72.006, 113.5), (60.0, 95.5), (72.006, 95.5)],
    ];
    let expected_quads = verified_source_quads(&page, &expected_corners);
    let indexed = selection_for(&page, &order, 1..=1);
    let point = PagePoint {
        page: 0,
        x: 63.0,
        y: 102.0,
    };
    let dragged = select_between(&page, point, point).expect("selects");
    let selections = [indexed, dragged];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![expected_quads.clone(), expected_quads]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "XY");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "XY");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}

#[test]
fn actual_text_partial_ordinary_neighbors_remain_clipped() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td (LZ) Tj /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC (RT) Tj ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order.len(), 8);
    assert_eq!(page.runs.len(), 4);
    assert!(page.runs.iter().all(|run| run.glyphs.len() == 2));
    assert_eq!(
        page.runs
            .iter()
            .map(|run| run.decoded_text.as_str())
            .collect::<Vec<_>>(),
        vec!["LZ", "AB", "CD", "RT"]
    );
    assert!(page.runs.iter().all(|run| run.font_name == "Courier"));
    assert!(page.runs.iter().all(|run| run.size == 10.0));
    assert!(page.runs[0].actual_text.is_none());
    assert!(page.runs[3].actual_text.is_none());
    assert!(page.runs[1].actual_text.is_some());
    assert_eq!(page.runs[1].actual_text, page.runs[2].actual_text);
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(22.0, 107.5), (28.0, 107.5), (22.0, 97.5), (28.0, 97.5)],
        [(28.0, 107.5), (34.0, 107.5), (28.0, 97.5), (34.0, 97.5)],
        [(34.0, 107.5), (40.0, 107.5), (34.0, 97.5), (40.0, 97.5)],
        [(40.0, 107.5), (46.0, 107.5), (40.0, 97.5), (46.0, 97.5)],
        [(46.0, 107.5), (52.0, 107.5), (46.0, 97.5), (52.0, 97.5)],
        [(52.0, 107.5), (58.0, 107.5), (52.0, 97.5), (58.0, 97.5)],
    ];
    let verified_quads = verified_source_quads(&page, &expected_corners);
    let expected_quads = verified_quads[1..7].to_vec();
    let indexed = selection_for(&page, &order, 1..=6);
    let from = PagePoint {
        page: 0,
        x: 19.0,
        y: 102.0,
    };
    let to = PagePoint {
        page: 0,
        x: 49.0,
        y: 102.0,
    };
    let dragged = select_between(&page, from, to).expect("selects");
    let selections = [indexed, dragged];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![expected_quads.clone(), expected_quads]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "ZXYR");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "ZXYR");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}

#[test]
fn actual_text_nested_members_do_not_select_the_hole() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (OUT) >> BDC (A) Tj /Span << /ActualText (INNER) >> BDC (H) Tj EMC (B) Tj EMC ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order, vec![(0, 0), (1, 0), (2, 0)]);
    assert_eq!(page.runs.len(), 3);
    assert!(page.runs.iter().all(|run| run.glyphs.len() == 1));
    assert_eq!(page.runs[0].decoded_text, "A");
    assert_eq!(page.runs[1].decoded_text, "H");
    assert_eq!(page.runs[2].decoded_text, "B");
    assert!(page.runs.iter().all(|run| run.font_name == "Courier"));
    assert!(page.runs.iter().all(|run| run.size == 10.0));
    assert!(page.runs.iter().all(|run| run.actual_text.is_some()));
    assert_eq!(page.runs[0].actual_text, page.runs[2].actual_text);
    assert_ne!(page.runs[0].actual_text, page.runs[1].actual_text);
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(22.0, 107.5), (28.0, 107.5), (22.0, 97.5), (28.0, 97.5)],
    ];
    let verified_quads = verified_source_quads(&page, &expected_corners);
    let expected_quads = vec![verified_quads[0], verified_quads[2]];
    let selections = [
        selection_for(&page, &order, 0..=0),
        selection_for(&page, &order, 2..=2),
    ];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![expected_quads.clone(), expected_quads]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "OUT");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "OUT");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}

#[test]
fn actual_text_separated_geometry_keeps_member_quads() {
    let mut doc = Document::open_bytes(actual_text_page(
        "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (AB) Tj 1 0 0 1 100 50 Tm (CD) Tj EMC ET",
    ))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(page.runs[1].decoded_text, "CD");
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Courier");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 10.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (22.0, 107.5), (16.0, 97.5), (22.0, 97.5)],
        [(100.0, 57.5), (106.0, 57.5), (100.0, 47.5), (106.0, 47.5)],
        [(106.0, 57.5), (112.0, 57.5), (106.0, 47.5), (112.0, 47.5)],
    ];
    let expected_quads = verified_source_quads(&page, &expected_corners);
    let indexed = selection_for(&page, &order, 2..=2);
    let point = PagePoint {
        page: 0,
        x: 103.0,
        y: 52.0,
    };
    let dragged = select_between(&page, point, point).expect("selects");
    let selections = [indexed, dragged];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![expected_quads.clone(), expected_quads]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "XY");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "XY");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}

#[test]
fn actual_text_unmapped_continuation_remains_selectable() {
    let mut doc = Document::open_bytes(common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> >> /Contents 4 0 R >>".to_vec(),
        common::stream(
            "BT /F1 10 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj /F3 10 Tf (A) Tj EMC ET",
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold >>".to_vec(),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /Sub /FirstChar 65 /LastChar 65 /Widths [500] /Encoding << /Differences [65 /g5] >> >>".to_vec(),
    ]))
    .expect("opens");
    let page = doc.page_text(0).expect("reads").clone();
    let order = glyph_order(&page);
    assert_eq!(order, vec![(0, 0), (1, 0)]);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs.len(), 1);
    assert_eq!(page.runs[1].glyphs.len(), 1);
    assert_eq!(page.runs[0].decoded_text, "A");
    assert!(page.runs[1].decoded_text.is_empty());
    assert_eq!(page.runs[0].glyphs[0].code, 65);
    assert_eq!(page.runs[1].glyphs[0].code, 65);
    assert!(!page.runs[0].glyphs[0].is_unmapped());
    assert!(page.runs[1].glyphs[0].is_unmapped());
    assert_eq!(page.runs[0].font_name, "Courier");
    assert_eq!(page.runs[1].font_name, "Sub");
    assert_eq!(page.runs[0].size, 10.0);
    assert_eq!(page.runs[1].size, 10.0);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    assert!(!page.has_unmapped());
    assert_eq!(
        page.warnings
            .iter()
            .filter(|warning| matches!(warning, onionskin_content::Warning::UnmappedGlyphs { font, count: 1 } if font == "Sub"))
            .count(),
        1
    );
    let expected_corners = [
        [(10.0, 107.5), (16.0, 107.5), (10.0, 97.5), (16.0, 97.5)],
        [(16.0, 107.5), (21.0, 107.5), (16.0, 97.5), (21.0, 97.5)],
    ];
    let expected_quads = verified_source_quads(&page, &expected_corners);
    let indexed = selection_for(&page, &order, 1..=1);
    let point = PagePoint {
        page: 0,
        x: 18.0,
        y: 102.0,
    };
    let dragged = select_between(&page, point, point).expect("selects");
    let selections = [indexed, dragged];
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.quads.clone())
            .collect::<Vec<_>>(),
        vec![expected_quads.clone(), expected_quads]
    );
    for selection in &selections {
        assert_eq!(selection.page, 0);
        assert_eq!(selection.text, "XY");
        assert_eq!(selection.spans.len(), 1);
        assert_eq!(selection.spans[0].text, "XY");
        assert_eq!(selection.spans[0].font, "Courier");
        assert_eq!(selection.spans[0].size, 10.0);
        let joined: String = selection
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, selection.text);
    }
}
