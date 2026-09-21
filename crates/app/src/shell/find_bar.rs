//! The find bar: Ctrl+F over the document text, the options Acrobat's find
//! toolbar carries, and the count of what the walk has turned up so far.
//!
//! The bar owns no results. It reads [`SearchState`], which the canvas fills
//! from the search worker one page at a time, so the count and the highlights
//! grow while the walk is still running instead of appearing at the end.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    actions, div, px, App, Context, Entity, InteractiveElement as _, IntoElement, KeyBinding,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{MatchMode, SearchOptions, SearchState};

use super::chrome::accessible::{Activation, Element, Rects, Surface, TextField};
use super::chrome::{SearchInput, ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;

actions!(onionskin_find, [Dismiss, FindNextMatch, FindPreviousMatch]);

/// Only the bar's own subtree carries this, so Enter means "next hit" while
/// the find field has focus and nothing anywhere else.
pub(in crate::shell) const FIND_KEY_CONTEXT: &str = "OnionskinFind";

/// The element id and the placeholder the find field renders with. Named here
/// rather than at the call site so the field and the node describing it cannot
/// be given different identities or different prompts.
/// What the count slot says before there is anything to count.
const NOTHING_TO_COUNT: &str = "No search yet";

pub(in crate::shell) const FIND_INPUT_ID: &str = "find-input";
pub(in crate::shell) const FIND_PLACEHOLDER: &str = "Find in document";

const BOOKMARKS_DEFERRED: &str = "Bookmarks arrive with the navigation panes in M2 P8";

/// The bar's own keys. Opening it is `edit.find` in the keymap, like every
/// other command, so a user who rebinds Ctrl+F rebinds it everywhere.
pub(in crate::shell) fn install_keybindings(cx: &mut App) {
    cx.bind_keys([
        // Escape dismisses the topmost overlay from wherever focus sits, so it
        // is bound window-wide; the handler propagates when there is none.
        KeyBinding::new("escape", Dismiss, None),
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
    /// Also look in the comments' text.
    IncludeComments,
    Mode(MatchMode),
}

/// The bar's three glyph buttons.
///
/// Each is punctuation on screen, and a screen reader reading the glyph says
/// the punctuation, so the name lives here beside the glyph rather than being
/// invented somewhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Previous,
    Next,
    Close,
}

impl Button {
    fn id(self) -> &'static str {
        match self {
            Self::Previous => "find-previous",
            Self::Next => "find-next",
            Self::Close => "find-close",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Previous => "Previous Match",
            Self::Next => "Next Match",
            Self::Close => "Close Find",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Previous => "‹",
            Self::Next => "›",
            Self::Close => "✕",
        }
    }

    fn activation(self) -> Activation {
        match self {
            Self::Previous => Activation::StepFind(FindDirection::Previous),
            Self::Next => Activation::StepFind(FindDirection::Next),
            Self::Close => Activation::DismissFindBar,
        }
    }
}

/// One thing in the query row, in the order the row builds it.
///
/// The row, its accessible description and the rectangles it reports after
/// prepaint all walk this list, so a control cannot be drawn with one name,
/// announced with another and measured as a third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    /// The query field.
    Field,
    /// The count beside it.
    Count,
    Button(Button),
}

impl Item {
    const ROW: [Item; 5] = [
        Item::Field,
        Item::Count,
        Item::Button(Button::Previous),
        Item::Button(Button::Next),
        Item::Button(Button::Close),
    ];
}

/// Acrobat's find-toolbar checkboxes: each is on or off on its own.
const CHECKBOXES: [(&str, &str, FindOption); 3] = [
    ("find-match-case", "Match Case", FindOption::CaseSensitive),
    ("find-whole-word", "Whole Word", FindOption::WholeWord),
    (
        "find-include-comments",
        "Include Comments",
        FindOption::IncludeComments,
    ),
];

/// Acrobat's Return Results Containing: one of three, not three switches.
const MODES: [(&str, &str, FindOption); 3] = [
    (
        "find-mode-phrase",
        "Phrase",
        FindOption::Mode(MatchMode::Phrase),
    ),
    (
        "find-mode-any",
        "Any Word",
        FindOption::Mode(MatchMode::AnyWord),
    ),
    (
        "find-mode-all",
        "All Words",
        FindOption::Mode(MatchMode::AllWords),
    ),
];

/// Row 149's checkbox still to come. It ships disabled with the reason it
/// is, not absent: a user looking for it learns when it arrives.
const DEFERRED: [(&str, &str, &str); 1] = [(
    "find-include-bookmarks",
    "Include Bookmarks",
    BOOKMARKS_DEFERRED,
)];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::shell) struct FindBarState {
    open: bool,
    options: SearchOptions,
}

impl FindBarState {
    /// The bar starts on the Search preferences: Acrobat's find toolbar
    /// remembers its checkboxes, and here they are a setting.
    pub(in crate::shell) fn with_options(options: SearchOptions) -> Self {
        Self {
            open: false,
            options,
        }
    }

    pub(in crate::shell) fn is_open(self) -> bool {
        self.open
    }

    pub(in crate::shell) fn options(self) -> SearchOptions {
        self.options
    }

    /// Take a whole set of options, as Advanced Search hands them over.
    pub(in crate::shell) fn set_options(&mut self, options: SearchOptions) {
        self.options = options;
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
            FindOption::IncludeComments => {
                self.options.include_comments = !self.options.include_comments;
            }
            FindOption::Mode(mode) => self.options.mode = mode,
        }
        self.options != before
    }

    fn is_selected(self, option: FindOption) -> bool {
        match option {
            FindOption::CaseSensitive => self.options.case_sensitive,
            FindOption::WholeWord => self.options.whole_word,
            FindOption::IncludeComments => self.options.include_comments,
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
    pub(in crate::shell) fn failure_label(&self) -> Option<String> {
        let first = self.first_failure.as_ref()?;
        Some(match self.failed_pages {
            1 => format!("1 page could not be read: {first}"),
            count => format!("{count} pages could not be read, first {first}"),
        })
    }
}

/// What the find bar tells a screen reader.
///
/// The query row comes first and in row order, so the rectangles the row
/// reports after prepaint land on the nodes describing it. Everything below
/// the row follows in the order it is drawn.
pub(in crate::shell) fn accessible(
    state: FindBarState,
    summary: &FindSummary,
    query: &str,
    rects: &Rects,
) -> Element {
    let mut bar = Element::new("find-bar", Role::Toolbar, "Find").with_children(
        Item::ROW
            .iter()
            .map(|item| describe(*item, summary, query))
            .collect(),
    );
    // The row is the head of the child list, so its rectangles land before
    // anything the rest of the bar adds.
    rects.place(Surface::FindBar, &mut bar);
    for (id, label, option) in CHECKBOXES {
        bar = bar.child(describe_option(id, label, option, Role::CheckBox, state));
    }
    for (id, label, option) in MODES {
        bar = bar.child(describe_option(id, label, option, Role::RadioButton, state));
    }
    for (id, label, reason) in DEFERRED {
        bar = bar.child(
            Element::new(id, Role::CheckBox, label)
                .with_state(A11yState {
                    toggled: Some(false),
                    selected: None,
                    disabled: true,
                    read_only: false,
                })
                .with_description(reason),
        );
    }
    if let Some(progress) = summary.progress_label() {
        bar = bar.child(Element::new("find-progress", Role::Label, progress));
    }
    if let Some(stopped) = summary.stopped_label() {
        bar = bar.child(Element::new("find-stopped", Role::Alert, stopped));
    }
    if let Some(failure) = summary.failure_label() {
        bar = bar.child(Element::new("find-failure", Role::Alert, failure));
    }
    bar
}

fn describe(item: Item, summary: &FindSummary, query: &str) -> Element {
    match item {
        Item::Field => {
            let mut node = Element::new(FIND_INPUT_ID, Role::SearchInput, "Find In Document")
                .with_value(query)
                .with_activation(Activation::Focus(TextField::Find));
            if query.is_empty() {
                node = node.with_description(FIND_PLACEHOLDER);
            }
            node
        }
        // The count keeps its place with nothing to count, because the row
        // draws it either way and the rectangles are paired by position.
        Item::Count => Element::new(
            "find-count",
            Role::Label,
            // The slot stays even with nothing to count, so the rectangles
            // the row reports keep lining up, but a node with no name is a
            // node a screen reader stops on and says nothing about.
            summary
                .count_label()
                .unwrap_or_else(|| NOTHING_TO_COUNT.to_owned()),
        ),
        Item::Button(button) => Element::new(button.id(), Role::Button, button.name())
            .with_activation(button.activation()),
    }
}

fn describe_option(
    id: &'static str,
    label: &'static str,
    option: FindOption,
    role: Role,
    state: FindBarState,
) -> Element {
    Element::new(id, role, label)
        .with_state(A11yState::toggled(state.is_selected(option)))
        .with_activation(Activation::ApplyFindOption(option))
}

pub(in crate::shell) fn render_find_bar(
    state: FindBarState,
    input: Entity<SearchInput>,
    summary: &FindSummary,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut query_row =
        div()
            .flex()
            .items_center()
            .gap_2()
            .on_children_prepainted(move |bounds, window, _cx| {
                rects.record(Surface::FindBar, &bounds, window);
            });
    for item in Item::ROW {
        query_row = match item {
            Item::Field => query_row.child(div().w(px(200.0)).flex_none().child(input.clone())),
            Item::Count => query_row.child(
                div()
                    .min_w(px(76.0))
                    .text_xs()
                    .text_color(theme.muted_text)
                    .children(summary.count_label()),
            ),
            Item::Button(button) => query_row.child(glyph_button(button, theme, cx)),
        };
    }

    let mut option_row = div().flex().items_center().gap_1();
    for (id, label, option) in CHECKBOXES {
        option_row = option_row.child(option_chip(id, label, option, state, theme, cx));
    }
    option_row = option_row.child(div().w(px(8.0)));
    for (id, label, option) in MODES {
        option_row = option_row.child(option_chip(id, label, option, state, theme, cx));
    }

    let mut bar = div()
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
        .on_action(cx.listener(ShellFrame::dismiss_overlay))
        .on_action(cx.listener(ShellFrame::find_next_match))
        .on_action(cx.listener(ShellFrame::find_previous_match))
        .child(query_row)
        .child(option_row);
    for (_, label, reason) in DEFERRED {
        bar = bar.child(deferred_checkbox(label, reason, theme));
    }
    bar.children(
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

fn glyph_button(
    button: Button,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let activation = button.activation();
    div()
        .id(button.id())
        .h(px(24.0))
        .w(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .cursor_pointer()
        .text_sm()
        .hover(move |style| style.bg(theme.hover))
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(activation.clone(), window, cx);
        }))
        .child(button.glyph())
}

fn option_chip(
    id: &'static str,
    label: &'static str,
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
        .on_click(cx.listener(move |frame, _event, window, cx| {
            frame.run_activation(Activation::ApplyFindOption(option), window, cx);
        }))
        .child(label)
}

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

    /// The step and close buttons draw as "‹", "›" and "✕". A screen reader
    /// reading those says punctuation, so each carries a name made of words.
    #[test]
    fn the_step_and_close_buttons_are_announced_by_name_and_not_by_their_glyph() {
        let described = accessible(
            FindBarState::default(),
            &summary(12, Some(3), false),
            "ink",
            &Rects::default(),
        );

        for button in [Button::Previous, Button::Next, Button::Close] {
            let node = described
                .find(&button.id().into())
                .unwrap_or_else(|| panic!("{} is not in the description", button.id()));
            assert_eq!(node.label, button.name());
            assert_ne!(node.label, button.glyph());
            assert!(
                node.label.chars().any(char::is_alphabetic),
                "{} is announced as {:?}",
                button.id(),
                node.label
            );
            assert_eq!(node.activation.as_ref(), Some(&button.activation()));
        }
        assert_eq!(
            described.find(&"find-close".into()).unwrap().activation,
            Some(Activation::DismissFindBar)
        );
    }

    /// A chosen chip is a background colour on screen and nothing else, so the
    /// state is the only thing that tells a screen reader it is on.
    #[test]
    fn an_option_chip_carries_its_selection_as_state_rather_than_in_its_name() {
        let mut state = FindBarState::default();
        state.apply(FindOption::CaseSensitive);
        state.apply(FindOption::Mode(MatchMode::AllWords));

        let described = accessible(state, &summary(0, None, false), "ink", &Rects::default());

        let case = described.find(&"find-match-case".into()).unwrap();
        assert_eq!(case.role, Role::CheckBox);
        assert_eq!(case.label, "Match Case");
        assert_eq!(case.state.toggled, Some(true));
        assert_eq!(
            case.activation,
            Some(Activation::ApplyFindOption(FindOption::CaseSensitive))
        );
        assert_eq!(
            described
                .find(&"find-whole-word".into())
                .unwrap()
                .state
                .toggled,
            Some(false)
        );

        let all_words = described.find(&"find-mode-all".into()).unwrap();
        assert_eq!(all_words.role, Role::RadioButton);
        assert_eq!(all_words.state.toggled, Some(true));
        assert_eq!(
            described
                .find(&"find-mode-phrase".into())
                .unwrap()
                .state
                .toggled,
            Some(false)
        );
    }

    #[test]
    fn a_deferred_checkbox_is_off_disabled_and_says_when_it_arrives() {
        let described = accessible(
            FindBarState::default(),
            &summary(0, None, false),
            "",
            &Rects::default(),
        );

        let bookmarks = described.find(&"find-include-bookmarks".into()).unwrap();
        assert_eq!(bookmarks.label, "Include Bookmarks");
        assert!(!bookmarks.label.contains('☐'));
        assert!(bookmarks.state.disabled);
        assert_eq!(bookmarks.state.toggled, Some(false));
        assert_eq!(bookmarks.description.as_deref(), Some(BOOKMARKS_DEFERRED));
    }

    /// Include Comments arrived with the Comments pane: a live checkbox that
    /// toggles the option the walk is started with.
    #[test]
    fn include_comments_is_a_live_checkbox_that_changes_the_search() {
        let mut state = FindBarState::default();
        let described = accessible(state, &summary(0, None, false), "", &Rects::default());
        let comments = described.find(&"find-include-comments".into()).unwrap();
        assert!(!comments.state.disabled);
        assert_eq!(comments.state.toggled, Some(false));
        assert_eq!(
            comments.activation,
            Some(Activation::ApplyFindOption(FindOption::IncludeComments))
        );
        assert!(
            state.apply(FindOption::IncludeComments),
            "it changes the query"
        );
        assert!(state.options().include_comments);
        let described = accessible(state, &summary(0, None, false), "", &Rects::default());
        assert_eq!(
            described
                .find(&"find-include-comments".into())
                .unwrap()
                .state
                .toggled,
            Some(true)
        );
    }

    /// Both halves of "one list drives both": the description opens with one
    /// node per rendered query-row item, in the same order, so the rectangles
    /// the row reports after prepaint land on the right nodes.
    #[test]
    fn the_description_opens_with_one_node_per_query_row_item_in_row_order() {
        let described = accessible(
            FindBarState::default(),
            &summary(12, Some(3), true),
            "ink",
            &Rects::default(),
        );

        let keys: Vec<gpui::ElementId> = described
            .children
            .iter()
            .take(Item::ROW.len())
            .map(|child| child.key.clone())
            .collect();
        assert_eq!(
            keys,
            vec![
                FIND_INPUT_ID.into(),
                "find-count".into(),
                "find-previous".into(),
                "find-next".into(),
                "find-close".into(),
            ]
        );
        assert!(described.children.len() > Item::ROW.len());
    }

    /// The count is drawn either way, so its node stays in place: dropping it
    /// would shift every rectangle after it onto the wrong control.
    #[test]
    fn the_count_is_announced_and_keeps_its_place_with_nothing_to_count() {
        let counted = accessible(
            FindBarState::default(),
            &summary(12, Some(3), true),
            "ink",
            &Rects::default(),
        );
        let mut empty = summary(0, None, false);
        empty.has_query = false;
        let uncounted = accessible(FindBarState::default(), &empty, "", &Rects::default());

        assert_eq!(counted.children[1].label, "3 of 12");
        assert_eq!(uncounted.children[1].key, "find-count".into());
        // Named even with nothing to count: a screen reader stops here and
        // would otherwise say only "text".
        assert_eq!(uncounted.children[1].label, NOTHING_TO_COUNT);
    }

    #[test]
    fn the_find_field_reads_what_was_typed_and_its_prompt_until_then() {
        let empty = accessible(
            FindBarState::default(),
            &summary(0, None, false),
            "",
            &Rects::default(),
        );
        let typed = accessible(
            FindBarState::default(),
            &summary(1, Some(1), false),
            "ink",
            &Rects::default(),
        );

        let empty = empty.find(&FIND_INPUT_ID.into()).unwrap();
        assert_eq!(empty.role, Role::SearchInput);
        assert_eq!(empty.label, "Find In Document");
        assert_eq!(empty.description.as_deref(), Some(FIND_PLACEHOLDER));
        assert_eq!(empty.value.as_deref(), Some(""));
        assert_eq!(empty.activation, Some(Activation::Focus(TextField::Find)));
        assert_eq!(
            typed.find(&FIND_INPUT_ID.into()).unwrap().value.as_deref(),
            Some("ink")
        );
        assert_eq!(typed.find(&FIND_INPUT_ID.into()).unwrap().description, None);
    }

    /// A walk that died and a page that could not be read are both on screen,
    /// so both are in the description, and as alerts rather than as text a
    /// screen reader only finds by hunting for it.
    #[test]
    fn a_walk_that_died_and_a_page_that_failed_are_announced_as_alerts() {
        let mut summary = summary(2, Some(1), true);
        summary.stopped = Some("search worker stopped before answering".to_owned());
        summary.failed_pages = 1;
        summary.first_failure = Some("page 9: content stream is not readable".to_owned());

        let described = accessible(FindBarState::default(), &summary, "ink", &Rects::default());

        let progress = described.find(&"find-progress".into()).unwrap();
        assert_eq!(progress.role, Role::Label);
        assert_eq!(progress.label, "3 of 10 pages searched");
        let stopped = described.find(&"find-stopped".into()).unwrap();
        assert_eq!(stopped.role, Role::Alert);
        assert_eq!(
            stopped.label,
            "The search stopped: search worker stopped before answering"
        );
        let failure = described.find(&"find-failure".into()).unwrap();
        assert_eq!(failure.role, Role::Alert);
        assert!(failure.label.contains("1 page could not be read"));
    }
}
