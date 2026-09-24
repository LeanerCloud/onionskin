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
