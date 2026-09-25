//! Redaction on a real window: marking through the Edit menu's dialogs, the
//! Redact tool's click and the canvas menu, Redaction Properties and its
//! code sets, and Apply Redactions writing and opening a verified copy.
//!
//! The file pickers are the platform's, which the test platform does not
//! implement, so the tests start from the path a picker would hand back.

use onionskin_core::redactions::RedactionLook;

use super::*;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::redact_dialog::{Panel, RedactAction, RedactField};
use crate::shell::chrome::tabs::RedactCommand;
use crate::shell::context_menu::CanvasContextCommand;
use crate::shell::dialog::ShellDialog;

/// Numbered object bodies as a classic-xref PDF.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// Two pages; the first says "Call 555-123-4567 about Secret plans".
fn document() -> Vec<u8> {
    let content = "BT /F1 10 Tf 10 50 Td (Call 555-123-4567 about Secret plans) Tj ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 100] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 6 0 R >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

/// A window on the document, its tools keeping their files under `data`.
fn window(
    data: &std::path::Path,
    cx: &mut TestAppContext,
) -> (gpui::WindowHandle<ShellFrame>, std::path::PathBuf) {
    let path = data.join("plans.pdf");
    std::fs::write(&path, document()).expect("writes");
    let (window, _) = bound_window_in(&[], crate::config::ConfigPaths::in_dir(data), cx);
    window
        .update(cx, |frame, _, cx| {
            frame.open_documents(std::slice::from_ref(&path), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, cx| {
            let environment = frame.settings.tool_environment();
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| canvas.model.configure_tools(&environment));
            frame.run_view_action(crate::shell::canvas::ViewAction::GoToPage(0), cx);
        })
        .unwrap();
    cx.run_until_parked();
    (window, path)
}

fn command(
    window: gpui::WindowHandle<ShellFrame>,
    command: RedactCommand,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Redact(command), window, cx)
                .expect("live");
        })
        .unwrap();
    cx.run_until_parked();
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: RedactAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Redact(action), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
}

fn type_in(
    window: gpui::WindowHandle<ShellFrame>,
    field: RedactField,
    text: &str,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |frame, _, cx| {
            let state = frame.redact_dialog().expect("open");
            let input = state
                .inputs
                .iter()
                .find(|(each, _)| *each == field)
                .map(|(_, input)| input.clone())
                .expect("the field");
            input.update(cx, |input, cx| input.set_query(text.to_owned(), cx));
        })
        .unwrap();
}

fn error(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<String> {
    window
        .update(cx, |frame, _, _| {
            frame.redact_dialog().and_then(|state| state.error.clone())
        })
        .unwrap()
}

fn dialog(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<ShellDialog> {
    window.update(cx, |frame, _, _| frame.dialog).unwrap()
}

fn marks(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> Vec<onionskin_core::redactions::RedactionMark> {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().redactions().expect("reads")
            })
        })
        .unwrap()
}

fn last_notice(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> String {
    window
        .update(cx, |frame, _, _| {
            frame.notices.last().cloned().unwrap_or_default()
        })
        .unwrap()
}

fn text_of(bytes: Vec<u8>) -> String {
    let mut doc = onionskin_core::Document::open_bytes(bytes).expect("opens");
    doc.page_text(0).expect("extracts").flatten().text
}

#[gpui::test]
fn find_text_and_redact_marks_what_it_found_and_apply_writes_a_clean_copy(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = window(data.path(), cx);
    command(window, RedactCommand::Find, cx);
    assert_eq!(dialog(window, cx), Some(ShellDialog::Redact(Panel::Find)));
    act(window, RedactAction::Find, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Type the words or phrase to find.")
    );

    type_in(window, RedactField::FindText, "Secret", cx);
    act(window, RedactAction::Find, cx);
    act(window, RedactAction::Toggle(0), cx);
    act(window, RedactAction::MarkChecked, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Check at least one result to mark.")
    );
    act(window, RedactAction::Toggle(0), cx);
    act(window, RedactAction::Patterns(true), cx);
    act(window, RedactAction::Find, cx);
    window
        .update(cx, |frame, window, cx| {
            let found = &frame.redact_dialog().expect("open").find.found;
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].text, "555-123-4567");
            let tree = frame.accessible(window, cx);
            assert!(
                tree.find(&("redact-hit", 0usize).into()).is_some(),
                "each hit is listed"
            );
        })
        .unwrap();
    act(window, RedactAction::MarkChecked, cx);
    assert_eq!(dialog(window, cx), None);
    assert_eq!(marks(window, cx).len(), 1);
    assert!(last_notice(window, cx).starts_with("Marked 1 area"));

    command(window, RedactCommand::Apply, cx);
    assert_eq!(
        dialog(window, cx),
        Some(ShellDialog::Redact(Panel::Apply { sanitize: false }))
    );
    let copy = data.path().join("plans_Redacted.pdf");
    window
        .update(cx, |frame, _, cx| {
            let origin = frame.active_canvas().expect("a tab").entity_id();
            frame.redact_into(origin, &copy, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(dialog(window, cx), None);
    let notice = window
        .update(cx, |frame, _, _| {
            frame
                .notices
                .iter()
                .find(|n| n.starts_with("Saved"))
                .cloned()
        })
        .unwrap()
        .expect("a notice");
    assert!(
        notice.contains("12 characters removed on 1 page"),
        "{notice}"
    );
    let text = text_of(std::fs::read(&copy).expect("written"));
    assert!(
        text.contains("Call") && text.contains("Secret") && !text.contains("4567"),
        "{text}"
    );
    let tabs = window
        .update(cx, |frame, _, _| frame.tabs.tabs().len())
        .unwrap();
    assert_eq!(tabs, 2, "the redacted copy opens in a tab of its own");
}

#[gpui::test]
fn properties_set_the_look_of_new_marks_and_of_a_clicked_mark(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = window(data.path(), cx);
    command(window, RedactCommand::Properties, cx);
    act(window, RedactAction::NextFill, cx);
    act(window, RedactAction::Overlay, cx);
    act(window, RedactAction::Save, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Type the overlay text, or turn the overlay off.")
    );
    act(window, RedactAction::UseCode, cx);
    act(window, RedactAction::Save, cx);
    assert_eq!(dialog(window, cx), None);
    let default = window
        .update(cx, |frame, _, _| {
            frame.settings.preferences.redaction.clone()
        })
        .unwrap()
        .expect("a default");
    assert_eq!(default.fill, Some([255, 255, 255]));
    assert_eq!(
        default.overlay.map(|overlay| overlay.text),
        Some("(b)(1)".to_owned())
    );

    command(window, RedactCommand::MarkPages, cx);
    type_in(window, RedactField::Pages, "3", cx);
    act(window, RedactAction::MarkPages, cx);
    assert!(error(window, cx).expect("an error").contains("no page 3"));
    type_in(window, RedactField::Pages, "1-2", cx);
    act(window, RedactAction::MarkPages, cx);
    let marked = marks(window, cx);
    assert_eq!(marked.len(), 2);
    assert_eq!(
        marked[0].look.fill,
        Some([1.0, 1.0, 1.0]),
        "the default look"
    );

    // The Redact tool's click on a mark opens its properties.
    let mark = marked[0].objref;
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .request_redaction_properties(mark)
            });
            frame.collect_redaction_request(cx);
            frame.run_pending_redaction(window, cx);
            assert_eq!(
                frame.dialog,
                Some(ShellDialog::Redact(Panel::Properties {
                    mark: Some((mark, 0))
                }))
            );
        })
        .unwrap();
    act(window, RedactAction::Overlay, cx);
    act(window, RedactAction::NextFill, cx);
    act(window, RedactAction::Save, cx);
    let restyled = marks(window, cx);
    assert_eq!(restyled[0].look.fill, Some([1.0, 0.0, 0.0]));
    assert_eq!(restyled[0].look.overlay, None);

    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas
                    .model
                    .document_mut()
                    .request_redaction_properties(mark)
            });
            frame.collect_redaction_request(cx);
            frame.run_pending_redaction(window, cx);
        })
        .unwrap();
    act(window, RedactAction::RemoveMark, cx);
    assert_eq!(marks(window, cx).len(), 1);
}

#[gpui::test]
fn code_sets_are_saved_used_renamed_exported_imported_and_removed(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = window(data.path(), cx);
    command(window, RedactCommand::Properties, cx);
    type_in(window, RedactField::SetName, "Legal", cx);
    act(window, RedactAction::SaveSet, cx);
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Type at least one code, separated by commas.")
    );
    type_in(window, RedactField::Codes, "Privileged, Work product", cx);
    act(window, RedactAction::SaveSet, cx);
    let current = |cx: &mut TestAppContext| {
        window
            .update(cx, |frame, _, _| {
                let form = &frame.redact_dialog().expect("open").properties;
                (
                    form.current_set().map(|set| set.name.clone()),
                    form.current_code().map(str::to_owned),
                )
            })
            .unwrap()
    };
    assert_eq!(
        current(cx),
        (Some("Legal".to_owned()), Some("Privileged".to_owned()))
    );
    act(window, RedactAction::NextCode, cx);
    act(window, RedactAction::UseCode, cx);
    let typed = window
        .update(cx, |frame, _, cx| {
            frame
                .redact_dialog()
                .expect("open")
                .text(RedactField::OverlayText, cx)
        })
        .unwrap();
    assert_eq!(typed, "Work product");

    type_in(window, RedactField::SetName, "Counsel", cx);
    act(window, RedactAction::RenameSet, cx);
    assert_eq!(current(cx).0.as_deref(), Some("Counsel"));

    let exported = data.path().join("counsel-codes.txt");
    window
        .update(cx, |frame, _, _| {
            let set = frame
                .redact_dialog()
                .and_then(|state| state.properties.current_set().cloned())
                .expect("a set");
            frame.export_code_set(&set, &exported);
            frame.import_code_set(&exported);
        })
        .unwrap();
    assert_eq!(current(cx).0.as_deref(), Some("counsel-codes"));
    window
        .update(cx, |frame, _, _| frame.import_code_set(&exported))
        .unwrap();
    assert!(error(window, cx).expect("an error").contains("already"));

    act(window, RedactAction::RemoveSet, cx);
    let names: Vec<String> = window
        .update(cx, |frame, _, _| {
            frame
                .redact_dialog()
                .expect("open")
                .properties
                .sets
                .iter()
                .map(|set| set.name.clone())
                .collect()
        })
        .unwrap();
    assert_eq!(names, ["U.S. FOIA", "U.S. Privacy Act", "Counsel"]);
    assert!(last_notice(window, cx).starts_with("Exported Counsel"));
}

#[gpui::test]
fn redact_text_marks_the_selection_and_sanitize_writes_a_copy(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = window(data.path(), cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::SelectAll, window, cx)
                .expect("selects");
            frame.run_canvas_context_command(CanvasContextCommand::RedactText, window, cx);
        })
        .unwrap();
    let marked = marks(window, cx);
    assert_eq!(marked.len(), 1);
    assert!(!marked[0].quads.is_empty());
    assert_eq!(marked[0].look, RedactionLook::default());

    command(window, RedactCommand::Sanitize, cx);
    let copy = data.path().join("sanitized.pdf");
    window
        .update(cx, |frame, _, cx| {
            let origin = frame.active_canvas().expect("a tab").entity_id();
            frame.redact_into(origin, &copy, cx);
        })
        .unwrap();
    cx.run_until_parked();
    let notice = last_notice(window, cx);
    assert!(notice.contains("Hidden information removed"), "{notice}");
    assert_eq!(text_of(std::fs::read(&copy).expect("written")), "");
}

#[gpui::test]
fn a_failed_apply_stays_in_the_dialog_and_says_why(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().expect("dir");
    let (window, _) = window(data.path(), cx);
    command(window, RedactCommand::Apply, cx);
    act(window, RedactAction::HiddenInformation, cx);
    act(window, RedactAction::HiddenInformation, cx);
    window
        .update(cx, |frame, _, cx| {
            let origin = frame.active_canvas().expect("a tab").entity_id();
            frame.redact_into(origin, &data.path().join("none.pdf"), cx);
        })
        .unwrap();
    assert_eq!(
        error(window, cx).as_deref(),
        Some("Nothing is marked for redaction")
    );
    assert_eq!(
        dialog(window, cx),
        Some(ShellDialog::Redact(Panel::Apply { sanitize: false }))
    );
}
