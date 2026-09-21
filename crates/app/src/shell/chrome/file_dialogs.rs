//! The two questions P18 asks: whether to save before closing, and whether
//! to recover edits a crash left behind.
//!
//! Each dialog names the documents it is about by their canvas identity, not
//! by a tab index: the tabs can change while it is open, and an answer given
//! for one document must not land on another (B4.2).

use accesskit::Role;
use gpui::{div, Context, EntityId, IntoElement, ParentElement as _, Styled as _};

use super::accessible::{Activation, Element};
use super::combine_dialog::button;
use super::{ShellFrame, ThemeTokens};

/// What a control in either dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FileAction {
    /// Save every unsaved document the close covers, then close.
    SaveAndClose,
    /// Close without saving.
    DiscardAndClose,
    /// Keep the documents open.
    CancelClose,
    /// Replay the recovered edits onto the document.
    Recover,
    /// Throw the recovery away.
    DiscardRecovery,
    /// Choose where the reduced copy goes, and write it.
    ReduceFileSize,
    /// Put Reduce File Size away.
    CancelReduce,
}

/// Which close the user asked for, remembered while the dialog asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PendingClose {
    /// One tab.
    Tab(EntityId),
    /// Every tab but this one.
    Others(EntityId),
    All,
}

pub(in crate::shell) struct UnsavedState {
    pub(in crate::shell) close: PendingClose,
    /// The documents with unsaved changes the close would lose.
    pub(in crate::shell) unsaved: Vec<(EntityId, String)>,
    pub(in crate::shell) error: Option<String>,
}

pub(in crate::shell) struct RecoverState {
    pub(in crate::shell) canvas: EntityId,
    pub(in crate::shell) title: String,
    pub(in crate::shell) offer: crate::shell::canvas::RecoveryOffer,
}

fn question(unsaved: &UnsavedState) -> String {
    match unsaved.unsaved.as_slice() {
        [(_, title)] => format!("Save changes to {title} before closing?"),
        many => format!(
            "{} documents have unsaved changes. Save them before closing?",
            many.len()
        ),
    }
}

fn recover_question(state: &RecoverState) -> String {
    format!(
        "{} has unsaved changes from a session that did not close properly. Recover them?",
        state.title
    )
}

const UNSAVED_BUTTONS: [(&str, &str, FileAction); 3] = [
    ("unsaved-save", "Save", FileAction::SaveAndClose),
    ("unsaved-discard", "Don't Save", FileAction::DiscardAndClose),
    ("unsaved-cancel", "Cancel", FileAction::CancelClose),
];

const RECOVER_BUTTONS: [(&str, &str, FileAction); 2] = [
    ("recover-accept", "Recover", FileAction::Recover),
    ("recover-discard", "Discard", FileAction::DiscardRecovery),
];

/// What Reduce File Size says before it does anything. The one M3 path
/// that rewrites a file, so it says in words that history is discarded.
pub(in crate::shell) const REDUCE_TEXT: &str = "Reduce File Size writes a smaller copy as a new \
file: images above 150 pixels per inch are downsampled and saved as JPEG, and anything nothing \
refers to is left out. The copy keeps no editing history: every earlier version of the document \
is discarded from it and cannot be restored. This document is not changed.";

const REDUCE_BUTTONS: [(&str, &str, FileAction); 2] = [
    (
        "reduce-save",
        "Save a Reduced Copy…",
        FileAction::ReduceFileSize,
    ),
    ("reduce-cancel", "Cancel", FileAction::CancelReduce),
];

pub(in crate::shell) fn accessible_reduce() -> Vec<Element> {
    described(REDUCE_TEXT.to_owned(), None, &REDUCE_BUTTONS)
}

pub(in crate::shell) fn render_reduce(
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    drawn(
        REDUCE_TEXT.to_owned(),
        None,
        &REDUCE_BUTTONS,
        focused,
        theme,
        cx,
    )
}

fn described(
    text: String,
    error: Option<&String>,
    buttons: &[(&'static str, &'static str, FileAction)],
) -> Vec<Element> {
    let mut body = vec![Element::new("file-question", Role::Label, text)];
    if let Some(error) = error {
        body.push(Element::new("file-error", Role::Alert, error.clone()));
    }
    body.extend(buttons.iter().map(|(id, label, action)| {
        Element::new(*id, Role::Button, *label).with_activation(Activation::File(*action))
    }));
    body
}

fn drawn(
    text: String,
    error: Option<&String>,
    buttons: &[(&'static str, &'static str, FileAction)],
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut body = div().flex().flex_col().gap_3().child(text);
    if let Some(error) = error {
        body = body.child(div().text_color(theme.error_text).child(error.clone()));
    }
    let mut row = div().flex().gap_2();
    for (id, label, action) in buttons {
        row = row.child(button(
            id,
            label,
            true,
            theme,
            focused,
            cx,
            Activation::File(*action),
        ));
    }
    body.child(row)
}

pub(in crate::shell) fn accessible_unsaved(state: &UnsavedState) -> Vec<Element> {
    described(question(state), state.error.as_ref(), &UNSAVED_BUTTONS)
}

pub(in crate::shell) fn render_unsaved(
    state: &UnsavedState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    drawn(
        question(state),
        state.error.as_ref(),
        &UNSAVED_BUTTONS,
        focused,
        theme,
        cx,
    )
}

pub(in crate::shell) fn accessible_recover(state: &RecoverState) -> Vec<Element> {
    described(recover_question(state), None, &RECOVER_BUTTONS)
}

pub(in crate::shell) fn render_recover(
    state: &RecoverState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    drawn(
        recover_question(state),
        None,
        &RECOVER_BUTTONS,
        focused,
        theme,
        cx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_one_document_and_counts_several() {
        let id = |n: u64| EntityId::from(n);
        let one = UnsavedState {
            close: PendingClose::All,
            unsaved: vec![(id(1), "a.pdf".into())],
            error: None,
        };
        assert_eq!(question(&one), "Save changes to a.pdf before closing?");
        let two = UnsavedState {
            unsaved: vec![(id(1), "a.pdf".into()), (id(2), "b.pdf".into())],
            ..one
        };
        assert!(question(&two).starts_with("2 documents"));
    }

    #[test]
    fn every_button_runs_its_action() {
        let state = UnsavedState {
            close: PendingClose::All,
            unsaved: Vec::new(),
            error: Some("disk full".into()),
        };
        let described = accessible_unsaved(&state);
        assert_eq!(described[1].role, Role::Alert);
        let actions: Vec<_> = described[2..]
            .iter()
            .map(|button| button.activation.clone())
            .collect();
        assert_eq!(
            actions,
            UNSAVED_BUTTONS.map(|(_, _, action)| Some(Activation::File(action)))
        );
    }
}
