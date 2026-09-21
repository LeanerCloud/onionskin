use onionskin_core::metadata::{PropertyField, PropertyTest};
use onionskin_core::{AttachmentHit, AttachmentSearch, SearchOptions};

use super::*;

#[test]
fn the_criterion_cycles_its_property_and_that_propertys_tests() {
    let mut form = AdvancedForm::new(SearchOptions::default());
    assert!(!form.apply(AdvancedAction::NextField), "no criterion yet");
    assert!(!form.apply(AdvancedAction::NextTest));
    assert!(form.apply(AdvancedAction::UseCriterion));
    assert_eq!(
        form.criterion,
        Some((PropertyField::Author, PropertyTest::Contains))
    );
    form.apply(AdvancedAction::NextTest);
    assert_eq!(
        form.criterion,
        Some((PropertyField::Author, PropertyTest::DoesNotContain))
    );
    form.apply(AdvancedAction::NextTest);
    assert_eq!(
        form.criterion,
        Some((PropertyField::Author, PropertyTest::Contains))
    );
    // Moving to a date field starts at its own first test, never a text one.
    let mut field = PropertyField::Author;
    while field != PropertyField::Created {
        form.apply(AdvancedAction::NextField);
        field = form.criterion.expect("still used").0;
    }
    assert_eq!(form.criterion.unwrap().1, PropertyTest::Before);
    for _ in 0..PropertyField::ALL.len() {
        form.apply(AdvancedAction::NextField);
    }
    assert_eq!(
        form.criterion.unwrap().0,
        PropertyField::Created,
        "a full turn"
    );
    assert!(form.apply(AdvancedAction::UseCriterion));
    assert_eq!(form.criterion, None);
    assert!(!form.apply(AdvancedAction::Search));
}

#[test]
fn an_option_that_changes_nothing_says_so() {
    let mut form = AdvancedForm::new(SearchOptions::default());
    assert!(!form.apply(AdvancedAction::Option(FindOption::Mode(
        onionskin_core::MatchMode::Phrase
    ))));
    assert!(form.apply(AdvancedAction::Option(FindOption::WholeWord)));
    assert!(form.options.whole_word);
}

fn outcome(criteria: CriteriaOutcome, attachments: Option<AttachmentSearch>) -> Outcome {
    Outcome {
        criteria,
        attachments,
        error: None,
    }
}

#[test]
fn the_outcome_says_why_nothing_was_searched() {
    let refused = outcome(CriteriaOutcome::NotMatched, None);
    assert_eq!(
        outcome_lines(&refused),
        ["The document does not meet the criteria; nothing was searched."]
    );
    let refused = outcome(CriteriaOutcome::Refused("\"x\" is not a date".into()), None);
    assert_eq!(outcome_lines(&refused), ["\"x\" is not a date"]);
    let empty = Outcome {
        error: Some(NO_WORDS.into()),
        ..outcome(CriteriaOutcome::NotUsed, None)
    };
    assert_eq!(outcome_lines(&empty), [NO_WORDS]);
}

#[test]
fn attachment_hits_are_listed_with_their_path_and_page() {
    let found = AttachmentSearch {
        hits: vec![AttachmentHit {
            path: vec!["annex.pdf".into(), "appendix.pdf".into()],
            page: 2,
            text: "pelican".into(),
        }],
        total: 1,
        skipped: vec!["broken.pdf was not searched: bad".into()],
    };
    let lines = outcome_lines(&outcome(CriteriaOutcome::Matched, Some(found)));
    assert_eq!(
        lines,
        [
            "The document meets the criteria.",
            "The pages' results are in the Search Results pane.",
            "1 result in the PDF attachments:",
            "annex.pdf > appendix.pdf, page 3: pelican",
            "broken.pdf was not searched: bad",
        ]
    );
    let none = outcome_lines(&outcome(
        CriteriaOutcome::NotUsed,
        Some(AttachmentSearch::default()),
    ));
    assert_eq!(none[1], "No results in the PDF attachments.");
    let capped = AttachmentSearch {
        total: 1500,
        hits: vec![
            AttachmentHit {
                path: vec!["a.pdf".into()],
                page: 0,
                text: "x".into(),
            };
            2
        ],
        skipped: Vec::new(),
    };
    assert_eq!(
        attachment_summary(&capped),
        "1500 results in the PDF attachments; the first 2 are listed:"
    );
    let several = AttachmentSearch { total: 2, ..capped };
    assert_eq!(
        attachment_summary(&several),
        "2 results in the PDF attachments:"
    );
}
