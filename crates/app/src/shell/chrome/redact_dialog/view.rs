//! The redaction dialog's body: one list of rows per panel that both the
//! accessibility tree and the drawing read.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::{
    name_of, FindForm, Panel, PropertiesForm, RedactAction, RedactDialogState, RedactField, COLORS,
    FILLS,
};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::{ShellFrame, ThemeTokens};

/// Results listed one to a row; beyond this many the rest are counted.
const LISTED: usize = 50;

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Field(RedactField),
    Control {
        id: gpui::ElementId,
        role: Role,
        label: String,
        checked: Option<bool>,
        action: RedactAction,
    },
    Line(gpui::ElementId, String, Role),
}

fn button(id: impl Into<gpui::ElementId>, label: impl Into<String>, action: RedactAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::Button,
        label: label.into(),
        checked: None,
        action,
    }
}

fn toggle(id: &'static str, label: &str, checked: bool, action: RedactAction) -> Row {
    Row::Control {
        id: id.into(),
        role: Role::CheckBox,
        label: label.to_owned(),
        checked: Some(checked),
        action,
    }
}

fn line(id: impl Into<gpui::ElementId>, text: impl Into<String>) -> Row {
    Row::Line(id.into(), text.into(), Role::Label)
}

fn properties_rows(form: &PropertiesForm, editing_mark: bool) -> Vec<Row> {
    let mut rows = vec![
        button(
            "redact-fill",
            format!("Fill: {}", name_of(&FILLS, &form.fill)),
            RedactAction::NextFill,
        ),
        button(
            "redact-outline",
            format!("Outline: {}", name_of(&COLORS, &form.outline)),
            RedactAction::NextOutline,
        ),
        toggle(
            "redact-overlay",
            "Overlay Text",
            form.overlay,
            RedactAction::Overlay,
        ),
    ];
    if form.overlay {
        rows.push(Row::Field(RedactField::OverlayText));
        rows.push(Row::Field(RedactField::FontSize));
        rows.extend([
            button(
                "redact-text-color",
                format!("Text Colour: {}", name_of(&COLORS, &form.text_color)),
                RedactAction::NextTextColor,
            ),
            button(
                "redact-align",
                format!("Alignment: {}", form.align.label()),
                RedactAction::NextAlign,
            ),
            toggle(
                "redact-repeat",
                "Repeat Overlay Text",
                form.repeat,
                RedactAction::Repeat,
            ),
        ]);
    }
    rows.extend(code_rows(form));
    rows.push(button("redact-save", "Save", RedactAction::Save));
    if editing_mark {
        rows.push(button(
            "redact-remove-mark",
            "Remove Mark",
            RedactAction::RemoveMark,
        ));
    }
    rows
}

fn code_rows(form: &PropertiesForm) -> Vec<Row> {
    let set = form.current_set();
    let mut rows = vec![button(
        "redact-set",
        format!("Code Set: {}", set.map_or("none", |set| set.name.as_str())),
        RedactAction::NextSet,
    )];
    if let Some(code) = form.current_code() {
        rows.push(button(
            "redact-code",
            format!("Code: {code}"),
            RedactAction::NextCode,
        ));
        rows.push(button(
            "redact-use-code",
            format!("Use {code} as Overlay Text"),
            RedactAction::UseCode,
        ));
    }
    rows.push(Row::Field(RedactField::SetName));
    rows.push(Row::Field(RedactField::Codes));
    rows.push(button(
        "redact-save-set",
        "Save Code Set",
        RedactAction::SaveSet,
    ));
    if set.is_some_and(|set| !set.built_in) {
        rows.extend([
            button(
                "redact-rename-set",
                "Rename Code Set",
                RedactAction::RenameSet,
            ),
            button(
                "redact-remove-set",
                "Remove Code Set",
                RedactAction::RemoveSet,
            ),
        ]);
    }
    rows.extend([
        button(
            "redact-import-set",
            "Import Code Set…",
            RedactAction::ImportSet,
        ),
        button(
            "redact-export-set",
            "Export Code Set…",
            RedactAction::ExportSet,
        ),
    ]);
    rows
}

fn find_rows(form: &FindForm) -> Vec<Row> {
    let radio = |id: &'static str, label: &str, patterns: bool| Row::Control {
        id: id.into(),
        role: Role::RadioButton,
        label: label.to_owned(),
        checked: Some(form.patterns == patterns),
        action: RedactAction::Patterns(patterns),
    };
    let mut rows = vec![
        radio("redact-find-words", "Words or phrase", false),
        radio("redact-find-patterns", "Patterns", true),
    ];
    if form.patterns {
        rows.push(button(
            "redact-pattern",
            format!("Pattern: {}", form.current_pattern().label()),
            RedactAction::NextPattern,
        ));
    } else {
        rows.extend([
            Row::Field(RedactField::FindText),
            toggle(
                "redact-whole-word",
                "Whole words only",
                form.whole_word,
                RedactAction::WholeWord,
            ),
            toggle(
                "redact-match-case",
                "Case sensitive",
                form.match_case,
                RedactAction::MatchCase,
            ),
        ]);
    }
    rows.push(button("redact-find", "Find", RedactAction::Find));
    if !form.searched {
        return rows;
    }
    let count = form.found.len();
    rows.push(line(
        "redact-found",
        match count {
            0 => "Nothing found.".to_owned(),
            1 => "1 found.".to_owned(),
            count => format!("{count} found."),
        },
    ));
    for (index, (found, checked)) in form
        .found
        .iter()
        .zip(&form.checked)
        .enumerate()
        .take(LISTED)
    {
        rows.push(Row::Control {
            id: ("redact-hit", index).into(),
            role: Role::CheckBox,
            label: format!("Page {}: {}", found.page + 1, found.text),
            checked: Some(*checked),
            action: RedactAction::Toggle(index),
        });
    }
    if count > LISTED {
        rows.push(line(
            "redact-more",
            format!("and {} more, all checked", count - LISTED),
        ));
    }
    if count > 0 {
        rows.push(button(
            "redact-mark-checked",
            "Mark Checked for Redaction",
            RedactAction::MarkChecked,
        ));
    }
    rows
}

fn apply_rows(sanitize: bool, hidden_information: bool) -> Vec<Row> {
    let mut rows = if sanitize {
        vec![
            line(
                "redact-apply-what",
                "Removes metadata, document scripts and actions that run things, attachments, \
                 comments, hidden layers, content outside the crop box, and private data.",
            ),
            line(
                "redact-apply-how",
                "Links and form fields stay. Any redaction marks are applied too.",
            ),
        ]
    } else {
        vec![line(
            "redact-apply-what",
            "Removes everything under every mark, for good, and fills the marked areas.",
        )]
    };
    rows.push(line(
        "redact-apply-file",
        "The result is saved as a new file with no editing history, checked before it is written. \
         This document is not changed.",
    ));
    if !sanitize {
        rows.push(toggle(
            "redact-hidden",
            "Also remove hidden information",
            hidden_information,
            RedactAction::HiddenInformation,
        ));
    }
    rows.push(button(
        "redact-apply",
        if sanitize {
            "Remove and Save As…"
        } else {
            "Apply and Save As…"
        },
        RedactAction::Apply,
    ));
    rows
}

fn state_rows(state: &RedactDialogState) -> Vec<Row> {
    let mut rows = match state.panel {
        Panel::Properties { mark } => properties_rows(&state.properties, mark.is_some()),
        Panel::Find => find_rows(&state.find),
        Panel::Pages => vec![
            Row::Field(RedactField::Pages),
            line(
                "redact-pages-hint",
                format!(
                    "Page numbers and ranges from 1 to {}, as 1-3, 5.",
                    state.page_count
                ),
            ),
            button("redact-mark-pages", "Mark Pages", RedactAction::MarkPages),
        ],
        Panel::Apply { sanitize } => apply_rows(sanitize, state.hidden_information),
    };
    if let Some(error) = &state.error {
        rows.push(Row::Line("redact-error".into(), error.clone(), Role::Alert));
    }
    rows
}

pub(in crate::shell) fn accessible(state: &RedactDialogState, cx: &gpui::App) -> Vec<Element> {
    state_rows(state)
        .into_iter()
        .filter_map(|row| match row {
            Row::Field(field) => state.text_field(field).map(|input| {
                input
                    .read(cx)
                    .accessible(field.label(), TextField::Redact(field))
            }),
            Row::Control {
                id,
                role,
                label,
                checked,
                action,
            } => {
                let mut element =
                    Element::new(id, role, label).with_activation(Activation::Redact(action));
                if let Some(checked) = checked {
                    element = element.with_state(A11yState::toggled(checked));
                }
                Some(element)
            }
            Row::Line(id, text, role) => Some(Element::new(id, role, text)),
        })
        .collect()
}

pub(in crate::shell) fn render(
    state: &RedactDialogState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
    for row in state_rows(state) {
        list = list.child(match row {
            Row::Field(field) => {
                let input = state.text_field(field).expect("the panel's field").clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(150.0)).child(field.label()))
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
                        frame.run_activation(Activation::Redact(action), window, cx);
                    }))
                    .child(match checked {
                        Some(true) => format!("✓ {label}"),
                        Some(false) => format!("○ {label}"),
                        None => label,
                    })
                    .into_any_element()
            }
            Row::Line(id, text, role) => div()
                .id(id)
                .when(role == Role::Alert, |line| {
                    line.text_color(theme.error_text)
                })
                .child(text)
                .into_any_element(),
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_core::redactions::RedactionLook;
    use onionskin_redact::codes::built_in;
    use onionskin_redact::find::{Found, Pattern};

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Field(field) => format!("[{}]", field.label()),
                Row::Control { label, .. } | Row::Line(_, label, _) => label.clone(),
            })
            .collect()
    }

    #[test]
    fn properties_show_the_overlay_only_when_it_is_on() {
        let (mut form, _, _) = PropertiesForm::of(&RedactionLook::default(), built_in());
        let plain = labels(&properties_rows(&form, false));
        assert_eq!(plain[..3], ["Fill: Black", "Outline: Red", "Overlay Text"]);
        assert!(!plain.contains(&"[Overlay text]".to_owned()));
        assert!(plain.contains(&"Code Set: U.S. FOIA".to_owned()));
        assert!(plain.contains(&"Use (b)(1) as Overlay Text".to_owned()));
        assert!(
            !plain.contains(&"Rename Code Set".to_owned()),
            "a built-in set is fixed"
        );
        assert_eq!(plain.last().map(String::as_str), Some("Save"));

        form.apply(RedactAction::Overlay);
        let overlaid = labels(&properties_rows(&form, true));
        for expected in [
            "[Overlay text]",
            "[Font size (empty fits)]",
            "Text Colour: White",
            "Alignment: Centre",
            "Repeat Overlay Text",
            "Remove Mark",
        ] {
            assert!(
                overlaid.contains(&expected.to_owned()),
                "{expected}: {overlaid:?}"
            );
        }
        form.sets[0].built_in = false;
        let own = labels(&properties_rows(&form, false));
        assert!(own.contains(&"Remove Code Set".to_owned()));
        form.sets.clear();
        assert!(labels(&properties_rows(&form, false)).contains(&"Code Set: none".to_owned()));
    }

    #[test]
    fn find_lists_what_it_found_and_counts_the_rest() {
        let mut form = FindForm::default();
        assert_eq!(
            labels(&find_rows(&form)),
            [
                "Words or phrase",
                "Patterns",
                "[Words or phrase]",
                "Whole words only",
                "Case sensitive",
                "Find"
            ]
        );
        form.apply(RedactAction::Patterns(true));
        assert!(labels(&find_rows(&form)).contains(&"Pattern: Phone Numbers".to_owned()));
        form.show(Vec::new());
        assert!(labels(&find_rows(&form)).contains(&"Nothing found.".to_owned()));
        let hit = |page| Found {
            page,
            text: "555-1234".to_owned(),
            quads: Vec::new(),
        };
        form.show((0..LISTED + 2).map(hit).collect());
        let many = labels(&find_rows(&form));
        assert!(many.contains(&format!("{} found.", LISTED + 2)));
        assert!(many.contains(&"Page 1: 555-1234".to_owned()));
        assert!(many.contains(&"and 2 more, all checked".to_owned()));
        assert_eq!(
            many.last().map(String::as_str),
            Some("Mark Checked for Redaction")
        );
        form.show(vec![hit(0)]);
        assert!(labels(&find_rows(&form)).contains(&"1 found.".to_owned()));
        assert_eq!(Pattern::ALL.len(), 5);
    }

    #[test]
    fn apply_says_what_goes_and_offers_hidden_information() {
        let apply = labels(&apply_rows(false, true));
        assert!(apply.contains(&"Also remove hidden information".to_owned()));
        assert_eq!(apply.last().map(String::as_str), Some("Apply and Save As…"));
        let sanitize = labels(&apply_rows(true, false));
        assert!(!sanitize.contains(&"Also remove hidden information".to_owned()));
        assert_eq!(
            sanitize.last().map(String::as_str),
            Some("Remove and Save As…")
        );
    }
}
