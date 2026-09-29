//! Bookmark Properties: a title's style, which is the `/F` bit position for
//! bold and italic and the `/C` colour a reader honours.
//!
//! The colour is a swatch from the comment inspector's palette rather than a
//! free picker. That palette is what the rest of the shell already offers for a
//! colour, so a second set of swatches in a second dialog would be two answers
//! to one question. "Automatic" is the absence of `/C`, which is how a title
//! goes back to the renderer's own colour.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element};
use super::combine_dialog::button;
use super::inspector::PALETTE;
use super::{ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum BookmarkPropertiesAction {
    /// Pick the palette entry, or `None` for the renderer's own colour.
    Colour(Option<usize>),
    ToggleBold,
    ToggleItalic,
    Apply,
}

pub(in crate::shell) struct BookmarkPropertiesState {
    /// The outline path of the title this is for.
    pub(in crate::shell) path: Vec<usize>,
    pub(in crate::shell) title: String,
    pub(in crate::shell) bold: bool,
    pub(in crate::shell) italic: bool,
    pub(in crate::shell) colour: Option<usize>,
    /// Why Apply is off: the document may not be edited.
    pub(in crate::shell) refusal: Option<&'static str>,
    pub(in crate::shell) error: Option<String>,
}

impl BookmarkPropertiesState {
    pub(in crate::shell) fn new(
        path: Vec<usize>,
        title: &str,
        refusal: Option<&'static str>,
    ) -> Self {
        Self {
            path,
            title: title.to_owned(),
            bold: false,
            italic: false,
            colour: None,
            refusal,
            error: None,
        }
    }

    /// The chosen colour as `/C` carries it, three 0..1 floats, and `None` for
    /// the renderer to choose.
    pub(in crate::shell) fn rgb(&self) -> Option<[f64; 3]> {
        let [red, green, blue] = PALETTE[*self.colour.as_ref()?].1;
        Some([
            f64::from(red) / 255.0,
            f64::from(green) / 255.0,
            f64::from(blue) / 255.0,
        ])
    }
}

pub(in crate::shell) fn accessible(state: &BookmarkPropertiesState) -> Vec<Element> {
    let mut body = vec![
        Element::new(
            "bookmark-properties-title",
            Role::Label,
            state.title.clone(),
        ),
        Element::new("bookmark-properties-bold", Role::CheckBox, "Bold")
            .with_state(A11yState::toggled(state.bold))
            .with_activation(Activation::BookmarkProperties(
                BookmarkPropertiesAction::ToggleBold,
            )),
        Element::new("bookmark-properties-italic", Role::CheckBox, "Italic")
            .with_state(A11yState::toggled(state.italic))
            .with_activation(Activation::BookmarkProperties(
                BookmarkPropertiesAction::ToggleItalic,
            )),
        Element::new(
            "bookmark-properties-automatic",
            Role::RadioButton,
            "Automatic",
        )
        .with_state(A11yState::selected(state.colour.is_none()))
        .with_activation(Activation::BookmarkProperties(
            BookmarkPropertiesAction::Colour(None),
        )),
    ];
    for (index, (name, _)) in PALETTE.iter().enumerate() {
        body.push(
            Element::new(
                ("bookmark-properties-colour", index),
                Role::RadioButton,
                *name,
            )
            .with_state(A11yState::selected(state.colour == Some(index)))
            .with_activation(Activation::BookmarkProperties(
                BookmarkPropertiesAction::Colour(Some(index)),
            )),
        );
    }
    if let Some(error) = state.error.as_deref().or(state.refusal) {
        body.push(Element::new(
            "bookmark-properties-error",
            Role::Alert,
            error,
        ));
    }
    let mut apply = Element::new("bookmark-properties-apply", Role::Button, "Apply");
    if state.refusal.is_none() {
        apply = apply.with_activation(Activation::BookmarkProperties(
            BookmarkPropertiesAction::Apply,
        ));
    }
    body.push(apply);
    body
}

pub(in crate::shell) fn render(
    state: &BookmarkPropertiesState,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().child(state.title.clone()));
    for (id, label, on, action) in [
        (
            "bookmark-properties-bold",
            "Bold",
            state.bold,
            BookmarkPropertiesAction::ToggleBold,
        ),
        (
            "bookmark-properties-italic",
            "Italic",
            state.italic,
            BookmarkPropertiesAction::ToggleItalic,
        ),
    ] {
        body = body.child(
            div()
                .id(id)
                .px_2()
                .py_1()
                .when(on, |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::BookmarkProperties(action), window, cx);
                }))
                .child(format!("[{}] {label}", if on { "x" } else { " " })),
        );
    }
    for (index, (name, [red, green, blue])) in PALETTE.iter().enumerate() {
        let hex = (u32::from(*red) << 16) | (u32::from(*green) << 8) | u32::from(*blue);
        body = body.child(
            div()
                .id(("bookmark-properties-swatch", index))
                .px_2()
                .py_1()
                .when(state.colour == Some(index), |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::BookmarkProperties(BookmarkPropertiesAction::Colour(Some(
                            index,
                        ))),
                        window,
                        cx,
                    );
                }))
                .child(format!("#{hex:06X} {name}")),
        );
    }
    if let Some(error) = state.error.as_deref().or(state.refusal) {
        body = body.child(div().text_color(theme.error_text).child(error.to_owned()));
    }
    body.child(button(
        "bookmark-properties-apply",
        "Apply",
        true,
        theme,
        focused,
        cx,
        Activation::BookmarkProperties(BookmarkPropertiesAction::Apply),
    ))
    .into_any_element()
}
