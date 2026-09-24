//! Preparing a form on a real window: a field placed by its tool, its
//! Properties opened by a double click, changed and saved, a bad value
//! kept in the dialog, and the field deleted.

use gpui::{point, px};
use onionskin_core::forms::{FieldKind, FieldValue};

use super::*;
use crate::shell::chrome::accessible::{Activation, TextField};
use crate::shell::chrome::field_dialog::{FieldAction, FieldInput, FormatKind, Shape, Tab};
use crate::shell::dialog::ShellDialog;

/// A window on hello.pdf with the tool `id` chosen.
fn window(tool: &str, cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let (window, _) = bound_window(&["hello.pdf"], cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let index = canvas
                    .model
                    .registry()
                    .tools()
                    .position(|each| each.id() == tool)
                    .expect("installed");
                canvas.model.activate_tool(index).expect("activates");
            });
        })
        .unwrap();
    window
}

/// Press and release at the middle of the first page, `clicks` deep.
fn press(window: gpui::WindowHandle<ShellFrame>, clicks: u8, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                let page = canvas.model.viewport().visible_pages().unwrap()[0].rect;
                let at = point(
                    px(page.origin.x + page.size.width / 2.0),
                    px(page.origin.y + page.size.height / 2.0),
                );
                let modifiers = gpui::Modifiers::default();
                canvas.model.set_click_count(usize::from(clicks));
                canvas.model.pointer_down(at, 1.0, modifiers).unwrap();
                canvas.model.pointer_up(at, 1.0, modifiers).unwrap();
            });
            frame.collect_field_request(cx);
            frame.run_pending_field(window, cx);
        })
        .unwrap();
}

fn form(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> onionskin_core::forms::Form {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().form().expect("reads")
            })
        })
        .unwrap()
}

fn dialog(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<ShellDialog> {
    window.update(cx, |frame, _, _| frame.dialog).unwrap()
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: FieldAction, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(Activation::Field(action), window, cx)
        })
        .unwrap();
}

fn type_in(
    window: gpui::WindowHandle<ShellFrame>,
    which: FieldInput,
    text: &str,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |frame, _, cx| {
            let input = frame
                .text_field(TextField::Field(which))
                .expect("on the tab shown")
                .clone();
            input.update(cx, |input, cx| input.set_query(text.to_owned(), cx));
        })
        .unwrap();
}

fn error(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<String> {
    window
        .update(cx, |frame, _, _| {
            frame.field_dialog().and_then(|dialog| dialog.error.clone())
        })
        .unwrap()
}

#[gpui::test]
fn a_placed_field_s_properties_are_changed_and_saved(cx: &mut TestAppContext) {
    let window = window("form-text", cx);
    press(window, 1, cx);
    assert_eq!(form(window, cx).fields[0].name, "Text1");
    assert_eq!(dialog(window, cx), None, "a single click only places it");
    press(window, 2, cx);
    assert_eq!(
        dialog(window, cx),
        Some(ShellDialog::FieldProperties(Shape::Text))
    );
    let published = window
        .update(cx, |frame, _, cx| {
            crate::shell::chrome::field_dialog::accessible(frame.field_dialog().expect("open"), cx)
                .into_iter()
                .map(|element| element.label)
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert!(published.contains(&"Name".to_owned()), "{published:?}");

    type_in(window, FieldInput::Name, "email", cx);
    act(window, FieldAction::Tab(Tab::Format), cx);
    act(window, FieldAction::SetFormat(FormatKind::Special), cx);
    act(window, FieldAction::Tab(Tab::Position), cx);
    type_in(window, FieldInput::Width, "wide", cx);
    act(window, FieldAction::Submit, cx);
    assert!(error(window, cx).is_some_and(|error| error.contains("must be a number")));
    type_in(window, FieldInput::Width, "200", cx);
    act(window, FieldAction::Submit, cx);
    assert_eq!(dialog(window, cx), None);
    let after = form(window, cx);
    let email = after.field("email").expect("renamed");
    assert_eq!(
        email.scripts.format.as_deref(),
        Some("AFSpecial_Format(0);")
    );
    let [x0, _, x1, _] = email.widgets[0].rect;
    assert_eq!(x1 - x0, 200.0);
}

#[gpui::test]
fn a_refused_name_stays_in_the_dialog_and_delete_takes_the_field_away(cx: &mut TestAppContext) {
    let window = window("form-check-box", cx);
    press(window, 1, cx);
    press(window, 2, cx);
    assert_eq!(
        dialog(window, cx),
        Some(ShellDialog::FieldProperties(Shape::CheckBox))
    );
    type_in(window, FieldInput::Name, "a.b", cx);
    act(window, FieldAction::Submit, cx);
    assert!(error(window, cx).is_some_and(|error| error.contains("full stop")));
    act(window, FieldAction::Delete, cx);
    assert_eq!(dialog(window, cx), None);
    assert!(form(window, cx).fields.is_empty());
}

#[gpui::test]
fn a_field_that_went_away_says_so(cx: &mut TestAppContext) {
    let window = window("form-list-box", cx);
    press(window, 1, cx);
    let field = form(window, cx).fields[0].clone();
    assert!(matches!(field.kind, FieldKind::Choice { combo: false, .. }));
    assert_eq!(field.value, FieldValue::None);
    window
        .update(cx, |frame, window, cx| {
            frame.open_field_dialog(
                onionskin_core::FieldRequest {
                    field: onionskin_core::ObjRef::new(999, 0),
                    widget: field.widgets[0].objref,
                    page: 0,
                    point: (0.0, 0.0),
                },
                window,
                cx,
            );
            assert!(frame.dialog.is_none());
            assert!(frame
                .notices
                .last()
                .is_some_and(|notice| notice.contains("not in the form")));
        })
        .unwrap();
}
