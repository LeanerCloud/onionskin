use super::*;

fn form(method: Method) -> SignatureForm {
    let mut form = SignatureForm::new(SignatureKind::Signature, false);
    form.apply(SignatureAction::SetMethod(method));
    form
}

#[test]
fn the_pad_draws_strokes_held_to_its_edges() {
    let mut form = form(Method::Draw);
    form.pad_move((5.0, 5.0));
    assert!(
        form.strokes.is_empty(),
        "moving without pressing draws nothing"
    );

    form.pad_down((-1.0, 5.0));
    assert!(form.strokes.is_empty(), "a press off the pad draws nothing");

    form.pad_down((10.0, 10.0));
    form.pad_move((20.0, 15.0));
    form.pad_move((500.0, -30.0));
    form.pad_up();
    form.pad_move((30.0, 30.0));
    assert_eq!(
        form.strokes,
        [vec![
            (10.0, 10.0),
            (20.0, 15.0),
            (f64::from(PAD_WIDTH), 0.0)
        ]]
    );

    form.pad_down((40.0, 40.0));
    assert_eq!(form.strokes.len(), 2);
    form.apply(SignatureAction::ClearPad);
    assert!(form.strokes.is_empty());
    assert!(!form.drawing);
}

#[test]
fn each_method_makes_a_page_or_says_what_is_missing() {
    let typed = request(&form(Method::Type), "  ").unwrap_err();
    assert_eq!(typed, "Type the name to sign with.");
    let Ok(Made::Page(page)) = request(&form(Method::Type), "Ada Lovelace") else {
        panic!("a typed page");
    };
    assert!(signature::page_size(&page).is_ok());

    let mut drawn = form(Method::Draw);
    assert_eq!(
        request(&drawn, "ignored").unwrap_err(),
        "Draw on the pad first."
    );
    drawn.pad_down((1.0, 1.0));
    drawn.pad_move((50.0, 20.0));
    assert!(matches!(request(&drawn, ""), Ok(Made::Page(_))));

    let mut image = form(Method::Image);
    assert_eq!(
        request(&image, "").unwrap_err(),
        "Choose the image to sign with."
    );
    image.image = Some("/a/sig.png".into());
    assert_eq!(request(&image, ""), Ok(Made::File("/a/sig.png".into())));
}

#[test]
fn only_typing_has_a_field_and_the_menu_names_the_kind() {
    assert_eq!(form(Method::Type).fields(), [SignatureField::Name]);
    assert!(form(Method::Draw).fields().is_empty());
    assert!(form(Method::Image).fields().is_empty());
    assert_eq!(kind(true), SignatureKind::Initials);
    assert_eq!(kind(false), SignatureKind::Signature);
    assert_eq!(SignatureField::Name.id(), "signature-name");
    assert_eq!(
        text_fields().collect::<Vec<_>>(),
        [TextField::Signature(SignatureField::Name)]
    );
}
