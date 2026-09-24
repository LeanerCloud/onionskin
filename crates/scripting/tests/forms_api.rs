//! The forms API subset, script by script, as Acrobat's panels write the
//! scripts.

use std::collections::BTreeMap;

use onionskin_scripting::{run, EventKind, Invocation, Outcome, ScriptError};

fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn event(
    kind: EventKind,
    script: &str,
    value: &str,
    others: &[(&str, &str)],
) -> Result<Outcome, ScriptError> {
    let fields = fields(others);
    run(
        script,
        &Invocation {
            kind,
            target: "total",
            value,
            change: "",
            will_commit: true,
            fields: &fields,
        },
    )
}

fn formatted(script: &str, value: &str) -> String {
    event(EventKind::Format, script, value, &[("total", value)])
        .expect("runs")
        .value
}

#[test]
fn numbers_format_with_each_separator_negative_and_currency_style() {
    assert_eq!(
        formatted("AFNumber_Format(2, 0, 0, 0, \"$\", true);", "1234.5"),
        "$1,234.50"
    );
    assert_eq!(
        formatted("AFNumber_Format(2, 1, 0, 0, \"\", true);", "1234.5"),
        "1234.50"
    );
    assert_eq!(
        formatted("AFNumber_Format(2, 2, 0, 0, \" €\", false);", "1234.5"),
        "1.234,50 €"
    );
    assert_eq!(
        formatted("AFNumber_Format(0, 4, 0, 0, \"\", true);", "1234567"),
        "1'234'567"
    );
    assert_eq!(
        formatted("AFNumber_Format(1, 0, 2, 0, \"\", true);", "-12.34"),
        "(12.3)"
    );
    assert_eq!(
        formatted("AFNumber_Format(1, 0, 0, 0, \"\", true);", "-12.34"),
        "-12.3"
    );
    assert_eq!(
        formatted("AFNumber_Format(2, 0, 0, 0, \"\", true);", ""),
        ""
    );
    assert_eq!(formatted("AFPercent_Format(1, 0);", "0.125"), "12.5%");
    assert_eq!(formatted("AFPercent_Format(0, 0, true);", "-0.5"), "%-50");
    assert_eq!(formatted("AFPercent_Format(0, 0);", "x"), "");
}

#[test]
fn a_number_keystroke_rejects_what_is_not_a_number() {
    let rejected = event(
        EventKind::Keystroke,
        "AFNumber_Keystroke(2, 0, 0, 0, \"\", true);",
        "12a",
        &[],
    )
    .expect("runs");
    assert!(!rejected.accepted);
    assert_eq!(
        rejected.alerts,
        ["The value entered does not match the format of the field [ total ]"]
    );
    let accepted = event(
        EventKind::Keystroke,
        "AFPercent_Keystroke(2, 0);",
        "1,234.5",
        &[],
    )
    .expect("runs");
    assert!(accepted.accepted && accepted.alerts.is_empty());
    let fields = BTreeMap::new();
    let typing = run(
        "AFNumber_Keystroke(2, 0, 0, 0, \"\", true);",
        &Invocation {
            kind: EventKind::Keystroke,
            target: "total",
            value: "1",
            change: "x",
            will_commit: false,
            fields: &fields,
        },
    )
    .expect("runs");
    assert!(!typing.accepted, "a letter is not typed into a number");
}

#[test]
fn dates_and_times_are_read_as_typed_and_shown_in_the_fields_format() {
    assert_eq!(
        formatted("AFDate_FormatEx(\"mmm d, yyyy\");", "3/7/2025"),
        "Mar 7, 2025"
    );
    assert_eq!(
        formatted("AFDate_FormatEx(\"yyyy-mm-dd\");", "2025 12 31"),
        "2025-12-31"
    );
    assert_eq!(
        formatted("AFDate_FormatEx(\"dd/mm/yy\");", "31 12 25"),
        "31/12/25"
    );
    assert_eq!(
        formatted("AFDate_FormatEx(\"mmmm d\");", "July 4"),
        "July 4"
    );
    assert_eq!(formatted("AFDate_Format(2);", "1/2/03"), "01/02/03");
    assert_eq!(
        formatted("AFDate_FormatEx(\"m/d/yy\");", "not a date"),
        "not a date"
    );
    let invalid = event(
        EventKind::Keystroke,
        "AFDate_KeystrokeEx(\"mm/dd/yyyy\");",
        "02/30/2025",
        &[],
    )
    .expect("runs");
    assert!(!invalid.accepted);
    assert!(
        invalid.alerts[0].starts_with("Invalid date/time"),
        "{:?}",
        invalid.alerts
    );
    assert!(
        event(
            EventKind::Keystroke,
            "AFDate_Keystroke(2);",
            "01/02/03",
            &[]
        )
        .expect("runs")
        .accepted
    );
    assert_eq!(formatted("AFTime_Format(1);", "14:05"), "2:05 pm");
    assert_eq!(
        formatted("AFTime_FormatEx(\"HH:MM:ss\");", "3:04:05 pm"),
        "15:04:05"
    );
}

#[test]
fn special_formats_mask_their_digits() {
    assert_eq!(formatted("AFSpecial_Format(0);", "12345"), "12345");
    assert_eq!(formatted("AFSpecial_Format(1);", "123456789"), "12345-6789");
    assert_eq!(formatted("AFSpecial_Format(1);", "12345"), "12345");
    assert_eq!(
        formatted("AFSpecial_Format(2);", "5551234567"),
        "(555) 123-4567"
    );
    assert_eq!(
        formatted("AFSpecial_Format(3);", "078051120"),
        "078-05-1120"
    );
    let short = event(
        EventKind::Keystroke,
        "AFSpecial_Keystroke(3);",
        "12345",
        &[],
    )
    .expect("runs");
    assert!(!short.accepted);
    let masked = event(
        EventKind::Keystroke,
        "AFSpecial_KeystrokeEx(\"999-999\");",
        "1234",
        &[],
    )
    .expect("runs");
    assert!(!masked.accepted);
}

#[test]
fn a_range_rejects_what_falls_outside_it() {
    let between = |value| {
        event(
            EventKind::Validate,
            "AFRange_Validate(true, 1, true, 10);",
            value,
            &[],
        )
        .expect("runs")
    };
    assert!(between("5").accepted);
    let over = between("11");
    assert!(!over.accepted);
    assert_eq!(
        over.alerts,
        ["The value entered must be greater than or equal to 1 and less than or equal to 10."]
    );
    assert!(
        !event(
            EventKind::Validate,
            "AFRange_Validate(true, 0, false, 0);",
            "-1",
            &[]
        )
        .expect("runs")
        .accepted
    );
    assert!(
        !event(
            EventKind::Validate,
            "AFRange_Validate(false, 0, true, 5);",
            "6",
            &[]
        )
        .expect("runs")
        .accepted
    );
    assert!(between("").accepted, "an empty field is not out of range");
}

#[test]
fn simple_calculations_aggregate_fields_and_their_children() {
    let others = [
        ("a", "2"),
        ("b", "3"),
        ("row.1", "4"),
        ("row.2", ""),
        ("total", ""),
    ];
    let calc = |function: &str, names: &str| {
        event(
            EventKind::Calculate,
            &format!("AFSimple_Calculate(\"{function}\", {names});"),
            "",
            &others,
        )
        .expect("runs")
        .value
    };
    assert_eq!(calc("SUM", "new Array(\"a\", \"b\")"), "5");
    assert_eq!(calc("SUM", "\"a, row\""), "6", "a group's children count");
    assert_eq!(calc("PRD", "[\"a\", \"b\"]"), "6");
    assert_eq!(calc("AVG", "[\"a\", \"b\"]"), "2.5");
    assert_eq!(calc("MIN", "[\"a\", \"b\"]"), "2");
    assert_eq!(calc("MAX", "[\"a\", \"b\", \"row\"]"), "4");
    assert_eq!(calc("SUM", "[\"missing\"]"), "0");
}

#[test]
fn a_custom_script_reads_and_sets_fields() {
    let script = "var q = this.getField(\"qty\").value;\n\
                  var p = this.getField(\"price\").value;\n\
                  event.value = q * p;\n\
                  this.getField(\"note\").value = util.printf(\"%.2f each\", p);\n\
                  if (this.getField(\"nothing\") !== null) event.value = -1;";
    let outcome = event(
        EventKind::Calculate,
        script,
        "",
        &[("qty", "3"), ("price", "1.5"), ("note", "")],
    )
    .expect("runs");
    assert_eq!(outcome.value, "4.5");
    assert_eq!(
        outcome.changed.get("note").map(String::as_str),
        Some("1.50 each")
    );
    let printed = event(
        EventKind::Format,
        "event.value = util.printf(\"%d items, %s, %x, 100%%\", 3.7, \"ok\", 255);",
        "",
        &[],
    )
    .expect("runs");
    assert_eq!(printed.value, "3 items, ok, ff, 100%");
    let dated = event(EventKind::Format, "event.value = util.printd(\"dddd d mmmm yyyy HH:MM\", util.scand(\"yyyy-mm-dd\", \"2025-03-07\"));", "", &[]).expect("runs");
    assert_eq!(dated.value, "Friday 7 March 2025 00:00");
    let text = event(
        EventKind::Format,
        "event.value = this.getField(\"name\").valueAsString + event.target.name;",
        "",
        &[("name", "007")],
    )
    .expect("runs");
    assert_eq!(text.value, "007total");
}

#[test]
fn what_the_subset_cannot_run_is_said() {
    let unsupported = event(EventKind::Calculate, "this.mailDoc(true);", "", &[]).unwrap_err();
    assert!(
        matches!(unsupported, ScriptError::Unsupported(_)),
        "{unsupported:?}"
    );
    let undefined = event(
        EventKind::Calculate,
        "event.value = xfa.host.name;",
        "",
        &[],
    )
    .unwrap_err();
    assert!(
        matches!(undefined, ScriptError::Unsupported(_)),
        "{undefined:?}"
    );
    let thrown = event(EventKind::Calculate, "throw new Error('no');", "", &[]).unwrap_err();
    assert!(matches!(thrown, ScriptError::Failed(_)), "{thrown:?}");
    let forever = event(EventKind::Calculate, "while (true) {}", "", &[]).unwrap_err();
    assert_eq!(forever, ScriptError::Exhausted);
    let deep = event(
        EventKind::Calculate,
        "function f() { return f(); } f();",
        "",
        &[],
    )
    .unwrap_err();
    assert_eq!(deep, ScriptError::Exhausted);
    assert!(forever.to_string().contains("too long"));
    assert!(thrown.to_string().starts_with("the script failed"));
    assert!(unsupported.to_string().contains("does not run"));
    let alert = event(
        EventKind::Validate,
        "app.alert({cMsg: 'Hi'}); app.alert('Two'); app.beep(0);",
        "",
        &[],
    )
    .expect("runs");
    assert_eq!(alert.alerts, ["Hi", "Two"]);
}
