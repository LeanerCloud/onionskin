//! Summarize Comments: choose a layout, then where the summary goes.
//!
//! Acrobat's two layouts that matter: the comments alone, or each page
//! followed by its comments. The summary itself is `tools-comment`'s; this is
//! the choice and the button.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element, Rects, Surface};
use super::combine_dialog::button;
use super::{ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// Which layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum SummaryChoice {
    #[default]
    CommentsOnly,
    DocumentAndComments,
}

impl SummaryChoice {
    const ALL: [Self; 2] = [Self::CommentsOnly, Self::DocumentAndComments];

    fn label(self) -> &'static str {
        match self {
            Self::CommentsOnly => "Comments only",
            Self::DocumentAndComments => "Each page, followed by its comments",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::CommentsOnly => "summary-comments-only",
            Self::DocumentAndComments => "summary-document-and-comments",
        }
    }
}

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SummaryAction {
    Choose(SummaryChoice),
    Submit,
}

#[derive(Debug, Default)]
pub(in crate::shell) struct SummaryDialogState {
    pub(in crate::shell) choice: SummaryChoice,
    pub(in crate::shell) error: Option<String>,
}

pub(in crate::shell) fn accessible(state: &SummaryDialogState, rects: &Rects) -> Vec<Element> {
    let mut body: Vec<Element> = SummaryChoice::ALL
        .into_iter()
        .map(|choice| {
            Element::new(choice.id(), Role::RadioButton, choice.label())
                .with_state(A11yState::selected(state.choice == choice))
                .with_activation(Activation::Summary(SummaryAction::Choose(choice)))
        })
        .collect();
    if let Some(error) = &state.error {
        body.push(Element::new("summary-error", Role::Alert, error.clone()));
    }
    body.push(
        Element::new("summary-submit", Role::Button, "Summarize…")
            .with_activation(Activation::Summary(SummaryAction::Submit)),
    );
    for (row, bounds) in body.iter_mut().zip(rects.of(Surface::SummaryDialog)) {
        row.bounds = Some(bounds);
    }
    body
}

pub(in crate::shell) fn render(
    state: &SummaryDialogState,
    rects: Rects,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::SummaryDialog, &bounds, window);
        })
        .flex()
        .flex_col()
        .gap_2();
    for choice in SummaryChoice::ALL {
        let chosen = state.choice == choice;
        body = body.child(
            div()
                .id(choice.id())
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(chosen, |row| row.bg(theme.selected))
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::Summary(SummaryAction::Choose(choice)),
                        window,
                        cx,
                    );
                }))
                .child(format!(
                    "{} {}",
                    if chosen { "(•)" } else { "( )" },
                    choice.label()
                )),
        );
    }
    if let Some(error) = &state.error {
        body = body.child(
            div()
                .id("summary-error")
                .text_color(theme.error_text)
                .child(error.clone()),
        );
    }
    body.child(button(
        "summary-submit",
        "Summarize…",
        true,
        theme,
        focused,
        cx,
        Activation::Summary(SummaryAction::Submit),
    ))
}
