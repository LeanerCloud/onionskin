//! The Crop Pages dialog's body: one list of rows that both the
//! accessibility tree and the drawing read, so a control cannot be drawn
//! without being described.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::pages::PageBox;

use super::{CropAction, CropDialogState, CropForm, CropScope};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::{ShellFrame, ThemeTokens};

/// One row of the dialog.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Field(TextField, &'static str),
    Control {
        id: gpui::ElementId,
        role: Role,
        label: String,
        /// Checked, for a checkbox or a radio button.
        checked: Option<bool>,
        action: CropAction,
    },
    Line(gpui::ElementId, String),
}

fn choice(id: gpui::ElementId, label: String, checked: bool, action: CropAction) -> Row {
    Row::Control {
        id,
        role: Role::RadioButton,
        label,
        checked: Some(checked),
        action,
    }
}

fn button(id: &'static str, label: &str, action: CropAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::Button,
        label: label.to_owned(),
        checked: None,
        action,
    }
}

/// The box choice, the margins, the pages, then Crop and any error.
fn rows(form: CropForm, chosen: &str, error: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = PageBox::ALL
        .into_iter()
        .enumerate()
        .map(|(index, which)| {
            choice(
                ("crop-box", index).into(),
                which.label().to_owned(),
                form.which == which,
                CropAction::SetBox(which),
            )
        })
        .collect();
    rows.push(Row::Control {
        id: "crop-remove-white".into(),
        role: Role::CheckBox,
        label: "Remove White Margins".to_owned(),
        checked: Some(form.remove_white),
        action: CropAction::RemoveWhiteMargins,
    });
    if !form.remove_white {
        rows.extend([
            Row::Field(TextField::CropTop, "Top (pt)"),
            Row::Field(TextField::CropBottom, "Bottom (pt)"),
            Row::Field(TextField::CropLeft, "Left (pt)"),
            Row::Field(TextField::CropRight, "Right (pt)"),
            button("crop-zero", "Set To Zero", CropAction::SetToZero),
            Row::Control {
                id: "crop-change-size".into(),
                role: Role::CheckBox,
                label: "Change Page Size".to_owned(),
                checked: Some(form.resize),
                action: CropAction::ChangePageSize,
            },
        ]);
    }
    if super::shows(form, TextField::CropWidth) {
        rows.extend([
            Row::Field(TextField::CropWidth, "Width (pt)"),
            Row::Field(TextField::CropHeight, "Height (pt)"),
        ]);
    }
    rows.extend([
        choice(
            "crop-scope-chosen".into(),
            chosen.to_owned(),
            form.scope == CropScope::Chosen,
            CropAction::SetScope(CropScope::Chosen),
        ),
        choice(
            "crop-scope-all".into(),
            "All pages".to_owned(),
            form.scope == CropScope::All,
            CropAction::SetScope(CropScope::All),
        ),
        button("crop-submit", "Crop", CropAction::Submit),
    ]);
    if let Some(error) = error {
        rows.push(Row::Line("crop-error".into(), error.to_owned()));
    }
    rows
}

fn state_rows(state: &CropDialogState) -> Vec<Row> {
    rows(state.form, &state.chosen_label(), state.error.as_deref())
}

/// The dialog, for a screen reader.
pub(in crate::shell) fn accessible(state: &CropDialogState, cx: &gpui::App) -> Vec<Element> {
    state_rows(state)
        .into_iter()
        .filter_map(|row| match row {
            Row::Field(field, label) => state
                .text_field(field)
                .map(|input| input.read(cx).accessible(label, field)),
            Row::Control {
                id,
                role,
                label,
                checked,
                action,
            } => {
                let mut element =
                    Element::new(id, role, label).with_activation(Activation::Crop(action));
                if let Some(checked) = checked {
                    element = element.with_state(A11yState::toggled(checked));
                }
                Some(element)
            }
            Row::Line(id, line) => Some(Element::new(id, Role::Alert, line)),
        })
        .collect()
}

pub(in crate::shell) fn render(
    state: &CropDialogState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
    for row in state_rows(state) {
        list = list.child(match row {
            Row::Field(field, label) => {
                let input = state.text_field(field).expect("the form's field").clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(90.0)).child(label))
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
                        frame.run_activation(Activation::Crop(action), window, cx);
                    }))
                    .child(match checked {
                        Some(true) => format!("✓ {label}"),
                        Some(false) => format!("○ {label}"),
                        None => label,
                    })
                    .into_any_element()
            }
            Row::Line(id, line) => div()
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

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Field(_, label) => format!("[{label}]"),
                Row::Control { label, .. } | Row::Line(_, label) => label.clone(),
            })
            .collect()
    }

    #[test]
    fn the_margins_show_unless_white_margins_are_removed() {
        let mut form = CropForm::default();
        let all = labels(&rows(form, "Page 2", None));
        assert_eq!(
            all,
            [
                "CropBox",
                "BleedBox",
                "TrimBox",
                "ArtBox",
                "Remove White Margins",
                "[Top (pt)]",
                "[Bottom (pt)]",
                "[Left (pt)]",
                "[Right (pt)]",
                "Set To Zero",
                "Change Page Size",
                "Page 2",
                "All pages",
                "Crop",
            ]
        );
        form.apply(CropAction::ChangePageSize);
        let sized = labels(&rows(form, "Page 2", None));
        let at = sized
            .iter()
            .position(|label| label == "Change Page Size")
            .expect("listed");
        assert_eq!(sized[at + 1..at + 3], ["[Width (pt)]", "[Height (pt)]"]);
        form.apply(CropAction::RemoveWhiteMargins);
        let fitted = labels(&rows(form, "Page 2", Some("wrong")));
        assert!(!fitted.iter().any(|label| label.starts_with('[')));
        assert!(!fitted.contains(&"Set To Zero".to_owned()));
        assert_eq!(fitted.last().map(String::as_str), Some("wrong"));
    }

    /// Each radio button and checkbox reports the form's state, and its
    /// action is the one that turns it on.
    #[test]
    fn every_choice_reports_the_state_its_action_sets() {
        let form = CropForm::default();
        for row in rows(form, "Page 1", None) {
            let Row::Control {
                checked: Some(_),
                action,
                label,
                ..
            } = row
            else {
                continue;
            };
            let mut changed = form;
            changed.apply(action);
            let now = rows(changed, "Page 1", None)
                .into_iter()
                .find_map(|row| match row {
                    Row::Control {
                        label: other,
                        checked,
                        ..
                    } if other == label => checked,
                    _ => None,
                })
                .expect("still listed");
            assert!(now, "{label} is on once chosen");
        }
    }
}
