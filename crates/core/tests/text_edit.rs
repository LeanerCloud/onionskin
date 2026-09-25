//! Editing text and replacing it, written as an incremental section and
//! read back from a fresh parse: the new text where the old was, the other
//! pages untouched, a standard font added when the page's own cannot draw
//! it, and a tagged page's structure still valid.

mod common;

use common::{apply, open, pdf, stream, tagged, FONT};
use onionskin_core::text_edit::{
    find_in_lines, page_lines, replace_matches, rewrite_lines, write_page_edit, MatchOptions,
};
use onionskin_core::{check, read_structure};

fn texts(bytes: &[u8], page: usize) -> Vec<String> {
    page_lines(&open(bytes), page)
        .expect("reads")
        .into_iter()
        .map(|line| line.text)
        .collect()
}

fn content_of(bytes: &[u8], page: usize) -> String {
    let doc = open(bytes);
    let loaded = onionskin_content::page(&doc, page).expect("page");
    let mut warnings = Vec::new();
    let content = onionskin_content::content(&doc, &loaded, &mut warnings).expect("content");
    String::from_utf8_lossy(&content.bytes).into_owned()
}

#[test]
fn a_line_is_rewritten_and_the_structure_still_finds_it() {
    let bytes = tagged();
    let doc = open(&bytes);
    let lines = page_lines(&doc, 1).expect("reads");
    assert_eq!(lines[0].text, "Page 2");
    let edit = rewrite_lines(&doc, 1, &[(&lines[0], "Chapter 2".to_owned())]).expect("works out");
    let edited = apply(&bytes, |tx, _| write_page_edit(tx, &edit));

    assert_eq!(texts(&edited, 1), ["Chapter 2"]);
    assert_eq!(texts(&edited, 0), ["Page 1"], "the other pages stay");
    assert_eq!(texts(&edited, 2), ["Page 3"]);
    let content = content_of(&edited, 1);
    assert!(
        content.contains("/MCID 0"),
        "the marked content stays: {content}"
    );
    let after = open(&edited);
    let structure = read_structure(&after).expect("reads the tree");
    let report = check(&after, &structure, 3).expect("the invariant runs");
    assert!(report.violations.is_empty(), "{report:?}");
}

#[test]
fn every_occurrence_is_replaced_in_one_edit() {
    let bytes = tagged();
    let doc = open(&bytes);
    let options = MatchOptions::default();
    let found = find_in_lines(&doc, 0..3, "page", options).expect("finds");
    assert_eq!(found.len(), 3, "case folded");
    let strict = MatchOptions {
        case_sensitive: true,
        ..options
    };
    assert!(find_in_lines(&doc, 0..3, "page", strict)
        .expect("finds")
        .is_empty());

    let edits = replace_matches(&doc, &found, "Folio").expect("works out");
    assert_eq!(edits.len(), 3, "one per page");
    let edited = apply(&bytes, |tx, _| {
        edits.iter().try_for_each(|edit| write_page_edit(tx, edit))
    });
    for page in 0..3 {
        assert_eq!(texts(&edited, page), [format!("Folio {}", page + 1)]);
    }
}

#[test]
fn a_character_the_font_lacks_is_set_in_a_standard_font() {
    let bytes = tagged();
    let doc = open(&bytes);
    let lines = page_lines(&doc, 0).expect("reads");
    // Helvetica here has no /Encoding: StandardEncoding, which has no euro.
    let edit = rewrite_lines(&doc, 0, &[(&lines[0], "Page 1 €5".to_owned())]).expect("works out");
    let edited = apply(&bytes, |tx, _| write_page_edit(tx, &edit));
    assert_eq!(texts(&edited, 0), ["Page 1 €5"]);
    let content = content_of(&edited, 0);
    assert!(content.contains("/OSFHelvetica 24 Tf"), "{content}");
    assert!(content.contains("/F1 24 Tf"), "the page's font again after");
    assert_eq!(
        texts(&edited, 1),
        ["Page 2"],
        "the inherited resources stay"
    );
}

#[test]
fn what_cannot_be_edited_is_refused() {
    let bytes = tagged();
    let doc = open(&bytes);
    let lines = page_lines(&doc, 0).expect("reads");
    let error = rewrite_lines(&doc, 0, &[(&lines[0], "日本".to_owned())]).expect_err("refused");
    assert!(error.to_string().contains("cannot be written"), "{error}");

    let mut unknown = lines[0].clone();
    unknown.glyphs[0].range = None;
    let error = rewrite_lines(&doc, 0, &[(&unknown, "x".to_owned())]).expect_err("refused");
    assert!(error.to_string().contains("unknown"), "{error}");

    let mut stale = find_in_lines(&doc, 0..1, "Page", MatchOptions::default()).expect("finds");
    stale[0].line = 7;
    let error = replace_matches(&doc, &stale, "x").expect_err("refused");
    assert!(error.to_string().contains("no longer"), "{error}");

    // Text drawn by a form XObject.
    let in_form = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Fm0 5 0 R >> >> >>"
            .to_vec(),
        stream("/Fm0 Do"),
        common::stream_with(
            "BT /F1 12 Tf 72 700 Td (Inside) Tj ET",
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources {}",
                String::from_utf8_lossy(FONT)
            ),
        ),
    ]);
    let doc = open(&in_form);
    let lines = page_lines(&doc, 0).expect("reads");
    assert_eq!(lines[0].text, "Inside");
    let error = rewrite_lines(&doc, 0, &[(&lines[0], "Outside".to_owned())]).expect_err("refused");
    assert!(error.to_string().contains("form"), "{error}");
}

#[test]
fn actual_text_rewrite_is_refused_before_page_edit() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources 5 0 R >>".to_vec(),
        stream("BT /Span << /ActualText (XY) >> BDC /F1 10 Tf 20 100 Td (AB) Tj (CD) Tj EMC ET"),
        FONT.to_vec(),
    ]);
    let doc = open(&bytes);
    let lines = page_lines(&doc, 0).expect("reads");
    let error = rewrite_lines(&doc, 0, &[(&lines[0], "replacement".to_owned())])
        .expect_err("protected ActualText");
    assert!(error.to_string().contains("cannot be edited"), "{error}");
}

#[test]
fn forged_mapped_actual_text_line_is_refused() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources 5 0 R >>".to_vec(),
        stream("BT /Span << /ActualText (XY) >> BDC /F1 10 Tf 20 100 Td (AB) Tj (CD) Tj EMC ET"),
        FONT.to_vec(),
    ]);
    let doc = open(&bytes);
    let mut line = page_lines(&doc, 0).expect("reads").remove(0);
    for glyph in &mut line.glyphs {
        glyph.range = Some(0..1);
    }
    let error =
        rewrite_lines(&doc, 0, &[(&line, "replacement".to_owned())]).expect_err("forged mapping");
    assert!(error.to_string().contains("cannot be written"), "{error}");
}

#[test]
fn actual_text_refusal_preserves_session_and_safe_neighbors_round_trip() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources 5 0 R >>".to_vec(),
        stream(
            "BT /F1 10 Tf 1 0 0 1 10 100 Tm (L) Tj 1 0 0 1 50 100 Tm /Span << /ActualText (XY) >> BDC (AB) Tj (CD) Tj EMC 1 0 0 1 100 100 Tm (R) Tj ET",
        ),
        FONT.to_vec(),
    ]);
    let mut session = onionskin_core::Document::open_bytes(bytes.clone()).expect("opens");
    let before_preview = session
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentOnly)
        .expect("preview")
        .to_vec();
    let before_history = session.edit().history().reach();
    let before_page =
        onionskin_content::extract_page(session.structure().expect("structure"), 0).expect("page");
    let protected_quads = before_page.runs[1..3]
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
        .collect::<Vec<_>>();
    let protected = {
        let structure = session.structure().expect("structure");
        find_in_lines(structure, 0..1, "AB", MatchOptions::default()).expect("finds")
    };
    assert_eq!(protected.len(), 1);
    assert_eq!(protected[0].page, 0);
    assert_eq!(protected[0].line, 1);
    assert_eq!(protected[0].range, 0..2);
    let protected_line = page_lines(session.structure().expect("structure"), 0).expect("lines")
        [protected[0].line]
        .text
        .clone();
    assert_eq!(&protected_line[protected[0].range.clone()], "AB");
    let refusal = replace_matches(session.structure().expect("structure"), &protected, "NOPE")
        .expect_err("protected replacement");
    assert!(refusal.to_string().contains("cannot"), "{refusal}");
    assert_eq!(session.edit().history().reach(), before_history);
    assert_eq!(
        session
            .preview_bytes(onionskin_core::AnnotationFilter::DocumentOnly)
            .expect("preview")
            .as_ref(),
        before_preview.as_slice()
    );

    let ordinary_edits = {
        let structure = session.structure().expect("structure");
        let matches = find_in_lines(structure, 0..1, "L", MatchOptions::default())
            .expect("ordinary neighbor");
        assert_eq!(matches.len(), 1);
        replace_matches(structure, &matches, "LATER").expect("neighbor")
    };
    session
        .edit_content("Replace", |tx, _| {
            ordinary_edits
                .iter()
                .try_for_each(|edit| write_page_edit(tx, edit))
        })
        .expect("ordinary neighbor edit");
    let changed = session
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentOnly)
        .expect("changed preview")
        .to_vec();
    assert_ne!(changed, before_preview);
    assert_eq!(session.edit().history().reach(), before_history + 1);
    assert!(session.undo().expect("undo"));
    assert_eq!(
        session
            .preview_bytes(onionskin_core::AnnotationFilter::DocumentOnly)
            .expect("undo preview")
            .as_ref(),
        before_preview.as_slice()
    );
    assert!(session.redo().expect("redo"));
    assert_eq!(
        session
            .preview_bytes(onionskin_core::AnnotationFilter::DocumentOnly)
            .expect("redo preview")
            .as_ref(),
        changed.as_slice()
    );
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("neighbor.pdf");
    let mut file = onionskin_core::DocumentFile::from_document(session);
    file.save_as(&path).expect("save");
    let mut reopened = onionskin_core::Document::open_path(&path).expect("reopen");
    let reopened_lines =
        page_lines(reopened.structure().expect("reopened structure"), 0).expect("reopened lines");
    assert_eq!(
        reopened_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["LATER", "ABCD", "R"]
    );
    let reopened_page =
        onionskin_content::extract_page(reopened.structure().expect("structure"), 0)
            .expect("reopened page");
    assert_eq!(reopened_page.flatten().text, "LATER XY R");
    assert_eq!(
        reopened_page.runs[1..3]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        [65, 66, 67, 68]
    );
    assert_eq!(
        reopened_page.runs[1..3]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>(),
        protected_quads
    );
    let flat = reopened_page.flatten();
    let piece = flat
        .pieces()
        .iter()
        .find(|piece| piece.style_run == 1)
        .expect("replacement piece");
    assert_eq!(&flat.text[piece.range.clone()], "XY");
    assert_eq!(
        piece
            .coverage_for(&reopened_page, piece.range.clone())
            .len(),
        2
    );

    let mut right_session = onionskin_core::Document::open_bytes(bytes).expect("opens right case");
    let right_edits = {
        let structure = right_session.structure().expect("structure");
        let matches =
            find_in_lines(structure, 0..1, "R", MatchOptions::default()).expect("right neighbor");
        assert_eq!(matches.len(), 1);
        replace_matches(structure, &matches, "RIGHT").expect("right replacement")
    };
    right_session
        .edit_content("Replace", |tx, _| {
            right_edits
                .iter()
                .try_for_each(|edit| write_page_edit(tx, edit))
        })
        .expect("right edit");
    let right_dir = tempfile::tempdir().expect("temporary directory");
    let right_path = right_dir.path().join("right.pdf");
    let mut right_file = onionskin_core::DocumentFile::from_document(right_session);
    right_file.save_as(&right_path).expect("right save");
    let mut right_reopened =
        onionskin_core::Document::open_path(&right_path).expect("right reopen");
    let right_page =
        onionskin_content::extract_page(right_reopened.structure().expect("structure"), 0)
            .expect("right page");
    assert_eq!(right_page.flatten().text, "L XY RIGHT");
    assert_eq!(
        right_page.runs[1..3]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.code))
            .collect::<Vec<_>>(),
        [65, 66, 67, 68]
    );
    assert_eq!(
        right_page.runs[1..3]
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.quad))
            .collect::<Vec<_>>(),
        protected_quads
    );
}

#[test]
fn a_tool_asks_the_shell_to_edit_a_line_once() {
    let mut doc = onionskin_core::Document::open_bytes(tagged()).expect("opens");
    let request = onionskin_core::TextEditRequest {
        page: 0,
        line: Some(0),
        text: "Page 1".to_owned(),
        bounds: [72.0, 690.0, 140.0, 720.0],
    };
    doc.request_text_edit(request.clone());
    doc.request_text_edit(onionskin_core::TextEditRequest {
        line: None,
        ..request.clone()
    });
    doc.request_text_edit(request.clone());
    assert_eq!(
        doc.take_text_edit_request(),
        Some(request),
        "the last click"
    );
    assert_eq!(doc.take_text_edit_request(), None, "taken once");
}

#[test]
fn new_text_is_drawn_after_the_page_in_a_standard_font() {
    use onionskin_core::text_edit::{add_text, TextStyle};
    let bytes = tagged();
    let added = apply(&bytes, |tx, _| {
        add_text(
            tx,
            0,
            (72.0, 400.0),
            "Added note €",
            &TextStyle {
                size: Some(18.0),
                ..TextStyle::default()
            },
        )
    });
    assert_eq!(texts(&added, 0), ["Page 1", "Added note €"]);
    let doc = open(&added);
    let lines = page_lines(&doc, 0).expect("reads");
    let [x0, y0, _, _] = lines[1].bounds();
    assert!(
        (x0 - 72.0).abs() < 1e-6 && y0 < 400.0 && y0 > 390.0,
        "{:?}",
        lines[1].bounds()
    );
    let again = apply(&added, |tx, _| {
        add_text(
            tx,
            0,
            (72.0, 300.0),
            "Again",
            &TextStyle {
                face: Some("Times-Roman"),
                fill: Some([1.0, 0.0, 0.0]),
                ..TextStyle::default()
            },
        )
    });
    assert_eq!(texts(&again, 0), ["Page 1", "Added note €", "Again"]);
    assert_eq!(texts(&again, 1), ["Page 2"], "the inherited resources stay");
    let content = content_of(&again, 0);
    assert!(
        content.contains("/OSF1_Times-Roman 12 Tf"),
        "a name of its own: {content}"
    );
    assert!(content.contains("1 0 0 rg BT"), "in red: {content}");

    for refused in ["日本", "   "] {
        let error = common::try_apply(&bytes, |tx, _| {
            add_text(tx, 0, (72.0, 400.0), refused, &TextStyle::default())
        })
        .expect_err("refused");
        assert!(error.to_string().contains("no"), "{error}");
    }
}

#[test]
fn a_page_deleted_after_its_text_was_edited_leaves_a_whole_file() {
    // The page's dictionary stays in the section, changed; the new content
    // stream it names must stay with it, reached or not.
    let mut doc = onionskin_core::Document::open_bytes(tagged()).expect("opens");
    let edit = {
        let structure = doc.structure().expect("reads");
        let lines = page_lines(structure, 2).expect("reads");
        rewrite_lines(structure, 2, &[(&lines[0], "Last page".to_owned())]).expect("works out")
    };
    doc.edit_document("Edit Text", |tx| write_page_edit(tx, &edit))
        .expect("edits");
    doc.edit_annotations("Delete Pages", |tx, structure| {
        onionskin_core::pages::delete_pages(tx, structure, &[2]).map(|_| ())
    })
    .expect("deletes");
    let current = doc.structure().expect("the edited file still reads");
    assert_eq!(current.page_count().expect("pages"), 2);
    let whole = doc
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentAndMarkups)
        .expect("the edited file writes");
    let reopened = open(&whole);
    assert_eq!(reopened.page_count().expect("pages"), 2);
}

#[test]
fn a_line_is_rewritten_in_another_font_size_and_colour() {
    use onionskin_core::text_edit::{rewrite_styled_lines, TextStyle};
    let bytes = tagged();
    let doc = open(&bytes);
    let lines = page_lines(&doc, 0).expect("reads");
    let style = TextStyle {
        face: Some("Courier"),
        size: Some(30.0),
        fill: Some([0.0, 0.5, 0.0]),
    };
    let edit = rewrite_styled_lines(&doc, 0, &[(&lines[0], "Page one".to_owned(), style)])
        .expect("works out");
    let edited = apply(&bytes, |tx, _| write_page_edit(tx, &edit));
    assert_eq!(texts(&edited, 0), ["Page one"]);
    let content = content_of(&edited, 0);
    assert!(
        content.contains("0 0.5 0 rg /OSFCourier 30 Tf"),
        "{content}"
    );
    assert!(
        content.contains("/F1 24 Tf 0 g"),
        "put back, black by default: {content}"
    );
}
