//! Regression and characterization tests over documents built to exercise one
//! extraction behavior at a time. Some characterization cases document valid
//! behavior rather than a previously failing baseline.
//!
//! The corpus tests prove extraction works on real files. These prove it
//! behaves on the file that broke it, which is a different question: a
//! malformation rare enough to matter is rare enough that no corpus file is
//! guaranteed to contain it.

mod common;

use std::collections::HashSet;

use common::{build_pdf, one_page, open_bytes, stream};
use onionskin_content::{extract_page, search, FontId, Mapping, RunCoverage, Warning};

const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

fn helvetica() -> Vec<u8> {
    HELVETICA.as_bytes().to_vec()
}
#[test]
fn actual_text_search_covers_every_member_operator() {
    let content = "BT /F1 12 Tf 10 100 Td (L ) Tj \
         /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC \
         ( R) Tj ET";
    let doc = open_bytes(one_page(
        content,
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");

    assert_eq!(page.runs.len(), 4, "two member operators and two neighbors");
    assert_eq!(
        page.runs[0]
            .glyphs
            .iter()
            .map(|g| g.code)
            .collect::<Vec<_>>(),
        vec![76, 32]
    );
    assert_eq!(
        page.runs[1]
            .glyphs
            .iter()
            .map(|g| g.code)
            .collect::<Vec<_>>(),
        vec![65, 66]
    );
    assert_eq!(
        page.runs[2]
            .glyphs
            .iter()
            .map(|g| g.code)
            .collect::<Vec<_>>(),
        vec![67, 68]
    );
    assert_eq!(
        page.runs[3]
            .glyphs
            .iter()
            .map(|g| g.code)
            .collect::<Vec<_>>(),
        vec![32, 82]
    );
    assert_eq!(page.flatten().text, "L XY R");

    let expected_quads = page.runs[1..3]
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect::<Vec<_>>();
    let expected_provenance = page.runs[1..3]
        .iter()
        .map(|run| run.provenance)
        .collect::<Vec<_>>();
    for needle in ["X", "Y"] {
        let hits = search(&page, needle, Default::default());
        assert_eq!(hits.len(), 1, "{needle} has one literal hit");
        assert_eq!(hits[0].text, needle);
        assert_eq!(
            hits[0].quads, expected_quads,
            "{needle} covers both operators"
        );
        assert_eq!(
            hits[0].provenance, expected_provenance,
            "{needle} keeps both operators"
        );
        assert!(
            hits[0].quads.iter().all(|quad| {
                !page.runs[0]
                    .glyphs
                    .iter()
                    .chain(page.runs[3].glyphs.iter())
                    .any(|glyph| glyph.quad == *quad)
            }),
            "{needle} excludes the ordinary neighbor geometry"
        );
        assert!(
            hits[0].provenance.iter().all(|provenance| {
                provenance != &page.runs[0].provenance && provenance != &page.runs[3].provenance
            }),
            "{needle} excludes the ordinary neighbor provenance"
        );
    }
}

#[test]
fn actual_text_search_scales_to_many_member_operators() {
    let mut content = String::from("BT /F1 12 Tf 10 100 Td /Span << /ActualText (X) >> BDC ");
    for _ in 0..10_000 {
        content.push_str("(A) Tj ");
    }
    content.push_str("EMC ET");
    let doc = open_bytes(one_page(
        &content,
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");

    assert_eq!(page.runs.len(), 10_000);
    let expected_quads = page
        .runs
        .iter()
        .map(|run| {
            assert_eq!(run.glyphs[0].code, 65);
            assert_eq!(run.glyphs.len(), 1);
            assert_eq!(run.decoded_text, "A");
            assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(4, 0));
            let start = run.provenance.decoded.start as usize;
            let end = run.provenance.decoded.end as usize;
            assert_eq!(&content.as_bytes()[start..end], b"(A) Tj");
            run.glyphs[0].quad
        })
        .collect::<Vec<_>>();
    let expected_provenance = page
        .runs
        .iter()
        .map(|run| run.provenance)
        .collect::<Vec<_>>();
    assert_eq!(
        expected_provenance.iter().collect::<HashSet<_>>().len(),
        10_000
    );
    assert!(expected_provenance
        .windows(2)
        .all(|pair| pair[0].decoded.start < pair[1].decoded.start));

    let hits = search(&page, "X", Default::default());
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.text, "X");
    assert_eq!(hit.quads, expected_quads);
    assert_eq!(hit.provenance, expected_provenance);
}

#[test]
fn repeated_actual_text_form_invocations_keep_occurrences_separate() {
    let form_content = b"BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC \
          (A) Tj (B) Tj EMC ET";
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200] \
         /Resources << /Font << /F1 5 0 R >> >>",
        form_content,
    );
    let doc = open_bytes(one_page(
        "q /Fm Do Q q 1 0 0 1 0 -30 cm /Fm Do Q",
        "<< /Font << /F1 5 0 R >> /XObject << /Fm 6 0 R >> >>",
        &[helvetica(), form],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");

    assert_eq!(page.flatten().text, "XY\nXY");
    assert_eq!(page.runs.len(), 4);
    assert_eq!(page.runs[0].glyphs[0].code, 65);
    assert_eq!(page.runs[1].glyphs[0].code, 66);
    assert_eq!(page.runs[2].glyphs[0].code, 65);
    assert_eq!(page.runs[3].glyphs[0].code, 66);
    assert_eq!(page.runs[0].decoded_text, "A");
    assert_eq!(page.runs[1].decoded_text, "B");
    assert_eq!(page.runs[2].decoded_text, "A");
    assert_eq!(page.runs[3].decoded_text, "B");
    for run in &page.runs {
        assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(6, 0));
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(
            &form_content[start..end],
            if run.glyphs[0].code == 65 {
                b"(A) Tj"
            } else {
                b"(B) Tj"
            }
        );
        assert!(run.actual_text.is_some());
    }
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    assert_eq!(page.runs[2].actual_text, page.runs[3].actual_text);
    assert_ne!(page.runs[0].actual_text, page.runs[2].actual_text);
    assert_eq!(page.runs[0].provenance, page.runs[2].provenance);
    for (first, second) in page.runs[0..2].iter().zip(page.runs[2..4].iter()) {
        for (left, right) in first.glyphs[0]
            .quad
            .corners
            .into_iter()
            .zip(second.glyphs[0].quad.corners)
        {
            assert_eq!(right.0, left.0);
            assert_eq!(right.1, left.1 - 30.0);
        }
    }

    let hits = search(&page, "XY", Default::default());
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].text, "XY");
    assert_eq!(hits[0].range, 0..2);
    assert_eq!(
        hits[0].quads,
        page.runs[0..2]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hits[0].provenance,
        page.runs[0..2]
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );
    assert_eq!(hits[1].text, "XY");
    assert_eq!(hits[1].range, 3..5);
    assert_eq!(
        hits[1].quads,
        page.runs[2..4]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hits[1].provenance,
        page.runs[2..4]
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );

    let whole = search(&page, "XY\nXY", Default::default());
    assert_eq!(whole.len(), 1);
    assert_eq!(whole[0].range, 0..5);
    assert_eq!(
        whole[0].quads,
        page.runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(whole[0].provenance, hits[0].provenance);
}

#[test]
fn actual_text_keeps_mixed_font_members_and_coverage() {
    let content = "BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC \
         (AB) Tj /F2 18 Tf (CD) Tj EMC ET";
    let doc = open_bytes(one_page(
        content,
        "<< /Font << /F1 5 0 R /F2 6 0 R >> >>",
        &[
            helvetica(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
        ],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");
    let flat = page.flatten();

    assert_eq!(flat.text, "XY");
    assert_eq!(page.runs.len(), 2);
    assert_eq!(flat.pieces().len(), 1);
    assert_eq!(flat.pieces()[0].range, 0..2);
    assert_eq!(flat.pieces()[0].style_run, 0);
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(page.runs[1].decoded_text, "CD");
    assert_eq!(
        page.runs[0]
            .glyphs
            .iter()
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        vec![65, 66]
    );
    assert_eq!(
        page.runs[1]
            .glyphs
            .iter()
            .map(|glyph| glyph.code)
            .collect::<Vec<_>>(),
        vec![67, 68]
    );
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert_eq!(
        page.runs[0].font,
        FontId::Object(onionskin_cos::ObjRef::new(5, 0))
    );
    assert_eq!(
        page.runs[1].font,
        FontId::Object(onionskin_cos::ObjRef::new(6, 0))
    );
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    assert_eq!(page.runs[0].font_name, "Helvetica");
    assert_eq!(page.runs[1].font_name, "Courier");
    assert_eq!(page.runs[0].size, 12.0);
    assert_eq!(page.runs[1].size, 18.0);
    for (run, expected) in page.runs.iter().zip([b"(AB) Tj" as &[u8], b"(CD) Tj"]) {
        assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(4, 0));
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(&content.as_bytes()[start..end], expected);
    }
    let coverage = flat.pieces()[0].coverage_for(&page, 1..2);
    assert_eq!(coverage.len(), 2);
    assert!(std::ptr::eq(coverage[0].run, &page.runs[0]));
    assert!(std::ptr::eq(coverage[1].run, &page.runs[1]));
    assert!(coverage
        .iter()
        .all(|member| member.coverage == RunCoverage::WholeActualText));

    let hits = search(&page, "Y", Default::default());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "Y");
    assert_eq!(hits[0].range, 1..2);
    assert_eq!(
        hits[0].quads,
        page.runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hits[0].provenance,
        page.runs
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );
}

#[test]
fn equal_actual_text_wrappers_keep_distinct_pieces() {
    let content = "BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC \
         (A) Tj EMC ( ) Tj /Span << /ActualText (XY) >> BDC (B) Tj EMC ET";
    let doc = open_bytes(one_page(
        content,
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");
    let flat = page.flatten();

    assert_eq!(flat.text, "XY XY");
    assert_eq!(flat.pieces().len(), 3);
    assert_eq!(flat.pieces()[0].range, 0..2);
    assert_eq!(flat.pieces()[1].range, 2..3);
    assert_eq!(flat.pieces()[2].range, 3..5);
    assert_eq!(flat.pieces()[0].style_run, 0);
    assert_eq!(flat.pieces()[1].style_run, 1);
    assert_eq!(flat.pieces()[2].style_run, 2);
    assert_eq!(page.runs.len(), 3);
    assert_eq!(
        page.runs
            .iter()
            .map(|run| run.glyphs[0].code)
            .collect::<Vec<_>>(),
        vec![65, 32, 66]
    );
    assert_eq!(
        page.runs
            .iter()
            .map(|run| run.decoded_text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", " ", "B"]
    );
    assert!(page.runs[0].actual_text.is_some());
    assert!(page.runs[2].actual_text.is_some());
    assert_ne!(page.runs[0].actual_text, page.runs[2].actual_text);
    assert!(page.runs[1].actual_text.is_none());
    for (index, expected) in [(0, b"(A) Tj" as &[u8]), (1, b"( ) Tj"), (2, b"(B) Tj")] {
        assert_eq!(page.runs[index].glyphs.len(), 1);
        assert_eq!(
            page.runs[index].provenance.stream,
            onionskin_cos::ObjRef::new(4, 0)
        );
        let start = page.runs[index].provenance.decoded.start as usize;
        let end = page.runs[index].provenance.decoded.end as usize;
        assert_eq!(&content.as_bytes()[start..end], expected);
    }
    for (piece, run) in [(0, 0), (2, 2)] {
        let coverage = flat.pieces()[piece].coverage_for(&page, flat.pieces()[piece].range.clone());
        assert_eq!(coverage.len(), 1);
        assert!(std::ptr::eq(coverage[0].run, &page.runs[run]));
        assert_eq!(coverage[0].coverage, RunCoverage::WholeActualText);
    }
    let middle = flat.pieces()[1].coverage_for(&page, 2..3);
    assert_eq!(middle.len(), 1);
    assert!(std::ptr::eq(middle[0].run, &page.runs[1]));
    assert_eq!(middle[0].coverage, RunCoverage::Decoded(0..1));

    let hits = search(&page, "XY", Default::default());
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].range, 0..2);
    assert_eq!(hits[0].text, "XY");
    assert_eq!(hits[1].range, 3..5);
    assert_eq!(hits[1].text, "XY");
    assert_eq!(hits[0].quads, vec![page.runs[0].glyphs[0].quad]);
    assert_eq!(hits[1].quads, vec![page.runs[2].glyphs[0].quad]);
    assert_eq!(hits[0].provenance, vec![page.runs[0].provenance]);
    assert_eq!(hits[1].provenance, vec![page.runs[2].provenance]);
}

#[test]
fn actual_text_survives_split_contents_streams() {
    let stream4_content = b"BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj";
    let stream5_content = b"(B) Tj EMC ET";
    let stream4 = stream("", stream4_content);
    let stream5 = stream("", stream5_content);
    let doc = open_bytes(build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
          /Resources << /Font << /F1 6 0 R >> >> /Contents [4 0 R 5 0 R] >>"
            .to_vec(),
        stream4,
        stream5,
        helvetica(),
    ]));
    let page = extract_page(&doc, 0).expect("page extracts");

    assert_eq!(page.flatten().text, "XY");
    assert_eq!(page.runs.len(), 2);
    let flat = page.flatten();
    assert_eq!(flat.pieces().len(), 1);
    assert_eq!(flat.pieces()[0].range, 0..2);
    assert_eq!(flat.pieces()[0].style_run, 0);
    assert_eq!(page.runs[0].decoded_text, "A");
    assert_eq!(page.runs[1].decoded_text, "B");
    assert_eq!(page.runs[0].glyphs.len(), 1);
    assert_eq!(page.runs[1].glyphs.len(), 1);
    assert_eq!(page.runs[0].glyphs[0].code, 65);
    assert_eq!(page.runs[1].glyphs[0].code, 66);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    assert_eq!(
        page.runs[0].provenance.stream,
        onionskin_cos::ObjRef::new(4, 0)
    );
    assert_eq!(
        page.runs[1].provenance.stream,
        onionskin_cos::ObjRef::new(5, 0)
    );
    for (run, source, expected) in [
        (
            &page.runs[0],
            stream4_content.as_slice(),
            b"(A) Tj" as &[u8],
        ),
        (&page.runs[1], stream5_content.as_slice(), b"(B) Tj"),
    ] {
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(&source[start..end], expected);
    }
    assert!(!page
        .warnings
        .iter()
        .any(|warning| matches!(warning, Warning::ProvenanceClamped { .. })));

    let coverage = flat.pieces()[0].coverage_for(&page, 1..2);
    assert_eq!(coverage.len(), 2);
    assert!(std::ptr::eq(coverage[0].run, &page.runs[0]));
    assert!(std::ptr::eq(coverage[1].run, &page.runs[1]));
    assert!(coverage
        .iter()
        .all(|member| member.coverage == RunCoverage::WholeActualText));
    let hit = search(&page, "Y", Default::default());
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].text, "Y");
    assert_eq!(hit[0].range, 1..2);
    assert_eq!(
        hit[0].quads,
        page.runs
            .iter()
            .map(|run| run.glyphs[0].quad)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hit[0].provenance,
        page.runs
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );
}

#[test]
fn actual_text_case_folding_preserves_source_coverage() {
    let content =
        "BT /F1 12 Tf 10 100 Td /Span << /ActualText <FEFF01300058> >> BDC (A) Tj (B) Tj EMC ET";
    let doc = open_bytes(one_page(
        content,
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");
    let flat = page.flatten();
    assert_eq!(flat.text, "İX");
    assert_eq!(flat.pieces().len(), 1);
    assert_eq!(flat.pieces()[0].range, 0..3);
    assert_eq!(flat.pieces()[0].style_run, 0);
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].glyphs[0].code, 65);
    assert_eq!(page.runs[1].glyphs[0].code, 66);
    assert_eq!(page.runs[0].decoded_text, "A");
    assert_eq!(page.runs[1].decoded_text, "B");
    assert_eq!(page.runs[0].glyphs.len(), 1);
    assert_eq!(page.runs[1].glyphs.len(), 1);
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    assert_eq!(
        page.runs[0].provenance.stream,
        onionskin_cos::ObjRef::new(4, 0)
    );
    assert_eq!(
        page.runs[1].provenance.stream,
        onionskin_cos::ObjRef::new(4, 0)
    );
    for (run, expected) in [
        (&page.runs[0], b"(A) Tj" as &[u8]),
        (&page.runs[1], b"(B) Tj"),
    ] {
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(&content.as_bytes()[start..end], expected);
    }
    let coverage = flat.pieces()[0].coverage_for(&page, 0..2);
    assert_eq!(coverage.len(), 2);
    assert!(std::ptr::eq(coverage[0].run, &page.runs[0]));
    assert!(std::ptr::eq(coverage[1].run, &page.runs[1]));
    assert!(coverage
        .iter()
        .all(|member| member.coverage == RunCoverage::WholeActualText));

    let default_hits = search(&page, "i", Default::default());
    assert_eq!(default_hits.len(), 1);
    assert_eq!(default_hits[0].text, "İ");
    assert_eq!(default_hits[0].range, 0..2);
    assert_eq!(
        default_hits[0].quads,
        page.runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        default_hits[0].provenance,
        page.runs
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );
    assert!(search(
        &page,
        "i",
        onionskin_content::SearchOptions {
            case_sensitive: true,
            ..Default::default()
        }
    )
    .is_empty());
    let sensitive_hits = search(
        &page,
        "İ",
        onionskin_content::SearchOptions {
            case_sensitive: true,
            ..Default::default()
        },
    );
    assert_eq!(sensitive_hits.len(), 1);
    assert_eq!(sensitive_hits[0].text, "İ");
    assert_eq!(sensitive_hits[0].range, 0..2);
    assert_eq!(sensitive_hits[0].quads, default_hits[0].quads);
    assert_eq!(sensitive_hits[0].provenance, default_hits[0].provenance);
}

#[test]
fn actual_text_replacement_whitespace_owns_replacement_glyph() {
    let cases = [
        (
            "BT /F1 12 Tf 10 100 Td /Span << /ActualText (X ) >> BDC (A) Tj EMC 100 0 Td (B) Tj ET",
            "X B",
            0..2,
            2..3,
            2..3,
            "B",
            b"(A) Tj" as &[u8],
            "A",
            [65_u32, 66],
            [1, 1],
        ),
        (
            "BT /F1 12 Tf 10 100 Td (A) Tj 100 0 Td /Span << /ActualText ( Y) >> BDC (B) Tj EMC ET",
            "A Y",
            0..1,
            1..3,
            2..3,
            "Y",
            b"(A) Tj",
            "A",
            [65_u32, 66],
            [1, 1],
        ),
        (
            "BT /F1 12 Tf 10 100 Td /Span << /ActualText (X) >> BDC (A ) Tj EMC 100 0 Td (B) Tj ET",
            "X B",
            0..1,
            2..3,
            2..3,
            "B",
            b"(A ) Tj",
            "A ",
            [65_u32, 66],
            [2, 1],
        ),
    ];
    for (
        case_index,
        (
            content,
            expected_text,
            first_range,
            second_piece_range,
            expected_range,
            needle,
            first_source,
            first_decoded,
            codes,
            glyph_counts,
        ),
    ) in cases.into_iter().enumerate()
    {
        let doc = open_bytes(one_page(
            content,
            "<< /Font << /F1 5 0 R >> >>",
            &[helvetica()],
        ));
        let page = extract_page(&doc, 0).expect("page extracts");
        let flat = page.flatten();
        assert_eq!(flat.text, expected_text);
        assert_eq!(flat.pieces()[0].range, first_range);
        assert_eq!(flat.pieces().len(), 2);
        assert_eq!(flat.pieces()[1].range, second_piece_range);
        assert_eq!(page.runs.len(), 2);
        assert_eq!(page.runs[0].decoded_text, first_decoded);
        assert_eq!(page.runs[1].decoded_text, "B");
        assert_eq!(page.runs[0].glyphs[0].code, codes[0]);
        assert_eq!(page.runs[1].glyphs[0].code, codes[1]);
        assert_eq!(page.runs[0].glyphs.len(), glyph_counts[0]);
        assert_eq!(page.runs[1].glyphs.len(), glyph_counts[1]);
        assert_eq!(
            page.runs[0].provenance.stream,
            onionskin_cos::ObjRef::new(4, 0)
        );
        assert_eq!(
            page.runs[1].provenance.stream,
            onionskin_cos::ObjRef::new(4, 0)
        );
        for (run, expected) in [(&page.runs[0], first_source), (&page.runs[1], b"(B) Tj")] {
            let start = run.provenance.decoded.start as usize;
            let end = run.provenance.decoded.end as usize;
            assert_eq!(&content.as_bytes()[start..end], expected);
        }
        if case_index == 1 {
            assert!(page.runs[0].actual_text.is_none());
            assert!(page.runs[1].actual_text.is_some());
        } else {
            assert!(page.runs[0].actual_text.is_some());
            assert!(page.runs[1].actual_text.is_none());
        }
        let replacement = flat.pieces()[if case_index == 1 { 1 } else { 0 }]
            .coverage_for(&page, if case_index == 1 { 1..3 } else { 0..2 });
        assert_eq!(replacement.len(), 1);
        assert!(std::ptr::eq(
            replacement[0].run,
            &page.runs[if case_index == 1 { 1 } else { 0 }]
        ));
        assert_eq!(replacement[0].coverage, RunCoverage::WholeActualText);
        let ordinary = flat.pieces()[if case_index == 1 { 0 } else { 1 }]
            .coverage_for(&page, if case_index == 1 { 0..1 } else { 2..3 });
        assert_eq!(ordinary.len(), 1);
        assert!(std::ptr::eq(
            ordinary[0].run,
            &page.runs[if case_index == 1 { 0 } else { 1 }]
        ));
        assert_eq!(ordinary[0].coverage, RunCoverage::Decoded(0..1));
        let hits = search(&page, needle, Default::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, needle);
        assert_eq!(hits[0].range, expected_range);
        assert_eq!(
            hits[0].quads,
            vec![page.runs[1].glyphs.last().unwrap().quad]
        );
        assert_eq!(hits[0].provenance, vec![page.runs[1].provenance]);
        if case_index == 2 {
            let separator = flat.pieces()[0].coverage_for(&page, 1..2);
            let next = flat.pieces()[1].coverage_for(&page, 1..2);
            assert!(separator.is_empty() && next.is_empty());
            assert_eq!(page.runs[0].glyphs[1].code, 32);
            let x = search(&page, "X", Default::default());
            assert_eq!(x.len(), 1);
            assert_eq!(x[0].text, "X");
            assert_eq!(x[0].range, 0..1);
            assert_eq!(
                x[0].quads,
                page.runs[0]
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.quad)
                    .collect::<Vec<_>>()
            );
            assert_eq!(x[0].provenance, vec![page.runs[0].provenance]);
        }
    }
}

#[test]
fn actual_text_continuation_endpoint_geometry_keeps_local_ranges() {
    let cases = [
        (
            "BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj 1 0 0 1 100 50 Tm (B) Tj EMC (C) Tj ET",
            "XYC",
            2..3,
        ),
        (
            "BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj 1 0 0 1 100 50 Tm (B) Tj EMC 1 0 0 1 10 100 Tm (C) Tj ET",
            "XY\nC",
            3..4,
        ),
    ];
    for (content, expected_text, expected_range) in cases {
        let doc = open_bytes(one_page(
            content,
            "<< /Font << /F1 5 0 R >> >>",
            &[helvetica()],
        ));
        let page = extract_page(&doc, 0).expect("page extracts");
        let flat = page.flatten();
        assert_eq!(flat.text, expected_text);
        assert_eq!(flat.pieces().len(), 2);
        assert_eq!(page.runs.len(), 3);
        assert_eq!(
            page.runs
                .iter()
                .map(|run| run.glyphs.len())
                .collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
        assert_eq!(
            page.runs
                .iter()
                .map(|run| run.glyphs[0].code)
                .collect::<Vec<_>>(),
            vec![65, 66, 67]
        );
        assert_eq!(
            page.runs
                .iter()
                .map(|run| run.decoded_text.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
        assert!(page.runs[0].actual_text.is_some());
        assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
        assert!(page.runs[2].actual_text.is_none());
        for (run, expected) in [
            (&page.runs[0], b"(A) Tj" as &[u8]),
            (&page.runs[1], b"(B) Tj"),
            (&page.runs[2], b"(C) Tj"),
        ] {
            assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(4, 0));
            let start = run.provenance.decoded.start as usize;
            let end = run.provenance.decoded.end as usize;
            assert_eq!(&content.as_bytes()[start..end], expected);
        }
        assert_eq!(flat.pieces()[0].range, 0..2);
        assert_eq!(flat.pieces()[1].range, expected_range);
        let owned = flat.pieces()[0].coverage_for(&page, 0..2);
        assert_eq!(owned.len(), 2);
        assert!(std::ptr::eq(owned[0].run, &page.runs[0]));
        assert!(std::ptr::eq(owned[1].run, &page.runs[1]));
        assert!(owned
            .iter()
            .all(|member| member.coverage == RunCoverage::WholeActualText));
        let coverage = flat.pieces()[1].coverage_for(&page, expected_range.clone());
        assert_eq!(coverage.len(), 1);
        assert!(std::ptr::eq(coverage[0].run, &page.runs[2]));
        assert_eq!(coverage[0].coverage, RunCoverage::Decoded(0..1));
        if expected_range.start == 3 {
            assert!(flat.pieces()[0].coverage_for(&page, 2..3).is_empty());
            assert!(flat.pieces()[1].coverage_for(&page, 2..3).is_empty());
        }
        let hit = search(&page, "C", Default::default());
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].text, "C");
        assert_eq!(hit[0].range, expected_range);
        assert_eq!(hit[0].quads, vec![page.runs[2].glyphs[0].quad]);
        assert_eq!(hit[0].provenance, vec![page.runs[2].provenance]);
    }
}

#[test]
fn actual_text_unknown_glyphs_are_semantically_covered() {
    let font = "<< /Type /Font /Subtype /TrueType /BaseFont /Sub \
                /FirstChar 65 /LastChar 65 /Widths [500] \
                /Encoding << /Differences [65 /g5] >> >>";
    let content = "BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj (A) Tj EMC ET";
    let doc = open_bytes(one_page(
        content,
        "<< /Font << /F1 5 0 R >> >>",
        &[font.as_bytes().to_vec()],
    ));
    let page = extract_page(&doc, 0).expect("page extracts");
    assert_eq!(page.flatten().text, "XY");
    assert!(!page.has_unmapped());
    assert_eq!(page.runs.len(), 2);
    assert!(page.runs.iter().all(|run| run.decoded_text.is_empty()
        && run.glyphs.len() == 1
        && run.glyphs[0].code == 65
        && run.glyphs[0].is_unmapped()));
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    assert!(page.runs[0].actual_text.is_some());
    for run in &page.runs {
        assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(4, 0));
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(&content.as_bytes()[start..end], b"(A) Tj");
    }
    let flat = page.flatten();
    assert_eq!(flat.pieces().len(), 1);
    assert_eq!(flat.pieces()[0].range, 0..2);
    let coverage = flat.pieces()[0].coverage_for(&page, 1..2);
    assert_eq!(coverage.len(), 2);
    assert!(std::ptr::eq(coverage[0].run, &page.runs[0]));
    assert!(std::ptr::eq(coverage[1].run, &page.runs[1]));
    assert!(coverage
        .iter()
        .all(|member| member.coverage == RunCoverage::WholeActualText));
    assert_eq!(page.warnings.iter().filter(|warning| matches!(warning, Warning::UnmappedGlyphs { font, count: 2 } if font == "Sub")).count(), 1);
    let hit = search(&page, "Y", Default::default());
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].text, "Y");
    assert_eq!(hit[0].range, 1..2);
    assert_eq!(
        hit[0].quads,
        page.runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hit[0].provenance,
        page.runs
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );

    let tail_content =
        "BT /F1 12 Tf 10 100 Td /Span << /ActualText (XY) >> BDC (A) Tj (A) Tj EMC (A) Tj ET";
    let tail_doc = open_bytes(one_page(
        tail_content,
        "<< /Font << /F1 5 0 R >> >>",
        &[font.as_bytes().to_vec()],
    ));
    let tail = extract_page(&tail_doc, 0).expect("tail page extracts");
    assert_eq!(tail.flatten().text, "XY");
    assert!(tail.has_unmapped());
    assert_eq!(tail.runs.len(), 3);
    assert!(tail.runs[2].actual_text.is_none());
    assert!(tail.runs[2].decoded_text.is_empty());
    assert!(tail.runs.iter().all(|run| run.decoded_text.is_empty()
        && run.glyphs.len() == 1
        && run.glyphs[0].code == 65
        && run.glyphs[0].is_unmapped()));
    assert!(tail.runs[..2].iter().all(|run| run.actual_text.is_some()));
    assert_eq!(tail.runs[0].actual_text, tail.runs[1].actual_text);
    for run in &tail.runs {
        assert_eq!(run.provenance.stream, onionskin_cos::ObjRef::new(4, 0));
        let start = run.provenance.decoded.start as usize;
        let end = run.provenance.decoded.end as usize;
        assert_eq!(&tail_content.as_bytes()[start..end], b"(A) Tj");
    }
    assert_eq!(tail.warnings.iter().filter(|warning| matches!(warning, Warning::UnmappedGlyphs { font, count: 3 } if font == "Sub")).count(), 1);
    let tail_flat = tail.flatten();
    assert_eq!(tail_flat.pieces().len(), 1);
    assert_eq!(tail_flat.pieces()[0].range, 0..2);
    let tail_coverage = tail_flat.pieces()[0].coverage_for(&tail, 1..2);
    assert_eq!(tail_coverage.len(), 2);
    assert!(std::ptr::eq(tail_coverage[0].run, &tail.runs[0]));
    assert!(std::ptr::eq(tail_coverage[1].run, &tail.runs[1]));
    assert!(tail_coverage
        .iter()
        .all(|member| member.coverage == RunCoverage::WholeActualText));
    let tail_hit = search(&tail, "Y", Default::default());
    assert_eq!(tail_hit.len(), 1);
    assert_eq!(tail_hit[0].text, "Y");
    assert_eq!(tail_hit[0].range, 1..2);
    assert_eq!(
        tail_hit[0].quads,
        tail.runs[0..2]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        tail_hit[0].provenance,
        tail.runs[0..2]
            .iter()
            .map(|run| run.provenance)
            .collect::<Vec<_>>()
    );
}

/// The page's runs joined, which is what a search or a diff against another
/// extractor actually sees.
fn text_of(doc: &onionskin_cos::Document) -> String {
    extract_page(doc, 0).expect("page extracts").flatten().text
}

#[test]
fn a_negative_root_count_is_not_clamped_to_zero() {
    let doc = open_bytes(build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count -1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".to_vec(),
    ]));

    let error = onionskin_content::page_count(&doc)
        .expect_err("a negative root count must not become an empty document");
    assert_eq!(error.category(), "invalid-page-count");
}

// ---- finding 1: no fabricated characters ------------------------------------

/// A subsetter that writes `/Differences [65 /g5]` is saying code 65 draws the
/// font's fifth glyph. `g5` names a position in a font program and says nothing
/// about which character it draws, so there is nothing to report.
///
/// Before the fix, the last-resort StandardEncoding lookup answered "A", which
/// contradicts the contract the crate documents and is a fabricated character
/// on the exact path where fabrication is most likely wrong.
#[test]
fn a_position_only_glyph_name_yields_no_character() {
    let font = "<< /Type /Font /Subtype /TrueType /BaseFont /Sub \
                /FirstChar 65 /LastChar 65 /Widths [500] \
                /Encoding << /Differences [65 /g5] >> >>";
    let doc = open_bytes(one_page(
        "BT /F1 12 Tf 10 100 Td (A) Tj ET",
        "<< /Font << /F1 5 0 R >> >>",
        &[font.as_bytes().to_vec()],
    ));
    let page = extract_page(&doc, 0).unwrap();

    assert_eq!(page.runs.len(), 1, "the glyph is still positioned");
    let run = &page.runs[0];
    assert_eq!(run.glyphs.len(), 1);
    assert_eq!(run.glyphs[0].code, 65, "its code survives");
    assert!(
        run.glyphs[0].is_unmapped(),
        "extracted {:?} for a glyph the font does not explain",
        run.decoded_text
    );
    assert_eq!(run.decoded_text, "");
    assert!(page
        .warnings
        .iter()
        .any(|w| matches!(w, Warning::UnmappedGlyphs { count: 1, .. })));
}

/// The same page with an encoding that does say what the code means still
/// reads, so the fix above removed a guess and not the answer.
#[test]
fn a_real_glyph_name_still_yields_its_character() {
    let font = "<< /Type /Font /Subtype /TrueType /BaseFont /Sub \
                /FirstChar 65 /LastChar 65 /Widths [500] \
                /Encoding << /Differences [65 /eacute] >> >>";
    let doc = open_bytes(one_page(
        "BT /F1 12 Tf 10 100 Td (A) Tj ET",
        "<< /Font << /F1 5 0 R >> >>",
        &[font.as_bytes().to_vec()],
    ));
    assert_eq!(text_of(&doc), "\u{e9}");
}

// ---- finding 2: /ActualText belongs to its span, once -----------------------

/// ISO 32000-2 14.9.4 replaces the whole marked-content sequence, not its
/// first showing operator. Before the fix the first run took the replacement
/// and every later run in the span kept its own text, so a two-operator span
/// extracted the replacement plus the tail it was supposed to replace.
#[test]
fn actual_text_replaces_the_whole_span_not_just_its_first_operator() {
    let doc = open_bytes(one_page(
        "/Span << /ActualText (XY) >> BDC \
         BT /F1 12 Tf 10 100 Td (AB) Tj (CD) Tj ET EMC",
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).unwrap();

    assert_eq!(page.flatten().text, "XY", "the span spells XY and only XY");
    assert_eq!(page.runs.len(), 2, "both operators still produce a run");
    assert_eq!(page.runs[0].decoded_text, "AB");
    assert_eq!(
        page.runs[1].decoded_text, "CD",
        "the tail remains separately decoded"
    );
    assert!(page.runs[0].actual_text.is_some());
    assert_eq!(page.runs[0].actual_text, page.runs[1].actual_text);
    // Every glyph stays positioned, in both runs.
    assert_eq!(page.runs[0].glyphs.len(), 2);
    assert_eq!(page.runs[1].glyphs.len(), 2);
    assert!(page.runs[1].glyphs.iter().all(|g| !g.is_unmapped()));
}

/// A span that closes releases its replacement, so the next span is unaffected.
#[test]
fn a_closed_actual_text_span_does_not_leak_into_the_next() {
    let doc = open_bytes(one_page(
        "/Span << /ActualText (XY) >> BDC BT /F1 12 Tf 10 100 Td (AB) Tj ET EMC \
         BT /F1 12 Tf 10 50 Td (CD) Tj ET",
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    assert_eq!(text_of(&doc), "XY\nCD");
}

// ---- finding 4: text with no font is not silently dropped -------------------

/// A showing operator with no `Tf` before it draws nothing, so there is no
/// text to extract. Saying so is the point: a redaction verifier that reports
/// "no text here" must be able to tell "the page has none" from "the page has
/// some and I could not read it".
#[test]
fn showing_text_with_no_font_selected_is_reported() {
    let doc = open_bytes(one_page("BT 10 100 Td (invisible) Tj ET", "<< >>", &[]));
    let page = extract_page(&doc, 0).unwrap();
    assert!(page.runs.is_empty());
    assert!(
        page.warnings
            .iter()
            .any(|w| matches!(w, Warning::TextWithoutFont { count: 1 })),
        "warnings were {:?}",
        page.warnings
    );
}

// ---- finding 6: operands are read from the top of the stack -----------------

/// ISO 32000-2 7.8.2 gives an operator the operands immediately preceding it.
/// A malformed stream that leaves a stray operand underneath must not shift
/// every later operand by one, which is what indexing from the bottom did:
/// `Tf` read the stray number as its font name and selected no font at all.
#[test]
fn a_stray_leading_operand_does_not_shift_the_operator_that_follows() {
    let doc = open_bytes(one_page(
        "BT 99 /F1 12 Tf 7 10 100 Td (Hi) Tj ET",
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).unwrap();
    assert_eq!(page.flatten().text, "Hi");
    // Td took the last two operands, so the run starts at x = 10.
    let first = page.runs[0].glyphs[0].quad.corners[2];
    assert!((first.0 - 10.0).abs() < 1e-9, "run starts at {}", first.0);
    assert!((first.1 - (100.0 - 0.25 * 12.0)).abs() < 1e-9);
}

// ---- finding 8: a /Contents entry that is not a stream is reported ----------

#[test]
fn a_contents_array_entry_that_is_not_a_reference_is_reported() {
    let doc = open_bytes(build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
           /Resources << /Font << /F1 5 0 R >> >> /Contents [4 0 R 42] >>"
            .to_vec(),
        stream("", b"BT /F1 12 Tf 10 100 Td (Hi) Tj ET"),
        helvetica(),
    ]));
    let page = extract_page(&doc, 0).unwrap();
    assert_eq!(page.flatten().text, "Hi", "the real part still reads");
    assert!(
        page.warnings
            .iter()
            .any(|w| matches!(w, Warning::ContentPartFailed { .. })),
        "warnings were {:?}",
        page.warnings
    );
}

// ---- finding 9: an operation straddling a /Contents join is reported --------

/// ISO 32000-2 7.8.2 lets a page split its content between streams at any
/// token boundary, so an operator can legally sit in a later part than its
/// operands. Provenance names one stream, so such a run's byte range is
/// clamped to the part it starts in - which is a limitation to state, not one
/// to describe in a comment as impossible.
#[test]
fn an_operation_split_across_content_parts_reports_the_clamp() {
    let doc = open_bytes(build_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
           /Resources << /Font << /F1 6 0 R >> >> /Contents [4 0 R 5 0 R] >>"
            .to_vec(),
        stream("", b"BT /F1 12 Tf 10 100 Td (Hi)"),
        stream("", b"Tj ET"),
        helvetica(),
    ]));
    let page = extract_page(&doc, 0).unwrap();
    assert_eq!(page.flatten().text, "Hi", "the split operator still runs");

    let run = &page.runs[0];
    assert_eq!(
        run.provenance.stream.number, 4,
        "provenance names the part the run starts in"
    );
    assert!(
        page.warnings
            .iter()
            .any(|w| matches!(w, Warning::ProvenanceClamped { .. })),
        "warnings were {:?}",
        page.warnings
    );
}

// ---- finding 10a: the graphics state stack past its cap ---------------------

/// A `q` past the depth cap is not stored, so its matching `Q` must pop
/// nothing. Before the counter, it popped the state an outer `q` had saved,
/// and every later glyph landed at the outer transform instead of the inner
/// one.
#[test]
fn a_q_past_the_stack_cap_does_not_let_its_q_pop_an_outer_state() {
    // 1024 is the cap, so the 1025th q is the one that cannot be stored.
    let mut content = "q ".repeat(1024);
    content.push_str("1 0 0 1 100 0 cm q 1 0 0 1 200 0 cm Q ");
    content.push_str("BT /F1 12 Tf 0 100 Td (X) Tj ET");
    let doc = open_bytes(one_page(
        &content,
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    let page = extract_page(&doc, 0).unwrap();
    let x = page.runs[0].glyphs[0].quad.corners[2].0;
    assert!(
        (x - 300.0).abs() < 1e-9,
        "glyph at x = {x}, so the unmatched Q undid a transform it never saved"
    );
}

// ---- finding 10b: the vertical word gap ------------------------------------

/// Vertical writing advances down the page, so a `TJ` gap is an adjustment
/// pushing the pen further negative. The horizontal sign test inserted a space
/// on kerns that pull the glyphs together and none on the gaps that separate
/// words, which is exactly backwards.
#[test]
fn a_vertical_tj_gap_separates_words_and_a_kern_does_not() {
    let to_unicode = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
        1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
        2 beginbfchar <0041> <0041> <0042> <0042> endbfchar\n\
        endcmap end end";
    let extra = vec![
        // 5: the Type 0 font
        b"<< /Type /Font /Subtype /Type0 /BaseFont /V /Encoding /Identity-V \
           /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>"
            .to_vec(),
        // 6: its descendant
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /V /DW 1000 \
           /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> >>"
            .to_vec(),
        // 7: the ToUnicode CMap
        stream("", to_unicode),
    ];

    let gap = open_bytes(one_page(
        "BT /F1 12 Tf 100 180 Td [<0041> 500 <0042>] TJ ET",
        "<< /Font << /F1 5 0 R >> >>",
        &extra,
    ));
    assert_eq!(text_of(&gap), "A B", "a downward gap separates the words");

    let kern = open_bytes(one_page(
        "BT /F1 12 Tf 100 180 Td [<0041> -500 <0042>] TJ ET",
        "<< /Font << /F1 5 0 R >> >>",
        &extra,
    ));
    assert_eq!(text_of(&kern), "AB", "an upward kern does not");
}

/// The horizontal rule is unchanged by the vertical one, and this is the case
/// that made it necessary: TeX writes its inter-word space as a TJ number.
#[test]
fn a_horizontal_tj_gap_still_separates_words() {
    let doc = open_bytes(one_page(
        "BT /F1 12 Tf 10 100 Td [(A)-333(B)28(C)] TJ ET",
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    ));
    assert_eq!(text_of(&doc), "A BC");
}

// ---- the contract, on a document that exercises all of it -------------------

/// Nothing above may quietly cost the ordinary case: a mapped glyph keeps its
/// character, its quad and the bytes that drew it.
#[test]
fn the_ordinary_case_still_traces_end_to_end() {
    let bytes = one_page(
        "BT /F1 12 Tf 10 100 Td (Hi) Tj ET",
        "<< /Font << /F1 5 0 R >> >>",
        &[helvetica()],
    );
    let doc = open_bytes(bytes.clone());
    let page = extract_page(&doc, 0).unwrap();
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);

    let run = &page.runs[0];
    assert_eq!(run.decoded_text, "Hi");
    assert!(matches!(run.glyphs[0].mapping, Mapping::Text(ref r) if *r == (0..1)));
    let span = run.provenance.file_span().unwrap();
    let object = &bytes[span.start as usize..span.end as usize];
    assert!(object.starts_with(b"4 0 obj"));
    assert!(object
        .windows(8)
        .any(|window| window == b"(Hi) Tj\n" || window == b"(Hi) Tj "));
}
