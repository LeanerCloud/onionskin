use super::*;

/// What a freshly opened dialog of `kind` holds.
fn initial(kind: MarkKind) -> BTreeMap<MarkField, String> {
    MarkField::ALL
        .into_iter()
        .map(|field| (field, field.initial(kind).to_owned()))
        .collect()
}

#[test]
fn a_header_and_footer_opens_ready_to_number_pages() {
    let form = MarkForm::new(MarkKind::HeaderFooter, false);
    let checked = request(
        &form,
        &initial(MarkKind::HeaderFooter),
        &[1],
        3,
        "2026-09-24",
    )
    .expect("valid");
    assert_eq!(checked.pages, [0, 1, 2], "all pages by default");
    let MarkRequest::HeaderFooter(header) = checked.request else {
        panic!("a header and footer");
    };
    assert_eq!(header.text[4], "Page [page] of [pages]");
    assert_eq!(header.numbering.start, 1);
    assert_eq!(header.numbering.date, "2026-09-24");
    assert_eq!(header.margins.top, 36.0);
    assert_eq!(header.style.size, 10.0);

    let mut empty = initial(MarkKind::HeaderFooter);
    empty.insert(MarkField::CenterFooter, String::new());
    assert!(request(&form, &empty, &[0], 1, "")
        .unwrap_err()
        .contains("at least one"));
}

#[test]
fn a_number_out_of_range_names_its_field() {
    let form = MarkForm::new(MarkKind::HeaderFooter, false);
    let mut typed = initial(MarkKind::HeaderFooter);
    typed.insert(MarkField::Size, "0".to_owned());
    let error = request(&form, &typed, &[0], 1, "").unwrap_err();
    assert!(error.starts_with("Size (pt)"), "{error}");
    typed.insert(MarkField::Size, "10".to_owned());
    typed.insert(MarkField::Left, "wide".to_owned());
    assert!(request(&form, &typed, &[0], 1, "")
        .unwrap_err()
        .starts_with("Left margin"));
}

#[test]
fn bates_numbers_take_their_parts_and_the_other_files() {
    let mut form = MarkForm::new(MarkKind::Bates, false);
    form.other_files = vec!["/a/two.pdf".into()];
    form.apply(MarkAction::SetScope(CropScope::Chosen));
    let mut typed = initial(MarkKind::Bates);
    typed.insert(MarkField::Prefix, "ACME".to_owned());
    typed.insert(MarkField::Start, "41".to_owned());
    typed.insert(MarkField::After, "-numbered".to_owned());
    let checked = request(&form, &typed, &[2], 3, "").expect("valid");
    assert_eq!(checked.pages, [2]);
    let MarkRequest::Bates {
        bates,
        others,
        naming,
    } = checked.request
    else {
        panic!("Bates");
    };
    assert_eq!(bates.number(0), "ACME000041");
    assert_eq!(bates.position, 5);
    assert_eq!(others, [PathBuf::from("/a/two.pdf")]);
    assert_eq!(naming.after, "-numbered");
    assert!(naming.numbers);
    typed.insert(MarkField::Digits, "99".to_owned());
    assert!(request(&form, &typed, &[2], 3, "")
        .unwrap_err()
        .starts_with("Number of digits"));
}

#[test]
fn a_text_watermark_is_turned_and_half_opaque_by_default() {
    let form = MarkForm::new(MarkKind::Watermark, false);
    let checked = request(&form, &initial(MarkKind::Watermark), &[0], 1, "").expect("valid");
    let MarkRequest::Art { art, appearance } = checked.request else {
        panic!("art");
    };
    assert_eq!(appearance.rotation, 45.0);
    assert_eq!(appearance.opacity, 0.5);
    assert!(!appearance.behind);
    let ArtChoice::Text { text, style } = art else {
        panic!("text");
    };
    assert_eq!(text, "DRAFT");
    assert_eq!(style.size, 72.0);
    assert_eq!(style.color, COLORS[3].1, "red");

    let mut blank = initial(MarkKind::Watermark);
    blank.insert(MarkField::Text, "  ".to_owned());
    assert!(request(&form, &blank, &[0], 1, "")
        .unwrap_err()
        .contains("text"));
}

#[test]
fn a_file_needs_choosing_and_a_colour_needs_nothing_typed() {
    let mut form = MarkForm::new(MarkKind::Background, false);
    let checked = request(&form, &initial(MarkKind::Background), &[0], 1, "").expect("valid");
    assert!(matches!(
        checked.request,
        MarkRequest::Art {
            art: ArtChoice::Color(_),
            appearance,
        } if appearance.rotation == 0.0 && appearance.opacity == 1.0
    ));
    form.apply(MarkAction::SetSource(Source::File));
    assert!(request(&form, &initial(MarkKind::Background), &[0], 1, "")
        .unwrap_err()
        .contains("Choose a PDF"));
    form.file = Some("/art/logo.pdf".into());
    let mut typed = initial(MarkKind::Background);
    typed.insert(MarkField::Scale, "50".to_owned());
    let checked = request(&form, &typed, &[0], 1, "").expect("valid");
    assert!(matches!(
        checked.request,
        MarkRequest::Art {
            art: ArtChoice::File { scale, .. },
            ..
        } if scale == 0.5
    ));
    assert!(request(&form, &typed, &[], 0, "").is_err(), "no pages");
    let chosen = MarkForm {
        scope: CropScope::Chosen,
        ..form
    };
    assert!(request(&chosen, &typed, &[], 1, "")
        .unwrap_err()
        .contains("no pages"));
}

#[test]
fn each_kind_shows_its_own_fields() {
    let header = fields(&MarkForm::new(MarkKind::HeaderFooter, false));
    assert_eq!(header.len(), 12);
    let watermark = fields(&MarkForm::new(MarkKind::Watermark, false));
    assert_eq!(
        watermark,
        [
            MarkField::Text,
            MarkField::Size,
            MarkField::Rotation,
            MarkField::Opacity
        ]
    );
    assert_eq!(
        fields(&MarkForm::new(MarkKind::Background, false)),
        [MarkField::Opacity]
    );
    for field in MarkField::ALL {
        assert!(field.id().starts_with("mark-"));
        assert!(!field.label().is_empty());
    }
    assert!(MarkField::Size.numeric());
    assert!(!MarkField::Prefix.numeric());
    for kind in MarkKind::ALL {
        assert!(!title(kind).is_empty());
    }
    assert_eq!(text_fields().count(), 20);
}

#[test]
fn the_date_is_read_from_a_pdf_date() {
    assert_eq!(iso_date("D:20260924120000Z00'00'"), "2026-09-24");
    assert_eq!(iso_date("nonsense"), "");
}
