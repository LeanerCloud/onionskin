//! The Comments pane: every comment in the document as a list to read,
//! reply to, set a status on, sort and filter, the way Acrobat's Comments
//! list works.
//!
//! Comments are typed on the page when they are placed (`inline_text`).
//! The pane is where they are read afterwards and where an existing one's
//! text is changed. What it lists is worked out in `model`; what a click does
//! is in `actions`; drawing and describing are in `view`.

mod actions;
mod model;
mod view;

use gpui::Entity;
use onionskin_core::ObjRef;

use super::super::chrome::SearchInput;

pub(in crate::shell) use self::actions::{install_keybindings, run, start_draft};
pub(in crate::shell) use self::model::{CommentFilter, CommentSort, FilterField};
pub(super) use self::view::{accessible, render};

/// The element id the draft field publishes.
pub(in crate::shell) const DRAFT_ID: &str = "comment-draft";

/// Everything the pane can be asked to do. `Copy`, like every pane action:
/// what is typed is read out of the draft field when it is saved, not
/// carried in the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum CommentAction {
    /// Choose a comment, and go to its page.
    Select(ObjRef),
    /// Step the list's order to the next key.
    CycleSort,
    /// Step one filter to its next value, and after the last back to all.
    CycleFilter(FilterField),
    /// Open the field on the chosen comment's own text.
    Edit,
    /// Open the field for a reply to the chosen comment.
    Reply,
    /// Write what the field holds, as one undoable step.
    SaveDraft,
    /// Put the field away and write nothing.
    CancelDraft,
    /// Set the chosen comment's review status: one of
    /// `onionskin_core::review::REVIEW_STATES`.
    SetStatus(&'static str),
    /// Tick or untick the chosen comment's checkmark.
    ToggleMark,
    /// Delete the chosen comment with its replies.
    Delete,
}

/// What the field is writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum DraftMode {
    Edit,
    Reply,
}

/// The open field, and the comment it writes into or answers.
pub(in crate::shell) struct Draft {
    pub(in crate::shell) target: ObjRef,
    pub(in crate::shell) mode: DraftMode,
    pub(in crate::shell) input: Entity<SearchInput>,
}

impl std::fmt::Debug for Draft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Draft")
            .field("target", &self.target)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

/// The pane's own state, kept while it is open. The comments themselves are
/// the pane's snapshot, read when it opens and after every edit.
#[derive(Debug, Default)]
pub(in crate::shell) struct CommentsState {
    pub(in crate::shell) sort: CommentSort,
    pub(in crate::shell) filter: CommentFilter,
    pub(in crate::shell) selected: Option<ObjRef>,
    pub(in crate::shell) draft: Option<Draft>,
}

impl CommentsState {
    /// Forget the choice and any open field: they named comments in a
    /// document that is no longer showing.
    pub(in crate::shell) fn document_changed(&mut self) {
        self.selected = None;
        self.draft = None;
    }

    /// The draft field, for the frame's focus ring.
    pub(in crate::shell) fn draft_input(&self) -> Option<&Entity<SearchInput>> {
        self.draft.as_ref().map(|draft| &draft.input)
    }
}

impl CommentSort {
    /// The key after this one, and after the last the first again.
    pub(in crate::shell) fn next(self) -> Self {
        let at = Self::ALL.iter().position(|sort| *sort == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sort_cycles_through_every_key_and_back() {
        let mut sort = CommentSort::default();
        let mut seen = vec![sort];
        for _ in 0..CommentSort::ALL.len() {
            sort = sort.next();
            seen.push(sort);
        }
        assert_eq!(&seen[..4], CommentSort::ALL);
        assert_eq!(seen[4], CommentSort::Page);
    }

    #[test]
    fn a_new_document_forgets_the_choice() {
        let mut state = CommentsState {
            selected: Some(ObjRef::new(4, 0)),
            ..CommentsState::default()
        };
        state.document_changed();
        assert_eq!(state.selected, None);
        assert!(state.draft_input().is_none());
    }
}
