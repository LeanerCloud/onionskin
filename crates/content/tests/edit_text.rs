//! Editing a line of text where it is, read back through extraction: the
//! new text drawn from where the old began, in the line's own font when it
//! can draw it and a standard font when it cannot, and everything else on
//! the page left where it was.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::edit_text::{edit_lines, EditError, EditedPage, LineEdit};
use onionskin_content::{extract_page, page, text_lines, PageText, TextLine};

const FONT: &str = "<< /Font << /F1 5 0 R >> >>";

fn helvetica() -> Vec<u8> {
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()
}

/// A subset whose program cannot be read: only the codes the page drew
/// with it can be trusted to have glyphs.
fn subset() -> Vec<Vec<u8>> {
    let widths = vec!["500"; 95].join(" ");
    vec![
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+Garamond-Bold /FirstChar 32 \
             /LastChar 126 /Widths [{widths}] /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /ABCDEF+Garamond-Bold /Flags 32 /FontFile 7 0 R >>"
            .to_vec(),
        stream("", b"not a font program"),
    ]
}

fn lines(bytes: &[u8]) -> (PageText, Vec<TextLine>) {
    let text = extract_page(&open_bytes(bytes.to_vec()), 0).expect("extracts");
    let lines = text_lines(&text);
    (text, lines)
}

fn line_edit(line: &TextLine, text: &str) -> LineEdit {
    LineEdit {
        glyphs: line.glyphs.iter().map(|glyph| glyph.at).collect(),
        text: text.to_owned(),
    }
}

fn edit_all(bytes: &[u8], edits: &[LineEdit]) -> Result<EditedPage, EditError> {
    let doc = open_bytes(bytes.to_vec());
    let page = page(&doc, 0).expect("page");
    edit_lines(&doc, &page, edits, "OSF")
}

fn edit(bytes: &[u8], line: &TextLine, text: &str) -> Result<EditedPage, EditError> {
    edit_all(bytes, &[line_edit(line, text)])
}

/// The page drawn by the edited content.
fn reread(edited: &EditedPage, resources: &str, extra: &[Vec<u8>]) -> Vec<TextLine> {
    let content = String::from_utf8(edited.content.bytes.clone()).expect("ascii");
    let bytes = one_page(&content, resources, extra);
    lines(&bytes).1
}

fn x_of(line: &TextLine, text: &str) -> f64 {
    let at = line.text.find(text).expect("the text is there");
    line.quads_for(&(at..at + text.len()))[0].corners[2].0
}

#[test]
fn a_line_is_rewritten_from_where_it_began_in_its_own_font() {
    let content = "BT /F1 10 Tf 20 100 Td (Hello World) Tj 0 -20 Td (Next line) Tj ET";
    let bytes = one_page(content, FONT, &[helvetica()]);
    let (_, before) = lines(&bytes);
    assert_eq!(before.len(), 2);
    assert!(before[0].is_mapped());

    let edited = edit(&bytes, &before[0], "Goodbye, World").expect("edits");
    assert!(edited.fallbacks.is_empty(), "Helvetica draws it");
    let after = reread(&edited, FONT, &[helvetica()]);
    assert_eq!(after[0].text, "Goodbye, World");
    assert!((x_of(&after[0], "Goodbye") - 20.0).abs() < 1e-6);
    assert_eq!(after[1].text, "Next line");
    assert!(
        (x_of(&after[1], "Next") - x_of(&before[1], "Next")).abs() < 1e-6,
        "the next line stays"
    );
    assert!((after[1].bounds()[1] - before[1].bounds()[1]).abs() < 1e-6);

    let both = edit_all(
        &bytes,
        &[
            line_edit(&before[0], "First"),
            line_edit(&before[1], "Second"),
        ],
    )
    .expect("edits both");
    let after = reread(&both, FONT, &[helvetica()]);
    let spelled: Vec<_> = after.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(spelled, ["First", "Second"], "one pass, two lines");
}

#[test]
fn part_of_an_operator_is_rewritten_and_the_rest_keeps_its_place() {
    let content = "BT /F1 10 Tf 20 100 Td [(Hello) -250 (World)] TJ ET";
    let bytes = one_page(content, FONT, &[helvetica()]);
    let (_, before) = lines(&bytes);
    let hello: Vec<_> = before[0].glyphs[..5].to_vec();
    let line = TextLine {
        glyphs: hello,
        ..before[0].clone()
    };
    let edited = edit(&bytes, &line, "Hi").expect("edits");
    let after = reread(&edited, FONT, &[helvetica()]);
    assert_eq!(after.len(), 1);
    assert!(after[0].text.starts_with("Hi"), "{:?}", after[0].text);
    assert!(after[0].text.ends_with("World"), "{:?}", after[0].text);
    assert!(
        (x_of(&after[0], "World") - x_of(&before[0], "World")).abs() < 1e-6,
        "World did not move"
    );
}

#[test]
fn a_subset_draws_what_it_drew_and_a_standard_font_the_rest() {
    let content = "BT /F1 12 Tf 30 150 Td (Bold deal) Tj ET";
    let extra = subset();
    let bytes = one_page(content, FONT, &extra);
    let (_, before) = lines(&bytes);

    let same = edit(&bytes, &before[0], "old lead").expect("edits");
    assert!(
        same.fallbacks.is_empty(),
        "every character was drawn already"
    );
    let after = reread(&same, FONT, &extra);
    assert_eq!(after[0].text, "old lead");

    let other = edit(&bytes, &before[0], "New deal").expect("edits");
    assert_eq!(
        other.fallbacks,
        [(onionskin_cos::Name::new("OSFTimes-Bold"), "Times-Bold")],
        "Garamond-Bold's nearest"
    );
    let written = String::from_utf8(other.content.bytes.clone()).expect("ascii");
    assert!(written.contains("/OSFTimes-Bold 12 Tf"), "{written}");
    assert!(written.contains("/F1 12 Tf"), "the line's font again after");
    let resources = "<< /Font << /F1 5 0 R /OSFTimes-Bold 8 0 R >> >>";
    let mut fonts = extra.clone();
    fonts.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    let after = reread(&other, resources, &fonts);
    assert_eq!(after[0].text, "New deal");
    assert!((x_of(&after[0], "New") - 30.0).abs() < 1e-6);
}

#[test]
fn text_no_font_can_draw_is_refused_and_a_missing_line_too() {
    let content = "BT /F1 10 Tf 20 100 Td (Hello) Tj ET";
    let bytes = one_page(content, FONT, &[helvetica()]);
    let (_, before) = lines(&bytes);
    let refused = edit(&bytes, &before[0], "日本").expect_err("refused");
    assert!(matches!(&refused, EditError::Undrawable(chars) if chars == "日本"));
    assert!(refused.to_string().contains("日本"));

    let empty = TextLine {
        glyphs: Vec::new(),
        ..before[0].clone()
    };
    assert!(matches!(
        edit(&bytes, &empty, "x"),
        Err(EditError::NotFound)
    ));
    let mut gone = before[0].clone();
    gone.glyphs[0].at = (9, 0);
    let missing = edit(&bytes, &gone, "x").expect_err("not there");
    assert!(missing.to_string().contains("no longer"));
}

#[test]
fn lines_join_runs_along_a_baseline_and_part_at_columns_and_new_lines() {
    let content = "BT /F1 10 Tf 20 100 Td (One) Tj 25 0 Td (two) Tj 150 0 Td (Far) Tj \
                   -175 -20 Td (Below) Tj ET";
    let bytes = one_page(content, FONT, &[helvetica()]);
    let (text, found) = lines(&bytes);
    let spelled: Vec<_> = found.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(spelled, ["One two", "Far", "Below"]);
    assert_eq!(
        found[0].glyphs[3].at,
        (1, 0),
        "the second run's first glyph"
    );
    assert_eq!(found[0].glyphs[3].range, Some(4..5), "after the space");
    let [x0, _, x1, _] = found[0].bounds();
    assert!((x0 - 20.0).abs() < 1e-6 && x1 > 40.0);
    assert!(found[0].contains(25.0, 102.0));
    assert!(!found[0].contains(25.0, 50.0));
    assert_eq!(text.runs.len(), 4);
}

#[test]
fn a_line_after_an_edited_one_is_edited_where_it_is() {
    // The first edit leaves an operator that only moves the pen back; it
    // draws no run, so the next line's run must not be taken for it.
    let content = "BT /F1 10 Tf 20 100 Td (Hello World) Tj 0 -20 Td (Next line) Tj ET";
    let bytes = one_page(content, FONT, &[helvetica()]);
    let (_, before) = lines(&bytes);
    let first = edit(&bytes, &before[0], "Hi").expect("edits");
    let edited = String::from_utf8(first.content.bytes.clone()).expect("ascii");
    let bytes = one_page(&edited, FONT, &[helvetica()]);
    let (_, middle) = lines(&bytes);
    let second = edit(&bytes, &middle[1], "Last line").expect("edits");
    let after = reread(&second, FONT, &[helvetica()]);
    let spelled: Vec<_> = after.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(spelled, ["Hi", "Last line"]);
    assert!((after[1].bounds()[1] - before[1].bounds()[1]).abs() < 1e-6);
}
