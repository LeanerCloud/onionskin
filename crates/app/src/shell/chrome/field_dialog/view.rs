//! The Properties dialog's body: one list of rows per tab, which both the
//! accessibility tree and the drawing read.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::forms::KindOptions;

use super::form::{
    name_of, FieldAction, FieldForm, FieldInput, Flag, FormatKind, RuleKind, Shape, Tab, ALIGNS,
    BORDERS, FILLS, NEGATIVES, SEPARATORS, SPECIALS, TEXT_COLORS, TIME_STYLES,
};
use super::FieldDialogState;
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::{ShellFrame, ThemeTokens};

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Field(FieldInput),
    Control {
        id: gpui::ElementId,
        role: Role,
        label: String,
        checked: Option<bool>,
        action: FieldAction,
    },
    Line(gpui::ElementId, String, Role),
}

fn button(id: impl Into<gpui::ElementId>, label: String, action: FieldAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::Button,
        label,
        checked: None,
        action,
    }
}

fn choice(id: impl Into<gpui::ElementId>, label: &str, on: bool, action: FieldAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::RadioButton,
        label: label.to_owned(),
        checked: Some(on),
        action,
    }
}

fn check(form: &FieldForm, id: &'static str, label: &str, flag: Flag) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::CheckBox,
        label: label.to_owned(),
        checked: Some(form.flag(flag)),
        action: FieldAction::Toggle(flag),
    }
}

fn rows(form: &FieldForm, error: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = form
        .shape
        .tabs()
        .into_iter()
        .enumerate()
        .map(|(index, tab)| {
            choice(
                ("field-tab", index),
                tab.label(),
                form.tab == tab,
                FieldAction::Tab(tab),
            )
        })
        .collect();
    match form.tab {
        Tab::General => general(form, &mut rows),
        Tab::Appearance => appearance(form, &mut rows),
        Tab::Position => rows.extend(form.inputs().into_iter().map(Row::Field)),
        Tab::Options => options(form, &mut rows),
        Tab::Format => format(form, &mut rows),
        Tab::Validate => validate(form, &mut rows),
        Tab::Calculate => calculate(form, &mut rows),
    }
    rows.push(button("field-save", "Save".to_owned(), FieldAction::Submit));
    rows.push(button(
        "field-delete",
        "Delete Field".to_owned(),
        FieldAction::Delete,
    ));
    if let Some(error) = error {
        rows.push(Row::Line(
            "field-error".into(),
            error.to_owned(),
            Role::Alert,
        ));
    }
    rows
}

fn general(form: &FieldForm, rows: &mut Vec<Row>) {
    rows.push(Row::Line(
        "field-kind".into(),
        form.shape.label().to_owned(),
        Role::Label,
    ));
    rows.extend(form.inputs().into_iter().map(Row::Field));
    rows.push(check(form, "field-hidden", "Hidden", Flag::Hidden));
    rows.push(check(form, "field-read-only", "Read Only", Flag::ReadOnly));
    rows.push(check(form, "field-required", "Required", Flag::Required));
}

fn appearance(form: &FieldForm, rows: &mut Vec<Row>) {
    let properties = &form.properties;
    let size = match properties.font_size {
        size if size <= 0.0 => "Auto".to_owned(),
        size => format!("{size}"),
    };
    rows.extend([
        button(
            "field-border",
            format!("Border Color: {}", name_of(&BORDERS, &properties.border)),
            FieldAction::NextBorder,
        ),
        button(
            "field-fill",
            format!("Fill Color: {}", name_of(&FILLS, &properties.fill)),
            FieldAction::NextFill,
        ),
        button(
            "field-font-size",
            format!("Font Size: {size}"),
            FieldAction::NextFontSize,
        ),
        button(
            "field-text-color",
            format!(
                "Text Color: {}",
                name_of(&TEXT_COLORS, &properties.text_color)
            ),
            FieldAction::NextTextColor,
        ),
    ]);
}

fn options(form: &FieldForm, rows: &mut Vec<Row>) {
    match (&form.shape, &form.properties.options) {
        (Shape::Text, KindOptions::Text { align, .. }) => {
            rows.push(button(
                "field-align",
                format!("Alignment: {}", ALIGNS[usize::from(*align).min(2)]),
                FieldAction::NextAlign,
            ));
            rows.extend(form.inputs().into_iter().map(Row::Field));
            rows.push(check(
                form,
                "field-multiline",
                "Multi-line",
                Flag::Multiline,
            ));
            rows.push(check(form, "field-password", "Password", Flag::Password));
            rows.push(check(
                form,
                "field-comb",
                "Comb of characters (needs a limit)",
                Flag::Comb,
            ));
        }
        (Shape::CheckBox | Shape::Radio, _) => {
            rows.extend(form.inputs().into_iter().map(Row::Field));
            let checked = match form.shape {
                Shape::Radio => "Button is checked by default",
                _ => "Check box is checked by default",
            };
            rows.push(check(
                form,
                "field-on-by-default",
                checked,
                Flag::OnByDefault,
            ));
            if form.shape == Shape::Radio {
                rows.push(check(
                    form,
                    "field-no-toggle",
                    "Clicking the chosen button leaves it chosen",
                    Flag::NoToggleToOff,
                ));
            }
        }
        (Shape::ListBox | Shape::Dropdown, _) => items(form, rows),
        _ => rows.extend(form.inputs().into_iter().map(Row::Field)),
    }
}

fn items(form: &FieldForm, rows: &mut Vec<Row>) {
    rows.extend(form.inputs().into_iter().map(Row::Field));
    rows.push(button(
        "field-add-item",
        "Add".to_owned(),
        FieldAction::AddOption,
    ));
    for (index, item) in form.items.iter().enumerate() {
        let default = if form.default_item.as_ref() == Some(&item.export) {
            ", default"
        } else {
            ""
        };
        rows.push(choice(
            ("field-item", index),
            &format!("{} ({}{default})", item.display, item.export),
            form.selected_item == Some(index),
            FieldAction::SelectOption(index),
        ));
    }
    if form.selected_item.is_some() {
        rows.extend([
            button("field-item-up", "Up".to_owned(), FieldAction::OptionUp),
            button(
                "field-item-down",
                "Down".to_owned(),
                FieldAction::OptionDown,
            ),
            button(
                "field-item-default",
                "Chosen by Default".to_owned(),
                FieldAction::DefaultOption,
            ),
            button(
                "field-item-remove",
                "Delete Item".to_owned(),
                FieldAction::RemoveOption,
            ),
        ]);
    }
    let (id, label, flag) = match form.shape {
        Shape::Dropdown => (
            "field-editable",
            "Allow user to enter custom text",
            Flag::Editable,
        ),
        _ => (
            "field-multi-select",
            "Multiple selection",
            Flag::MultiSelect,
        ),
    };
    rows.push(check(form, id, label, flag));
}

fn format(form: &FieldForm, rows: &mut Vec<Row>) {
    rows.extend(
        FormatKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                choice(
                    ("field-format", index),
                    kind.label(),
                    form.format == kind,
                    FieldAction::SetFormat(kind),
                )
            }),
    );
    let decimals = button(
        "field-decimals",
        format!("Decimal Places: {}", form.decimals),
        FieldAction::NextDecimals,
    );
    let separator = button(
        "field-separator",
        format!(
            "Separator Style: {}",
            SEPARATORS[usize::from(form.separator)]
        ),
        FieldAction::NextSeparator,
    );
    match form.format {
        FormatKind::Number => {
            rows.extend([
                decimals,
                separator,
                button(
                    "field-negative",
                    format!(
                        "Negative Number Style: {}",
                        NEGATIVES[usize::from(form.negative)]
                    ),
                    FieldAction::NextNegative,
                ),
            ]);
            rows.extend(form.inputs().into_iter().map(Row::Field));
            rows.push(check(
                form,
                "field-currency-first",
                "Currency symbol before the number",
                Flag::CurrencyFirst,
            ));
        }
        FormatKind::Percent => rows.extend([decimals, separator]),
        FormatKind::Time => rows.push(button(
            "field-time",
            format!("Time Format: {}", TIME_STYLES[usize::from(form.time_style)]),
            FieldAction::NextTimeStyle,
        )),
        FormatKind::Special => rows.push(button(
            "field-special",
            format!("Special: {}", SPECIALS[usize::from(form.special)]),
            FieldAction::NextSpecial,
        )),
        FormatKind::None | FormatKind::Date | FormatKind::Custom => {
            rows.extend(form.inputs().into_iter().map(Row::Field));
        }
    }
}

fn rules(
    rows: &mut Vec<Row>,
    id: &'static str,
    labels: [&str; 3],
    current: RuleKind,
    action: fn(RuleKind) -> FieldAction,
) {
    let kinds = [RuleKind::None, RuleKind::Simple, RuleKind::Custom];
    rows.extend(
        kinds
            .into_iter()
            .zip(labels)
            .enumerate()
            .map(|(index, (kind, label))| {
                choice((id, index), label, current == kind, action(kind))
            }),
    );
}

fn validate(form: &FieldForm, rows: &mut Vec<Row>) {
    rules(
        rows,
        "field-validate",
        [
            "Field value is not validated",
            "Field value is in range",
            "Run custom validation script",
        ],
        form.validate,
        FieldAction::SetValidate,
    );
    rows.extend(form.inputs().into_iter().map(Row::Field));
}

fn calculate(form: &FieldForm, rows: &mut Vec<Row>) {
    rules(
        rows,
        "field-calculate",
        [
            "Value is not calculated",
            "Value is calculated from other fields",
            "Custom calculation script",
        ],
        form.calculate,
        FieldAction::SetCalculate,
    );
    if form.calculate == RuleKind::Simple {
        rows.push(button(
            "field-op",
            format!("Value is the {} of the fields", form.op.label()),
            FieldAction::NextOp,
        ));
    }
    rows.extend(form.inputs().into_iter().map(Row::Field));
}

pub(in crate::shell) fn accessible(state: &FieldDialogState, cx: &gpui::App) -> Vec<Element> {
    rows(&state.form, state.error.as_deref())
        .into_iter()
        .filter_map(|row| match row {
            Row::Field(which) => state.text_field(which).map(|input| {
                input
                    .read(cx)
                    .accessible(which.label(), TextField::Field(which))
            }),
            Row::Control {
                id,
                role,
                label,
                checked,
                action,
            } => {
                let mut element =
                    Element::new(id, role, label).with_activation(Activation::Field(action));
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
    state: &FieldDialogState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
    for row in rows(&state.form, state.error.as_deref()) {
        list = list.child(match row {
            Row::Field(which) => {
                let input = state.text_field(which).expect("the tab's field").clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(170.0)).child(which.label()))
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
                        frame.run_activation(Activation::Field(action), window, cx);
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
                .child(line)
                .into_any_element(),
        });
    }
    list
}

#[cfg(test)]
pub(super) fn labels(form: &FieldForm) -> Vec<String> {
    rows(form, Some("oops"))
        .iter()
        .map(|row| match row {
            Row::Field(which) => format!("[{}]", which.label()),
            Row::Control {
                label,
                checked: Some(true),
                ..
            } => format!("* {label}"),
            Row::Control { label, .. } | Row::Line(_, label, _) => label.clone(),
        })
        .collect()
}
