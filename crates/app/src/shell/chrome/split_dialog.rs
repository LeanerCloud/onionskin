//! Split Document: choose how to cut the open document, and cut it.
//!
//! Three ways, as Acrobat offers them - by page count, by file size, at the
//! top-level bookmarks - and the parts land beside the document, named after
//! it. What the choice and its value mean is [`SplitChoice`], plain data,
//! tested without a window; the frame runs the split.

use std::num::NonZeroUsize;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element, Rects, Surface, TextField};
use super::combine_dialog::button;
use super::{SearchInput, ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

/// How to cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SplitMode {
    PageCount,
    FileSize,
    TopLevelBookmarks,
}

impl SplitMode {
    const ALL: [Self; 3] = [Self::PageCount, Self::FileSize, Self::TopLevelBookmarks];

    fn label(self) -> &'static str {
        match self {
            Self::PageCount => "Number of pages",
            Self::FileSize => "File size (MB)",
            Self::TopLevelBookmarks => "Top-level bookmarks",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::PageCount => "split-mode-pages",
            Self::FileSize => "split-mode-size",
            Self::TopLevelBookmarks => "split-mode-bookmarks",
        }
    }

    fn takes_value(self) -> bool {
        self != Self::TopLevelBookmarks
    }
}

/// What a control in the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SplitAction {
    SetMode(SplitMode),
    Submit,
}

/// A split the user asked for, validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum SplitChoice {
    PageCount(NonZeroUsize),
    FileSize { bytes: u64 },
    TopLevelBookmarks,
}

/// Read the mode and the value field into a split, or say what is wrong.
pub(in crate::shell) fn choose(mode: SplitMode, value: &str) -> Result<SplitChoice, String> {
    let value = value.trim();
    match mode {
        SplitMode::PageCount => value
            .parse::<usize>()
            .ok()
            .and_then(NonZeroUsize::new)
            .map(SplitChoice::PageCount)
            .ok_or_else(|| format!("{value:?} is not a number of pages")),
        SplitMode::FileSize => value
            .parse::<f64>()
            .ok()
            .filter(|megabytes| megabytes.is_finite() && *megabytes > 0.0)
            .map(|megabytes| SplitChoice::FileSize {
                bytes: (megabytes * 1024.0 * 1024.0) as u64,
            })
            .ok_or_else(|| format!("{value:?} is not a size in megabytes")),
        SplitMode::TopLevelBookmarks => Ok(SplitChoice::TopLevelBookmarks),
    }
}

/// The dialog's state in the frame.
pub(in crate::shell) struct SplitDialogState {
    pub(in crate::shell) mode: SplitMode,
    pub(super) value: Entity<SearchInput>,
    pub(in crate::shell) error: Option<String>,
}

impl SplitDialogState {
    pub(super) fn new(theme: ThemeTokens, cx: &mut Context<ShellFrame>) -> Self {
        let value = cx.new(|cx| {
            let mut input = SearchInput::with_placeholder("split-value", "e.g. 10", theme, cx);
            input.set_query("10", cx);
            input
        });
        Self {
            mode: SplitMode::PageCount,
            value,
            error: None,
        }
    }

    pub(super) fn choice(&self, cx: &gpui::App) -> Result<SplitChoice, String> {
        choose(self.mode, self.value.read(cx).query())
    }
}

pub(in crate::shell) fn accessible(
    state: &SplitDialogState,
    rects: &Rects,
    cx: &gpui::App,
) -> Vec<Element> {
    let mut body: Vec<Element> = SplitMode::ALL
        .into_iter()
        .map(|mode| {
            Element::new(mode.id(), Role::RadioButton, mode.label())
                .with_state(A11yState::selected(state.mode == mode))
                .with_activation(Activation::Split(SplitAction::SetMode(mode)))
        })
        .collect();
    if state.mode.takes_value() {
        body.push(
            state
                .value
                .read(cx)
                .accessible(state.mode.label(), TextField::SplitValue),
        );
    }
    if let Some(error) = &state.error {
        body.push(Element::new("split-error", Role::Alert, error.clone()));
    }
    body.push(
        Element::new("split-submit", Role::Button, "Split")
            .with_activation(Activation::Split(SplitAction::Submit)),
    );
    for (row, bounds) in body.iter_mut().zip(rects.of(Surface::SplitDialog)) {
        row.bounds = Some(bounds);
    }
    body
}

pub(in crate::shell) fn render(
    state: &SplitDialogState,
    rects: Rects,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::SplitDialog, &bounds, window);
        })
        .flex()
        .flex_col()
        .gap_2();
    for mode in SplitMode::ALL {
        let chosen = state.mode == mode;
        body = body.child(
            div()
                .id(mode.id())
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(chosen, |row| row.bg(theme.selected))
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::Split(SplitAction::SetMode(mode)), window, cx);
                }))
                .child(format!(
                    "{} {}",
                    if chosen { "(•)" } else { "( )" },
                    mode.label()
                )),
        );
    }
    if state.mode.takes_value() {
        body = body.child(state.value.clone());
    }
    if let Some(error) = &state.error {
        body = body.child(
            div()
                .id("split-error")
                .text_color(theme.error_text)
                .child(error.clone()),
        );
    }
    body.child(button(
        "split-submit",
        "Split",
        true,
        theme,
        focused,
        cx,
        Activation::Split(SplitAction::Submit),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_count_is_a_positive_whole_number() {
        assert_eq!(
            choose(SplitMode::PageCount, " 3 "),
            Ok(SplitChoice::PageCount(NonZeroUsize::new(3).unwrap()))
        );
        assert!(choose(SplitMode::PageCount, "0").is_err());
        assert!(choose(SplitMode::PageCount, "2.5").is_err());
    }

    #[test]
    fn a_file_size_is_megabytes() {
        assert_eq!(
            choose(SplitMode::FileSize, "1.5"),
            Ok(SplitChoice::FileSize { bytes: 1_572_864 })
        );
        assert!(choose(SplitMode::FileSize, "-1").is_err());
        assert!(choose(SplitMode::FileSize, "inf").is_err());
    }

    #[test]
    fn bookmarks_need_no_value() {
        assert_eq!(
            choose(SplitMode::TopLevelBookmarks, "anything"),
            Ok(SplitChoice::TopLevelBookmarks)
        );
    }
}
