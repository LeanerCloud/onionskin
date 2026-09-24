//! The signature dialog's body: one list of rows that both the
//! accessibility tree and the drawing read.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    canvas as gpui_canvas, div, fill, point, px, size, Bounds, Context, InteractiveElement as _,
    IntoElement as _, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement as _, Pixels,
    Point, StatefulInteractiveElement as _, Styled as _, Window,
};

use super::{
    Method, SignatureAction, SignatureDialogState, SignatureField, SignatureForm, PAD_HEIGHT,
    PAD_WIDTH,
};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::{ShellFrame, ThemeTokens};

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Field(SignatureField),
    Pad(String),
    Control {
        id: gpui::ElementId,
        role: Role,
        label: String,
        checked: Option<bool>,
        action: SignatureAction,
    },
    Line(gpui::ElementId, String, Role),
}

fn button(id: &'static str, label: String, action: SignatureAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::Button,
        label,
        checked: None,
        action,
    }
}

/// What the pad tells a screen reader: that it is there, and what is on it.
fn pad_label(form: &SignatureForm) -> String {
    match form.strokes.len() {
        0 => "Drawing pad, empty: draw with the pointer".to_owned(),
        1 => "Drawing pad, 1 stroke".to_owned(),
        count => format!("Drawing pad, {count} strokes"),
    }
}

fn rows(form: &SignatureForm, error: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = Method::ALL
        .into_iter()
        .enumerate()
        .map(|(index, method)| Row::Control {
            id: ("signature-method", index).into(),
            role: Role::RadioButton,
            label: method.label().to_owned(),
            checked: Some(form.method == method),
            action: SignatureAction::SetMethod(method),
        })
        .collect();
    rows.extend(form.fields().into_iter().map(Row::Field));
    match form.method {
        Method::Type => {}
        Method::Draw => {
            rows.push(Row::Pad(pad_label(form)));
            rows.push(button(
                "signature-clear-pad",
                "Clear".to_owned(),
                SignatureAction::ClearPad,
            ));
        }
        Method::Image => {
            let chosen = form
                .image
                .as_ref()
                .map_or("none chosen".to_owned(), |file| file.display().to_string());
            rows.push(button(
                "signature-image",
                format!("Choose Image… ({chosen})"),
                SignatureAction::ChooseImage,
            ));
        }
    }
    rows.push(button(
        "signature-save",
        "Save".to_owned(),
        SignatureAction::Save,
    ));
    if form.saved {
        rows.push(button(
            "signature-clear-saved",
            format!("Clear Saved {}", form.kind.label()),
            SignatureAction::ClearSaved,
        ));
    }
    if let Some(error) = error {
        rows.push(Row::Line(
            "signature-error".into(),
            error.to_owned(),
            Role::Alert,
        ));
    }
    rows
}

fn state_rows(state: &SignatureDialogState) -> Vec<Row> {
    rows(&state.form, state.error.as_deref())
}

pub(in crate::shell) fn accessible(state: &SignatureDialogState, cx: &gpui::App) -> Vec<Element> {
    state_rows(state)
        .into_iter()
        .filter_map(|row| match row {
            Row::Field(field) => state.text_field(field).map(|input| {
                input
                    .read(cx)
                    .accessible(field.label(), TextField::Signature(field))
            }),
            Row::Pad(label) => Some(Element::new("signature-pad", Role::Canvas, label)),
            Row::Control {
                id,
                role,
                label,
                checked,
                action,
            } => {
                let mut element =
                    Element::new(id, role, label).with_activation(Activation::Signature(action));
                if let Some(checked) = checked {
                    element = element.with_state(A11yState::toggled(checked));
                }
                Some(element)
            }
            Row::Line(id, line, role) => Some(Element::new(id, role, line)),
        })
        .collect()
}

/// A pointer event on the pad, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) enum PadEvent {
    Down(Point<Pixels>),
    Move(Point<Pixels>),
    Up,
}

/// Paint the strokes as runs of small squares a pixel apart, which is a
/// line at the pad's scale without the path a stroke outline would need.
fn paint_strokes(origin: Point<Pixels>, strokes: &[Vec<(f64, f64)>], window: &mut Window) {
    let ink = gpui::black();
    let dot = |window: &mut Window, (x, y): (f64, f64)| {
        let at = point(origin.x + px(x as f32 - 1.0), origin.y + px(y as f32 - 1.0));
        window.paint_quad(fill(Bounds::new(at, size(px(2.0), px(2.0))), ink));
    };
    for stroke in strokes {
        dot(window, stroke[0]);
        for pair in stroke.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as usize;
            for step in 1..=steps {
                let t = step as f64 / steps as f64;
                dot(window, (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t));
            }
        }
    }
}

fn pad(
    state: &SignatureDialogState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let strokes = state.form.strokes.clone();
    let origin = state.pad_origin.clone();
    div()
        .id("signature-pad")
        .w(px(PAD_WIDTH))
        .h(px(PAD_HEIGHT))
        .bg(gpui::white())
        .border_1()
        .border_color(theme.selected)
        .cursor_crosshair()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|frame, event: &MouseDownEvent, _, cx| {
                frame.signature_pad(PadEvent::Down(event.position), cx);
            }),
        )
        .on_mouse_move(cx.listener(|frame, event: &MouseMoveEvent, _, cx| {
            frame.signature_pad(PadEvent::Move(event.position), cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|frame, _, _, cx| frame.signature_pad(PadEvent::Up, cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|frame, _, _, cx| frame.signature_pad(PadEvent::Up, cx)),
        )
        .child(
            gpui_canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    origin.set(Some(bounds.origin));
                    paint_strokes(bounds.origin, &strokes, window);
                },
            )
            .size_full(),
        )
        .into_any_element()
}

pub(in crate::shell) fn render(
    state: &SignatureDialogState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
    for row in state_rows(state) {
        list = list.child(match row {
            Row::Field(field) => {
                let input = state.text_field(field).expect("the form's field").clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(110.0)).child(field.label()))
                    .child(
                        div()
                            .flex_1()
                            .p_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme.selected)
                            .child(input),
                    )
                    .into_any_element()
            }
            Row::Pad(_) => pad(state, theme, cx),
            Row::Control {
                id,
                label,
                checked,
                action,
                ..
            } => {
                let is_focused = focused == Some(&id);
                div()
                    .id(id)
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(is_focused, |row| row.bg(theme.selected))
                    .hover(move |row| row.bg(theme.subtle_hover))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(Activation::Signature(action), window, cx);
                    }))
                    .child(match checked {
                        Some(true) => format!("✓ {label}"),
                        Some(false) => format!("○ {label}"),
                        None => label,
                    })
                    .into_any_element()
            }
            Row::Line(id, line, _) => div()
                .id(id)
                .text_color(theme.error_text)
                .child(line)
                .into_any_element(),
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_tools_fill_sign::signature::SignatureKind;

    fn labels(form: &SignatureForm) -> Vec<String> {
        rows(form, Some("oops"))
            .iter()
            .map(|row| match row {
                Row::Field(field) => format!("[{}]", field.label()),
                Row::Pad(label) | Row::Control { label, .. } | Row::Line(_, label, _) => {
                    label.clone()
                }
            })
            .collect()
    }

    #[test]
    fn typing_is_offered_first_and_nothing_saved_offers_no_clear() {
        let form = SignatureForm::new(SignatureKind::Signature, false);
        assert_eq!(
            labels(&form),
            ["Type", "Draw", "Image", "[Name]", "Save", "oops"]
        );
    }

    #[test]
    fn drawing_shows_the_pad_and_an_image_its_file() {
        let mut form = SignatureForm::new(SignatureKind::Initials, true);
        form.apply(SignatureAction::SetMethod(Method::Draw));
        assert_eq!(
            labels(&form),
            [
                "Type",
                "Draw",
                "Image",
                "Drawing pad, empty: draw with the pointer",
                "Clear",
                "Save",
                "Clear Saved Initials",
                "oops"
            ]
        );
        form.pad_down((1.0, 1.0));
        assert!(labels(&form).contains(&"Drawing pad, 1 stroke".to_owned()));
        form.pad_up();
        form.pad_down((2.0, 2.0));
        assert!(labels(&form).contains(&"Drawing pad, 2 strokes".to_owned()));

        form.apply(SignatureAction::SetMethod(Method::Image));
        assert!(labels(&form).contains(&"Choose Image… (none chosen)".to_owned()));
        form.image = Some("/a/sig.png".into());
        assert!(labels(&form).contains(&"Choose Image… (/a/sig.png)".to_owned()));
    }
}
