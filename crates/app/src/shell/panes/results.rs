//! The search results pane, fed by P9's document search.
//!
//! The one pane that draws from live state rather than from a snapshot: a
//! walk fills its results in while the pane is open, so reading them once
//! when it opened would show a search that never finished. Everything here
//! comes from `SearchState` on each frame, and clicking a row makes that hit
//! the current one, which is the same cursor the find bar's next and
//! previous move.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{PageIndex, SearchMatch, SearchState};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::{empty_message, list, PaneAction, ROW_HEIGHT};
use crate::a11y::State as A11yState;

/// Drawing every hit of a find that matched half a long document would cost
/// more than it tells anyone. The header states the true count either way.
const MAX_ROWS: usize = 500;
/// Said in place of the rows while the walk has found nothing.
const NO_RESULTS: &str = "No results on the pages searched so far.";
/// Said after the last row when the find matched more than the pane draws.
const CAPPED: &str = "Only the first results are listed. Narrow the search to see the rest.";

/// One row: which hit it is, in the terms the cursor uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultRow {
    page: PageIndex,
    /// Position among that page's hits, which is the other half of the
    /// cursor. Derived by counting, because the hits arrive in page order
    /// and a hit's position on its page is what `SearchState` selects by.
    index: usize,
    text: String,
    current: bool,
}

impl ResultRow {
    /// What the row says, in the order it draws it: the page it sits on, then
    /// the line the find matched.
    fn announcement(&self) -> String {
        format!("Page {}. {}", self.page + 1, self.text)
    }
}

/// The hits found so far, in document order, capped.
fn rows(search: &SearchState) -> Vec<ResultRow> {
    rows_from(search.matches(), search.cursor())
}

/// The row list, given the hits and where the cursor is.
///
/// Split from the state it usually comes from because the numbering is the
/// part that can be wrong: a hit's index is its position among its own
/// page's hits, not its position in the list.
fn rows_from<'a>(
    matches: impl Iterator<Item = &'a SearchMatch>,
    cursor: Option<(PageIndex, usize)>,
) -> Vec<ResultRow> {
    let mut previous: Option<(PageIndex, usize)> = None;
    let mut rows = Vec::new();
    for hit in matches.take(MAX_ROWS) {
        let index = match previous {
            Some((page, index)) if page == hit.page => index + 1,
            _ => 0,
        };
        previous = Some((hit.page, index));
        rows.push(ResultRow {
            page: hit.page,
            index,
            text: hit.text.clone(),
            current: cursor == Some((hit.page, index)),
        });
    }
    rows
}

/// The line above the rows.
fn summary(search: &SearchState) -> String {
    summary_of(
        search.needle(),
        search.len(),
        search.is_running(),
        search.stopped(),
        search.failures().len(),
    )
}

/// How many hits, whether the walk is still going, and whatever stopped it.
///
/// A walk that died has to say so beside the results it did find, or a short
/// list reads as a finished search that found little.
fn summary_of(
    needle: &str,
    found: usize,
    running: bool,
    stopped: Option<&str>,
    failures: usize,
) -> String {
    if needle.is_empty() {
        return "Use Find to search this document.".to_owned();
    }
    let mut summary = format!("{found} results for {needle:?}");
    if running {
        summary.push_str(", still searching");
    }
    if let Some(stopped) = stopped {
        summary.push_str(&format!(", stopped: {stopped}"));
    }
    if failures > 0 {
        summary.push_str(&format!(", {failures} pages could not be read"));
    }
    summary
}

/// What the search results pane tells a screen reader.
///
/// The header, the rows and the two notes, in the order the pane draws them,
/// because a reader that heard only the rows would read a capped list as the
/// whole find.
pub(super) fn accessible(
    canvas: Option<&Entity<Canvas>>,
    cx: &Context<ShellFrame>,
) -> Vec<Element> {
    let Some(canvas) = canvas else {
        return vec![Element::new(
            "search-result-rows-empty",
            Role::Label,
            super::NO_DOCUMENT,
        )];
    };
    let search = canvas.read(cx).model.search();
    let found = search.len();
    let rows = rows(search);
    let capped = found > rows.len();
    described(&summary(search), rows, capped)
}

/// The description, given the header, the rows and whether the find had more
/// than the pane draws.
///
/// Split from the state it usually comes from for the same reason [`rows_from`]
/// is: what the rows say and which hit each one selects is the part that can
/// be wrong.
fn described(header: &str, rows: Vec<ResultRow>, capped: bool) -> Vec<Element> {
    let mut children = vec![Element::new("search-result-summary", Role::Label, header)];
    if rows.is_empty() {
        children.push(Element::new("search-result-none", Role::Label, NO_RESULTS));
    } else {
        children.extend(rows.into_iter().enumerate().map(|(position, row)| {
            Element::new(
                ("search-result-row", position),
                Role::ListItem,
                row.announcement(),
            )
            .with_state(A11yState::selected(row.current))
            .with_activation(Activation::Pane(PaneAction::SelectMatch(row.page, row.index)))
        }));
        if capped {
            children.push(Element::new("search-result-capped", Role::Label, CAPPED));
        }
    }
    vec![Element::new("search-result-rows", Role::List, "Search Results").with_children(children)]
}

pub(super) fn render(
    canvas: Option<&Entity<Canvas>>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let Some(canvas) = canvas else {
        return empty_message(super::NO_DOCUMENT, theme).into_any_element();
    };
    let search = canvas.read(cx).model.search();
    let header = summary(search);
    let found = search.len();
    let rows = rows(search);
    let capped = found > rows.len();

    let mut body = list("search-result-rows").child(
        div()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(theme.muted_text)
            .child(header),
    );
    if rows.is_empty() {
        return body
            .child(empty_message(NO_RESULTS, theme))
            .into_any_element();
    }
    for (position, row) in rows.into_iter().enumerate() {
        let (page, index) = (row.page, row.index);
        body = body.child(
            div()
                .id(("search-result-row", position))
                .min_h(px(ROW_HEIGHT))
                .flex()
                .flex_col()
                .justify_center()
                .px_2()
                .py_1()
                .cursor_pointer()
                .when(row.current, |element| element.bg(theme.selected))
                .hover(move |element| element.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::Pane(PaneAction::SelectMatch(page, index)),
                        window,
                        cx,
                    );
                }))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_text)
                        .child(format!("Page {}", row.page + 1)),
                )
                .child(div().text_sm().text_color(theme.text).child(row.text)),
        );
    }
    if capped {
        body = body.child(empty_message(CAPPED, theme));
    }
    body.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(page: PageIndex, text: &str) -> SearchMatch {
        SearchMatch {
            page,
            text: text.to_owned(),
            quads: Vec::new(),
        }
    }

    /// A row's index is its position among its own page's hits, which is what
    /// the cursor selects by. Numbering rows straight through the list would
    /// select the wrong hit on every page after the first.
    #[test]
    fn a_rows_index_is_its_position_on_its_own_page() {
        let matches = [
            hit(0, "alpha one"),
            hit(0, "alpha two"),
            hit(4, "alpha three"),
            hit(4, "alpha four"),
            hit(4, "alpha five"),
        ];

        let rows = rows_from(matches.iter(), None);

        assert_eq!(
            rows.iter()
                .map(|row| (row.page, row.index))
                .collect::<Vec<_>>(),
            [(0, 0), (0, 1), (4, 0), (4, 1), (4, 2)]
        );
        assert_eq!(rows[2].text, "alpha three");
    }

    /// The cursor the find bar moves is the one the pane marks, so the hit
    /// the user is on is the row drawn as current. Exactly one row, even
    /// when another page has a hit at the same position.
    #[test]
    fn exactly_the_hit_under_the_cursor_is_marked_current() {
        let matches = [hit(0, "alpha"), hit(4, "alpha"), hit(4, "alpha")];

        let rows = rows_from(matches.iter(), Some((4, 0)));

        let current: Vec<_> = rows
            .iter()
            .filter(|row| row.current)
            .map(|row| (row.page, row.index))
            .collect();
        assert_eq!(current, [(4, 0)]);
    }

    #[test]
    fn a_cursor_on_a_hit_the_walk_no_longer_has_marks_nothing() {
        let matches = [hit(0, "alpha")];

        let rows = rows_from(matches.iter(), Some((9, 3)));

        assert!(rows.iter().all(|row| !row.current));
    }

    /// A long find lists a bounded number of rows and still reports the true
    /// count, so the pane cannot claim the document has only 500 hits.
    #[test]
    fn a_very_long_result_list_is_capped_but_the_count_is_not() {
        let matches: Vec<_> = (0..MAX_ROWS + 50)
            .map(|hit| self::hit(hit, "alpha"))
            .collect();

        let rows = rows_from(matches.iter(), None);

        assert_eq!(rows.len(), MAX_ROWS);
        assert!(summary_of("alpha", matches.len(), false, None, 0).contains("550 results"));
    }

    /// A walk still running says so, so a partial list is not read as a
    /// finished search, and one that died says what stopped it.
    #[test]
    fn the_summary_reports_the_count_and_how_the_walk_ended() {
        assert_eq!(
            summary_of("alpha", 2, false, None, 0),
            "2 results for \"alpha\""
        );
        assert!(summary_of("alpha", 2, true, None, 0).contains("still searching"));

        let stopped = summary_of("alpha", 1, false, Some("the worker died"), 2);
        assert!(stopped.contains("1 results"), "{stopped}");
        assert!(stopped.contains("the worker died"), "{stopped}");
        assert!(stopped.contains("2 pages could not be read"), "{stopped}");
    }

    #[test]
    fn a_pane_with_no_query_says_where_results_come_from() {
        let summary = summary_of("", 0, false, None, 0);

        assert!(summary.contains("Find"), "{summary}");
    }

    /// One described row per drawn row, in the same order and with the same
    /// key, each selecting the hit its click selects. A row announced with
    /// another row's coordinates would send a reader to the wrong hit.
    #[test]
    fn the_described_rows_are_the_drawn_rows_and_select_the_hit_they_name() {
        let matches = [hit(0, "alpha one"), hit(4, "alpha two"), hit(4, "alpha three")];
        let rows = rows_from(matches.iter(), Some((4, 0)));

        let described = described("3 results for \"alpha\"", rows.clone(), false);
        let children = &described[0].children;

        assert_eq!(described[0].role, Role::List);
        assert_eq!(children.len(), rows.len() + 1, "the header is drawn first");
        assert_eq!(children[0].label, "3 results for \"alpha\"");
        for (position, (child, row)) in children[1..].iter().zip(rows.iter()).enumerate() {
            assert_eq!(
                child.key,
                gpui::ElementId::from(("search-result-row", position))
            );
            assert_eq!(child.label, format!("Page {}. {}", row.page + 1, row.text));
            assert_eq!(
                child.activation,
                Some(Activation::Pane(PaneAction::SelectMatch(
                    row.page, row.index
                )))
            );
        }
        assert_eq!(children[2].state.selected, Some(true), "the cursor is on (4, 0)");
        assert_eq!(children[1].state.selected, Some(false));
        assert_eq!(children[3].state.selected, Some(false));
    }

    /// A capped list says so after its last row. A reader that heard only the
    /// rows would take the first five hundred for the whole find.
    #[test]
    fn a_capped_list_says_so_after_the_last_row() {
        let matches = [hit(0, "alpha")];
        let rows = rows_from(matches.iter(), None);

        let capped = described("550 results for \"alpha\"", rows.clone(), true);
        let whole = described("1 results for \"alpha\"", rows, false);

        let last = capped[0].children.last().expect("a capped list has rows");
        assert_eq!(last.label, CAPPED);
        assert_eq!(capped[0].children.len(), whole[0].children.len() + 1);
    }

    /// A find that has matched nothing yet says where the rows would be,
    /// rather than leaving a list with only a header in it.
    #[test]
    fn a_find_with_no_hits_yet_says_so_in_place_of_the_rows() {
        let described = described("0 results for \"alpha\"", Vec::new(), false);

        assert_eq!(described[0].children.len(), 2);
        assert_eq!(described[0].children[1].label, NO_RESULTS);
    }
}
