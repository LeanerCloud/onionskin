//! Drawing the Comments pane, and describing it to a screen reader from the
//! same rows and the same commands, so the two cannot disagree.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::review::REVIEW_STATES;
use onionskin_core::ReadAnnotation;

use super::super::super::chrome::accessible::{Activation, Element, TextField};
use super::super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::{empty_message, error_message, list, PaneAction, ROW_HEIGHT};
use super::actions::{CancelCommentDraft, SaveCommentDraft, DRAFT_KEY_CONTEXT};
use super::model::{display_date, kind, listing, FilterField, Listing, Thread};
use super::{CommentAction, CommentsState, DraftMode};
use crate::a11y::State as A11yState;

/// Said where the list would be when there is nothing to list.
const NO_COMMENTS: &str = "This document has no comments.";
const NOTHING_MATCHES: &str = "No comment matches the filters.";
const NO_TEXT: &str = "(no text)";
const REPLY_INDENT: f32 = 16.0;

/// A control the pane shows: a label, whether it can be used now, and what
/// it runs.
type Command = (String, MenuAvailability, Activation);

fn activation(action: CommentAction) -> Activation {
    Activation::Pane(PaneAction::Comment(action))
}

/// The sort and filter controls above the list, each showing its value.
fn toolbar(state: &CommentsState) -> Vec<Command> {
    let mut controls = vec![(
        format!("Sort: {}", state.sort.label()),
        MenuAvailability::Enabled,
        activation(CommentAction::CycleSort),
    )];
    controls.extend(FilterField::ALL.into_iter().map(|field| {
        (
            format!(
                "{}: {}",
                field.label(),
                state.filter.value(field).unwrap_or("All")
            ),
            MenuAvailability::Enabled,
            activation(CommentAction::CycleFilter(field)),
        )
    }));
    controls
}

/// What can be done to the chosen comment. A document that may not be
/// edited disables every one with its reason.
fn commands(thread: &Thread, refusal: Option<&'static str>) -> Vec<Command> {
    let availability = refusal.map_or(MenuAvailability::Enabled, MenuAvailability::Disabled);
    let mut listed = vec![
        ("Reply".to_owned(), CommentAction::Reply),
        ("Edit Text".to_owned(), CommentAction::Edit),
    ];
    listed.extend(REVIEW_STATES.into_iter().map(|status| {
        let label = if status == "None" {
            "Clear Status".to_owned()
        } else {
            status.to_owned()
        };
        (label, CommentAction::SetStatus(status))
    }));
    listed.push((
        if thread.checked { "Uncheck" } else { "Check" }.to_owned(),
        CommentAction::ToggleMark,
    ));
    listed.push(("Delete".to_owned(), CommentAction::Delete));
    listed
        .into_iter()
        .map(|(label, action)| (label, availability, activation(action)))
        .collect()
}

fn draft_commands() -> Vec<Command> {
    vec![
        (
            "Save".to_owned(),
            MenuAvailability::Enabled,
            activation(CommentAction::SaveDraft),
        ),
        (
            "Cancel".to_owned(),
            MenuAvailability::Enabled,
            activation(CommentAction::CancelDraft),
        ),
    ]
}

/// The comment's first line in the list: what kind, by whom, and its status.
fn heading(thread: &Thread) -> String {
    let mut heading = kind(&thread.comment);
    if let Some(author) = &thread.comment.author {
        heading.push_str(" · ");
        heading.push_str(author);
    }
    if let Some(status) = &thread.status {
        heading.push_str(" · ");
        heading.push_str(status);
    }
    if thread.checked {
        heading.push_str(" · ✓");
    }
    heading
}

/// Where and when, as the second line.
fn place(annotation: &ReadAnnotation) -> String {
    let page = format!("Page {}", annotation.page + 1);
    match &annotation.modified {
        Some(date) => format!("{page} · {}", display_date(date)),
        None => page,
    }
}

fn text(annotation: &ReadAnnotation) -> String {
    annotation
        .contents
        .clone()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| NO_TEXT.to_owned())
}

fn reply_line(reply: &ReadAnnotation) -> String {
    match &reply.author {
        Some(author) => format!("{author}: {}", text(reply)),
        None => text(reply),
    }
}

fn current_listing(state: &CommentsState, annotations: &[ReadAnnotation]) -> Listing {
    listing(annotations, state.sort, &state.filter)
}

/// What the pane tells a screen reader.
pub(in crate::shell::panes) fn accessible(
    state: &CommentsState,
    annotations: Result<&[ReadAnnotation], &String>,
    refusal: Option<&'static str>,
    cx: &Context<ShellFrame>,
) -> Vec<Element> {
    let annotations = match annotations {
        Ok(annotations) => annotations,
        Err(message) => {
            return vec![Element::new(
                "comment-rows-error",
                Role::Alert,
                message.clone(),
            )]
        }
    };
    let mut described = vec![
        Element::new("comment-toolbar", Role::Toolbar, "Sort and Filter")
            .with_children(buttons("comment-control", toolbar(state))),
    ];
    let listed = current_listing(state, annotations);
    if listed.threads.is_empty() && listed.orphans.is_empty() {
        let message = if annotations.is_empty() {
            NO_COMMENTS
        } else {
            NOTHING_MATCHES
        };
        described.push(Element::new("comment-rows-empty", Role::Label, message));
        return described;
    }
    let rows = listed
        .threads
        .iter()
        .enumerate()
        .map(|(index, thread)| describe_thread(state, index, thread, refusal, cx))
        .chain(listed.orphans.iter().enumerate().map(|(index, orphan)| {
            Element::new(
                ("comment-orphan", index),
                Role::ListItem,
                reply_line(orphan),
            )
            .with_description("A reply to a comment that is no longer in the document")
        }))
        .collect();
    described.push(Element::new("comment-rows", Role::List, "Comments").with_children(rows));
    described
}

fn describe_thread(
    state: &CommentsState,
    index: usize,
    thread: &Thread,
    refusal: Option<&'static str>,
    cx: &Context<ShellFrame>,
) -> Element {
    let selected = state.selected == Some(thread.comment.objref);
    let mut children: Vec<Element> = thread
        .replies
        .iter()
        .enumerate()
        .map(|(reply, answer)| {
            Element::new(
                ("comment-reply", index * 1000 + reply),
                Role::ListItem,
                reply_line(answer),
            )
        })
        .collect();
    if selected {
        children.extend(buttons("comment-command", commands(thread, refusal)));
        if let Some(draft) = state.draft.as_ref() {
            children.push(
                draft
                    .input
                    .read(cx)
                    .accessible(draft_label(draft.mode), TextField::CommentDraft),
            );
            children.extend(buttons("comment-draft-command", draft_commands()));
        }
    }
    Element::new(
        ("comment-row", index),
        Role::ListItem,
        text(&thread.comment),
    )
    .with_description(format!("{}. {}", heading(thread), place(&thread.comment)))
    .with_state(A11yState::selected(selected))
    .with_activation(activation(CommentAction::Select(thread.comment.objref)))
    .with_children(children)
}

fn draft_label(mode: DraftMode) -> &'static str {
    match mode {
        DraftMode::Edit => "Comment text",
        DraftMode::Reply => "Reply",
    }
}

fn buttons(id: &'static str, commands: Vec<Command>) -> Vec<Element> {
    commands
        .into_iter()
        .enumerate()
        .map(|(index, (label, availability, activation))| {
            let button = Element::new((id, index), Role::Button, label)
                .with_state(A11yState::enabled(availability.is_enabled()))
                .with_activation(activation);
            match availability.reason() {
                Some(reason) => button.with_description(reason),
                None => button,
            }
        })
        .collect()
}

/// The pane's body: the controls, then the list.
pub(in crate::shell::panes) fn render(
    state: &CommentsState,
    annotations: Result<&[ReadAnnotation], &String>,
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let annotations = match annotations {
        Ok(annotations) => annotations,
        Err(message) => return error_message(message, theme),
    };
    let body = div().flex_1().min_h_0().flex().flex_col().child(
        render_buttons("comment-control", toolbar(state), theme, cx)
            .px_2()
            .pb_1(),
    );
    let listed = current_listing(state, annotations);
    if listed.threads.is_empty() && listed.orphans.is_empty() {
        let message = if annotations.is_empty() {
            NO_COMMENTS
        } else {
            NOTHING_MATCHES
        };
        return body.child(empty_message(message, theme)).into_any_element();
    }
    let mut rows = list("comment-rows");
    for (index, thread) in listed.threads.iter().enumerate() {
        rows = rows.child(render_thread(state, index, thread, refusal, theme, cx));
    }
    for (index, orphan) in listed.orphans.iter().enumerate() {
        rows = rows.child(
            div()
                .id(("comment-orphan", index))
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.muted_text)
                .child(format!(
                    "Reply to a deleted comment. {}",
                    reply_line(orphan)
                )),
        );
    }
    body.child(rows).into_any_element()
}

fn render_thread(
    state: &CommentsState,
    index: usize,
    thread: &Thread,
    refusal: Option<&'static str>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let selected = state.selected == Some(thread.comment.objref);
    let select = activation(CommentAction::Select(thread.comment.objref));
    let mut row = div()
        .id(("comment-row", index))
        .min_h(px(ROW_HEIGHT))
        .flex()
        .flex_col()
        .gap_0p5()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.surface)
        .cursor_pointer()
        .when(selected, |row| row.bg(theme.selected))
        .hover(move |row| row.bg(theme.hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(select.clone(), window, cx);
        }))
        .child(
            div()
                .text_xs()
                .text_color(theme.secondary_text)
                .child(heading(thread)),
        )
        .child(div().text_sm().child(text(&thread.comment)))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_text)
                .child(place(&thread.comment)),
        );
    for (reply, answer) in thread.replies.iter().enumerate() {
        row = row.child(
            div()
                .id(("comment-reply", index * 1000 + reply))
                .pl(px(REPLY_INDENT))
                .text_xs()
                .child(reply_line(answer)),
        );
    }
    if selected {
        row = row.child(render_buttons(
            "comment-command",
            commands(thread, refusal),
            theme,
            cx,
        ));
        if let Some(draft) = state.draft.as_ref() {
            row = row.child(
                div()
                    .key_context(DRAFT_KEY_CONTEXT)
                    .on_action(cx.listener(|frame, _: &SaveCommentDraft, window, cx| {
                        frame.run_activation(activation(CommentAction::SaveDraft), window, cx);
                    }))
                    .on_action(cx.listener(|frame, _: &CancelCommentDraft, window, cx| {
                        frame.run_activation(activation(CommentAction::CancelDraft), window, cx);
                    }))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .p_1()
                            .rounded_sm()
                            .bg(theme.raised)
                            .border_1()
                            .border_color(theme.selected)
                            .child(draft.input.clone()),
                    )
                    .child(render_buttons(
                        "comment-draft-command",
                        draft_commands(),
                        theme,
                        cx,
                    )),
            );
        }
    }
    row
}

/// A wrapping row of small buttons; a disabled one is greyed and says why on
/// hover through its tooltip text in the tree.
fn render_buttons(
    id: &'static str,
    commands: Vec<Command>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut row = div().flex().flex_wrap().gap_1().pt_1();
    for (index, (label, availability, activation)) in commands.into_iter().enumerate() {
        let enabled = availability.is_enabled();
        let mut button = div()
            .id((id, index))
            .px_2()
            .py_0p5()
            .rounded_sm()
            .border_1()
            .border_color(theme.surface)
            .text_xs()
            .text_color(if enabled {
                theme.text
            } else {
                theme.disabled_text
            })
            .child(label);
        if enabled {
            button = button
                .cursor_pointer()
                .hover(move |button| button.bg(theme.hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(activation.clone(), window, cx);
                }));
        }
        row = row.child(button);
    }
    row
}

#[cfg(test)]
mod tests {
    use onionskin_core::{Flags, ObjRef, Rect, Subtype};

    use super::*;

    fn comment(status: Option<&str>, checked: bool) -> Thread {
        Thread {
            comment: ReadAnnotation {
                objref: ObjRef::new(5, 0),
                page: 2,
                subtype: None::<Subtype>,
                raw_subtype: "Highlight".into(),
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                quads: Vec::new(),
                contents: None,
                author: Some("Ana".into()),
                modified: Some("D:20260921143000".into()),
                color: None,
                flags: Flags(4),
                in_reply_to: None,
                has_appearance: true,
                ink: Vec::new(),
                border_width: 1.0,
                subject: None,
                state: None,
            },
            replies: Vec::new(),
            status: status.map(str::to_owned),
            checked,
        }
    }

    #[test]
    fn a_row_says_what_by_whom_where_and_when() {
        let thread = comment(Some("Accepted"), true);
        assert_eq!(heading(&thread), "Highlight · Ana · Accepted · ✓");
        assert_eq!(place(&thread.comment), "Page 3 · 2026-09-21 14:30");
        assert_eq!(text(&thread.comment), NO_TEXT);
    }

    #[test]
    fn a_refused_document_disables_every_command_with_its_reason() {
        let labels: Vec<String> = commands(&comment(None, false), None)
            .into_iter()
            .map(|(label, availability, _)| {
                assert!(availability.is_enabled());
                label
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Reply",
                "Edit Text",
                "Accepted",
                "Rejected",
                "Cancelled",
                "Completed",
                "Clear Status",
                "Check",
                "Delete"
            ]
        );
        assert!(
            commands(&comment(None, false), Some("The document is encrypted"))
                .iter()
                .all(|(_, availability, _)| availability.reason()
                    == Some("The document is encrypted"))
        );
        assert_eq!(commands(&comment(None, true), None)[7].0, "Uncheck");
    }
}
