//! The page-marks dialog's body: one list of rows that both the
//! accessibility tree and the drawing read, so a control cannot be drawn
//! without being described.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::pages::MarkKind;
use onionskin_tools_edit::marks::{HAlign, VAlign, POSITIONS};

use super::{fields, MarkAction, MarkField, MarkForm, MarksDialogState, Source, COLORS};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::crop_dialog::CropScope;
use crate::shell::chrome::{ShellFrame, ThemeTokens};

/// One row of the dialog.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Field(MarkField),
    Control {
        id: gpui::ElementId,
        role: Role,
        label: String,
        /// Checked, for a checkbox or a radio button.
        checked: Option<bool>,
        action: MarkAction,
    },
    Line(gpui::ElementId, String, Role),
}

fn control(
    id: &'static str,
    role: Role,
    label: String,
    checked: Option<bool>,
    action: MarkAction,
) -> Row {
    Row::Control {
        id: id.into(),
        role,
        label,
        checked,
        action,
    }
}

fn button(id: &'static str, label: String, action: MarkAction) -> Row {
    control(id, Role::Button, label, None, action)
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::Text => "Text",
        Source::Color => "Colour",
        Source::File => "File",
    }
}

fn horizontal_label(align: HAlign) -> &'static str {
    match align {
        HAlign::Left => "Left",
        HAlign::Center => "Center",
        HAlign::Right => "Right",
    }
}

fn vertical_label(align: VAlign) -> &'static str {
    match align {
        VAlign::Top => "Top",
        VAlign::Center => "Center",
        VAlign::Bottom => "Bottom",
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The source, the look, the fields, placement, pages, then the buttons.
fn rows(form: &MarkForm, chosen: &str, error: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = form
        .sources()
        .iter()
        .enumerate()
        .map(|(index, source)| Row::Control {
            id: ("mark-source", index).into(),
            role: Role::RadioButton,
            label: source_label(*source).to_owned(),
            checked: Some(form.source == *source),
            action: MarkAction::SetSource(*source),
        })
        .collect();
    if form.source == Source::File && !form.sources().is_empty() {
        let chosen_file = form
            .file
            .as_deref()
            .map_or("none chosen".to_owned(), file_name);
        rows.push(button(
            "mark-file",
            format!("Choose PDF File… ({chosen_file})"),
            MarkAction::ChooseFile,
        ));
    }
    if form.sets_text() {
        rows.push(button(
            "mark-font",
            format!("Font: {}", form.font.base_font()),
            MarkAction::NextFont,
        ));
    }
    if form.sets_text() || form.source == Source::Color {
        rows.push(button(
            "mark-color",
            format!("Colour: {}", COLORS[form.color].0),
            MarkAction::NextColor,
        ));
    }
    if form.kind == MarkKind::Bates {
        rows.push(button(
            "mark-position",
            format!("Position: {}", POSITIONS[form.position]),
            MarkAction::NextPosition,
        ));
    }
    rows.extend(fields(form).into_iter().map(Row::Field));
    if form.kind == MarkKind::HeaderFooter {
        rows.push(Row::Line(
            "mark-tokens".into(),
            "[page], [pages] and [date] are filled in on each page.".to_owned(),
            Role::Label,
        ));
    }
    if form.is_placed() {
        rows.push(button(
            "mark-horizontal",
            format!("Horizontal: {}", horizontal_label(form.horizontal)),
            MarkAction::NextHorizontal,
        ));
        rows.push(button(
            "mark-vertical",
            format!("Vertical: {}", vertical_label(form.vertical)),
            MarkAction::NextVertical,
        ));
    }
    if form.kind == MarkKind::Watermark {
        rows.push(control(
            "mark-behind",
            Role::CheckBox,
            "Appear Behind Page".to_owned(),
            Some(form.behind),
            MarkAction::Behind,
        ));
    }
    if form.kind == MarkKind::Bates {
        rows.push(button(
            "mark-add-files",
            "Also Number Other Files…".to_owned(),
            MarkAction::AddFiles,
        ));
        rows.extend(form.other_files.iter().enumerate().map(|(index, path)| {
            Row::Line(
                ("mark-other-file", index).into(),
                file_name(path),
                Role::ListItem,
            )
        }));
        if !form.other_files.is_empty() {
            rows.push(control(
                "mark-numbers-in-names",
                Role::CheckBox,
                "Add Bates Numbers to File Names".to_owned(),
                Some(form.numbers_in_names),
                MarkAction::NumbersInNames,
            ));
        }
    }
    for (id, label, scope) in [
        ("mark-scope-chosen", chosen.to_owned(), CropScope::Chosen),
        ("mark-scope-all", "All pages".to_owned(), CropScope::All),
    ] {
        rows.push(control(
            id,
            Role::RadioButton,
            label,
            Some(form.scope == scope),
            MarkAction::SetScope(scope),
        ));
    }
    let submit = if form.existing && form.kind != MarkKind::Bates {
        "Update"
    } else {
        "Add"
    };
    rows.push(button("mark-submit", submit.to_owned(), MarkAction::Submit));
    if form.existing {
        rows.push(button(
            "mark-remove",
            "Remove".to_owned(),
            MarkAction::Remove,
        ));
    }
    if let Some(error) = error {
        rows.push(Row::Line(
            "mark-error".into(),
            error.to_owned(),
            Role::Alert,
        ));
    }
    rows
}

fn state_rows(state: &MarksDialogState) -> Vec<Row> {
    rows(&state.form, &state.chosen_label(), state.error.as_deref())
}

/// The dialog, for a screen reader.
pub(in crate::shell) fn accessible(state: &MarksDialogState, cx: &gpui::App) -> Vec<Element> {
    state_rows(state)
        .into_iter()
        .filter_map(|row| match row {
            Row::Field(field) => state.text_field(field).map(|input| {
                input
                    .read(cx)
                    .accessible(field.label(), TextField::Mark(field))
            }),
            Row::Control {
                id,
                role,
                label,
                checked,
                action,
            } => {
                let mut element =
                    Element::new(id, role, label).with_activation(Activation::Marks(action));
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
    state: &MarksDialogState,
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
                    .child(div().w(px(130.0)).child(field.label()))
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
                        frame.run_activation(Activation::Marks(action), window, cx);
                    }))
                    .child(match checked {
                        Some(true) => format!("✓ {label}"),
                        Some(false) => format!("○ {label}"),
                        None => label,
                    })
                    .into_any_element()
            }
            Row::Line(id, line, role) => div()
                .id(id)
                .when(role == Role::Alert, |row| row.text_color(theme.error_text))
                .when(role != Role::Alert, |row| {
                    row.text_sm().text_color(theme.secondary_text)
                })
                .child(line)
                .into_any_element(),
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(form: &MarkForm) -> Vec<String> {
        rows(form, "Page 1", None)
            .iter()
            .map(|row| match row {
                Row::Field(field) => format!("[{}]", field.label()),
                Row::Control { label, .. } | Row::Line(_, label, _) => label.clone(),
            })
            .collect()
    }

    #[test]
    fn a_header_and_footer_offers_six_lines_its_style_and_margins() {
        let form = MarkForm::new(MarkKind::HeaderFooter, false);
        let all = labels(&form);
        assert_eq!(all[0], "Font: Helvetica");
        assert_eq!(all[1], "Colour: Black");
        assert_eq!(all[2..8].len(), 6);
        assert!(all.contains(&"[Center Footer]".to_owned()));
        assert!(all.contains(&"[Start number]".to_owned()));
        assert!(all.iter().any(|label| label.contains("[page]")));
        assert_eq!(all.last().map(String::as_str), Some("Add"));
    }

    #[test]
    fn an_existing_mark_is_updated_or_removed() {
        let form = MarkForm::new(MarkKind::Watermark, true);
        let all = labels(&form);
        let at = all.len();
        assert_eq!(all[at - 2..], ["Update", "Remove"]);
        assert!(all.contains(&"Appear Behind Page".to_owned()));
        assert!(all.contains(&"Horizontal: Center".to_owned()));
        let bates = labels(&MarkForm::new(MarkKind::Bates, true));
        assert!(
            bates.contains(&"Add".to_owned()),
            "Bates numbers are added again"
        );
    }

    #[test]
    fn a_file_source_asks_for_a_file_and_its_scale() {
        let mut form = MarkForm::new(MarkKind::Background, false);
        assert!(labels(&form).contains(&"Colour: Light Gray".to_owned()));
        assert!(!labels(&form)
            .iter()
            .any(|label| label.starts_with("Horizontal")));
        form.apply(MarkAction::SetSource(Source::File));
        let all = labels(&form);
        assert!(all.contains(&"Choose PDF File… (none chosen)".to_owned()));
        assert!(all.contains(&"[Scale (%)]".to_owned()));
        assert!(all.contains(&"Vertical: Center".to_owned()));
        form.file = Some("/art/logo.pdf".into());
        assert!(labels(&form).contains(&"Choose PDF File… (logo.pdf)".to_owned()));
    }

    #[test]
    fn bates_lists_the_other_files_and_how_to_name_them() {
        let mut form = MarkForm::new(MarkKind::Bates, false);
        assert!(labels(&form).contains(&"Position: Right Footer".to_owned()));
        assert!(!labels(&form).contains(&"[Add to file names]".to_owned()));
        form.other_files = vec!["/a/one.pdf".into()];
        let all = labels(&form);
        assert!(all.contains(&"one.pdf".to_owned()));
        assert!(all.contains(&"[Add to file names]".to_owned()));
        assert!(all.contains(&"Add Bates Numbers to File Names".to_owned()));
        let with_error = rows(&form, "Page 1", Some("wrong"));
        assert!(
            matches!(with_error.last(), Some(Row::Line(_, line, Role::Alert)) if line == "wrong")
        );
    }

    #[test]
    fn every_choice_reports_the_state_its_action_sets() {
        for kind in MarkKind::ALL {
            let form = MarkForm::new(kind, false);
            for row in rows(&form, "Page 1", None) {
                let Row::Control {
                    checked: Some(before),
                    action,
                    label,
                    role,
                    ..
                } = row
                else {
                    continue;
                };
                let mut changed = form.clone();
                changed.apply(action);
                let now = rows(&changed, "Page 1", None)
                    .into_iter()
                    .find_map(|row| match row {
                        Row::Control {
                            label: other,
                            checked,
                            ..
                        } if other == label => checked,
                        _ => None,
                    });
                match role {
                    Role::RadioButton => assert_eq!(now, Some(true), "{label}"),
                    _ => assert_eq!(now, Some(!before), "{label} toggles"),
                }
            }
        }
    }

    #[test]
    fn the_cycles_come_round_again() {
        let mut form = MarkForm::new(MarkKind::Watermark, false);
        for _ in 0..3 {
            form.apply(MarkAction::NextHorizontal);
            form.apply(MarkAction::NextVertical);
        }
        assert_eq!(
            (form.horizontal, form.vertical),
            (HAlign::Center, VAlign::Center)
        );
        for _ in 0..COLORS.len() {
            form.apply(MarkAction::NextColor);
        }
        assert_eq!(form.color, 3);
        for _ in 0..5 {
            form.apply(MarkAction::NextFont);
        }
        assert_eq!(form.font, onionskin_tools_edit::marks::Font::Helvetica);
        for _ in 0..POSITIONS.len() {
            form.apply(MarkAction::NextPosition);
        }
        assert_eq!(form.position, 5);
        assert_eq!(horizontal_label(HAlign::Left), "Left");
        assert_eq!(horizontal_label(HAlign::Right), "Right");
        assert_eq!(vertical_label(VAlign::Top), "Top");
        assert_eq!(vertical_label(VAlign::Bottom), "Bottom");
    }
}
