//! The Advanced Search dialog's body: one list of rows that both the
//! accessibility tree and the drawing read, so a control cannot be drawn
//! without being described.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::MatchMode;

use super::{outcome_lines, AdvancedAction, AdvancedForm, AdvancedSearchState};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::{ShellFrame, ThemeTokens};
use crate::shell::find_bar::FindOption;

/// Acrobat's wording for Return Results Containing.
const MODES: [(&str, MatchMode); 3] = [
    ("Match Exact Word Or Phrase", MatchMode::Phrase),
    ("Any Of The Words", MatchMode::AnyWord),
    ("All Of The Words", MatchMode::AllWords),
];

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
        action: AdvancedAction,
    },
    Line(gpui::ElementId, String),
}

fn check(id: &'static str, label: &str, checked: bool, action: AdvancedAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::CheckBox,
        label: label.to_owned(),
        checked: Some(checked),
        action,
    }
}

fn button(id: &'static str, label: String, action: AdvancedAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::Button,
        label,
        checked: None,
        action,
    }
}

/// The form's rows, then the last search's lines.
fn rows(form: &AdvancedForm, lines: &[String]) -> Vec<Row> {
    let options = form.options;
    let mut rows = vec![Row::Field(TextField::AdvancedQuery, "Search for")];
    rows.extend(
        MODES
            .iter()
            .enumerate()
            .map(|(index, (label, mode))| Row::Control {
                id: ("advanced-mode", index).into(),
                role: Role::RadioButton,
                label: (*label).to_owned(),
                checked: Some(options.mode == *mode),
                action: AdvancedAction::Option(FindOption::Mode(*mode)),
            }),
    );
    rows.extend([
        check(
            "advanced-whole-word",
            "Whole words only",
            options.whole_word,
            AdvancedAction::Option(FindOption::WholeWord),
        ),
        check(
            "advanced-case",
            "Case-Sensitive",
            options.case_sensitive,
            AdvancedAction::Option(FindOption::CaseSensitive),
        ),
        check(
            "advanced-comments",
            "Include Comments",
            options.include_comments,
            AdvancedAction::Option(FindOption::IncludeComments),
        ),
        check(
            "advanced-attachments",
            "Include PDF Attachments",
            form.include_attachments,
            AdvancedAction::IncludeAttachments,
        ),
        check(
            "advanced-criteria",
            "Use These Additional Criteria",
            form.criterion.is_some(),
            AdvancedAction::UseCriterion,
        ),
    ]);
    if let Some((field, test)) = form.criterion {
        rows.push(button(
            "advanced-field",
            format!("Property: {}", field.label()),
            AdvancedAction::NextField,
        ));
        rows.push(button(
            "advanced-test",
            format!("Test: {}", test.label()),
            AdvancedAction::NextTest,
        ));
        rows.push(Row::Field(TextField::AdvancedValue, "Value"));
    }
    rows.push(button(
        "advanced-search",
        "Search".to_owned(),
        AdvancedAction::Search,
    ));
    rows.extend(
        lines
            .iter()
            .enumerate()
            .map(|(index, line)| Row::Line(("advanced-outcome", index).into(), line.clone())),
    );
    rows
}

fn state_rows(state: &AdvancedSearchState) -> Vec<Row> {
    let lines = state
        .outcome
        .as_ref()
        .map(outcome_lines)
        .unwrap_or_default();
    rows(&state.form, &lines)
}

/// The dialog, for a screen reader.
pub(in crate::shell) fn accessible(state: &AdvancedSearchState, cx: &gpui::App) -> Vec<Element> {
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
                let mut element = Element::new(id, role, label)
                    .with_activation(Activation::AdvancedSearch(action));
                if let Some(checked) = checked {
                    element = element.with_state(A11yState::toggled(checked));
                }
                Some(element)
            }
            Row::Line(id, line) => Some(Element::new(id, Role::Label, line)),
        })
        .collect()
}

pub(in crate::shell) fn render(
    state: &AdvancedSearchState,
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
                        frame.run_activation(Activation::AdvancedSearch(action), window, cx);
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
                .text_sm()
                .text_color(theme.secondary_text)
                .child(line)
                .into_any_element(),
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use onionskin_core::metadata::{PropertyField, PropertyTest};
    use onionskin_core::SearchOptions;

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
    fn the_criterion_rows_appear_only_while_a_criterion_is_used() {
        let mut form = AdvancedForm::new(SearchOptions::default());
        let plain = labels(&rows(&form, &[]));
        assert!(!plain.iter().any(|label| label.starts_with("Property")));
        assert_eq!(plain.first().map(String::as_str), Some("[Search for]"));
        assert_eq!(plain.last().map(String::as_str), Some("Search"));

        form.criterion = Some((PropertyField::Created, PropertyTest::After));
        let with = labels(&rows(&form, &["a line".to_owned()]));
        let at = with
            .iter()
            .position(|label| label == "Property: Date Created")
            .expect("the property button");
        assert_eq!(with[at + 1], "Test: is after");
        assert_eq!(with[at + 2], "[Value]");
        assert_eq!(with.last().map(String::as_str), Some("a line"));
    }

    /// Each checkbox and radio button reports the form's state, and its
    /// action is the one that changes that state.
    #[test]
    fn every_control_reports_the_state_its_action_changes() {
        let mut form = AdvancedForm::new(SearchOptions::default());
        for row in rows(&form.clone(), &[]) {
            let Row::Control {
                checked: Some(before),
                action,
                label,
                ..
            } = row
            else {
                continue;
            };
            let mut changed = form;
            changed.apply(action);
            let after = rows(&changed, &[])
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
            if before {
                // A radio button already chosen stays chosen.
                assert!(after, "{label}");
            } else {
                assert!(after, "{label} did not turn on");
            }
        }
        form.apply(AdvancedAction::IncludeAttachments);
        assert!(form.include_attachments);
    }
}
