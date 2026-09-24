//! The Preferences dialog's body: the categories, and the rows in each.
//!
//! Four categories, because those are the four with a setting M2 can change
//! (see [`crate::preferences`]). Every row is a choice among named values or
//! a switch, and every one of them takes effect where the user can see it:
//! the theme repaints the window, the recents length shortens the list,
//! Page Display decides how the next document opens and whether every open
//! one draws its line weights, Search seeds the find bar's options.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::chrome::accessible::{Activation, Element};
use super::chrome::{ShellFrame, ThemeTokens};
use crate::a11y::State as A11yState;
use crate::preferences::{
    layout_label, PreferenceCategory, Preferences, ThemePreference, VerificationTime, WebLinks,
    ZoomPreference, MAX_RECENT_DOCUMENTS,
};
use onionskin_core::signatures::TrustAnchor;
use onionskin_core::{MatchMode, PageLayoutMode};

/// A setting the dialog changed. The frame applies it, saves it and repaints;
/// the rows only say what was clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PreferenceChange {
    Theme(ThemePreference),
    RecentDocuments(usize),
    Layout(PageLayoutMode),
    Zoom(ZoomPreference),
    LineWeights(bool),
    /// JavaScript: whether a form's scripts run.
    JavaScript(bool),
    /// Forms: Auto-Complete on (Basic) or off.
    AutoComplete(bool),
    /// Forms: "Remember numerical data".
    AutoCompleteNumbers(bool),
    /// Forms: take the remembered entry at this index out of the list.
    ForgetEntry(usize),
    /// Keep it: the choice in force, which changes nothing.
    KeepEntry(usize),
    /// Forms: forget every remembered entry.
    ClearEntries,
    SearchCaseSensitive(bool),
    SearchWholeWord(bool),
    SearchMode(MatchMode),
    /// Trust Manager: what a web link does.
    WebLinks(WebLinks),
    /// Trust Manager: stop always allowing the site at this index of the
    /// sorted list.
    ForgetSite(usize),
    /// Keep always allowing it: the choice in force, which changes nothing.
    KeepSite(usize),
    /// Signatures: "Verify signatures when the document is opened".
    VerifyOnOpen(bool),
    /// Signatures: "Verify signatures using".
    VerificationTime(VerificationTime),
    /// Signatures: choose a certificate file to trust.
    AddTrustedCertificate,
    /// Signatures: trust the certificate at this index for approval
    /// signatures, or with `certified` for certified documents, the other
    /// way.
    ToggleTrust {
        index: usize,
        certified: bool,
    },
    /// Signatures: stop trusting the certificate at this index.
    RemoveTrusted(usize),
}

/// The rows of one category: a title, the choices, and which is in force.
///
/// Built as data rather than as elements so the dialog's content is
/// testable without a window, which is where the "only categories with a
/// real setting" claim is checked.
pub(in crate::shell) struct PreferenceRow {
    pub(in crate::shell) label: String,
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
        // Commenting's one setting is a name, typed rather than chosen; see
        // `AUTHOR_LABEL`.
        PreferenceCategory::Commenting => Vec::new(),
        PreferenceCategory::General => vec![PreferenceRow {
            label: "Display theme".to_owned(),
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
            label: "Documents in recently used list".to_owned(),
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
                label: "Page layout".to_owned(),
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
                label: "Zoom".to_owned(),
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
            PreferenceRow {
                label: "Use line weights".to_owned(),
                choices: switch(preferences.line_weights, PreferenceChange::LineWeights),
            },
        ],
        PreferenceCategory::Forms => vec![
            PreferenceRow {
                label: "Auto-Complete".to_owned(),
                choices: [("Off", false), ("Basic", true)]
                    .into_iter()
                    .map(|(label, on)| {
                        choice(
                            label.to_owned(),
                            PreferenceChange::AutoComplete(on),
                            preferences.autocomplete == on,
                        )
                    })
                    .collect(),
            },
            PreferenceRow {
                label: "Remember numerical data".to_owned(),
                choices: switch(
                    preferences.autocomplete_numbers,
                    PreferenceChange::AutoCompleteNumbers,
                ),
            },
        ],
        PreferenceCategory::Signatures => vec![
            PreferenceRow {
                label: "Verify signatures when the document is opened".to_owned(),
                choices: switch(preferences.verify_on_open, PreferenceChange::VerifyOnOpen),
            },
            PreferenceRow {
                label: "Verify signatures using".to_owned(),
                choices: VERIFICATION_TIMES
                    .into_iter()
                    .map(|(label, time)| {
                        choice(
                            label.to_owned(),
                            PreferenceChange::VerificationTime(time),
                            preferences.verification_time == time,
                        )
                    })
                    .collect(),
            },
        ],
        PreferenceCategory::JavaScript => vec![PreferenceRow {
            label: "Enable Acrobat JavaScript".to_owned(),
            choices: switch(preferences.javascript, PreferenceChange::JavaScript),
        }],
        PreferenceCategory::TrustManager => {
            let mut rows = vec![PreferenceRow {
                label: "Open web links".to_owned(),
                choices: WebLinks::ALL
                    .into_iter()
                    .map(|links| {
                        choice(
                            links.label().to_owned(),
                            PreferenceChange::WebLinks(links),
                            preferences.web_links == links,
                        )
                    })
                    .collect(),
            }];
            rows.extend(
                preferences
                    .trusted_sites
                    .iter()
                    .enumerate()
                    .map(|(index, site)| PreferenceRow {
                        label: site.clone(),
                        choices: vec![
                            choice(
                                "Always allow".to_owned(),
                                PreferenceChange::KeepSite(index),
                                true,
                            ),
                            choice(
                                "Forget".to_owned(),
                                PreferenceChange::ForgetSite(index),
                                false,
                            ),
                        ],
                    }),
            );
            rows
        }
        PreferenceCategory::Search => vec![
            PreferenceRow {
                label: "Whole words only".to_owned(),
                choices: switch(
                    preferences.search.whole_word,
                    PreferenceChange::SearchWholeWord,
                ),
            },
            PreferenceRow {
                label: "Case sensitive".to_owned(),
                choices: switch(
                    preferences.search.case_sensitive,
                    PreferenceChange::SearchCaseSensitive,
                ),
            },
            PreferenceRow {
                label: "Return results containing".to_owned(),
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

/// Acrobat's wording for the two times offered. Its third, the secure time
/// in a signature's timestamp, waits on timestamps being verified.
const VERIFICATION_TIMES: [(&str, VerificationTime); 2] = [
    ("Current time", VerificationTime::Current),
    (
        "Time at which the signature was created",
        VerificationTime::Creation,
    ),
];

/// Preferences > Signatures' trusted certificates: a count with Add
/// Certificate, then each certificate with what it is trusted for.
fn trusted_rows(trusted: &[TrustAnchor]) -> Vec<PreferenceRow> {
    let heading = PreferenceRow {
        label: match trusted.len() {
            0 => "No trusted certificates".to_owned(),
            1 => "1 trusted certificate".to_owned(),
            count => format!("{count} trusted certificates"),
        },
        choices: vec![PreferenceChoice {
            label: "Add Certificate...".to_owned(),
            change: PreferenceChange::AddTrustedCertificate,
            selected: false,
        }],
    };
    let certificates = trusted.iter().enumerate().map(|(index, anchor)| {
        let certificate = &anchor.certificate;
        let toggle = |label: &str, certified: bool, selected: bool| PreferenceChoice {
            label: label.to_owned(),
            change: PreferenceChange::ToggleTrust { index, certified },
            selected,
        };
        PreferenceRow {
            label: format!(
                "{}, issued by {}, expires {}",
                certificate.display_name(),
                certificate.issuer,
                certificate.not_after
            ),
            choices: vec![
                toggle("Signed documents", false, anchor.for_signatures),
                toggle("Certified documents", true, anchor.for_certified),
                PreferenceChoice {
                    label: "Remove".to_owned(),
                    change: PreferenceChange::RemoveTrusted(index),
                    selected: false,
                },
            ],
        }
    });
    std::iter::once(heading).chain(certificates).collect()
}

/// Commenting's setting: the name every comment is signed with. Acrobat
/// calls it the author name under Commenting and Identity.
pub(in crate::shell) const AUTHOR_LABEL: &str = "Author name";
/// The id the author field publishes.
pub(in crate::shell) const AUTHOR_FIELD_ID: &str = "preference-author-name";
/// The button that saves the typed name.
const AUTHOR_SAVE_ID: &str = "preference-author-save";

/// The element id a choice renders with. Built here so the button and the
/// node describing it cannot be given different identities.
fn choice_id(row: usize, choice: usize) -> gpui::ElementId {
    gpui::ElementId::NamedInteger(format!("preference-choice-{row}").into(), choice as u64)
}

/// What the Preferences body tells a screen reader.
///
/// A chosen category and a chosen value are a background colour on screen and
/// nothing else, so the selected state is the whole point of these nodes.
/// A category's rows, with Forms' remembered entries after its settings,
/// each of which can be removed, and a row to remove them all.
pub(in crate::shell) fn rows_for(
    preferences: &Preferences,
    entries: &[String],
    trusted: &[TrustAnchor],
    category: PreferenceCategory,
) -> Vec<PreferenceRow> {
    let mut rows = category_rows(preferences, category);
    if category == PreferenceCategory::Signatures {
        rows.extend(trusted_rows(trusted));
    }
    if category == PreferenceCategory::Forms {
        rows.push(PreferenceRow {
            label: match entries.len() {
                0 => "No entries remembered".to_owned(),
                1 => "1 entry remembered".to_owned(),
                count => format!("{count} entries remembered"),
            },
            choices: vec![PreferenceChoice {
                label: "Clear All".to_owned(),
                change: PreferenceChange::ClearEntries,
                selected: false,
            }],
        });
        rows.extend(
            entries
                .iter()
                .enumerate()
                .map(|(index, entry)| PreferenceRow {
                    label: entry.clone(),
                    choices: vec![
                        PreferenceChoice {
                            label: "Keep".to_owned(),
                            change: PreferenceChange::KeepEntry(index),
                            selected: true,
                        },
                        PreferenceChoice {
                            label: "Remove".to_owned(),
                            change: PreferenceChange::ForgetEntry(index),
                            selected: false,
                        },
                    ],
                }),
        );
    }
    rows
}

pub(in crate::shell) fn accessible(
    preferences: &Preferences,
    entries: &[String],
    trusted: &[TrustAnchor],
    category: PreferenceCategory,
    author_field: Option<Element>,
) -> Element {
    let categories = Element::new("preference-categories", Role::TabList, "Categories")
        .with_children(
            PreferenceCategory::ALL
                .into_iter()
                .enumerate()
                .map(|(index, entry)| {
                    Element::new(("preference-category", index), Role::Tab, entry.label())
                        .with_state(A11yState::selected(entry == category))
                        .with_activation(Activation::ShowPreferences(entry))
                })
                .collect(),
        );

    let mut described =
        Element::new("preferences", Role::Group, category.label()).child(categories);
    if category == PreferenceCategory::Commenting {
        if let Some(field) = author_field {
            described = described.child(field);
        }
        described = described.child(
            Element::new(AUTHOR_SAVE_ID, Role::Button, "Save Name")
                .with_activation(Activation::SaveCommentingAuthor),
        );
    }
    for (index, row) in rows_for(preferences, entries, trusted, category)
        .into_iter()
        .enumerate()
    {
        described = described.child(
            Element::new(("preference-row", index), Role::RadioGroup, row.label).with_children(
                row.choices
                    .into_iter()
                    .enumerate()
                    .map(|(choice_index, choice)| {
                        Element::new(
                            choice_id(index, choice_index),
                            Role::RadioButton,
                            choice.label,
                        )
                        .with_state(A11yState::selected(choice.selected))
                        .with_activation(Activation::ChangePreference(choice.change))
                    })
                    .collect(),
            ),
        );
    }
    described
}

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
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::ShowPreferences(entry), window, cx);
                }))
                .child(entry.label()),
        );
    }

    let mut settings = div().flex_1().flex().flex_col().gap_3().pl_4();
    if category == PreferenceCategory::Commenting {
        settings = settings.child(render_author_field(frame, theme, cx));
    }
    let entries = frame.autocomplete_entries();
    for (index, row) in rows_for(
        frame.preferences(),
        entries,
        frame.trusted_certificates(),
        category,
    )
    .into_iter()
    .enumerate()
    {
        let mut choices = div().flex().flex_wrap().gap_2();
        for (choice_index, choice) in row.choices.into_iter().enumerate() {
            let change = choice.change;
            choices = choices.child(
                div()
                    .id(choice_id(index, choice_index))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(choice.selected, |button| button.bg(theme.selected))
                    .hover(move |button| button.bg(theme.subtle_hover))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(Activation::ChangePreference(change), window, cx);
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

/// The author name field and its Save button. Enter in the field saves too.
fn render_author_field(
    frame: &ShellFrame,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme.secondary_text)
                .child(AUTHOR_LABEL),
        )
        .child(
            div()
                .key_context(AUTHOR_KEY_CONTEXT)
                .on_action(cx.listener(|frame, _: &SaveAuthorName, window, cx| {
                    frame.run_activation(Activation::SaveCommentingAuthor, window, cx);
                }))
                .flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .w(px(260.0))
                        .p_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(theme.selected)
                        .child(frame.commenting_author_input().clone()),
                )
                .child(
                    div()
                        .id(AUTHOR_SAVE_ID)
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |button| button.bg(theme.subtle_hover))
                        .on_click(cx.listener(|frame, _event, window, cx| {
                            frame.run_activation(Activation::SaveCommentingAuthor, window, cx);
                        }))
                        .child("Save Name"),
                ),
        )
        .child(
            div().text_xs().text_color(theme.muted_text).child(
                "Signs every new comment, reply and status. Leave it empty to sign nothing.",
            ),
        )
}

gpui::actions!(onionskin_preferences, [SaveAuthorName]);

/// The author field's key context, so Enter saves the name instead of
/// activating the focus ring.
const AUTHOR_KEY_CONTEXT: &str = "OnionskinAuthorName";

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([gpui::KeyBinding::new(
        "enter",
        SaveAuthorName,
        Some(AUTHOR_KEY_CONTEXT),
    )]);
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
            if category == PreferenceCategory::Commenting {
                // Its setting is the typed author name, not a choice.
                let described = accessible(&preferences, &[], &[], category, None);
                assert!(described.find(&AUTHOR_SAVE_ID.into()).is_some());
                continue;
            }
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

    /// Trust Manager: the web link policy, then one row per trusted site
    /// whose other choice forgets it.
    #[test]
    fn the_trust_manager_lists_each_trusted_site() {
        let preferences = Preferences {
            trusted_sites: ["b.example".to_owned(), "a.example".to_owned()].into(),
            ..Preferences::default()
        };
        let rows = category_rows(&preferences, PreferenceCategory::TrustManager);
        let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["Open web links", "a.example", "b.example"]);
        assert!(matches!(
            rows[2].choices[1].change,
            PreferenceChange::ForgetSite(1)
        ));
        assert_eq!(rows[0].choices.len(), 3);
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

    /// Page Display's Line Weights switch shows the setting in force and
    /// changes it, as the View menu's entry does.
    #[test]
    fn forms_offers_auto_complete_and_lists_what_it_remembers() {
        let preferences = Preferences::default();
        let none = rows_for(&preferences, &[], &[], PreferenceCategory::Forms);
        let labels: Vec<&str> = none.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Auto-Complete",
                "Remember numerical data",
                "No entries remembered"
            ]
        );
        let on: Vec<_> = none[0]
            .choices
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.label.as_str())
            .collect();
        assert_eq!(on, ["Basic"]);
        assert!(none[1].choices.iter().any(|choice| choice.selected
            && choice.change == PreferenceChange::AutoCompleteNumbers(false)));
        let entries = ["Ada".to_owned(), "Alan".to_owned()];
        let two = rows_for(&preferences, &entries, &[], PreferenceCategory::Forms);
        assert_eq!(two[2].label, "2 entries remembered");
        assert_eq!(two[2].choices[0].change, PreferenceChange::ClearEntries);
        assert_eq!(two[4].label, "Alan");
        assert_eq!(two[4].choices[1].change, PreferenceChange::ForgetEntry(1));
        assert_eq!(
            rows_for(&preferences, &entries[..1], &[], PreferenceCategory::Forms)[2].label,
            "1 entry remembered"
        );
        assert_eq!(
            rows_for(&preferences, &entries, &[], PreferenceCategory::General).len(),
            1,
            "entries are Forms' alone"
        );
    }

    #[test]
    fn javascript_is_a_switch_that_starts_on() {
        let rows = category_rows(&Preferences::default(), PreferenceCategory::JavaScript);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Enable Acrobat JavaScript");
        let selected: Vec<_> = rows[0]
            .choices
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.change)
            .collect();
        assert_eq!(selected, [PreferenceChange::JavaScript(true)]);
    }

    #[test]
    fn page_display_offers_line_weights_as_a_switch() {
        let off = Preferences {
            line_weights: false,
            ..Preferences::default()
        };
        let rows = category_rows(&off, PreferenceCategory::PageDisplay);
        let row = rows
            .iter()
            .find(|row| row.label == "Use line weights")
            .expect("a Line Weights row");
        let selected: Vec<_> = row
            .choices
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| (choice.label.as_str(), choice.change))
            .collect();
        assert_eq!(selected, [("Off", PreferenceChange::LineWeights(false))]);
        assert!(row
            .choices
            .iter()
            .any(|choice| choice.change == PreferenceChange::LineWeights(true)));
    }

    /// A chosen value is a background colour and nothing else on screen, so
    /// the state is the only thing that tells a screen reader which one is in
    /// force.
    #[test]
    fn a_preference_choice_carries_the_value_in_force_as_state() {
        let preferences = Preferences {
            theme: ThemePreference::Dark,
            ..Preferences::default()
        };

        let described = accessible(&preferences, &[], &[], PreferenceCategory::General, None);

        let row = described.find(&("preference-row", 0usize).into()).unwrap();
        assert_eq!(row.role, Role::RadioGroup);
        assert_eq!(row.label, "Display theme");
        let chosen: Vec<&str> = row
            .children
            .iter()
            .filter(|choice| choice.state.selected == Some(true))
            .map(|choice| choice.label.as_str())
            .collect();
        assert_eq!(chosen, vec![ThemePreference::Dark.label()]);
        assert!(row
            .children
            .iter()
            .all(|choice| choice.state.selected.is_some()));
        let dark = row
            .children
            .iter()
            .find(|choice| choice.label == ThemePreference::Dark.label())
            .unwrap();
        assert_eq!(
            dark.activation,
            Some(Activation::ChangePreference(PreferenceChange::Theme(
                ThemePreference::Dark
            )))
        );
    }

    /// Every choice is described under the id it renders with, so a screen
    /// reader pressing one presses the button the mouse would.
    #[test]
    fn every_described_choice_is_keyed_as_the_button_it_describes() {
        let preferences = Preferences::default();

        let described = accessible(
            &preferences,
            &[],
            &[],
            PreferenceCategory::PageDisplay,
            None,
        );

        for (index, row) in category_rows(&preferences, PreferenceCategory::PageDisplay)
            .into_iter()
            .enumerate()
        {
            for choice_index in 0..row.choices.len() {
                assert!(
                    described.find(&choice_id(index, choice_index)).is_some(),
                    "row {index} choice {choice_index} is not described"
                );
            }
        }
        assert!(described.find(&choice_id(0, 99)).is_none());
    }

    #[test]
    fn the_category_list_says_which_category_is_showing() {
        let described = accessible(
            &Preferences::default(),
            &[],
            &[],
            PreferenceCategory::Search,
            None,
        );

        let categories = described.find(&"preference-categories".into()).unwrap();
        assert_eq!(categories.role, Role::TabList);
        assert_eq!(categories.children.len(), PreferenceCategory::ALL.len());
        for (index, entry) in PreferenceCategory::ALL.into_iter().enumerate() {
            let tab = described
                .find(&("preference-category", index).into())
                .unwrap();
            assert_eq!(tab.role, Role::Tab);
            assert_eq!(tab.label, entry.label());
            assert_eq!(
                tab.state.selected,
                Some(entry == PreferenceCategory::Search)
            );
            assert_eq!(tab.activation, Some(Activation::ShowPreferences(entry)));
        }
    }
}
