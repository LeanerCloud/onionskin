//! The link dialog's body: one list of rows that both the accessibility
//! tree and the drawing read.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::{color_name, LinkAction, LinkDialogState, LinkField, LinkForm, LinkMode, TargetKind};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::{ShellFrame, ThemeTokens};

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Field(LinkField),
    Control {
        id: gpui::ElementId,
        role: Role,
        label: String,
        checked: Option<bool>,
        action: LinkAction,
    },
    Line(gpui::ElementId, String, Role),
}

fn button(id: &'static str, label: String, action: LinkAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::Button,
        label,
        checked: None,
        action,
    }
}

fn rows(form: &LinkForm, editing: bool, error: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = form
        .kinds()
        .into_iter()
        .enumerate()
        .map(|(index, kind)| Row::Control {
            id: ("link-kind", index).into(),
            role: Role::RadioButton,
            label: match (kind, &form.kept) {
                (TargetKind::Keep, Some(action)) => format!("Keep its {action} action"),
                _ => kind.label().to_owned(),
            },
            checked: Some(form.kind == kind),
            action: LinkAction::SetKind(kind),
        })
        .collect();
    rows.extend(form.fields().into_iter().map(Row::Field));
    if form.kind == TargetKind::File {
        let chosen = form
            .file
            .as_ref()
            .map_or("none chosen".to_owned(), |file| file.display().to_string());
        rows.push(button(
            "link-file",
            format!("Choose File… ({chosen})"),
            LinkAction::ChooseFile,
        ));
    }
    let look = form.look;
    rows.push(Row::Control {
        id: "link-visible".into(),
        role: Role::CheckBox,
        label: "Visible Rectangle".to_owned(),
        checked: Some(look.visible),
        action: LinkAction::Visible,
    });
    if look.visible {
        let width = match look.width as u8 {
            0 | 1 => "Thin",
            2 => "Medium",
            _ => "Thick",
        };
        rows.extend([
            button(
                "link-width",
                format!("Line Thickness: {width}"),
                LinkAction::NextWidth,
            ),
            button(
                "link-style",
                format!("Line Style: {}", look.style.label()),
                LinkAction::NextStyle,
            ),
            button(
                "link-color",
                format!("Colour: {}", color_name(look.color)),
                LinkAction::NextColor,
            ),
        ]);
    }
    rows.push(button(
        "link-highlight",
        format!("Highlight Style: {}", look.highlight.label()),
        LinkAction::NextHighlight,
    ));
    rows.push(button(
        "link-submit",
        if editing { "Save" } else { "Create" }.to_owned(),
        LinkAction::Submit,
    ));
    if editing {
        rows.push(button(
            "link-delete",
            "Delete Link".to_owned(),
            LinkAction::Delete,
        ));
    }
    if let Some(error) = error {
        rows.push(Row::Line(
            "link-error".into(),
            error.to_owned(),
            Role::Alert,
        ));
    }
    rows
}

fn state_rows(state: &LinkDialogState) -> Vec<Row> {
    let editing = matches!(state.mode, LinkMode::Edit { .. });
    rows(&state.form, editing, state.error.as_deref())
}

pub(in crate::shell) fn accessible(state: &LinkDialogState, cx: &gpui::App) -> Vec<Element> {
    state_rows(state)
        .into_iter()
        .filter_map(|row| match row {
            Row::Field(field) => state.text_field(field).map(|input| {
                input
                    .read(cx)
                    .accessible(field.label(), TextField::Link(field))
            }),
            Row::Control {
                id,
                role,
                label,
                checked,
                action,
            } => {
                let mut element =
                    Element::new(id, role, label).with_activation(Activation::Link(action));
                if let Some(checked) = checked {
                    element = element.with_state(A11yState::toggled(checked));
                }
                Some(element)
            }
            Row::Line(id, line, role) => Some(Element::new(id, role, line)),
        })
        .collect()
}

pub(in crate::shell) fn render(
    state: &LinkDialogState,
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
                        frame.run_activation(Activation::Link(action), window, cx);
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
    use onionskin_core::links::{Highlight, LineStyle, LinkLook};

    fn labels(form: &LinkForm, editing: bool) -> Vec<String> {
        rows(form, editing, Some("oops"))
            .iter()
            .map(|row| match row {
                Row::Field(field) => format!("[{}]", field.label()),
                Row::Control { label, .. } | Row::Line(_, label, _) => label.clone(),
            })
            .collect()
    }

    #[test]
    fn a_new_link_goes_to_a_page_invisibly() {
        let all = labels(&LinkForm::default(), false);
        assert_eq!(
            all,
            [
                "Go to a page",
                "Open a web page",
                "Open a file",
                "[Page number]",
                "Visible Rectangle",
                "Highlight Style: Invert",
                "Create",
                "oops",
            ]
        );
    }

    #[test]
    fn a_visible_link_offers_its_line_and_an_edit_offers_delete() {
        let form = LinkForm {
            kind: TargetKind::File,
            look: LinkLook {
                visible: true,
                width: 3.0,
                color: [0.0, 0.0, 1.0],
                style: LineStyle::Dashed,
                highlight: Highlight::Push,
            },
            file: Some("/a/b.pdf".into()),
            kept: Some("JavaScript".into()),
        };
        let all = labels(&form, true);
        for expected in [
            "Keep its JavaScript action",
            "Choose File… (/a/b.pdf)",
            "Line Thickness: Thick",
            "Line Style: Dashed",
            "Colour: Blue",
            "Highlight Style: Inset",
            "Save",
            "Delete Link",
        ] {
            assert!(all.contains(&expected.to_owned()), "{expected}: {all:?}");
        }
        let medium = LinkForm {
            look: LinkLook {
                visible: true,
                width: 2.0,
                ..LinkLook::default()
            },
            ..LinkForm::default()
        };
        assert!(labels(&medium, false).contains(&"Line Thickness: Medium".to_owned()));
    }
}
