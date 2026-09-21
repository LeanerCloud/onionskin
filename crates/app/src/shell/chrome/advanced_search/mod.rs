//! Edit > Advanced Search over the document in front (P22).
//!
//! Acrobat's panel, the parts that apply to one open document: the words,
//! the find options, "Include PDF attachments", and one "additional
//! criteria" line over the document's properties. Searching across folders
//! and indexes is the post-1.0 row and is not offered.
//!
//! What a search does:
//!
//! - **Criteria first.** With a criterion set, a document that does not meet
//!   it has no results, as a document outside Acrobat's criteria is not
//!   listed; its pages are not searched.
//! - **The pages** are searched by the find bar's walk, with the dialog's
//!   words and options, and listed in the Search Results pane.
//! - **Attachments**, when asked for, are searched here, two levels deep, and
//!   their hits listed in the dialog with the attachment each came from.

mod view;

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::metadata::{PropertyCriterion, PropertyField, PropertyTest};
use onionskin_core::{AttachmentSearch, SearchOptions};

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};
use crate::shell::find_bar::FindOption;

pub(in crate::shell) use view::{accessible, render};

/// What the dialog's controls do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum AdvancedAction {
    /// A find option: case, whole word, comments, or the match mode.
    Option(FindOption),
    IncludeAttachments,
    /// Turn the additional criterion on or off.
    UseCriterion,
    /// The criterion's property, then its test, each moving to the next.
    NextField,
    NextTest,
    Search,
}

/// Whether the document met the criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) enum CriteriaOutcome {
    NotUsed,
    Matched,
    NotMatched,
    /// The criterion could not be applied; the message says why.
    Refused(String),
}

/// What the last search found, for the dialog to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct Outcome {
    pub(in crate::shell) criteria: CriteriaOutcome,
    /// Present when attachments were searched.
    pub(in crate::shell) attachments: Option<AttachmentSearch>,
    /// Why the search could not run at all, such as no words.
    pub(in crate::shell) error: Option<String>,
}

/// The dialog's choices, apart from what is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct AdvancedForm {
    pub(in crate::shell) options: SearchOptions,
    pub(in crate::shell) include_attachments: bool,
    /// The criterion's property and test, while one is used.
    pub(in crate::shell) criterion: Option<(PropertyField, PropertyTest)>,
}

impl AdvancedForm {
    pub(in crate::shell) fn new(options: SearchOptions) -> Self {
        Self {
            options,
            include_attachments: false,
            criterion: None,
        }
    }

    /// Apply a control that only changes the form. Whether it did; Search is
    /// the frame's to run.
    pub(in crate::shell) fn apply(&mut self, action: AdvancedAction) -> bool {
        match action {
            AdvancedAction::Option(option) => {
                let mut find = crate::shell::find_bar::FindBarState::with_options(self.options);
                let changed = find.apply(option);
                self.options = find.options();
                return changed;
            }
            AdvancedAction::IncludeAttachments => {
                self.include_attachments = !self.include_attachments;
            }
            AdvancedAction::UseCriterion => {
                self.criterion = match self.criterion {
                    Some(_) => None,
                    None => Some(first_criterion(PropertyField::Author)),
                };
            }
            AdvancedAction::NextField => {
                let Some((field, _)) = self.criterion else {
                    return false;
                };
                self.criterion = Some(first_criterion(next_field(field)));
            }
            AdvancedAction::NextTest => {
                let Some((field, test)) = self.criterion else {
                    return false;
                };
                self.criterion = Some((field, next_test(field, test)));
            }
            AdvancedAction::Search => return false,
        }
        true
    }
}

/// The dialog, open.
pub(in crate::shell) struct AdvancedSearchState {
    pub(in crate::shell) query: Entity<SearchInput>,
    pub(in crate::shell) value: Entity<SearchInput>,
    pub(in crate::shell) form: AdvancedForm,
    pub(in crate::shell) outcome: Option<Outcome>,
}

/// The fields' ids, which the tree and the focus ring share.
pub(in crate::shell) const TEXT_FIELDS: [TextField; 2] =
    [TextField::AdvancedQuery, TextField::AdvancedValue];

/// Said when Search is pressed with no words.
pub(in crate::shell) const NO_WORDS: &str = "Type the words to search for.";

impl AdvancedSearchState {
    pub(in crate::shell) fn new(
        query: String,
        options: SearchOptions,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let query = cx.new(|cx| {
            let mut input =
                SearchInput::with_placeholder("advanced-query", "Words to search for", theme, cx);
            input.set_query(query, cx);
            input
        });
        let value = cx.new(|cx| {
            SearchInput::with_placeholder("advanced-value", "Value, or YYYY-MM-DD", theme, cx)
        });
        Self {
            query,
            value,
            form: AdvancedForm::new(options),
            outcome: None,
        }
    }

    pub(in crate::shell) fn text_field(&self, field: TextField) -> Option<&Entity<SearchInput>> {
        match field {
            TextField::AdvancedQuery => Some(&self.query),
            TextField::AdvancedValue => Some(&self.value),
            _ => None,
        }
    }

    /// Apply a form control; a changed form drops the last outcome, which
    /// no longer describes it.
    pub(in crate::shell) fn apply(&mut self, action: AdvancedAction) -> bool {
        let changed = self.form.apply(action);
        if changed {
            self.outcome = None;
        }
        changed
    }

    /// The criterion as the form states it, with the typed value.
    pub(in crate::shell) fn criterion(&self, cx: &gpui::App) -> Option<PropertyCriterion> {
        self.form.criterion.map(|(field, test)| PropertyCriterion {
            field,
            test,
            value: self.value.read(cx).query().trim().to_owned(),
        })
    }
}

fn first_criterion(field: PropertyField) -> (PropertyField, PropertyTest) {
    (field, field.tests()[0])
}

fn next_field(field: PropertyField) -> PropertyField {
    let all = PropertyField::ALL;
    let at = all.iter().position(|each| *each == field).unwrap_or(0);
    all[(at + 1) % all.len()]
}

fn next_test(field: PropertyField, test: PropertyTest) -> PropertyTest {
    let tests = field.tests();
    let at = tests.iter().position(|each| *each == test).unwrap_or(0);
    tests[(at + 1) % tests.len()]
}

/// What the dialog says about a search, one line each.
pub(in crate::shell) fn outcome_lines(outcome: &Outcome) -> Vec<String> {
    if let Some(error) = &outcome.error {
        return vec![error.clone()];
    }
    let mut lines = Vec::new();
    match &outcome.criteria {
        CriteriaOutcome::NotUsed => {}
        CriteriaOutcome::Matched => lines.push("The document meets the criteria.".to_owned()),
        CriteriaOutcome::NotMatched => {
            lines.push("The document does not meet the criteria; nothing was searched.".to_owned());
            return lines;
        }
        CriteriaOutcome::Refused(message) => {
            lines.push(message.clone());
            return lines;
        }
    }
    lines.push("The pages' results are in the Search Results pane.".to_owned());
    if let Some(found) = &outcome.attachments {
        lines.push(attachment_summary(found));
        lines.extend(found.hits.iter().map(|hit| {
            format!(
                "{}, page {}: {}",
                hit.path.join(" > "),
                hit.page + 1,
                hit.text
            )
        }));
        lines.extend(found.skipped.iter().cloned());
    }
    lines
}

fn attachment_summary(found: &AttachmentSearch) -> String {
    match (found.total, found.hits.len()) {
        (0, _) => "No results in the PDF attachments.".to_owned(),
        (1, _) => "1 result in the PDF attachments:".to_owned(),
        (total, listed) if listed < total => {
            format!("{total} results in the PDF attachments; the first {listed} are listed:")
        }
        (total, _) => format!("{total} results in the PDF attachments:"),
    }
}

#[cfg(test)]
mod tests;
