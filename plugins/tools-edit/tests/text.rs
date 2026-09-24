//! Editing text through the plugin: the Edit Text tool asking for a line,
//! the line rewritten as one undo step, a line changed since it was chosen
//! refused, and find and replace across pages.

mod common;

use common::{at, content, pdf, Page};
use onionskin_core::text_edit::MatchOptions;
use onionskin_core::Document;
use onionskin_plugin_api::{CommandError, ToolCapability, ToolPlugin};
use onionskin_tools_edit::text::{add_text, edit_line, find, line_at, match_at, replace};
use onionskin_tools_edit::{AddTextTool, EditTextTool};

const FONT: &[u8] = b"<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>";

/// Two Letter pages, each with a heading at (72, 700) and a line below.
fn document() -> Document {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] /Resources 5 0 R >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>".to_vec(),
        FONT.to_vec(),
    ];
    for page in 1..=2 {
        objects.push(content(&format!(
            "BT /F1 24 Tf 72 700 Td (Draft page {page}) Tj 0 -40 Td (The draft is final.) Tj ET"
        )));
    }
    Document::open_bytes(pdf(&objects)).expect("opens")
}

fn lines(doc: &mut Document, page: usize) -> Vec<String> {
    onionskin_core::text_edit::page_lines(doc.structure().expect("reads"), page)
        .expect("lines")
        .into_iter()
        .map(|line| line.text)
        .collect()
}

#[test]
fn a_click_asks_for_the_line_under_it_and_a_drag_does_not() {
    let mut page = Page::new(document());
    let mut tool = EditTextTool::new();
    assert_eq!(tool.capabilities(), [ToolCapability::EditText]);
    assert_eq!(
        (tool.id(), tool.name(), tool.group()),
        ("edit-text", "Edit Text", "edit-text")
    );
    assert!(tool.hint().is_some());

    page.drag(&mut tool, (100.0, 708.0), (100.0, 708.0));
    let request = page.doc.take_text_edit_request().expect("asked");
    assert_eq!(
        (request.page, request.line, request.text.as_str()),
        (0, Some(0), "Draft page 1")
    );
    assert_eq!(tool.overlays(&page.doc).len(), 1, "the line outlined");

    page.drag(&mut tool, (100.0, 708.0), (300.0, 500.0));
    assert!(page.doc.take_text_edit_request().is_none(), "a drag");
    page.drag(&mut tool, (400.0, 300.0), (400.0, 300.0));
    assert!(page.doc.take_text_edit_request().is_none(), "no text there");
    assert!(tool.overlays(&page.doc).is_empty());

    tool.on_pointer_down(&mut page.ctx(), at((100.0, 708.0)));
    tool.on_cancel(&mut page.ctx());
    tool.on_pointer_up(&mut page.ctx(), at((100.0, 708.0)));
    assert!(page.doc.take_text_edit_request().is_none(), "Escape");
    page.drag(&mut tool, (100.0, 708.0), (100.0, 708.0));
    tool.on_deactivate(&mut page.ctx());
    assert!(tool.overlays(&page.doc).is_empty());
}

#[test]
fn a_line_is_rewritten_as_one_undo_step() {
    let mut doc = document();
    let (line, found) = line_at(&mut doc, 0, (100.0, 668.0)).expect("a line");
    assert_eq!((line, found.text.as_str()), (1, "The draft is final."));
    edit_line(&mut doc, 0, line, &found.text, "The text is final.").expect("edits");
    assert_eq!(lines(&mut doc, 0), ["Draft page 1", "The text is final."]);
    assert_eq!(doc.edit().history().undo_label(), Some("Edit Text"));

    edit_line(&mut doc, 0, 1, "The text is final.", "The text is final.").expect("nothing to do");
    assert_eq!(doc.edit().history().undo_label(), Some("Edit Text"));

    let stale = edit_line(&mut doc, 0, 1, "The draft is final.", "x").expect_err("changed");
    assert!(stale.to_string().contains("changed"), "{stale}");
    let undrawable = edit_line(&mut doc, 0, 0, "Draft page 1", "日本").expect_err("refused");
    assert!(
        matches!(
            undrawable,
            CommandError::Edit {
                label: "Edit Text",
                ..
            }
        ),
        "{undrawable:?}"
    );

    assert!(doc.undo().expect("undoes"));
    assert_eq!(lines(&mut doc, 0), ["Draft page 1", "The draft is final."]);
}

#[test]
fn every_match_is_replaced_in_one_step_and_one_on_its_own() {
    let mut doc = document();
    let options = MatchOptions {
        case_sensitive: false,
        whole_word: true,
    };
    let found = find(&mut doc, "draft", options).expect("finds");
    assert_eq!(found.len(), 4, "two a page, either case");

    let second = match_at(&mut doc, &found, 0, (150.0, 668.0)).expect("on screen");
    assert_eq!(second, found[1], "the one in the second line");
    assert!(match_at(&mut doc, &found, 0, (500.0, 100.0)).is_none());
    assert!(match_at(&mut doc, &found, 1, (150.0, 668.0)).is_some_and(|hit| hit.page == 1));
    let one = replace(&mut doc, &[second], "copy", "Replace").expect("replaces one");
    assert_eq!(one, 1);
    assert_eq!(lines(&mut doc, 0), ["Draft page 1", "The copy is final."]);
    assert_eq!(doc.edit().history().undo_label(), Some("Replace"));

    let rest = find(&mut doc, "draft", options).expect("finds");
    assert_eq!(rest.len(), 3);
    let all = replace(&mut doc, &rest, "Proof", "Replace All").expect("replaces");
    assert_eq!(all, 3);
    assert_eq!(lines(&mut doc, 0), ["Proof page 1", "The copy is final."]);
    assert_eq!(lines(&mut doc, 1), ["Proof page 2", "The Proof is final."]);
    assert_eq!(doc.edit().history().undo_label(), Some("Replace All"));
    assert_eq!(
        replace(&mut doc, &[], "x", "Replace All").expect("nothing"),
        0
    );
    assert!(find(&mut doc, "", options).expect("finds").is_empty());
}

#[test]
fn the_add_text_tool_asks_for_new_text_where_it_is_clicked() {
    let mut page = Page::new(document());
    let mut tool = AddTextTool::new();
    assert_eq!(
        (tool.id(), tool.name(), tool.group(), tool.capabilities()),
        (
            "add-text",
            "Add Text",
            "edit-text",
            &[ToolCapability::EditText][..]
        )
    );
    assert!(tool.hint().is_some() && tool.overlays(&page.doc).is_empty());
    page.drag(&mut tool, (100.0, 400.0), (100.0, 400.0));
    let request = page.doc.take_text_edit_request().expect("asked");
    assert_eq!((request.line, request.text.as_str()), (None, ""));
    assert!((request.bounds[0] - 100.0).abs() < 1e-6 && (request.bounds[1] - 400.0).abs() < 1e-6);
    page.drag(&mut tool, (100.0, 400.0), (300.0, 300.0));
    assert!(page.doc.take_text_edit_request().is_none(), "a drag");
    tool.on_pointer_down(&mut page.ctx(), at((100.0, 400.0)));
    tool.on_cancel(&mut page.ctx());
    tool.on_pointer_up(&mut page.ctx(), at((100.0, 400.0)));
    assert!(page.doc.take_text_edit_request().is_none(), "Escape");

    let doc = &mut page.doc;
    add_text(doc, 0, (100.0, 400.0), "A new line").expect("adds");
    assert_eq!(
        lines(doc, 0),
        ["Draft page 1", "The draft is final.", "A new line"]
    );
    assert_eq!(doc.edit().history().undo_label(), Some("Add Text"));
    add_text(doc, 0, (100.0, 300.0), "  ").expect("nothing typed");
    assert_eq!(lines(doc, 0).len(), 3);
    let refused = add_text(doc, 0, (100.0, 300.0), "日本").expect_err("refused");
    assert!(matches!(
        refused,
        CommandError::Edit {
            label: "Add Text",
            ..
        }
    ));
}

#[test]
fn a_line_and_new_text_take_a_style() {
    use onionskin_tools_edit::text::{add_styled_text, edit_styled_line, TextStyle};
    let mut doc = document();
    let bold = TextStyle {
        face: Some("Helvetica-Bold"),
        size: Some(30.0),
        fill: Some([0.8, 0.0, 0.0]),
    };
    edit_styled_line(&mut doc, 0, 0, "Draft page 1", "Draft page 1", bold)
        .expect("the same words, restyled");
    assert_eq!(doc.edit().history().undo_label(), Some("Edit Text"));
    let lines = onionskin_core::text_edit::page_lines(doc.structure().expect("reads"), 0)
        .expect("lines");
    let quad = lines[0].glyphs[0].quad.corners;
    assert!((quad[0].1 - quad[2].1).abs() > 24.0, "set at 30 pt");
    add_styled_text(&mut doc, 0, (72.0, 300.0), "Signed", bold).expect("adds");
    assert_eq!(
        lines_of(&mut doc)[2],
        "Signed",
        "the new line is there in its own style"
    );
}

fn lines_of(doc: &mut Document) -> Vec<String> {
    lines(doc, 0)
}
