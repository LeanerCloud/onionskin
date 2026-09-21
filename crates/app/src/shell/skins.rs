//! The skins panel: the file's own history, one skin per generation.
//!
//! Every save appends a section and changes nothing already in the file,
//! so a PDF Onionskin has saved is a stack of versions. This lists them,
//! newest at the top and the original at the bottom, says which ones
//! Onionskin wrote and which another program appended, and offers the two
//! things a version is for:
//!
//! - **Open a Copy of This Version…** writes the file as it was then to a
//!   new file and opens it. The open document is not touched: previewing
//!   an old version never mutates the current one.
//! - **Roll Back to This Version…** truncates the file to the end of that
//!   version, after a confirmation that says what will be discarded. It is
//!   the one way the history loses anything, and only newer versions can
//!   go, so it is always a truncation.
//!
//! It is not a navigation pane: those view the document's content, and this
//! is the file's history. It lives in the side panel, opened from the rail.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::GenerationDetail;

use super::chrome::accessible::{Activation, Element};
use super::chrome::properties_dialog::{date_label, size_label};
use super::chrome::{ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// What a control in the panel or its confirmation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SkinsAction {
    /// Show the panel, or put it away.
    Toggle,
    /// Choose a version by its generation index.
    Select(usize),
    OpenCopy,
    /// Ask to roll back to the chosen version.
    RollBack,
    ConfirmRollBack,
    CancelRollBack,
}

/// The panel, open on one document.
pub(in crate::shell) struct SkinsState {
    pub(in crate::shell) canvas: gpui::EntityId,
    /// The document's byte generation when the rows were read. The rows
    /// open the file once per version, so they are read again when this
    /// moves, never per frame.
    pub(in crate::shell) read_at: u64,
    /// Oldest first, as `core` hands them out.
    pub(in crate::shell) rows: Vec<GenerationDetail>,
    /// The chosen version's generation index.
    pub(in crate::shell) selected: usize,
    /// Why Roll Back cannot run now: unsaved edits.
    pub(in crate::shell) dirty: bool,
    pub(in crate::shell) error: Option<String>,
}

impl SkinsState {
    pub(in crate::shell) fn new(
        canvas: gpui::EntityId,
        read_at: u64,
        rows: Vec<GenerationDetail>,
        dirty: bool,
    ) -> Self {
        let selected = rows.len().saturating_sub(1);
        SkinsState {
            canvas,
            read_at,
            rows,
            selected,
            dirty,
            error: None,
        }
    }

    /// The rows read again after the file changed. The choice stays on the
    /// same version while it exists, and moves to the newest when it does
    /// not.
    pub(in crate::shell) fn reread(&mut self, read_at: u64, rows: Vec<GenerationDetail>) {
        self.read_at = read_at;
        if self.selected >= rows.len() {
            self.selected = rows.len().saturating_sub(1);
        }
        self.rows = rows;
    }

    /// Why Roll Back to the chosen version cannot run, if it cannot.
    pub(in crate::shell) fn roll_back_refusal(&self) -> Option<&'static str> {
        if self.selected + 1 >= self.rows.len() {
            Some("This is the current version")
        } else if self.dirty {
            Some("Save or undo your changes first")
        } else {
            None
        }
    }

    /// What the confirmation says will go.
    pub(in crate::shell) fn roll_back_question(&self) -> String {
        let newer = &self.rows[(self.selected + 1).min(self.rows.len())..];
        let bytes: u64 = newer.iter().map(|row| row.generation.len()).sum();
        let count = match newer.len() {
            1 => "1 newer version".to_owned(),
            many => format!("{many} newer versions"),
        };
        format!(
            "Roll back to {}? {count} ({}) will be removed from the file. This cannot be undone.",
            version_name(self.selected),
            size_label(bytes),
        )
    }
}

fn version_name(index: usize) -> String {
    if index == 0 {
        "the original".to_owned()
    } else {
        format!("version {index}")
    }
}

/// One version, as the panel lists it and a screen reader reads it.
pub(in crate::shell) fn row_label(row: &GenerationDetail, newest: bool) -> String {
    let index = row.generation.index;
    let name = if index == 0 {
        "Original".to_owned()
    } else {
        format!("Version {index}")
    };
    let who = match (&row.producer, row.ours) {
        (Some(producer), true) => format!("saved by {producer}"),
        (None, true) => "saved by Onionskin".to_owned(),
        (Some(producer), false) if index > 0 => {
            format!("added by {producer}, not by Onionskin")
        }
        (None, false) if index > 0 => "added by another program, not by Onionskin".to_owned(),
        (Some(producer), false) => format!("made by {producer}"),
        (None, false) => "maker unknown".to_owned(),
    };
    let mut parts = vec![name, who];
    if let Some(date) = &row.date {
        parts.push(date_label(date));
    }
    parts.push(size_label(row.generation.len()));
    let label = parts.join(", ");
    if newest {
        format!("{label} (current)")
    } else {
        label
    }
}

fn activation(action: SkinsAction) -> Activation {
    Activation::Skins(action)
}

/// The panel, for a screen reader: the versions newest first, then the
/// two actions with why each cannot run when it cannot.
pub(in crate::shell) fn accessible(state: &SkinsState) -> Element {
    let newest = state.rows.len().saturating_sub(1);
    let rows = state
        .rows
        .iter()
        .rev()
        .map(|row| {
            let index = row.generation.index;
            Element::new(
                ("skins-version", index),
                Role::RadioButton,
                row_label(row, index == newest),
            )
            .with_state(A11yState::selected(state.selected == index))
            .with_activation(activation(SkinsAction::Select(index)))
        })
        .collect();
    let mut roll_back = Element::new(
        "skins-roll-back",
        Role::Button,
        "Roll Back to This Version…",
    )
    .with_state(A11yState::enabled(state.roll_back_refusal().is_none()))
    .with_activation(activation(SkinsAction::RollBack));
    if let Some(reason) = state.roll_back_refusal() {
        roll_back = roll_back.with_description(reason);
    }
    let mut panel = Element::new("skins", Role::Group, "Skins")
        .child(Element::new("skins-versions", Role::RadioGroup, "Versions").with_children(rows))
        .child(
            Element::new(
                "skins-open-copy",
                Role::Button,
                "Open a Copy of This Version…",
            )
            .with_activation(activation(SkinsAction::OpenCopy)),
        )
        .child(roll_back);
    if let Some(error) = &state.error {
        panel = panel.child(Element::new("skins-error", Role::Alert, error.clone()));
    }
    panel
}

fn button(
    id: &'static str,
    label: &'static str,
    action: SkinsAction,
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let base = div().id(id).px_2().py_1().rounded_sm().text_xs();
    match refusal {
        Some(reason) => base
            .text_color(theme.disabled_text)
            .child(format!("{label} ({reason})"))
            .into_any_element(),
        None => base
            .border_1()
            .border_color(theme.hover)
            .cursor_pointer()
            .hover(move |button| button.bg(theme.subtle_hover))
            .on_click(cx.listener(move |frame, _event, window, cx| {
                frame.run_activation(activation(action), window, cx);
            }))
            .child(label)
            .into_any_element(),
    }
}

/// The panel, drawn.
pub(in crate::shell) fn render(
    state: &SkinsState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let newest = state.rows.len().saturating_sub(1);
    let mut list = div().flex().flex_col().gap_1();
    for row in state.rows.iter().rev() {
        let index = row.generation.index;
        let chosen = state.selected == index;
        list = list.child(
            div()
                .id(("skins-version", index))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .when(chosen, |row| row.bg(theme.selected))
                .when(!row.ours && index > 0, |row| {
                    row.text_color(theme.muted_text)
                })
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(activation(SkinsAction::Select(index)), window, cx);
                }))
                .child(row_label(row, index == newest)),
        );
    }
    div()
        .id("skins")
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_sm().child("Skins"))
        .child(list)
        .child(button(
            "skins-open-copy",
            "Open a Copy of This Version…",
            SkinsAction::OpenCopy,
            None,
            theme,
            cx,
        ))
        .child(button(
            "skins-roll-back",
            "Roll Back to This Version…",
            SkinsAction::RollBack,
            state.roll_back_refusal(),
            theme,
            cx,
        ))
        .when_some(state.error.clone(), |panel, error| {
            panel.child(div().text_xs().text_color(theme.error_text).child(error))
        })
        .into_any_element()
}

/// The roll back confirmation, for a screen reader.
pub(in crate::shell) fn accessible_confirm(state: &SkinsState) -> Vec<Element> {
    vec![
        Element::new("skins-question", Role::Label, state.roll_back_question()),
        Element::new("skins-confirm", Role::Button, "Roll Back")
            .with_activation(activation(SkinsAction::ConfirmRollBack)),
        Element::new("skins-cancel", Role::Button, "Cancel")
            .with_activation(activation(SkinsAction::CancelRollBack)),
    ]
}

/// The roll back confirmation, drawn.
pub(in crate::shell) fn render_confirm(
    state: &SkinsState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    use super::chrome::combine_dialog::button;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(state.roll_back_question())
        .child(
            div()
                .flex()
                .gap_2()
                .child(button(
                    "skins-confirm",
                    "Roll Back",
                    true,
                    theme,
                    focused,
                    cx,
                    activation(SkinsAction::ConfirmRollBack),
                ))
                .child(button(
                    "skins-cancel",
                    "Cancel",
                    true,
                    theme,
                    focused,
                    cx,
                    activation(SkinsAction::CancelRollBack),
                )),
        )
}

#[cfg(test)]
mod tests {
    use onionskin_core::Generation;

    use super::*;

    fn row(
        index: usize,
        start: u64,
        end: u64,
        ours: bool,
        producer: Option<&str>,
    ) -> GenerationDetail {
        GenerationDetail {
            generation: Generation { index, start, end },
            ours,
            producer: producer.map(str::to_owned),
            date: (index > 0).then(|| "D:20260921201000Z00'00'".to_owned()),
        }
    }

    fn state(dirty: bool) -> SkinsState {
        SkinsState::new(
            gpui::EntityId::from(1u64),
            0,
            vec![
                row(0, 0, 1000, false, Some("Word")),
                row(1, 1000, 1500, true, Some("Onionskin 0.1.0")),
                row(2, 1500, 3548, false, Some("Acrobat")),
            ],
            dirty,
        )
    }

    #[test]
    fn each_version_says_who_wrote_it_when_and_how_big() {
        let state = state(false);
        assert_eq!(
            row_label(&state.rows[0], false),
            "Original, made by Word, 1000 bytes"
        );
        assert_eq!(
            row_label(&state.rows[1], false),
            "Version 1, saved by Onionskin 0.1.0, 2026-09-21 20:10:00 UTC, 500 bytes"
        );
        assert!(row_label(&state.rows[2], true)
            .starts_with("Version 2, added by Acrobat, not by Onionskin"));
        assert!(row_label(&state.rows[2], true).ends_with("(current)"));
    }

    #[test]
    fn the_panel_lists_newest_first_with_the_newest_chosen() {
        let described = accessible(&state(false));
        let versions = described.find(&"skins-versions".into()).expect("listed");
        let labels: Vec<_> = versions
            .children
            .iter()
            .map(|row| row.label.split(',').next().unwrap().to_owned())
            .collect();
        assert_eq!(labels, ["Version 2", "Version 1", "Original"]);
        assert_eq!(versions.children[0].state.selected, Some(true));
    }

    #[test]
    fn roll_back_is_refused_on_the_current_version_and_with_unsaved_edits() {
        let mut current = state(false);
        assert_eq!(
            current.roll_back_refusal(),
            Some("This is the current version")
        );
        current.selected = 0;
        assert_eq!(current.roll_back_refusal(), None);
        let mut dirty = state(true);
        dirty.selected = 0;
        assert_eq!(
            dirty.roll_back_refusal(),
            Some("Save or undo your changes first")
        );
        let described = accessible(&dirty);
        let button = described.find(&"skins-roll-back".into()).unwrap();
        assert!(button.state.disabled);
        assert_eq!(
            button.description.as_deref(),
            Some("Save or undo your changes first")
        );
    }

    #[test]
    fn the_confirmation_says_what_goes() {
        let mut state = state(false);
        state.selected = 0;
        assert!(state
            .roll_back_question()
            .starts_with("Roll back to the original? 2 newer versions ("));
        state.selected = 1;
        assert!(state
            .roll_back_question()
            .contains("version 1? 1 newer version ("));
        assert!(state
            .roll_back_question()
            .ends_with("This cannot be undone."));
    }

    #[test]
    fn a_reread_keeps_the_choice_while_the_version_exists() {
        let mut state = state(false);
        state.selected = 1;
        let rows = state.rows.clone();
        state.reread(5, rows[..2].to_vec());
        assert_eq!(state.selected, 1);
        state.reread(6, rows[..1].to_vec());
        assert_eq!(state.selected, 0, "moved to the newest that is left");
        assert_eq!(state.read_at, 6);
    }
}
