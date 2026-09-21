//! Drawing the Comments pane, and describing it to a screen reader from the
//! same rows and the same commands, so the two cannot disagree.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::ReadAnnotation;

use super::super::super::chrome::accessible::{Activation, Element, TextField};
use super::super::super::chrome::{MenuAvailability, ShellFrame, ThemeTokens};
use super::super::{empty_message, error_message, list, PaneAction, ROW_HEIGHT};
use super::actions::{CancelCommentDraft, SaveCommentDraft, DRAFT_KEY_CONTEXT};
use super::commands::{
    accessible_menu, activation, commands, draft_commands, render_menu, Command,
};
use super::model::{display_date, kind, listing, FilterField, Listing, Thread};
use super::{CommentAction, CommentsState, DraftMode};
use crate::a11y::State as A11yState;

/// Said where the list would be when there is nothing to list.
const NO_COMMENTS: &str = "This document has no comments.";
const NOTHING_MATCHES: &str = "No comment matches the filters.";
const NO_TEXT: &str = "(no text)";
const REPLY_INDENT: f32 = 16.0;

/// The sort and filter controls above the list, each showing its value.
fn toolbar(state: &CommentsState) -> Vec<(String, Activation)> {
    let mut controls = vec![(
        format!("Sort: {}", state.sort.label()),
        activation(CommentAction::CycleSort),
    )];
    controls.extend(FilterField::ALL.into_iter().map(|field| {
        (
            format!(
                "{}: {}",
                field.label(),
                state.filter.value(field).unwrap_or("All")
            ),
            activation(CommentAction::CycleFilter(field)),
        )
    }));
    controls
}

/// What the pane is drawn from besides the comments themselves.
pub(in crate::shell::panes) struct Facts<'a> {
    pub(in crate::shell::panes) state: &'a CommentsState,
    /// Why the document may not be edited, if it may not.
    pub(in crate::shell::panes) refusal: Option<&'static str>,
    /// Whether the user has read a comment, this session.
    pub(in crate::shell::panes) is_read: &'a dyn Fn(&ReadAnnotation) -> bool,
}

impl Facts<'_> {
    fn commands(&self, thread: &Thread) -> Vec<Command> {
        commands(thread, (self.is_read)(&thread.comment), self.refusal)
    }

    /// The chosen comment's thread, when it is listed.
    fn chosen<'l>(&self, listed: &'l Listing) -> Option<&'l Thread> {
        let chosen = self.state.selected?;
        listed
            .threads
            .iter()
            .find(|thread| thread.comment.objref == chosen)
    }
}

/// The comment's first line in the list: what kind, by whom, and its status.
fn heading(thread: &Thread, read: bool) -> String {
    let mut heading = if read {
        String::new()
    } else {
        "● ".to_owned()
    };
    heading.push_str(&kind(&thread.comment));
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

fn empty_text(annotations: &[ReadAnnotation]) -> &'static str {
    if annotations.is_empty() {
        NO_COMMENTS
    } else {
        NOTHING_MATCHES
    }
}

/// What the pane tells a screen reader.
pub(in crate::shell::panes) fn accessible(
    facts: &Facts<'_>,
    annotations: Result<&[ReadAnnotation], &String>,
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
    let controls = toolbar(facts.state)
        .into_iter()
        .enumerate()
        .map(|(index, (label, activation))| {
            Element::new(("comment-control", index), Role::Button, label)
                .with_activation(activation)
        })
        .collect();
    let mut described = vec![
        Element::new("comment-toolbar", Role::Toolbar, "Sort and Filter").with_children(controls),
    ];
    let listed = current_listing(facts.state, annotations);
    if listed.threads.is_empty() && listed.orphans.is_empty() {
        described.push(Element::new(
            "comment-rows-empty",
            Role::Label,
            empty_text(annotations),
        ));
        return described;
    }
    let rows = listed
        .threads
        .iter()
        .enumerate()
        .map(|(index, thread)| describe_thread(facts, index, thread, cx))
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
    if let (Some(_), Some(thread)) = (facts.state.menu, facts.chosen(&listed)) {
        described.push(accessible_menu(facts.commands(thread)));
    }
    described
}

fn describe_thread(
    facts: &Facts<'_>,
    index: usize,
    thread: &Thread,
    cx: &Context<ShellFrame>,
) -> Element {
    let state = facts.state;
    let selected = state.selected == Some(thread.comment.objref);
    let read = (facts.is_read)(&thread.comment);
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
        children.extend(buttons("comment-command", facts.commands(thread)));
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
    let unread = if read { "" } else { "Unread. " };
    Element::new(
        ("comment-row", index),
        Role::ListItem,
        text(&thread.comment),
    )
    .with_description(format!(
        "{unread}{}. {}",
        heading(thread, true),
        place(&thread.comment)
    ))
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

/// The pane's body: the controls, then the list, then the context menu when
/// one is open.
pub(in crate::shell::panes) fn render(
    facts: &Facts<'_>,
    annotations: Result<&[ReadAnnotation], &String>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let annotations = match annotations {
        Ok(annotations) => annotations,
        Err(message) => return error_message(message, theme),
    };
    let controls = toolbar(facts.state)
        .into_iter()
        .map(|(label, activation)| (label, MenuAvailability::Enabled, activation))
        .collect();
    let body = div().relative().flex_1().min_h_0().flex().flex_col().child(
        render_buttons("comment-control", controls, theme, cx)
            .px_2()
            .pb_1(),
    );
    let listed = current_listing(facts.state, annotations);
    if listed.threads.is_empty() && listed.orphans.is_empty() {
        return body
            .child(empty_message(empty_text(annotations), theme))
            .into_any_element();
    }
    let mut rows = list("comment-rows");
    for (index, thread) in listed.threads.iter().enumerate() {
        rows = rows.child(render_thread(facts, index, thread, theme, cx));
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
    let menu = match (facts.state.menu, facts.chosen(&listed)) {
        (Some(at), Some(thread)) => Some(render_menu(facts.commands(thread), at, theme, cx)),
        _ => None,
    };
    body.child(rows)
        .when_some(menu, |body, menu| body.child(menu))
        .into_any_element()
}

fn render_thread(
    facts: &Facts<'_>,
    index: usize,
    thread: &Thread,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let state = facts.state;
    let comment = thread.comment.objref;
    let selected = state.selected == Some(comment);
    let read = (facts.is_read)(&thread.comment);
    let select = activation(CommentAction::Select(comment));
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |frame, event: &gpui::MouseDownEvent, _window, cx| {
                frame.run_pane_action(
                    PaneAction::Comment(CommentAction::OpenMenu {
                        comment,
                        at: event.position,
                    }),
                    cx,
                );
                cx.stop_propagation();
            }),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.secondary_text)
                .child(heading(thread, read)),
        )
        .child(
            div()
                .text_sm()
                .when(!read, |text| text.font_weight(gpui::FontWeight::SEMIBOLD))
                .child(text(&thread.comment)),
        )
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
            facts.commands(thread),
            theme,
            cx,
        ));
        if let Some(draft) = state.draft.as_ref() {
            row = row.child(render_draft(draft, theme, cx));
        }
    }
    row
}

fn render_draft(
    draft: &super::Draft,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
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
            draft_commands()
                .into_iter()
                .map(|(label, availability, activation)| {
                    (label.to_owned(), availability, activation)
                })
                .collect(),
            theme,
            cx,
        ))
}

/// A wrapping row of small buttons. A disabled one is greyed; its reason is
/// in the accessibility tree.
fn render_buttons<L: Into<gpui::SharedString>>(
    id: &'static str,
    commands: Vec<(L, MenuAvailability, Activation)>,
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
            .child(label.into());
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
    use super::super::commands::tests::thread;
    use super::*;

    #[test]
    fn a_row_says_what_by_whom_where_and_when_and_whether_it_is_read() {
        let mut listed = thread(true);
        listed.status = Some("Accepted".into());
        assert_eq!(heading(&listed, true), "Highlight · Ana · Accepted · ✓");
        assert_eq!(heading(&listed, false), "● Highlight · Ana · Accepted · ✓");
        assert_eq!(place(&listed.comment), "Page 3 · 2026-09-21 14:30");
        assert_eq!(text(&listed.comment), NO_TEXT);
    }
}
