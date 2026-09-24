//! The Properties dialog as data: each tab's rows for each kind of field,
//! the controls, and the properties the choices come to.

use onionskin_core::forms::{ChoiceOption, FieldProperties, FieldScripts, KindOptions};
use onionskin_tools_form::scripts::Op;

use super::form::{FieldAction, FieldForm, FieldInput, Flag, FormatKind, RuleKind, Shape, Tab};
use super::view::labels;

fn properties(options: KindOptions) -> FieldProperties {
    FieldProperties {
        name: "total".into(),
        tooltip: String::new(),
        hidden: false,
        read_only: false,
        required: false,
        border: Some([0.0; 3]),
        fill: None,
        font_size: 0.0,
        text_color: [0.0; 3],
        rect: [10.0, 20.0, 154.0, 42.0],
        options,
        scripts: FieldScripts::default(),
    }
}

fn text() -> KindOptions {
    KindOptions::Text {
        align: 0,
        default: String::new(),
        multiline: false,
        password: false,
        comb: false,
        max_len: None,
    }
}

/// The typed values `typed` holds, the rest as the form started.
fn reader(
    start: &[(FieldInput, String)],
    typed: &[(FieldInput, &str)],
) -> impl Fn(FieldInput) -> String {
    let mut all: Vec<(FieldInput, String)> = start.to_vec();
    for (input, text) in typed {
        all.retain(|(each, _)| each != input);
        all.push((*input, (*text).to_owned()));
    }
    move |which| {
        all.iter()
            .find(|(each, _)| *each == which)
            .map(|(_, text)| text.clone())
            .unwrap_or_default()
    }
}

#[test]
fn a_text_field_has_every_tab_and_general_first() {
    let (form, typed) = FieldForm::of(Shape::Text, properties(text()));
    assert_eq!(
        labels(&form),
        [
            "* General",
            "Appearance",
            "Position",
            "Options",
            "Format",
            "Validate",
            "Calculate",
            "Text Field",
            "[Name]",
            "[Tooltip]",
            "Hidden",
            "Read Only",
            "Required",
            "Save",
            "Delete Field",
            "oops"
        ]
    );
    let start = |which| {
        typed
            .iter()
            .find(|(each, _)| *each == which)
            .map(|(_, text)| text.as_str())
    };
    assert_eq!(start(FieldInput::Width), Some("144"));
    assert_eq!(start(FieldInput::Bottom), Some("20"));
    assert_eq!(start(FieldInput::DatePattern), Some("mm/dd/yyyy"));
    assert_eq!(Shape::Text.title(), "Text Field Properties");
    assert_eq!(
        Shape::Signature.tabs(),
        [Tab::General, Tab::Appearance, Tab::Position]
    );
    assert_eq!(Shape::CheckBox.tabs().len(), 4, "no Format for a check box");
}

#[test]
fn appearance_and_general_controls_change_the_properties() {
    let (mut form, typed) = FieldForm::of(Shape::Text, properties(text()));
    let read = reader(&typed, &[]);
    for action in [
        FieldAction::Toggle(Flag::Hidden),
        FieldAction::Toggle(Flag::ReadOnly),
        FieldAction::Toggle(Flag::Required),
        FieldAction::Tab(Tab::Appearance),
        FieldAction::NextBorder,
        FieldAction::NextFill,
        FieldAction::NextFontSize,
        FieldAction::NextFontSize,
        FieldAction::NextTextColor,
    ] {
        form.apply(action, &read);
    }
    assert_eq!(
        labels(&form)[7..11],
        [
            "Border Color: Gray",
            "Fill Color: White",
            "Font Size: 8",
            "Text Color: Blue"
        ]
    );
    let written = form.request(&read).expect("writes");
    assert!(written.hidden && written.read_only && written.required);
    assert_eq!(written.border, Some([0.5; 3]));
    assert_eq!(written.font_size, 8.0);
    for _ in 0..7 {
        form.apply(FieldAction::NextFontSize, &read);
    }
    assert_eq!(form.properties.font_size, 0.0, "round to Auto");
    let odd = FieldForm::of(
        Shape::Text,
        FieldProperties {
            text_color: [0.2, 0.3, 0.4],
            ..properties(text())
        },
    )
    .0;
    assert!(labels(&FieldForm {
        tab: Tab::Appearance,
        ..odd
    })
    .contains(&"Text Color: Custom".to_owned()));
}

#[test]
fn position_and_text_options_are_read_from_what_is_typed() {
    let (mut form, typed) = FieldForm::of(Shape::Text, properties(text()));
    let read = reader(
        &typed,
        &[
            (FieldInput::Name, " email "),
            (FieldInput::Left, "50"),
            (FieldInput::Bottom, "60.5"),
            (FieldInput::Width, "100"),
            (FieldInput::Height, "20"),
            (FieldInput::MaxLength, "9"),
            (FieldInput::DefaultText, "none"),
        ],
    );
    form.apply(FieldAction::Tab(Tab::Options), &read);
    for flag in [Flag::Multiline, Flag::Password, Flag::Comb] {
        form.apply(FieldAction::Toggle(flag), &read);
        assert!(form.flag(flag));
    }
    form.apply(FieldAction::NextAlign, &read);
    assert!(labels(&form).contains(&"Alignment: Center".to_owned()));
    let written = form.request(&read).expect("writes");
    assert_eq!(written.name, "email");
    assert_eq!(written.rect, [50.0, 60.5, 150.0, 80.5]);
    assert_eq!(
        written.options,
        KindOptions::Text {
            align: 1,
            default: "none".into(),
            multiline: true,
            password: true,
            comb: true,
            max_len: Some(9),
        }
    );
    let bad = reader(&typed, &[(FieldInput::Width, "wide")]);
    assert_eq!(
        form.request(&bad).unwrap_err(),
        "Width (points) must be a number, not \"wide\""
    );
    let bad = reader(&typed, &[(FieldInput::MaxLength, "0")]);
    assert!(form.request(&bad).unwrap_err().contains("whole number"));
}

#[test]
fn a_dropdown_s_items_are_added_ordered_chosen_and_removed() {
    let (mut form, typed) = FieldForm::of(
        Shape::Dropdown,
        properties(KindOptions::Choice {
            options: vec![ChoiceOption {
                export: "r".into(),
                display: "Red".into(),
            }],
            editable: false,
            multi_select: false,
            default: Some("r".into()),
        }),
    );
    form.apply(FieldAction::Tab(Tab::Options), &reader(&typed, &[]));
    let add = reader(
        &typed,
        &[
            (FieldInput::OptionItem, "Green"),
            (FieldInput::OptionExport, ""),
        ],
    );
    form.apply(FieldAction::AddOption, &add);
    form.apply(
        FieldAction::AddOption,
        &reader(&typed, &[(FieldInput::OptionItem, " ")]),
    );
    assert_eq!(form.items.len(), 2, "an empty item is not added");
    assert_eq!(form.items[1].export, "Green", "the item is its own export");
    let read = reader(&typed, &[]);
    form.apply(FieldAction::OptionUp, &read);
    assert_eq!(form.items[0].display, "Green");
    form.apply(FieldAction::OptionUp, &read);
    form.apply(FieldAction::OptionDown, &read);
    assert_eq!(form.items[1].display, "Green");
    form.apply(FieldAction::OptionDown, &read);
    form.apply(FieldAction::DefaultOption, &read);
    form.apply(FieldAction::Toggle(Flag::Editable), &read);
    let shown = labels(&form);
    assert!(
        shown.contains(&"* Green (Green, default)".to_owned()),
        "{shown:?}"
    );
    assert!(shown.contains(&"Red (r)".to_owned()));
    assert!(shown.contains(&"* Allow user to enter custom text".to_owned()));
    form.apply(FieldAction::DefaultOption, &read);
    assert_eq!(
        form.default_item, None,
        "chosen again, it is not the default"
    );
    form.apply(FieldAction::DefaultOption, &read);
    form.apply(FieldAction::RemoveOption, &read);
    assert_eq!(form.default_item, None, "the default went with it");
    assert!(!labels(&form).contains(&"Delete Item".to_owned()));
    form.apply(FieldAction::SelectOption(5), &read);
    assert_eq!(form.selected_item, None);
    form.apply(FieldAction::OptionUp, &read);
    form.apply(FieldAction::RemoveOption, &read);
    let written = form.request(&read).expect("writes");
    assert_eq!(
        written.options,
        KindOptions::Choice {
            options: vec![ChoiceOption {
                export: "r".into(),
                display: "Red".into(),
            }],
            editable: true,
            multi_select: false,
            default: None,
        }
    );
    let (list, _) = FieldForm::of(Shape::ListBox, form.properties.clone());
    let list = FieldForm {
        tab: Tab::Options,
        ..list
    };
    assert!(labels(&list).contains(&"Multiple selection".to_owned()));
}

#[test]
fn buttons_and_signatures_offer_their_own_options() {
    let check = KindOptions::Button {
        export: "Yes".into(),
        on_by_default: false,
        no_toggle_to_off: None,
    };
    let (mut form, typed) = FieldForm::of(Shape::CheckBox, properties(check));
    let read = reader(&typed, &[(FieldInput::Export, "Agree")]);
    form.apply(FieldAction::Tab(Tab::Options), &read);
    form.apply(FieldAction::Toggle(Flag::OnByDefault), &read);
    form.apply(FieldAction::Toggle(Flag::NoToggleToOff), &read);
    assert_eq!(
        labels(&form)[4..6],
        ["[Export value]", "* Check box is checked by default"]
    );
    assert_eq!(
        form.request(&read).expect("writes").options,
        KindOptions::Button {
            export: "Agree".into(),
            on_by_default: true,
            no_toggle_to_off: None,
        }
    );
    let radio = KindOptions::Button {
        export: "Choice1".into(),
        on_by_default: false,
        no_toggle_to_off: Some(true),
    };
    let (mut form, typed) = FieldForm::of(Shape::Radio, properties(radio));
    let read = reader(&typed, &[]);
    form.apply(FieldAction::Tab(Tab::Options), &read);
    form.apply(FieldAction::Toggle(Flag::NoToggleToOff), &read);
    assert!(labels(&form).contains(&"Clicking the chosen button leaves it chosen".to_owned()));
    assert!(!form.flag(Flag::NoToggleToOff));

    let (mut form, typed) = FieldForm::of(
        Shape::Button,
        properties(KindOptions::PushButton {
            caption: "Go".into(),
        }),
    );
    let read = reader(&typed, &[(FieldInput::Caption, "Print")]);
    form.apply(FieldAction::Tab(Tab::Options), &read);
    assert_eq!(labels(&form)[4], "[Label]");
    assert_eq!(
        form.request(&read).expect("writes").options,
        KindOptions::PushButton {
            caption: "Print".into()
        }
    );
    let (form, typed) = FieldForm::of(Shape::Signature, properties(KindOptions::Signature));
    assert_eq!(
        form.request(&reader(&typed, &[])).expect("writes").options,
        KindOptions::Signature
    );
    assert!(
        !form.flag(Flag::Multiline),
        "a flag the kind has not is off"
    );
}

#[test]
fn the_format_tab_writes_the_scripts_acrobat_writes() {
    let (mut form, typed) = FieldForm::of(Shape::Text, properties(text()));
    let read = reader(&typed, &[(FieldInput::Currency, "$")]);
    form.apply(FieldAction::Tab(Tab::Format), &read);
    form.apply(FieldAction::SetFormat(FormatKind::Number), &read);
    for action in [
        FieldAction::NextDecimals,
        FieldAction::NextSeparator,
        FieldAction::NextNegative,
        FieldAction::Toggle(Flag::CurrencyFirst),
    ] {
        form.apply(action, &read);
    }
    let shown = labels(&form);
    for expected in [
        "* Number",
        "Decimal Places: 3",
        "Separator Style: 1234.56",
        "Negative Number Style: 1,234.01 in red",
        "[Currency symbol]",
        "Currency symbol before the number",
    ] {
        assert!(
            shown.contains(&expected.to_owned()),
            "{expected}: {shown:?}"
        );
    }
    let scripts = form.request(&read).expect("writes").scripts;
    assert_eq!(
        scripts.format.as_deref(),
        Some("AFNumber_Format(3, 1, 1, 0, \"$\", false);")
    );
    assert!(scripts
        .keystroke
        .expect("a keystroke")
        .starts_with("AFNumber_Keystroke"));

    let (again, _) = FieldForm::of(Shape::Text, form.request(&read).expect("writes"));
    assert_eq!(again.format, FormatKind::Number);
    assert_eq!((again.decimals, again.separator, again.negative), (3, 1, 1));

    for (kind, action, expected) in [
        (
            FormatKind::Percent,
            FieldAction::NextDecimals,
            "AFPercent_Format(4, 1);",
        ),
        (
            FormatKind::Time,
            FieldAction::NextTimeStyle,
            "AFTime_Format(1);",
        ),
        (
            FormatKind::Special,
            FieldAction::NextSpecial,
            "AFSpecial_Format(1);",
        ),
        (
            FormatKind::Date,
            FieldAction::Tab(Tab::Format),
            "AFDate_FormatEx(\"mm/dd/yyyy\");",
        ),
    ] {
        form.apply(FieldAction::SetFormat(kind), &read);
        form.apply(action, &read);
        assert!(!labels(&form).is_empty());
        assert_eq!(
            form.request(&read)
                .expect("writes")
                .scripts
                .format
                .as_deref(),
            Some(expected)
        );
    }
    let blank = reader(&typed, &[(FieldInput::DatePattern, " ")]);
    assert!(form.request(&blank).unwrap_err().contains("date format"));
    form.apply(FieldAction::SetFormat(FormatKind::Custom), &read);
    let custom = reader(&typed, &[(FieldInput::CustomFormat, "event.value = 'x';")]);
    let scripts = form.request(&custom).expect("writes").scripts;
    assert_eq!(scripts.keystroke, None);
    assert_eq!(scripts.format.as_deref(), Some("event.value = 'x';"));
    let (read_back, typed_back) =
        FieldForm::of(Shape::Text, form.request(&custom).expect("writes"));
    assert_eq!(read_back.format, FormatKind::Custom);
    assert!(typed_back.contains(&(FieldInput::CustomFormat, "event.value = 'x';".into())));
    form.apply(FieldAction::SetFormat(FormatKind::None), &read);
    assert_eq!(
        form.request(&read).expect("writes").scripts,
        FieldScripts::default()
    );
}

#[test]
fn validate_and_calculate_write_a_range_and_a_simple_calculation() {
    let (mut form, typed) = FieldForm::of(
        Shape::Dropdown,
        properties(KindOptions::Choice {
            options: Vec::new(),
            editable: true,
            multi_select: false,
            default: None,
        }),
    );
    let read = reader(
        &typed,
        &[
            (FieldInput::RangeMin, "0"),
            (FieldInput::RangeMax, ""),
            (FieldInput::CalculateFields, "a, b ,"),
        ],
    );
    form.apply(FieldAction::Tab(Tab::Validate), &read);
    form.apply(FieldAction::SetValidate(RuleKind::Simple), &read);
    assert!(labels(&form).contains(&"[Greater than or equal to]".to_owned()));
    form.apply(FieldAction::Tab(Tab::Calculate), &read);
    form.apply(FieldAction::SetCalculate(RuleKind::Simple), &read);
    form.apply(FieldAction::NextOp, &read);
    assert!(labels(&form).contains(&"Value is the product (x) of the fields".to_owned()));
    let scripts = form.request(&read).expect("writes").scripts;
    assert_eq!(
        scripts.validate.as_deref(),
        Some("AFRange_Validate(true, 0, false, 0);")
    );
    assert_eq!(
        scripts.calculate.as_deref(),
        Some("AFSimple_Calculate(\"PRD\", new Array (\"a\", \"b\"));")
    );
    let (back, typed_back) = FieldForm::of(Shape::Dropdown, form.request(&read).expect("writes"));
    assert_eq!(
        (back.validate, back.calculate, back.op),
        (RuleKind::Simple, RuleKind::Simple, Op::Product)
    );
    assert!(typed_back.contains(&(FieldInput::RangeMin, "0".into())));
    assert!(typed_back.contains(&(FieldInput::CalculateFields, "a, b".into())));

    let bad = reader(&typed, &[(FieldInput::RangeMin, "zero")]);
    assert!(form.request(&bad).unwrap_err().contains("must be a number"));
    let none = reader(
        &typed,
        &[
            (FieldInput::RangeMin, "1"),
            (FieldInput::CalculateFields, " "),
        ],
    );
    assert_eq!(
        form.request(&none).unwrap_err(),
        "Name the fields to calculate from."
    );

    form.apply(FieldAction::SetValidate(RuleKind::Custom), &read);
    form.apply(FieldAction::SetCalculate(RuleKind::Custom), &read);
    let custom = reader(
        &typed,
        &[
            (FieldInput::CustomValidate, "event.rc = true;"),
            (FieldInput::CustomCalculate, ""),
        ],
    );
    let scripts = form.request(&custom).expect("writes").scripts;
    assert_eq!(scripts.validate.as_deref(), Some("event.rc = true;"));
    assert_eq!(scripts.calculate, None, "an empty custom script is none");
    let (back, typed_back) = FieldForm::of(Shape::Dropdown, form.request(&custom).expect("writes"));
    assert_eq!(back.validate, RuleKind::Custom);
    assert!(typed_back.contains(&(FieldInput::CustomValidate, "event.rc = true;".into())));
    form.apply(FieldAction::SetCalculate(RuleKind::None), &read);
    form.apply(FieldAction::SetValidate(RuleKind::None), &read);
    assert_eq!(
        form.request(&read).expect("writes").scripts,
        FieldScripts::default()
    );

    let with_calc = FieldProperties {
        scripts: FieldScripts {
            calculate: Some("event.value = 2;".into()),
            ..FieldScripts::default()
        },
        ..properties(text())
    };
    let (custom_calc, typed_calc) = FieldForm::of(Shape::Text, with_calc);
    assert_eq!(custom_calc.calculate, RuleKind::Custom);
    assert!(typed_calc.contains(&(FieldInput::CustomCalculate, "event.value = 2;".into())));
}

#[test]
fn every_typed_field_has_an_id_and_a_label() {
    let mut ids: Vec<&str> = FieldInput::ALL.iter().map(|input| input.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), FieldInput::ALL.len());
    assert!(FieldInput::ALL
        .iter()
        .all(|input| !input.label().is_empty()));
    assert!(FieldInput::Width.numeric() && !FieldInput::Name.numeric());
    assert!(FormatKind::ALL.iter().all(|kind| !kind.label().is_empty()));
    for shape in [
        Shape::Text,
        Shape::CheckBox,
        Shape::Radio,
        Shape::ListBox,
        Shape::Dropdown,
        Shape::Button,
        Shape::Signature,
    ] {
        assert!(shape.title().ends_with("Properties"));
        assert!(!shape.label().is_empty());
    }
}
