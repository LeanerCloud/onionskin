//! Editing text on a real window: the find bar's Replace and Replace All,
//! and the editor the Edit Text tool opens on a line.

use super::*;
use crate::shell::chrome::SearchInput;
use crate::shell::A11yElement;

/// Page 1 of a document with a heading and a line below it, in Helvetica.
fn text_window(cx: &mut TestAppContext) -> (tempfile::TempDir, gpui::WindowHandle<ShellFrame>) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("draft.pdf");
    let content = "BT /F1 24 Tf 72 700 Td (Draft page 1) Tj 0 -40 Td (The draft is final.) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font \
         /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    std::fs::write(&path, bytes).expect("writes");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("the model builds");
    let window = bound_window_with_models(
        vec![(path, model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0;
    (dir, window)
}

fn lines(frame: &ShellFrame, cx: &App) -> Vec<String> {
    let canvas = frame.tabs.active().expect("a tab").canvas.read(cx);
    let mut document = canvas.model.document_mut();
    onionskin_core::text_edit::page_lines(document.structure().expect("reads"), 0)
        .expect("lines")
        .into_iter()
        .map(|line| line.text)
        .collect()
}

fn has_label(node: &A11yElement, label: &str) -> bool {
    node.label == label || node.children.iter().any(|child| has_label(child, label))
}

fn type_into(input: &Entity<SearchInput>, text: &str, cx: &mut App) {
    input.update(cx, |input, cx| input.set_query(text.to_owned(), cx));
}

#[gpui::test]
fn replace_takes_one_match_and_replace_all_the_rest(cx: &mut TestAppContext) {
    let (_dir, window) = text_window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::ReplaceText { all: false }, window, cx);
            assert!(frame.notices.last().unwrap().starts_with("Type the text"));

            frame.open_find_bar(Some("draft".to_owned()), window, cx);
            assert_eq!(frame.replace_refusal(cx), None);
            let replace = frame.replace_input.clone();
            type_into(&replace, "copy", cx);
            frame.run_activation(Activation::ReplaceText { all: false }, window, cx);
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some("Replaced 1 match")
            );
            let after = lines(frame, cx);
            assert_eq!(after.len(), 2);
            assert!(
                after.iter().filter(|line| line.contains("copy")).count() == 1,
                "{after:?}"
            );

            frame.run_activation(Activation::ReplaceText { all: true }, window, cx);
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some("Replaced 1 match")
            );
            assert_eq!(lines(frame, cx), ["copy page 1", "The copy is final."]);
            frame.run_activation(Activation::ReplaceText { all: true }, window, cx);
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some("No match to replace")
            );

            let described = frame.accessible(window, cx);
            assert!(has_label(&described, "Replace With"));
            assert!(has_label(&described, "Replace All"));
        })
        .unwrap();
}

/// Click the middle of page rectangle `rect` with the Edit Text tool.
fn click_line(window: gpui::WindowHandle<ShellFrame>, rect: [f64; 4], cx: &mut TestAppContext) {
    click_with("edit-text", window, rect, cx);
}

/// Choose tool `tool`, click the middle of page rectangle `rect` with it,
/// and let the canvas answer.
fn click_with(
    tool: &str,
    window: gpui::WindowHandle<ShellFrame>,
    rect: [f64; 4],
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, cx| {
                let index = canvas
                    .model
                    .registry()
                    .tools()
                    .position(|found| found.id() == tool)
                    .expect("installed");
                canvas.model.activate_tool(index).expect("activates");
                let (at, width, height) = canvas.model.view_rect(0, rect).expect("in view");
                let origin = canvas.model.canvas_origin();
                let at = gpui::point(
                    gpui::px(origin.x + at.x + width / 2.0),
                    gpui::px(origin.y + at.y + height / 2.0),
                );
                let modifiers = gpui::Modifiers::default();
                canvas.model.pointer_down(at, 1.0, modifiers).unwrap();
                canvas.model.pointer_up(at, 1.0, modifiers).unwrap();
                canvas.answer_text_edit(window, cx);
            });
        })
        .unwrap();
}

const SECOND_LINE: [f64; 4] = [72.0, 656.0, 240.0, 676.0];

#[gpui::test]
fn the_edit_text_tool_opens_an_editor_on_the_line_and_keeps_what_is_typed(cx: &mut TestAppContext) {
    let (_dir, window) = text_window(cx);
    click_line(window, SECOND_LINE, cx);
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).line_editor_input().expect("open");
            assert_eq!(input.read(cx).query(), "The draft is final.");
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
            assert!(has_label(&frame.accessible(window, cx), "Line of text"));
            assert!(frame.focused_text_field(window, cx).is_some());

            // Escape leaves it as it was.
            canvas.update(cx, |canvas, cx| canvas.cancel_line_editor(cx));
            assert!(canvas.read(cx).line_editor_input().is_none());
            assert_eq!(lines(frame, cx)[1], "The draft is final.");
        })
        .unwrap();

    click_line(window, SECOND_LINE, cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).line_editor_input().expect("open");
            type_into(&input, "The text is final.", cx);
            canvas.update(cx, |canvas, cx| canvas.commit_line_editor(cx));
            assert!(canvas.read(cx).line_editor_input().is_none());
            assert_eq!(lines(frame, cx), ["Draft page 1", "The text is final."]);
            let label = canvas
                .read(cx)
                .model
                .document_mut()
                .edit()
                .history()
                .undo_label()
                .map(str::to_owned);
            assert_eq!(label.as_deref(), Some("Edit Text"));
        })
        .unwrap();

    click_line(window, SECOND_LINE, cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).line_editor_input().expect("open");
            type_into(&input, "日本", cx);
            canvas.update(cx, |canvas, cx| canvas.commit_line_editor(cx));
            let status = canvas
                .read(cx)
                .model
                .status()
                .map(|status| format!("{status:?}"));
            assert!(
                status
                    .as_deref()
                    .is_some_and(|status| status.contains("cannot be written")),
                "{status:?}"
            );
            assert_eq!(lines(frame, cx)[1], "The text is final.");
        })
        .unwrap();
}

#[gpui::test]
fn the_add_text_tool_draws_what_is_typed_where_it_was_clicked(cx: &mut TestAppContext) {
    let (_dir, window) = text_window(cx);
    click_with("add-text", window, [72.0, 390.0, 72.0, 410.0], cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            let input = canvas.read(cx).line_editor_input().expect("open");
            assert_eq!(input.read(cx).query(), "", "empty, for new text");
            type_into(&input, "A new line", cx);
            canvas.update(cx, |canvas, cx| canvas.commit_line_editor(cx));
            assert_eq!(
                lines(frame, cx),
                ["Draft page 1", "The draft is final.", "A new line"]
            );
        })
        .unwrap();
}
