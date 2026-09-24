//! Check Spelling on a real window: word by word through a document's
//! comment and text field, with Ignore, Ignore All, Add to Dictionary and
//! Change, and the dictionary kept in the data folder.

use super::*;
use crate::shell::chrome::spelling_dialog::SpellingAction;
use crate::shell::A11yElement;

/// A page with a text field reading "Jhon Smith recieved", and a note
/// reading "Teh meeting is at noon".
fn spelling_window(data: &Path, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R /Annots [5 0 R 4 0 R] >>",
        "<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] /P 3 0 R /Contents (Teh meeting is at noon) >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /V (Jhon Smith recieved) /Rect [10 700 200 720] /P 3 0 R >>",
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    let path = data.join("form.pdf");
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
    bound_window_with_models(
        vec![(path, model)],
        crate::config::ConfigPaths::in_dir(data),
        cx,
    )
    .0
}

fn shown(frame: &ShellFrame) -> Option<String> {
    let state = frame.spelling_dialog()?;
    state.current().map(|(_, word)| word.word.clone())
}

fn change_to(frame: &ShellFrame, cx: &App) -> String {
    frame
        .spelling_dialog()
        .expect("open")
        .change_to
        .read(cx)
        .query()
        .to_owned()
}

fn has_label(node: &A11yElement, label: &str) -> bool {
    node.label == label || node.children.iter().any(|child| has_label(child, label))
}

#[gpui::test]
fn check_spelling_walks_the_words_and_changes_one(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = spelling_window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::CheckSpelling, window, cx)
                .expect("live");
            assert_eq!(frame.dialog, Some(ShellDialog::Spelling));
            assert_eq!(shown(frame).as_deref(), Some("Teh"), "the comment first");
            assert_eq!(change_to(frame, cx), "The", "the likeliest");
            let context = frame.spelling_dialog().and_then(|state| state.context());
            assert_eq!(
                context.as_deref(),
                Some("Comment on page 1: «Teh» meeting is at noon")
            );
            let described = frame.accessible(window, cx);
            assert!(has_label(&described, "Change To"));
            assert!(has_label(&described, "Add to Dictionary"));

            frame.run_activation(Activation::Spelling(SpellingAction::IgnoreAll), window, cx);
            assert_eq!(shown(frame).as_deref(), Some("Jhon"), "on to the field");
            frame.run_activation(Activation::Spelling(SpellingAction::Ignore), window, cx);
            assert_eq!(shown(frame).as_deref(), Some("recieved"));
            let suggestions = frame.spelling_dialog().expect("open").suggestions.clone();
            let received = suggestions
                .iter()
                .position(|word| word == "received")
                .expect("suggested");
            frame.run_activation(
                Activation::Spelling(SpellingAction::Suggestion(received)),
                window,
                cx,
            );
            assert_eq!(change_to(frame, cx), "received");
            frame.run_activation(Activation::Spelling(SpellingAction::Change), window, cx);
            assert_eq!(shown(frame), None, "nothing after it");
            let state = frame.spelling_dialog().expect("open");
            assert_eq!(
                state.done_label(),
                "Check Spelling is done: 1 word changed."
            );
            assert!(has_label(
                &frame.accessible(window, cx),
                "Check Spelling is done: 1 word changed."
            ));

            let canvas = frame.active_canvas().expect("a tab").clone();
            let value = canvas
                .read(cx)
                .model
                .document_mut()
                .form()
                .expect("reads")
                .field("Name")
                .expect("the field")
                .value
                .as_text();
            assert_eq!(value, "Jhon Smith received");
        })
        .unwrap();
}

#[gpui::test]
fn added_words_are_kept_for_the_next_check(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let window = spelling_window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::CheckSpelling, window, cx)
                .expect("live");
            frame.run_activation(Activation::Spelling(SpellingAction::Ignore), window, cx);
            assert_eq!(shown(frame).as_deref(), Some("Jhon"));
            frame.run_activation(
                Activation::Spelling(SpellingAction::AddToDictionary),
                window,
                cx,
            );
            assert_eq!(shown(frame).as_deref(), Some("recieved"));
            frame.close_dialog(window, cx);
            assert!(frame.spelling_dialog().is_none());

            frame
                .run_main_menu_command(MenuCommand::CheckSpelling, window, cx)
                .expect("live");
            assert_eq!(shown(frame).as_deref(), Some("Teh"));
            frame.run_activation(Activation::Spelling(SpellingAction::Ignore), window, cx);
            assert_eq!(
                shown(frame).as_deref(),
                Some("recieved"),
                "Jhon is known now"
            );
        })
        .unwrap();
    let saved = std::fs::read_to_string(data.path().join("data").join("dictionary.txt"))
        .or_else(|_| std::fs::read_to_string(data.path().join("dictionary.txt")))
        .expect("kept");
    assert_eq!(saved, "Jhon\n");
}
