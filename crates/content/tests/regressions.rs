//! One test per defect, over a document built to exhibit exactly that defect.
//!
//! The corpus tests prove extraction works on real files. These prove it
//! behaves on the file that broke it, which is a different question: a
//! malformation rare enough to matter is rare enough that no corpus file is
//! guaranteed to contain it. Every test here failed before the fix it pins.

mod common;

use common::{build_pdf, one_page, open_bytes, stream};
use onionskin_content::{extract_page, Mapping, Warning};

const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

fn helvetica() -> Vec<u8> {
    HELVETICA.as_bytes().to_vec()
}

/// The page's runs joined, which is what a search or a diff against another
/// extractor actually sees.
fn text_of(doc: &onionskin_cos::Document) -> String {
    extract_page(doc, 0).expect("page extracts").flatten().text
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
        run.text
    );
    assert_eq!(run.text, "");
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
    assert_eq!(page.runs[0].text, "XY");
    assert_eq!(
        page.runs[1].text, "",
        "the tail of the span contributes no text of its own"
    );
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
    assert_eq!(run.text, "Hi");
    assert!(matches!(run.glyphs[0].mapping, Mapping::Text(ref r) if *r == (0..1)));
    let span = run.provenance.file_span().unwrap();
    let object = &bytes[span.start as usize..span.end as usize];
    assert!(object.starts_with(b"4 0 obj"));
    assert!(object
        .windows(8)
        .any(|window| window == b"(Hi) Tj\n" || window == b"(Hi) Tj "));
}
