use super::*;
use onionskin_redact::codes::built_in;

fn form() -> PropertiesForm {
    PropertiesForm::of(&RedactionLook::default(), built_in()).0
}

#[test]
fn a_look_opens_on_its_own_choices() {
    let look = RedactionLook {
        fill: None,
        outline: [0.0, 0.0, 1.0],
        overlay: Some(Overlay {
            text: "(b)(6)".to_owned(),
            size: 9.0,
            color: [1.0, 1.0, 0.0],
            align: Align::Right,
            repeat: true,
        }),
    };
    let (form, text, size) = PropertiesForm::of(&look, Vec::new());
    assert_eq!((text.as_str(), size.as_str()), ("(b)(6)", "9"));
    assert!(form.overlay && form.repeat);
    assert_eq!(super::look(&form, &text, &size), Ok(look));
    let (plain, text, size) = PropertiesForm::of(&RedactionLook::default(), Vec::new());
    assert_eq!((text.as_str(), size.as_str()), ("", ""));
    assert_eq!(super::look(&plain, "", ""), Ok(RedactionLook::default()));
}

#[test]
fn every_choice_cycles() {
    let mut form = form();
    form.apply(RedactAction::NextFill);
    assert_eq!(name_of(&FILLS, &form.fill), "White");
    for _ in 0..3 {
        form.apply(RedactAction::NextFill);
    }
    assert_eq!(form.fill, None, "No Fill is a choice");
    form.apply(RedactAction::NextOutline);
    assert_eq!(name_of(&COLORS, &form.outline), "Black");
    form.apply(RedactAction::NextTextColor);
    assert_eq!(name_of(&COLORS, &form.text_color), "Blue");
    form.apply(RedactAction::NextAlign);
    assert_eq!(form.align, Align::Right);
    form.apply(RedactAction::Repeat);
    assert!(form.repeat);
    assert_eq!(name_of(&COLORS, &[0.3, 0.3, 0.3]), "Custom");
}

#[test]
fn code_sets_and_codes_cycle_and_a_chosen_set_is_found_by_name() {
    let mut form = form();
    assert_eq!(form.current_code(), Some("(b)(1)"));
    form.apply(RedactAction::NextCode);
    assert_eq!(form.current_code(), Some("(b)(2)"));
    form.apply(RedactAction::NextSet);
    assert_eq!(
        form.current_set().map(|set| set.name.as_str()),
        Some("U.S. Privacy Act")
    );
    assert_eq!(
        form.current_code(),
        Some("(d)(5)"),
        "a new set starts at its first code"
    );
    form.apply(RedactAction::NextSet);
    assert_eq!(form.set, 0, "round again");
    form.choose_set(built_in(), "U.S. Privacy Act");
    assert_eq!(form.set, 1);
    assert_eq!(
        codes(" (a), ,(b) ,(c)"),
        ["(a)", "(b)", "(c)"].map(str::to_owned)
    );
}

#[test]
fn an_overlay_needs_its_text_and_a_usable_size() {
    let mut form = form();
    form.apply(RedactAction::Overlay);
    assert_eq!(
        form.fields(),
        [
            RedactField::OverlayText,
            RedactField::FontSize,
            RedactField::SetName,
            RedactField::Codes
        ]
    );
    assert_eq!(
        look(&form, "  ", ""),
        Err("Type the overlay text, or turn the overlay off.".to_owned())
    );
    assert_eq!(
        look(&form, "x", "big"),
        Err("\"big\" is not a font size in points".to_owned())
    );
    assert!(look(&form, "x", "-2").is_err());
    let fitted = look(&form, "(b)(6)", "").expect("fits");
    assert_eq!(fitted.overlay.map(|overlay| overlay.size), Some(0.0));
}

#[test]
fn find_looks_for_words_or_a_pattern() {
    let mut form = FindForm::default();
    assert_eq!(
        query(&form, " "),
        Err("Type the words or phrase to find.".to_owned())
    );
    form.apply(RedactAction::WholeWord);
    form.apply(RedactAction::MatchCase);
    let Ok(Query::Text(text, options)) = query(&form, " Secret ") else {
        panic!("a text query");
    };
    assert_eq!(text, "Secret");
    assert!(options.whole_word && options.case_sensitive);
    form.apply(RedactAction::Patterns(true));
    form.apply(RedactAction::NextPattern);
    assert_eq!(
        query(&form, ""),
        Ok(Query::Pattern(Pattern::EmailAddresses))
    );
    assert!(form.fields().is_empty());

    let hit = |page| Found {
        page,
        text: "x".to_owned(),
        quads: Vec::new(),
    };
    form.show(vec![hit(0), hit(1)]);
    form.apply(RedactAction::Toggle(0));
    form.apply(RedactAction::Toggle(9));
    assert_eq!(form.chosen(), [hit(1)]);
}

#[test]
fn each_panel_has_its_title() {
    assert_eq!(
        Panel::Properties { mark: None }.title(),
        "Redaction Properties"
    );
    assert_eq!(Panel::Find.title(), "Find Text & Redact");
    assert_eq!(Panel::Pages.title(), "Mark Pages for Redaction");
    assert_eq!(Panel::Apply { sanitize: false }.title(), "Apply Redactions");
    assert_eq!(
        Panel::Apply { sanitize: true }.title(),
        "Remove Hidden Information"
    );
    assert!(RedactField::FontSize.numeric() && !RedactField::Codes.numeric());
    assert_eq!(text_fields().count(), 6);
}
