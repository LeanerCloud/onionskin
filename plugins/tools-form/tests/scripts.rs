//! The Format, Validate and Calculate tabs: each choice written as the
//! `AF` call Acrobat writes, read back as the same choice, running as it
//! should, and anything else kept as a custom script.

use std::collections::BTreeMap;

use onionskin_scripting::{run, EventKind, Invocation};
use onionskin_tools_form::scripts::{Calculate, Format, Op, Validate};

fn round_trip(format: Format) {
    let (keystroke, script) = format.scripts();
    assert_eq!(
        Format::of(keystroke.as_deref(), script.as_deref()),
        format,
        "{keystroke:?} {script:?}"
    );
}

fn formatted(script: &str, value: &str) -> String {
    let fields = BTreeMap::from([("f".to_owned(), value.to_owned())]);
    run(
        script,
        &Invocation {
            kind: EventKind::Format,
            target: "f",
            value,
            change: "",
            will_commit: true,
            fields: &fields,
        },
    )
    .expect("runs")
    .value
}

#[test]
fn every_format_is_written_and_read_back() {
    for format in [
        Format::None,
        Format::Number {
            decimals: 2,
            separator: 0,
            negative: 2,
            currency: "$".into(),
            prepend: true,
        },
        Format::Number {
            decimals: 0,
            separator: 2,
            negative: 0,
            currency: " €".into(),
            prepend: false,
        },
        Format::Percent {
            decimals: 1,
            separator: 1,
        },
        Format::Date("dd-mmm-yyyy".into()),
        Format::Time(1),
        Format::Special(2),
        Format::Custom {
            keystroke: None,
            format: Some("event.value = 'x';".into()),
        },
    ] {
        round_trip(format);
    }
}

#[test]
fn the_written_formats_run() {
    let (_, number) = Format::Number {
        decimals: 2,
        separator: 0,
        negative: 0,
        currency: "$".into(),
        prepend: true,
    }
    .scripts();
    assert_eq!(formatted(&number.expect("a format"), "1234.5"), "$1,234.50");
    let (_, date) = Format::Date("yyyy-mm-dd".into()).scripts();
    assert_eq!(
        formatted(&date.expect("a format"), "2025 3 7"),
        "2025-03-07"
    );
    let (_, special) = Format::Special(3).scripts();
    assert_eq!(
        formatted(&special.expect("a format"), "078051120"),
        "078-05-1120"
    );
}

#[test]
fn scripts_that_are_not_one_af_call_are_custom() {
    let odd = [
        (
            Some("AFNumber_Keystroke(2, 0, 0, 0, \"\", true);"),
            Some("AFNumber_Format(1, 0, 0, 0, \"\", true);"),
        ),
        (Some("x;"), Some("AFPercent_Format(1, 0);")),
        (None, Some("AFDate_FormatEx(\"mm/dd\");")),
        (
            Some("AFTime_Keystroke(0);"),
            Some("AFTime_Format(0); app.beep();"),
        ),
        (Some("f(1"), Some("AFSpecial_Format(\"zip\");")),
        (Some("k"), Some("(1);")),
        (Some("k"), Some("AFNumber_Format(2, 0, 0, 0, 5, true);")),
        (Some("k"), Some("AFDate_FormatEx(3);")),
        (Some("k"), Some("AFPercent_Format(1, 0, true);")),
    ];
    for (keystroke, format) in odd {
        assert_eq!(
            Format::of(keystroke, format),
            Format::Custom {
                keystroke: keystroke.map(str::to_owned),
                format: format.map(str::to_owned),
            },
            "{keystroke:?} {format:?}"
        );
    }
    assert!(matches!(
        Format::of(Some("AFTime_Keystroke(9);"), Some("AFTime_Format(9);")),
        Format::Custom { .. }
    ));
}

#[test]
fn a_range_and_custom_validation_round_trip() {
    for validate in [
        Validate::None,
        Validate::Range {
            min: Some(0.0),
            max: Some(130.0),
        },
        Validate::Range {
            min: None,
            max: Some(-2.5),
        },
        Validate::Range {
            min: Some(1.0),
            max: None,
        },
        Validate::Custom("event.rc = event.value != '';".into()),
    ] {
        assert_eq!(Validate::of(validate.script().as_deref()), validate);
    }
    assert!(matches!(
        Validate::of(Some("AFRange_Validate(1, 2, 3, 4);")),
        Validate::Custom(_)
    ));
    assert!(matches!(
        Validate::of(Some("AFRange_Validate(true, 1);")),
        Validate::Custom(_)
    ));
}

#[test]
fn a_simple_calculation_names_its_fields() {
    for op in Op::ALL {
        let calculate = Calculate::Simple {
            op,
            fields: vec!["qty".into(), "unit \"price\"".into()],
        };
        assert_eq!(Calculate::of(calculate.script().as_deref()), calculate);
        assert!(!op.label().is_empty());
    }
    assert_eq!(
        Calculate::of(Some("AFSimple_Calculate(\"SUM\", \"a, b\");")),
        Calculate::Simple {
            op: Op::Sum,
            fields: vec!["a".into(), "b".into()],
        }
    );
    assert_eq!(
        Calculate::of(Some("AFSimple_Calculate(\"PRD\", [\"a\"]);")),
        Calculate::Simple {
            op: Op::Product,
            fields: vec!["a".into()],
        }
    );
    for custom in [
        "event.value = 1;",
        "AFSimple_Calculate(\"XOR\", \"a\");",
        "AFSimple_Calculate(\"SUM\", 3);",
        "AFSimple_Calculate(\"SUM\", new Array (1));",
        "Other(\"SUM\", \"a\");",
    ] {
        assert_eq!(
            Calculate::of(Some(custom)),
            Calculate::Custom(custom.to_owned())
        );
    }
    assert_eq!(Calculate::of(None), Calculate::None);
    assert_eq!(Calculate::None.script(), None);
}
