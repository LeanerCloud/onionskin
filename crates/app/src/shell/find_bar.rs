//! The find bar: Ctrl+F over the document text, the options Acrobat's find
//! toolbar carries, and the count of what the walk has turned up so far.
//!
//! The bar owns no results. It reads [`SearchState`], which the canvas fills
//! from the search worker one page at a time, so the count and the highlights
//! grow while the walk is still running instead of appearing at the end.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    actions, div, px, App, Context, Entity, InteractiveElement as _, IntoElement, KeyBinding,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{MatchMode, SearchOptions, SearchState};

use super::chrome::{SearchInput, ShellFrame, ThemeTokens};

actions!(
    onionskin_find,
    [OpenFindBar, CloseFindBar, FindNextMatch, FindPreviousMatch]
);

/// Only the bar's own subtree carries this, so Enter means "next hit" while
/// the find field has focus and nothing anywhere else.
pub(in crate::shell) const FIND_KEY_CONTEXT: &str = "OnionskinFind";

const BOOKMARKS_DEFERRED: &str = "Bookmarks arrive with the navigation panes in M2 P8";
const COMMENTS_DEFERRED: &str = "Comments arrive with the comment tools in M3";

pub(in crate::shell) fn install_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-f", OpenFindBar, None),
        KeyBinding::new("cmd-f", OpenFindBar, None),
        // Escape closes the bar from wherever focus sits, so it is bound
        // window-wide; the handler propagates when the bar is already closed.
        KeyBinding::new("escape", CloseFindBar, None),
        KeyBinding::new("enter", FindNextMatch, Some(FIND_KEY_CONTEXT)),
        KeyBinding::new("shift-enter", FindPreviousMatch, Some(FIND_KEY_CONTEXT)),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FindDirection {
    Next,
    Previous,
}

/// The options the bar can change. Case and whole word are Acrobat's find
/// toolbar checkboxes; the three modes are its Return Results Containing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FindOption {
    CaseSensitive,
    WholeWord,
    Mode(MatchMode),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::shell) struct FindBarState {
    open: bool,
    options: SearchOptions,
}

impl FindBarState {
    pub(in crate::shell) fn is_open(self) -> bool {
        self.open
    }

    pub(in crate::shell) fn options(self) -> SearchOptions {
        self.options
    }

    pub(in crate::shell) fn open(&mut self) {
        self.open = true;
    }

    pub(in crate::shell) fn close(&mut self) {
        self.open = false;
    }

    /// Returns whether the options changed, which is what decides if the
    /// search has to run again.
    pub(in crate::shell) fn apply(&mut self, option: FindOption) -> bool {
        let before = self.options;
        match option {
            FindOption::CaseSensitive => {
                self.options.case_sensitive = !self.options.case_sensitive;
            }
            FindOption::WholeWord => self.options.whole_word = !self.options.whole_word,
            FindOption::Mode(mode) => self.options.mode = mode,
        }
        self.options != before
    }

    fn is_selected(self, option: FindOption) -> bool {
        match option {
            FindOption::CaseSensitive => self.options.case_sensitive,
            FindOption::WholeWord => self.options.whole_word,
            FindOption::Mode(mode) => self.options.mode == mode,
        }
    }
}

/// What the bar prints about the walk. Built per frame from the live state, so
/// there is nothing here to keep in step with the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct FindSummary {
    has_query: bool,
    matches: usize,
    current: Option<usize>,
    running: bool,
    searched_pages: usize,
    page_count: usize,
    failed_pages: usize,
    first_failure: Option<String>,
    stopped: Option<String>,
}

impl FindSummary {
    pub(in crate::shell) fn new(state: &SearchState, page_count: usize) -> Self {
        Self {
            has_query: !state.needle().is_empty(),
            matches: state.len(),
            current: state.current_ordinal(),
            running: state.is_running(),
            searched_pages: state.searched_pages(),
            page_count,
            failed_pages: state.failures().len(),
            first_failure: state.failures().first().map(ToString::to_string),
            stopped: state.stopped().map(str::to_owned),
        }
    }

    /// The count, or the state that stands in for one. A hit found is always
    /// a hit in hand: the first page to report one places the cursor, so there
    /// is no counted-but-unselected state to print.
    fn count_label(&self) -> Option<String> {
        if !self.has_query {
            return None;
        }
        Some(match self.current {
            Some(current) => format!("{current} of {}", self.matches),
            None if self.running => "Searching".to_owned(),
            None => "No results".to_owned(),
        })
    }

    /// Progress, while there is progress left to report.
    fn progress_label(&self) -> Option<String> {
        self.running.then(|| {
            format!(
                "{} of {} pages searched",
                self.searched_pages, self.page_count
            )
        })
    }

    /// The walk died before it finished. Said plainly: the hits on screen are
    /// not the whole document, and the next query starts a new worker.
    fn stopped_label(&self) -> Option<String> {
        self.stopped
            .as_ref()
            .map(|error| format!("The search stopped: {error}"))
    }

    /// A page whose text could not be read is reported here rather than
    /// dropped from the count, so a find that missed something says so.
    fn failure_label(&self) -> Option<String> {
        let first = self.first_failure.as_ref()?;
        Some(match self.failed_pages {
            1 => format!("1 page could not be read: {first}"),
            count => format!("{count} pages could not be read, first {first}"),
        })
    }
}

pub(in crate::shell) fn render_find_bar(
    state: FindBarState,
    input: Entity<SearchInput>,
    summary: &FindSummary,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let query_row = div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().w(px(200.0)).flex_none().child(input))
        .child(
            div()
                .min_w(px(76.0))
                .text_xs()
                .text_color(theme.muted_text)
                .children(summary.count_label()),
        )
        .child(step_button(
            "find-previous",
            "‹",
            FindDirection::Previous,
            theme,
            cx,
        ))
        .child(step_button(
            "find-next",
            "›",
            FindDirection::Next,
            theme,
            cx,
        ))
        .child(
            div()
                .id("find-close")
                .h(px(24.0))
                .w(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .cursor_pointer()
                .text_sm()
                .hover(move |button| button.bg(theme.hover))
                .on_click(cx.listener(|frame, _event, _window, cx| {
                    frame.dismiss_find_bar(cx);
                }))
                .child("✕"),
        );

    let option_row = div()
        .flex()
        .items_center()
        .gap_1()
        .child(option_chip(
            "find-match-case",
            "Match Case",
            FindOption::CaseSensitive,
            state,
            theme,
            cx,
        ))
        .child(option_chip(
            "find-whole-word",
            "Whole Word",
            FindOption::WholeWord,
            state,
            theme,
            cx,
        ))
        .child(div().w(px(8.0)))
        .child(option_chip(
            "find-mode-phrase",
            "Phrase",
            FindOption::Mode(MatchMode::Phrase),
            state,
            theme,
            cx,
        ))
        .child(option_chip(
            "find-mode-any",
            "Any Word",
            FindOption::Mode(MatchMode::AnyWord),
            state,
            theme,
            cx,
        ))
        .child(option_chip(
            "find-mode-all",
            "All Words",
            FindOption::Mode(MatchMode::AllWords),
            state,
            theme,
            cx,
        ));

    div()
        .id("find-bar")
        .key_context(FIND_KEY_CONTEXT)
        .absolute()
        .top(px(8.0))
        .right(px(8.0))
        .w(px(430.0))
        .p_2()
        .flex()
        .flex_col()
        .gap_1()
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text)
        .on_action(cx.listener(ShellFrame::close_find_bar))
        .on_action(cx.listener(ShellFrame::find_next_match))
        .on_action(cx.listener(ShellFrame::find_previous_match))
        .child(query_row)
        .child(option_row)
        .child(deferred_checkbox(
            "Include Bookmarks",
            BOOKMARKS_DEFERRED,
            theme,
        ))
        .child(deferred_checkbox(
            "Include Comments",
            COMMENTS_DEFERRED,
            theme,
        ))
        .children(
            summary
                .progress_label()
                .map(|progress| div().text_xs().text_color(theme.muted_text).child(progress)),
        )
        .children(
            summary
                .stopped_label()
                .into_iter()
                .chain(summary.failure_label())
                .map(|problem| {
                    div()
                        .px_1()
                        .py_1()
                        .rounded_sm()
                        .bg(theme.error_surface)
                        .text_xs()
                        .text_color(theme.error_text)
                        .child(problem)
                }),
        )
}

fn step_button(
    id: &'static str,
    label: &'static str,
    direction: FindDirection,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(24.0))
        .w(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .cursor_pointer()
        .text_sm()
        .hover(move |button| button.bg(theme.hover))
        .on_click(cx.listener(move |frame, _event, _window, cx| {
            frame.step_find(direction, cx);
        }))
        .child(label)
}

fn option_chip(
    id: &'static str,
    label: impl Into<SharedString>,
    option: FindOption,
    state: FindBarState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let selected = state.is_selected(option);
    div()
        .id(id)
        .h(px(22.0))
        .px_2()
        .flex()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .text_xs()
        .when(selected, |chip| chip.bg(theme.selected))
        .hover(move |chip| chip.bg(theme.hover))
        .on_click(cx.listener(move |frame, _event, _window, cx| {
            frame.apply_find_option(option, cx);
        }))
        .child(label.into())
}

/// Row 149's two checkboxes. They ship disabled with the reason they are, not
/// absent: a user looking for them learns when they arrive.
fn deferred_checkbox(
    label: &'static str,
    reason: &'static str,
    theme: ThemeTokens,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(theme.disabled_text)
        .child(format!("☐ {label}"))
        .child(div().child(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(matches: usize, current: Option<usize>, running: bool) -> FindSummary {
        FindSummary {
            has_query: true,
            matches,
            current,
            running,
            searched_pages: 3,
            page_count: 10,
            failed_pages: 0,
            first_failure: None,
            stopped: None,
        }
    }

    #[test]
    fn the_count_reads_as_the_position_in_the_hits_found_so_far() {
        assert_eq!(
            summary(12, Some(3), true).count_label().as_deref(),
            Some("3 of 12")
        );
        assert_eq!(
            summary(1, Some(1), false).count_label().as_deref(),
            Some("1 of 1")
        );
    }

    #[test]
    fn no_results_is_only_said_once_the_walk_is_over() {
        assert_eq!(
            summary(0, None, true).count_label().as_deref(),
            Some("Searching")
        );
        assert_eq!(
            summary(0, None, false).count_label().as_deref(),
            Some("No results")
        );
    }

    #[test]
    fn an_empty_query_has_nothing_to_count() {
        let mut summary = summary(0, None, false);
        summary.has_query = false;

        assert_eq!(summary.count_label(), None);
        assert_eq!(summary.progress_label(), None);
    }

    #[test]
    fn progress_is_reported_while_the_walk_runs_and_not_after() {
        assert_eq!(
            summary(1, Some(1), true).progress_label().as_deref(),
            Some("3 of 10 pages searched")
        );
        assert_eq!(summary(1, Some(1), false).progress_label(), None);
    }

    #[test]
    fn a_walk_that_died_says_so_beside_the_hits_it_did_find() {
        let mut summary = summary(2, Some(1), false);
        summary.stopped = Some("search worker stopped before answering".to_owned());

        assert_eq!(
            summary.stopped_label().as_deref(),
            Some("The search stopped: search worker stopped before answering")
        );
        assert_eq!(summary.count_label().as_deref(), Some("1 of 2"));
    }

    #[test]
    fn a_page_that_could_not_be_read_is_named_rather_than_dropped() {
        let mut one = summary(1, Some(1), false);
        one.failed_pages = 1;
        one.first_failure = Some("page 9: content stream is not readable".to_owned());
        let mut several = one.clone();
        several.failed_pages = 4;

        assert_eq!(
            one.failure_label().as_deref(),
            Some("1 page could not be read: page 9: content stream is not readable")
        );
        assert_eq!(
            several.failure_label().as_deref(),
            Some("4 pages could not be read, first page 9: content stream is not readable")
        );
        assert_eq!(summary(1, Some(1), false).failure_label(), None);
    }

    #[test]
    fn a_summary_reads_the_search_state_it_is_given() {
        let mut state = SearchState::default();
        state.set_query("alpha", SearchOptions::default());

        let summary = FindSummary::new(&state, 42);

        assert!(summary.has_query);
        assert_eq!(summary.matches, 0);
        assert_eq!(summary.page_count, 42);
        assert!(!summary.running);
        assert_eq!(summary.count_label().as_deref(), Some("No results"));
    }

    #[test]
    fn the_bar_opens_and_closes_without_touching_its_options() {
        let mut state = FindBarState::default();
        state.apply(FindOption::WholeWord);

        state.open();
        assert!(state.is_open());
        state.close();
        assert!(!state.is_open());
        // Closing keeps the options, so reopening searches the way the user
        // last asked rather than resetting under them.
        assert!(state.options().whole_word);
    }

    #[test]
    fn the_toggles_flip_and_the_modes_replace_one_another() {
        let mut state = FindBarState::default();

        assert!(state.apply(FindOption::CaseSensitive));
        assert!(state.options().case_sensitive);
        assert!(state.is_selected(FindOption::CaseSensitive));
        assert!(state.apply(FindOption::CaseSensitive));
        assert!(!state.options().case_sensitive);

        assert!(state.apply(FindOption::WholeWord));
        assert!(state.options().whole_word);

        assert!(state.is_selected(FindOption::Mode(MatchMode::Phrase)));
        assert!(state.apply(FindOption::Mode(MatchMode::AllWords)));
        assert_eq!(state.options().mode, MatchMode::AllWords);
        assert!(!state.is_selected(FindOption::Mode(MatchMode::Phrase)));
        // Choosing the mode already chosen changes nothing, so it does not
        // restart the walk.
        assert!(!state.apply(FindOption::Mode(MatchMode::AllWords)));
    }
}
