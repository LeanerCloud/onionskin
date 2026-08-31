//! The Preferences dialog's body: the categories, and the rows in each.
//!
//! Four categories, because those are the four with a setting M2 can change
//! (see [`crate::preferences`]). Every row is a choice among named values or
//! a switch, and every one of them takes effect where the user can see it:
//! the theme repaints the window, the recents length shortens the list,
//! Page Display decides how the next document opens, Search seeds the find
//! bar's options.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::chrome::{ShellFrame, ThemeTokens};
use crate::preferences::{
    layout_label, PreferenceCategory, Preferences, ThemePreference, ZoomPreference,
    MAX_RECENT_DOCUMENTS,
};
use onionskin_core::{MatchMode, PageLayoutMode};

/// A setting the dialog changed. The frame applies it, saves it and repaints;
/// the rows only say what was clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PreferenceChange {
    Theme(ThemePreference),
    RecentDocuments(usize),
    Layout(PageLayoutMode),
    Zoom(ZoomPreference),
    SearchCaseSensitive(bool),
    SearchWholeWord(bool),
    SearchMode(MatchMode),
}

/// The rows of one category: a title, the choices, and which is in force.
///
/// Built as data rather than as elements so the dialog's content is
/// testable without a window, which is where the "only categories with a
/// real setting" claim is checked.
pub(in crate::shell) struct PreferenceRow {
    pub(in crate::shell) label: &'static str,
    pub(in crate::shell) choices: Vec<PreferenceChoice>,
}

pub(in crate::shell) struct PreferenceChoice {
    pub(in crate::shell) label: String,
    pub(in crate::shell) change: PreferenceChange,
    pub(in crate::shell) selected: bool,
}

/// How many documents the Documents category offers to keep. Acrobat's field
/// takes a number; these are the steps a click can reach without a numeric
/// input, including the zero that turns the list off.
const RECENT_STEPS: [usize; 5] = [0, 5, 10, 20, MAX_RECENT_DOCUMENTS];

pub(in crate::shell) fn category_rows(
    preferences: &Preferences,
    category: PreferenceCategory,
) -> Vec<PreferenceRow> {
    let choice = |label: String, change, selected| PreferenceChoice {
        label,
        change,
        selected,
    };
    match category {
        PreferenceCategory::General => vec![PreferenceRow {
            label: "Display theme",
            choices: ThemePreference::ALL
                .into_iter()
                .map(|theme| {
                    choice(
                        theme.label().to_owned(),
                        PreferenceChange::Theme(theme),
                        preferences.theme == theme,
                    )
                })
                .collect(),
        }],
        PreferenceCategory::Documents => vec![PreferenceRow {
            label: "Documents in recently used list",
            choices: recent_steps(preferences.recent_documents)
                .into_iter()
                .map(|count| {
                    choice(
                        count.to_string(),
                        PreferenceChange::RecentDocuments(count),
                        preferences.recent_documents == count,
                    )
                })
                .collect(),
        }],
        PreferenceCategory::PageDisplay => vec![
            PreferenceRow {
                label: "Page layout",
                choices: LAYOUTS
                    .into_iter()
                    .map(|layout| {
                        choice(
                            layout_label(layout).to_owned(),
                            PreferenceChange::Layout(layout),
                            preferences.layout == layout,
                        )
                    })
                    .collect(),
            },
            PreferenceRow {
                label: "Zoom",
                choices: ZoomPreference::ALL
                    .into_iter()
                    .map(|zoom| {
                        choice(
                            zoom.label().to_owned(),
                            PreferenceChange::Zoom(zoom),
                            preferences.zoom == zoom,
                        )
                    })
                    .collect(),
            },
        ],
        PreferenceCategory::Search => vec![
            PreferenceRow {
                label: "Whole words only",
                choices: switch(
                    preferences.search.whole_word,
                    PreferenceChange::SearchWholeWord,
                ),
            },
            PreferenceRow {
                label: "Case sensitive",
                choices: switch(
                    preferences.search.case_sensitive,
                    PreferenceChange::SearchCaseSensitive,
                ),
            },
            PreferenceRow {
                label: "Return results containing",
                choices: MODES
                    .into_iter()
                    .map(|(label, mode)| {
                        choice(
                            label.to_owned(),
                            PreferenceChange::SearchMode(mode),
                            preferences.search.mode == mode,
                        )
                    })
                    .collect(),
            },
        ],
    }
}

/// The steps the row offers, always including the value in force: the file
/// takes any count up to the maximum, and a dialog showing nothing selected
/// would overwrite a value the user set by hand on the first click.
fn recent_steps(in_force: usize) -> Vec<usize> {
    let mut steps = RECENT_STEPS.to_vec();
    if !steps.contains(&in_force) {
        steps.push(in_force);
        steps.sort_unstable();
    }
    steps
}

fn switch(on: bool, change: fn(bool) -> PreferenceChange) -> Vec<PreferenceChoice> {
    [("On", true), ("Off", false)]
        .into_iter()
        .map(|(label, value)| PreferenceChoice {
            label: label.to_owned(),
            change: change(value),
            selected: on == value,
        })
        .collect()
}

const LAYOUTS: [PageLayoutMode; 4] = [
    PageLayoutMode::SinglePage,
    PageLayoutMode::SinglePageContinuous,
    PageLayoutMode::TwoPage,
    PageLayoutMode::TwoPageContinuous,
];

/// Acrobat's own wording for the three Advanced Search modes, which the find
/// bar already carries.
const MODES: [(&str, MatchMode); 3] = [
    ("Match Exact Word Or Phrase", MatchMode::Phrase),
    ("Any Of The Words", MatchMode::AnyWord),
    ("All Of The Words", MatchMode::AllWords),
];

pub(in crate::shell) fn render_preferences(
    frame: &ShellFrame,
    category: PreferenceCategory,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut categories = div().w(px(180.0)).flex_none().flex().flex_col().gap_1();
    for (index, entry) in PreferenceCategory::ALL.into_iter().enumerate() {
        let selected = entry == category;
        categories = categories.child(
            div()
                .id(("preference-category", index))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.selected))
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    frame.show_preferences(entry, cx);
                }))
                .child(entry.label()),
        );
    }

    let mut settings = div().flex_1().flex().flex_col().gap_3().pl_4();
    for (index, row) in category_rows(frame.preferences(), category)
        .into_iter()
        .enumerate()
    {
        let mut choices = div().flex().flex_wrap().gap_2();
        for (choice_index, choice) in row.choices.into_iter().enumerate() {
            let change = choice.change;
            choices = choices.child(
                div()
                    .id(gpui::ElementId::NamedInteger(
                        format!("preference-choice-{index}").into(),
                        choice_index as u64,
                    ))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(choice.selected, |button| button.bg(theme.selected))
                    .hover(move |button| button.bg(theme.subtle_hover))
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        frame.change_preference(change, cx);
                    }))
                    .child(choice.label),
            );
        }
        settings = settings.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.secondary_text)
                        .child(row.label),
                )
                .child(choices),
        );
    }

    div().flex().child(categories).child(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every category the dialog lists has at least one row, and every row
    /// at least two choices. A category with nothing behind it is the thing
    /// the parity row's "partial" is meant to keep out.
    #[test]
    fn every_category_offers_a_setting_that_changes_something() {
        let preferences = Preferences::default();

        for category in PreferenceCategory::ALL {
            let rows = category_rows(&preferences, category);
            assert!(!rows.is_empty(), "{} has no rows", category.label());
            for row in rows {
                assert!(
                    row.choices.len() >= 2,
                    "{} / {} offers no choice",
                    category.label(),
                    row.label
                );
                assert_eq!(
                    row.choices.iter().filter(|choice| choice.selected).count(),
                    1,
                    "{} / {} does not show exactly one value in force",
                    category.label(),
                    row.label
                );
            }
        }
    }

    /// A count the file allows but the row does not list still shows as the
    /// one in force, so opening the dialog cannot silently change it.
    #[test]
    fn a_count_off_the_offered_steps_is_still_the_one_selected() {
        let preferences = Preferences {
            recent_documents: 7,
            ..Preferences::default()
        };

        let rows = category_rows(&preferences, PreferenceCategory::Documents);

        let selected: Vec<_> = rows[0]
            .choices
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.label.clone())
            .collect();
        assert_eq!(selected, vec!["7".to_owned()]);
    }

    /// The rows show what is in force, not what the defaults were.
    #[test]
    fn the_selected_choice_follows_the_preferences_it_is_given() {
        let preferences = Preferences {
            theme: ThemePreference::Dark,
            ..Preferences::default()
        };

        let rows = category_rows(&preferences, PreferenceCategory::General);

        let selected = rows[0]
            .choices
            .iter()
            .find(|choice| choice.selected)
            .expect("one theme is in force");
        assert_eq!(
            selected.change,
            PreferenceChange::Theme(ThemePreference::Dark)
        );
    }
}
