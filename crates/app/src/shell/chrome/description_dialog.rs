//! Edit Description: the text the Attachments pane shows under a file's
//! name, one field and OK. An empty description removes it.

use accesskit::Role;
use gpui::{div, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Styled as _};

use super::accessible::{Activation, Element, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum DescriptionAction {
    Submit,
}

pub(in crate::shell) struct DescriptionState {
    /// The embedded file stream the attachment is, which is how every
    /// specification naming it is found.
    pub(in crate::shell) stream: u32,
    pub(in crate::shell) text: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
}

impl DescriptionState {
    pub(in crate::shell) fn new(
        stream: u32,
        description: &str,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let description = description.to_owned();
        let text = cx.new(|cx| {
            let mut input =
                SearchInput::with_placeholder("attachment-description", "Description", theme, cx);
            input.set_query(description, cx);
            input
        });
        Self {
            stream,
            text,
            error: None,
        }
    }
}

pub(in crate::shell) fn accessible(state: &DescriptionState, cx: &gpui::App) -> Vec<Element> {
    let mut body = vec![state
        .text
        .read(cx)
        .accessible("Description", TextField::AttachmentDescription)];
    if let Some(error) = &state.error {
        body.push(Element::new(
            "attachment-description-error",
            Role::Alert,
            error.clone(),
        ));
    }
    body.push(
        Element::new("attachment-description-submit", Role::Button, "OK")
            .with_activation(Activation::Description(DescriptionAction::Submit)),
    );
    body
}

pub(in crate::shell) fn render(
    state: &DescriptionState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_2().child(state.text.clone());
    if let Some(error) = &state.error {
        body = body.child(div().text_color(theme.error_text).child(error.clone()));
    }
    body.child(button(
        "attachment-description-submit",
        "OK",
        true,
        theme,
        focused,
        cx,
        Activation::Description(DescriptionAction::Submit),
    ))
}
