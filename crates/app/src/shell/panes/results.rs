//! The search results pane, fed by P9's document search.
//!
//! The one pane that draws from live state rather than from a snapshot: a
//! walk fills its results in while the pane is open, so reading them once
//! when it opened would show a search that never finished. Everything here
//! comes from `SearchState` on each frame, and clicking a row makes that hit
//! the current one, which is the same cursor the find bar's next and
//! previous move.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{PageIndex, SearchMatch, SearchState};

use super::super::chrome::{ShellFrame, ThemeTokens};
use super::super::Canvas;
use super::{empty_message, list, PaneAction, ROW_HEIGHT};

/// Drawing every hit of a find that matched half a long document would cost
/// more than it tells anyone. The header states the true count either way.
const MAX_ROWS: usize = 500;

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

pub(super) fn render(
    canvas: Option<&Entity<Canvas>>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let Some(canvas) = canvas else {
        return empty_message("No document is open.", theme).into_any_element();
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
            .child(empty_message(
                "No results on the pages searched so far.",
                theme,
            ))
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
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.run_pane_action(PaneAction::SelectMatch(page, index), cx);
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
        body = body.child(empty_message(
            "Only the first results are listed. Narrow the search to see the rest.",
            theme,
        ));
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
}
