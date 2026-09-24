//! Guarantee test 7's harness on a form of our own: a scenario replayed,
//! compared with what was recorded, and every difference named.

use onionskin_core::Document;
use onionskin_tools_form::replay::{parse_expectation, parse_scenario, replay, Action, Step};

mod common;

use common::document;

const SCENARIO: &str = r#"[
    {"field": "price", "action": "enter", "value": "1234.5"},
    {"field": "qty", "action": "blur", "value": "2"},
    {"field": "agree", "action": "check", "value": "Yes"},
    {"field": "size", "action": "check", "value": "L"},
    {"field": "age", "action": "enter", "value": "200"}
]"#;

fn expected(total: &str) -> String {
    format!(
        r#"{{
        "acrobat": "the version it was recorded in",
        "fields": {{
            "total": {{"value": {total}, "display": "$2,469.00"}},
            "price": {{"display": "1,234.50"}},
            "agree": {{"value": "Yes"}},
            "size": {{"value": "L"}},
            "age": {{"value": "", "display": ""}}
        }},
        "alerts": ["The value entered must be greater than or equal to 0 and less than or equal to 130."]
    }}"#
    )
}

fn run(expected: &str) -> Vec<String> {
    let mut doc = Document::open_bytes(document()).expect("opens");
    let steps = parse_scenario(SCENARIO).expect("a scenario");
    let expectation = parse_expectation(expected).expect("an expectation");
    replay(&mut doc, &steps, &expectation)
}

#[test]
fn a_session_filled_the_way_it_was_recorded_has_no_differences() {
    assert_eq!(run(&expected("\"2469\"")), Vec::<String>::new());
    assert_eq!(
        run(&expected("[\"2470\", \"2469\"]")),
        Vec::<String>::new(),
        "a value two versions disagree on passes on either"
    );
}

#[test]
fn every_difference_is_named() {
    let differences = run(&expected("\"2470\""));
    assert_eq!(
        differences,
        ["total: value expected [\"2470\"], got \"2469\""]
    );
    let mut doc = Document::open_bytes(document()).expect("opens");
    let steps = vec![
        Step {
            field: "nowhere".into(),
            action: Action::Enter,
            value: "1".into(),
        },
        Step {
            field: "size".into(),
            action: Action::Check,
            value: "XL".into(),
        },
    ];
    let expectation = parse_expectation(
        r#"{"fields": {"ghost": {"value": "1"}, "agree": {"value": "Yes"}}, "alerts": ["Hi"]}"#,
    )
    .expect("parses");
    let differences = replay(&mut doc, &steps, &expectation);
    assert_eq!(
        differences,
        [
            "step 1: nowhere is not a field of this form",
            "step 2: size has no XL button",
            "agree: value expected [\"Yes\"], got \"Off\"",
            "ghost: not a field of this form",
            "alerts: expected [\"Hi\"], got []",
        ]
    );
}

#[test]
fn a_file_out_of_format_says_where() {
    let steps = parse_scenario(
        r#"[{"field": "a", "action": "choose", "value": "x"}, {"field": "b", "action": "blur"}]"#,
    )
    .expect("parses");
    assert_eq!(steps[0].action, Action::Choose);
    assert_eq!(steps[1].value, "", "a value may be left out");
    for (json, says) in [
        ("{}", "a list of steps"),
        ("[{\"action\": \"enter\"}]", "step 1: `field`"),
        (
            "[{\"field\": \"a\", \"action\": \"type\"}]",
            "`type` is not",
        ),
        ("nonsense", "expected"),
    ] {
        let error = parse_scenario(json).unwrap_err();
        assert!(error.to_string().contains(says), "{json}: {error}");
    }
    for (json, says) in [
        ("[]", "`fields` object"),
        ("{\"fields\": {\"a\": {\"value\": 1}}}", "a: a string"),
        (
            "{\"fields\": {\"a\": {\"value\": [1]}}}",
            "a: every version",
        ),
        ("{\"fields\": {}, \"alerts\": 3}", "alerts: a string"),
        ("{", "EOF"),
    ] {
        let error = parse_expectation(json).unwrap_err();
        assert!(error.to_string().contains(says), "{json}: {error}");
    }
}
