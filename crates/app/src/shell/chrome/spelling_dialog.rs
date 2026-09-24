//! Edit > Check Spelling: each word the dictionary does not know, in the
//! comments' text and the text fields' values, one at a time.
//!
//! The dialog shows the word, the passage it is in, a Change To field
//! holding the likeliest suggestion, and the rest of the suggestions to
//! pick from. Ignore goes on to the next word; Ignore All passes over the
//! word everywhere until the dialog closes; Add to Dictionary passes over
//! it for good; Change puts Change To in its place. The checking is
//! `onionskin-spelling`'s.

use std::collections::{BTreeSet, VecDeque};

use accesskit::Role;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_spelling::passages::{Misspelling, Passage};
use onionskin_spelling::Checker;

use super::accessible::{Activation, Element, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};

/// The Change To field's id.
pub(in crate::shell) const CHANGE_TO_ID: &str = "spelling-change-to";

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SpellingAction {
    Ignore,
    IgnoreAll,
    AddToDictionary,
    Change,
    /// Put suggestion `n` in Change To.
    Suggestion(usize),
}

const BUTTONS: [(&str, &str, SpellingAction); 4] = [
    ("spelling-ignore", "Ignore", SpellingAction::Ignore),
    (
        "spelling-ignore-all",
        "Ignore All",
        SpellingAction::IgnoreAll,
    ),
    (
        "spelling-add",
        "Add to Dictionary",
        SpellingAction::AddToDictionary,
    ),
    ("spelling-change", "Change", SpellingAction::Change),
];

pub(in crate::shell) struct SpellingState {
    pub(in crate::shell) checker: Checker,
    pub(in crate::shell) passages: Vec<Passage>,
    /// The words still to show, the current one first.
    pub(in crate::shell) queue: VecDeque<Misspelling>,
    /// Ignore All's words, for as long as the dialog is open.
    pub(in crate::shell) ignored: BTreeSet<String>,
    pub(in crate::shell) suggestions: Vec<String>,
    pub(in crate::shell) change_to: Entity<SearchInput>,
    pub(in crate::shell) changed: usize,
    pub(in crate::shell) error: Option<String>,
}

impl SpellingState {
    /// The word on show, and the passage it is in.
    pub(in crate::shell) fn current(&self) -> Option<(&Passage, &Misspelling)> {
        let word = self.queue.front()?;
        Some((self.passages.get(word.passage)?, word))
    }

    /// The passage with the word marked, for the dialog and a screen
    /// reader: `Comment on page 2: «Teh» meeting is at noon`.
    pub(in crate::shell) fn context(&self) -> Option<String> {
        let (passage, word) = self.current()?;
        let text = &passage.text;
        Some(format!(
            "{}: {}«{}»{}",
            passage.label,
            &text[..word.range.start],
            word.word,
            &text[word.range.end..]
        ))
    }

    /// What the dialog says when there is nothing left to show.
    pub(in crate::shell) fn done_label(&self) -> String {
        match self.changed {
            0 => "Check Spelling is done.".to_owned(),
            1 => "Check Spelling is done: 1 word changed.".to_owned(),
            count => format!("Check Spelling is done: {count} words changed."),
        }
    }
}

pub(in crate::shell) fn accessible(state: &SpellingState, cx: &gpui::App) -> Vec<Element> {
    let mut body = Vec::new();
    match state.current() {
        None => body.push(Element::new(
            "spelling-done",
            Role::Label,
            state.done_label(),
        )),
        Some((_, word)) => {
            body.push(
                Element::new("spelling-word", Role::Label, "Not in dictionary")
                    .with_value(word.word.clone()),
            );
            body.push(Element::new(
                "spelling-context",
                Role::Label,
                state.context().unwrap_or_default(),
            ));
            body.push(
                state
                    .change_to
                    .read(cx)
                    .accessible("Change To", TextField::SpellingChangeTo),
            );
            for (index, suggestion) in state.suggestions.iter().enumerate() {
                body.push(
                    Element::new(
                        ("spelling-suggestion", index),
                        Role::ListBoxOption,
                        suggestion.clone(),
                    )
                    .with_activation(Activation::Spelling(SpellingAction::Suggestion(index))),
                );
            }
            for (id, label, action) in BUTTONS {
                body.push(
                    Element::new(id, Role::Button, label)
                        .with_activation(Activation::Spelling(action)),
                );
            }
        }
    }
    if let Some(error) = &state.error {
        body.push(Element::new("spelling-error", Role::Alert, error.clone()));
    }
    body
}

pub(in crate::shell) fn render(
    state: &SpellingState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut body = div().flex().flex_col().gap_2().w(px(420.0));
    match state.current() {
        None => body = body.child(div().id("spelling-done").child(state.done_label())),
        Some((_, word)) => {
            body = body
                .child(
                    div()
                        .id("spelling-word")
                        .child(format!("Not in dictionary: {}", word.word)),
                )
                .child(
                    div()
                        .id("spelling-context")
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(state.context().unwrap_or_default()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child("Change To")
                        .child(div().w(px(220.0)).child(state.change_to.clone())),
                );
            let mut list = div().flex().flex_col();
            for (index, suggestion) in state.suggestions.iter().enumerate() {
                list = list.child(
                    div()
                        .id(("spelling-suggestion", index))
                        .px_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |row| row.bg(theme.subtle_hover))
                        .on_click(cx.listener(move |frame, _event, window, cx| {
                            frame.run_activation(
                                Activation::Spelling(SpellingAction::Suggestion(index)),
                                window,
                                cx,
                            );
                        }))
                        .child(suggestion.clone()),
                );
            }
            let mut buttons = div().flex().gap_2();
            for (id, label, action) in BUTTONS {
                buttons = buttons.child(button(
                    id,
                    label,
                    true,
                    theme,
                    focused,
                    cx,
                    Activation::Spelling(action),
                ));
            }
            body = body.child(list).child(buttons);
        }
    }
    if let Some(error) = &state.error {
        body = body.child(
            div()
                .id("spelling-error")
                .text_color(theme.error_text)
                .child(error.clone()),
        );
    }
    body
}
