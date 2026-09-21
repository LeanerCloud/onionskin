//! The bookmark title dialog: what New Bookmark and Rename Bookmark ask.
//!
//! One field and one button. The bookmark it names is addressed by its path
//! in the outline, taken when the dialog opened.

use accesskit::Role;
use gpui::{
    div, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _,
};

use super::accessible::{Activation, Element, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum BookmarkTitleAction {
    Submit,
}

pub(in crate::shell) struct BookmarkTitleState {
    /// The bookmark being titled.
    pub(in crate::shell) path: Vec<usize>,
    pub(super) title: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
}

impl BookmarkTitleState {
    pub(super) fn new(
        path: Vec<usize>,
        title: &str,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let title = title.to_owned();
        let input = cx.new(|cx| {
            let mut input = SearchInput::with_placeholder("bookmark-title", "Title", theme, cx);
            input.set_query(title, cx);
            input
        });
        Self {
            path,
            title: input,
            error: None,
        }
    }

    /// The title as typed, or why it cannot be one.
    pub(super) fn title(&self, cx: &gpui::App) -> Result<String, String> {
        let title = self.title.read(cx).query().trim().to_owned();
        if title.is_empty() {
            return Err("A bookmark needs a title".to_owned());
        }
        Ok(title)
    }
}

pub(in crate::shell) fn accessible(state: &BookmarkTitleState, cx: &gpui::App) -> Vec<Element> {
    let mut body = vec![state
        .title
        .read(cx)
        .accessible("Title", TextField::BookmarkTitle)];
    if let Some(error) = &state.error {
        body.push(Element::new(
            "bookmark-title-error",
            Role::Alert,
            error.clone(),
        ));
    }
    body.push(
        Element::new("bookmark-title-submit", Role::Button, "OK")
            .with_activation(Activation::BookmarkTitle(BookmarkTitleAction::Submit)),
    );
    body
}

pub(in crate::shell) fn render(
    state: &BookmarkTitleState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_2().child(state.title.clone());
    if let Some(error) = &state.error {
        body = body.child(
            div()
                .id("bookmark-title-error")
                .text_color(theme.error_text)
                .child(error.clone()),
        );
    }
    body.child(button(
        "bookmark-title-submit",
        "OK",
        true,
        theme,
        focused,
        cx,
        Activation::BookmarkTitle(BookmarkTitleAction::Submit),
    ))
}
