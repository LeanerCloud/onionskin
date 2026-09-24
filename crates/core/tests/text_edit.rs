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
    use onionskin_core::text_edit::add_text;
    let bytes = tagged();
    let added = apply(&bytes, |tx, _| {
        add_text(tx, 0, (72.0, 400.0), 18.0, "Added note €")
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
        add_text(tx, 0, (72.0, 300.0), 12.0, "Again")
    });
    assert_eq!(texts(&again, 0), ["Page 1", "Added note €", "Again"]);
    assert_eq!(texts(&again, 1), ["Page 2"], "the inherited resources stay");
    let content = content_of(&again, 0);
    assert!(
        content.contains("/OSF1_Helvetica 12 Tf"),
        "a name of its own: {content}"
    );

    for refused in ["日本", "   "] {
        let error = common::try_apply(&bytes, |tx, _| {
            add_text(tx, 0, (72.0, 400.0), 12.0, refused)
        })
        .expect_err("refused");
        assert!(error.to_string().contains("no"), "{error}");
    }
}
