//! Stamps: every stamp the stamp tool can place, and the user's own to
//! create and delete.
//!
//! The list is the tool's [`ToolPlugin::choices`], asked each time the
//! dialog draws, so it cannot disagree with what a click would place. Built-in
//! stamps have no Delete: they are compiled into the plugin, and nothing here
//! can remove one. A custom stamp's Delete removes its file from the library.
//!
//! [`ToolPlugin::choices`]: onionskin_plugin_api::ToolPlugin::choices

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_plugin_api::ToolChoice;

use super::accessible::{Activation, Element, Rects, Surface};
use super::combine_dialog::button;
use super::{ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// What a control in the dialog does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) enum StampAction {
    /// Make this the stamp the tool places, and switch to the tool.
    Choose(String),
    /// Remove this custom stamp from the library.
    Delete(String),
    /// Make a custom stamp from a PDF page or an image the user picks.
    CreateCustom,
    /// Make a stamp of the image on the clipboard, and choose it.
    PasteClipboard,
}

/// What a custom stamp's id starts with, as `tools-comment` writes it.
const CUSTOM: &str = "custom:";

/// The dialog's state in the frame: what went wrong last, if anything.
#[derive(Debug, Default)]
pub(in crate::shell) struct StampsDialogState {
    pub(in crate::shell) error: Option<String>,
}

/// One row of the dialog, in the order it draws and is described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) enum Row {
    Category(String),
    Stamp { choice: ToolChoice, chosen: bool },
    Delete(ToolChoice),
    Create,
    Paste,
    Error(String),
}

/// The rows for these choices: each category's heading, its stamps, and a
/// Delete after each custom one; then the two ways to make a stamp.
pub(in crate::shell) fn rows(
    choices: &[ToolChoice],
    chosen: Option<&str>,
    error: Option<&str>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut category: Option<&str> = None;
    for choice in choices {
        if category != Some(choice.category.as_str()) {
            category = Some(&choice.category);
            rows.push(Row::Category(choice.category.clone()));
        }
        rows.push(Row::Stamp {
            choice: choice.clone(),
            chosen: chosen == Some(choice.id.as_str()),
        });
        if choice.id.starts_with(CUSTOM) {
            rows.push(Row::Delete(choice.clone()));
        }
    }
    rows.extend([Row::Create, Row::Paste]);
    if let Some(error) = error {
        rows.push(Row::Error(error.to_owned()));
    }
    rows
}

fn element(index: usize, row: &Row) -> Element {
    match row {
        Row::Category(name) => Element::new(("stamps-category", index), Role::Label, name.clone()),
        Row::Stamp { choice, chosen } => {
            Element::new(("stamps-stamp", index), Role::Button, choice.label.clone())
                .with_state(A11yState::selected(*chosen))
                .with_activation(Activation::Stamps(StampAction::Choose(choice.id.clone())))
        }
        Row::Delete(choice) => Element::new(
            ("stamps-delete", index),
            Role::Button,
            format!("Delete {}", choice.label),
        )
        .with_activation(Activation::Stamps(StampAction::Delete(choice.id.clone()))),
        Row::Create => Element::new("stamps-create", Role::Button, "Create Custom Stamp…")
            .with_activation(Activation::Stamps(StampAction::CreateCustom)),
        Row::Paste => Element::new(
            "stamps-paste",
            Role::Button,
            "Paste Clipboard Image as Stamp",
        )
        .with_activation(Activation::Stamps(StampAction::PasteClipboard)),
        Row::Error(error) => Element::new("stamps-error", Role::Alert, error.clone()),
    }
}

pub(in crate::shell) fn accessible(rows: &[Row], rects: &Rects) -> Vec<Element> {
    let mut body: Vec<Element> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| element(index, row))
        .collect();
    for (row, bounds) in body.iter_mut().zip(rects.of(Surface::StampsDialog)) {
        row.bounds = Some(bounds);
    }
    body
}

pub(in crate::shell) fn render(
    rows: &[Row],
    rects: Rects,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::StampsDialog, &bounds, window);
        })
        .flex()
        .flex_col()
        .gap_1();
    for (index, row) in rows.iter().enumerate() {
        let described = element(index, row);
        body = body.child(match row {
            Row::Category(name) => div()
                .id(described.key)
                .mt_2()
                .text_xs()
                .text_color(theme.secondary_text)
                .child(name.clone())
                .into_any_element(),
            Row::Stamp { choice, chosen } => {
                let activation = described.activation.clone().expect("a stamp activates");
                let chosen = *chosen;
                div()
                    .id(described.key)
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(chosen, |row| row.bg(theme.selected))
                    .hover(move |row| row.bg(theme.subtle_hover))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(activation.clone(), window, cx);
                    }))
                    .child(choice.label.clone())
                    .into_any_element()
            }
            Row::Delete(_) => {
                let activation = described.activation.clone().expect("a delete activates");
                div()
                    .id(described.key)
                    .pl_6()
                    .text_xs()
                    .cursor_pointer()
                    .text_color(theme.muted_text)
                    .hover(move |row| row.bg(theme.subtle_hover))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(activation.clone(), window, cx);
                    }))
                    .child(described.label.clone())
                    .into_any_element()
            }
            Row::Create => button(
                "stamps-create",
                "Create Custom Stamp…",
                true,
                theme,
                focused,
                cx,
                Activation::Stamps(StampAction::CreateCustom),
            ),
            Row::Paste => button(
                "stamps-paste",
                "Paste Clipboard Image as Stamp",
                true,
                theme,
                focused,
                cx,
                Activation::Stamps(StampAction::PasteClipboard),
            ),
            Row::Error(error) => div()
                .id("stamps-error")
                .text_color(theme.error_text)
                .child(error.clone())
                .into_any_element(),
        });
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, label: &str, category: &str) -> ToolChoice {
        ToolChoice {
            id: id.to_owned(),
            label: label.to_owned(),
            category: category.to_owned(),
        }
    }

    #[test]
    fn categories_head_their_stamps_and_only_a_custom_stamp_can_be_deleted() {
        let choices = [
            choice("business-approved", "Approved", "Standard Business"),
            choice("business-draft", "Draft", "Standard Business"),
            choice("custom:Mine/Receipt", "Receipt", "Mine"),
        ];
        let rows = rows(&choices, Some("business-draft"), Some("it went wrong"));
        assert_eq!(
            rows,
            [
                Row::Category("Standard Business".into()),
                Row::Stamp {
                    choice: choices[0].clone(),
                    chosen: false
                },
                Row::Stamp {
                    choice: choices[1].clone(),
                    chosen: true
                },
                Row::Category("Mine".into()),
                Row::Stamp {
                    choice: choices[2].clone(),
                    chosen: false
                },
                Row::Delete(choices[2].clone()),
                Row::Create,
                Row::Paste,
                Row::Error("it went wrong".into()),
            ]
        );
    }

    #[test]
    fn every_row_but_a_heading_and_an_error_does_something() {
        let rows = rows(&[choice("custom:A/b", "b", "A")], None, Some("e"));
        let described: Vec<Element> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| element(index, row))
            .collect();
        for (row, node) in rows.iter().zip(&described) {
            let acts = node.activation.is_some();
            assert_eq!(
                acts,
                !matches!(row, Row::Category(_) | Row::Error(_)),
                "{row:?}"
            );
        }
        assert_eq!(described[2].label, "Delete b");
    }
}
