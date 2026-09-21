//! What each Comments pane action does to the document.
//!
//! Every change is one undoable step through `core`'s review functions, so
//! a reply, a status or a changed text is taken back by Edit > Undo like
//! any other edit, and the pane is read again from the document after it.

use gpui::{actions, AppContext as _, Context, Entity, Focusable as _, KeyBinding, Window};
use onionskin_core::review::{add_reply, set_contents, set_status, MARKED_MODEL, REVIEW_MODEL};
use onionskin_core::{remove_annotation, ObjRef, ReadAnnotation};

use super::super::super::chrome::{SearchInput, ShellFrame, ThemeTokens};
use super::super::super::Canvas;
use super::super::{navigate, NavigationPanesState};
use super::model::{filter_values, listing, CommentFilter, CommentSort};
use super::{CommentAction, Draft, DraftMode, DRAFT_ID};

actions!(
    onionskin_comment_draft,
    [SaveCommentDraft, CancelCommentDraft]
);

/// The draft field's own key context, more specific than the shell's, so
/// Enter saves the draft instead of activating the focus ring.
pub(in crate::shell) const DRAFT_KEY_CONTEXT: &str = "OnionskinCommentDraft";

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("enter", SaveCommentDraft, Some(DRAFT_KEY_CONTEXT)),
        KeyBinding::new("escape", CancelCommentDraft, Some(DRAFT_KEY_CONTEXT)),
    ]);
}

/// Run one action that needs no window. Edit and Reply open a field, which
/// needs one, and go through [`start_draft`] instead.
pub(in crate::shell) fn run(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    action: CommentAction,
    cx: &mut Context<ShellFrame>,
) {
    state.comments.menu = None;
    match action {
        CommentAction::Select(comment) => select(state, canvas, comment, cx),
        CommentAction::OpenMenu { comment, at } => {
            select(state, canvas, comment, cx);
            state.comments.menu = Some(at);
        }
        CommentAction::ToggleRead => toggle_read(state, canvas, cx),
        CommentAction::CycleSort => state.comments.sort = state.comments.sort.next(),
        CommentAction::CycleFilter(field) => {
            let values = filter_values(state.comment_snapshot().unwrap_or(&[]), field);
            state.comments.filter.cycle(field, &values);
        }
        CommentAction::Edit | CommentAction::Reply => {}
        CommentAction::CancelDraft => state.comments.draft = None,
        CommentAction::SaveDraft => save_draft(state, canvas, cx),
        CommentAction::SetStatus(status) => {
            answer_with_state(state, canvas, cx, "Set Status", REVIEW_MODEL, status);
        }
        CommentAction::ToggleMark => {
            let state_name = if selected_is_checked(state) {
                "Unmarked"
            } else {
                "Marked"
            };
            answer_with_state(state, canvas, cx, "Set Checkmark", MARKED_MODEL, state_name);
        }
        CommentAction::Delete => delete_selected(state, canvas, cx),
    }
}

/// Open the field on the chosen comment, for its text or for a reply, and
/// put the cursor in it.
pub(in crate::shell) fn start_draft(
    state: &mut NavigationPanesState,
    mode: DraftMode,
    theme: ThemeTokens,
    window: &mut Window,
    cx: &mut Context<ShellFrame>,
) {
    let Some(target) = state.comments.selected else {
        return;
    };
    let (placeholder, text) = match mode {
        DraftMode::Edit => (
            "Comment text",
            find(state.comment_snapshot().unwrap_or(&[]), target)
                .and_then(|comment| comment.contents.clone())
                .unwrap_or_default(),
        ),
        DraftMode::Reply => ("Type a reply", String::new()),
    };
    let input = cx.new(|cx| {
        let mut input = SearchInput::with_placeholder(DRAFT_ID, placeholder, theme, cx);
        input.set_query(text, cx);
        input
    });
    window.focus(&input.read(cx).focus_handle(cx));
    state.comments.draft = Some(Draft {
        target,
        mode,
        input,
    });
}

fn find(annotations: &[ReadAnnotation], comment: ObjRef) -> Option<&ReadAnnotation> {
    annotations
        .iter()
        .find(|annotation| annotation.objref == comment)
}

fn select(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    comment: ObjRef,
    cx: &mut Context<ShellFrame>,
) {
    state.comments.selected = Some(comment);
    state.comments.draft = None;
    if let Some(canvas) = canvas {
        canvas.update(cx, |canvas, _| canvas.model.set_comment_read(comment, true));
    }
    let page = find(state.comment_snapshot().unwrap_or(&[]), comment).map(|found| found.page);
    if let Some(page) = page {
        navigate(state, canvas, cx, move |canvas| {
            canvas.model.go_to_page(page)
        });
    }
}

fn toggle_read(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
) {
    let (Some(canvas), Some(chosen)) = (canvas, state.comments.selected) else {
        return;
    };
    let Some(comment) = find(state.comment_snapshot().unwrap_or(&[]), chosen).cloned() else {
        return;
    };
    canvas.update(cx, |canvas, _| {
        let model = &mut canvas.model;
        let read = model.comment_reads().is_read(&comment, model.author());
        model.set_comment_read(chosen, !read);
    });
}

fn selected_is_checked(state: &NavigationPanesState) -> bool {
    let Some(selected) = state.comments.selected else {
        return false;
    };
    listing(
        state.comment_snapshot().unwrap_or(&[]),
        CommentSort::Page,
        &CommentFilter::default(),
    )
    .threads
    .iter()
    .any(|thread| thread.comment.objref == selected && thread.checked)
}

/// Seconds since the epoch, which `/M` and `/CreationDate` are written from.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn save_draft(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
) {
    let Some(draft) = state.comments.draft.take() else {
        return;
    };
    let text = draft.input.read(cx).query().to_owned();
    let target = draft.target;
    match draft.mode {
        DraftMode::Edit => {
            annotation_edit(
                state,
                canvas,
                cx,
                "Edit Comment Text",
                move |tx, _, _, _| set_contents(tx, target, &text, now()),
            );
        }
        // An empty reply is not a reply: nothing is written.
        DraftMode::Reply if text.trim().is_empty() => {}
        DraftMode::Reply => {
            annotation_edit(
                state,
                canvas,
                cx,
                "Add Reply",
                move |tx, structure, page, author| {
                    add_reply(tx, structure, page, target, &text, author, now()).map(|_| ())
                },
            );
        }
    }
}

fn answer_with_state(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
    label: &'static str,
    model: &'static str,
    value: &'static str,
) {
    let Some(target) = state.comments.selected else {
        return;
    };
    annotation_edit(
        state,
        canvas,
        cx,
        label,
        move |tx, structure, page, author| {
            set_status(tx, structure, page, target, model, value, author, now()).map(|_| ())
        },
    );
}

/// Delete the chosen comment and everything that answers it, as Acrobat
/// does: a reply left behind would answer nothing.
fn delete_selected(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
) {
    let Some(target) = state.comments.selected else {
        return;
    };
    let doomed: Vec<ObjRef> = state
        .comment_snapshot()
        .unwrap_or(&[])
        .iter()
        .filter(|annotation| annotation.objref == target || annotation.in_reply_to == Some(target))
        .map(|annotation| annotation.objref)
        .collect();
    let deleted = annotation_edit(
        state,
        canvas,
        cx,
        "Delete Comment",
        move |tx, _, page, _| {
            for annotation in doomed {
                remove_annotation(tx, page, annotation)?;
            }
            Ok(())
        },
    );
    if deleted {
        state.comments.selected = None;
    }
}

/// One change to the chosen comment's page, as one undoable step; then the
/// pane read again, or the refusal in its feedback line. `change` is handed
/// the page the comment is on and the author the user comments as.
fn annotation_edit(
    state: &mut NavigationPanesState,
    canvas: Option<&Entity<Canvas>>,
    cx: &mut Context<ShellFrame>,
    label: &'static str,
    change: impl FnOnce(
        &mut onionskin_core::Transaction<'_>,
        &onionskin_core::Structure,
        ObjRef,
        Option<&str>,
    ) -> onionskin_core::Result<()>,
) -> bool {
    let Some(canvas) = canvas else {
        return false;
    };
    let target = state.comments.selected.or(state
        .comments
        .draft
        .as_ref()
        .map(|draft| draft.target));
    let Some(page_index) = target
        .and_then(|target| find(state.comment_snapshot().unwrap_or(&[]), target))
        .map(|comment| comment.page)
    else {
        return false;
    };
    let outcome = canvas.update(cx, |canvas, cx| {
        let author = canvas.model.author().map(str::to_owned);
        let document = canvas.model.document_mut();
        let result = document
            .structure()
            .and_then(|structure| Ok(structure.page(page_index)?.objref))
            .and_then(|page| {
                document.edit_annotations(label, |tx, structure| {
                    change(tx, structure, page, author.as_deref())
                })
            });
        if result.is_ok() {
            canvas.handle_change(Ok(true), cx);
        }
        result
    });
    match outcome {
        Ok(()) => {
            state.report(None);
            state.reread(canvas, cx);
            true
        }
        Err(error) => {
            state.report(Some(error.to_string()));
            false
        }
    }
}
