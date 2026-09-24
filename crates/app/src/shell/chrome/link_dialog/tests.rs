use super::*;

#[test]
fn a_page_is_typed_from_one() {
    let form = LinkForm::default();
    assert_eq!(
        request(&form, " 3 ", "", 3),
        Ok((LinkTarget::Page(2), LinkLook::default()))
    );
    for bad in ["0", "4", "two", ""] {
        assert!(
            request(&form, bad, "", 3)
                .unwrap_err()
                .contains("not a page"),
            "{bad}"
        );
    }
}

#[test]
fn a_web_address_gets_a_scheme_when_it_has_none() {
    let form = LinkForm {
        kind: TargetKind::Web,
        ..LinkForm::default()
    };
    let web = |typed| request(&form, "", typed, 1).map(|(target, _)| target);
    assert_eq!(
        web("example.com/a"),
        Ok(LinkTarget::Web("http://example.com/a".into()))
    );
    assert_eq!(
        web("https://x.example"),
        Ok(LinkTarget::Web("https://x.example".into()))
    );
    assert_eq!(
        web("mailto:a@b.c"),
        Ok(LinkTarget::Web("mailto:a@b.c".into()))
    );
    assert!(web("  ").is_err());
    assert!(web("two words").is_err());
}

#[test]
fn a_file_must_be_chosen_and_a_kept_action_is_kept() {
    let mut form = LinkForm {
        kind: TargetKind::File,
        ..LinkForm::default()
    };
    assert!(request(&form, "", "", 1).unwrap_err().contains("Choose"));
    form.file = Some("/a/b.pdf".into());
    assert_eq!(
        request(&form, "", "", 1).map(|(target, _)| target),
        Ok(LinkTarget::File("/a/b.pdf".into()))
    );
    let kept = LinkForm {
        kind: TargetKind::Keep,
        kept: Some("JavaScript".into()),
        ..LinkForm::default()
    };
    assert_eq!(
        request(&kept, "", "", 1).map(|(target, _)| target),
        Ok(LinkTarget::Other("JavaScript".into()))
    );
    assert_eq!(kept.kinds().len(), 4);
    assert_eq!(LinkForm::default().kinds().len(), 3);
    assert!(kept.fields().is_empty());
}

#[test]
fn the_look_cycles_through_its_choices() {
    let mut form = LinkForm::default();
    form.apply(LinkAction::Visible);
    assert!(form.look.visible);
    let widths: Vec<f64> = (0..3)
        .map(|_| {
            form.apply(LinkAction::NextWidth);
            form.look.width
        })
        .collect();
    assert_eq!(widths, [2.0, 3.0, 1.0]);
    form.apply(LinkAction::NextStyle);
    assert_eq!(form.look.style, LineStyle::Dashed);
    form.apply(LinkAction::NextHighlight);
    assert_eq!(form.look.highlight, Highlight::Outline);
    assert_eq!(color_name(form.look.color), "Blue");
    form.apply(LinkAction::NextColor);
    assert_eq!(color_name(form.look.color), "Black");
    form.look.color = [0.3, 0.2, 0.1];
    assert_eq!(color_name(form.look.color), "Custom");
    form.apply(LinkAction::NextColor);
    assert_eq!(
        color_name(form.look.color),
        "Blue",
        "a custom colour starts the list again"
    );
    form.apply(LinkAction::SetKind(TargetKind::Web));
    assert_eq!(form.fields(), [LinkField::Url]);
    let before = form.clone();
    for action in [
        LinkAction::ChooseFile,
        LinkAction::Submit,
        LinkAction::Delete,
    ] {
        form.apply(action);
    }
    assert_eq!(form, before);
    for kind in [
        TargetKind::Page,
        TargetKind::Web,
        TargetKind::File,
        TargetKind::Keep,
    ] {
        assert!(!kind.label().is_empty());
    }
    for field in LinkField::ALL {
        assert!(field.id().starts_with("link-"));
    }
}

#[test]
fn an_existing_link_fills_the_form() {
    let link = |target| Link {
        objref: ObjRef::new(9, 0),
        page: 0,
        rect: [0.0; 4],
        target,
        look: LinkLook::default(),
    };
    assert_eq!(
        LinkForm::of(&link(LinkTarget::Page(2))).kind,
        TargetKind::Page
    );
    assert_eq!(
        LinkForm::of(&link(LinkTarget::Web("x".into()))).kind,
        TargetKind::Web
    );
    let file = LinkForm::of(&link(LinkTarget::File("a.pdf".into())));
    assert_eq!(
        (file.kind, file.file),
        (TargetKind::File, Some("a.pdf".into()))
    );
    let other = LinkForm::of(&link(LinkTarget::Other("Named".into())));
    assert_eq!(
        (other.kind, other.kept.as_deref()),
        (TargetKind::Keep, Some("Named"))
    );
    assert_eq!(text_fields().count(), 2);
}
