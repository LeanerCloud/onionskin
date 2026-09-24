use super::*;
use crate::shell::chrome::accessible::TextField;

const TYPED: Typed<'static> = ["10", "20", " 30 ", "40.5", "612", "792"];

#[test]
fn the_typed_margins_make_the_crop_on_the_chosen_pages() {
    let form = CropForm::default();
    assert_eq!(
        request(form, &[1, 3], 5, TYPED),
        Ok(CropRequest {
            pages: vec![1, 3],
            which: PageBox::Crop,
            margins: Some(Margins {
                top: 10.0,
                bottom: 20.0,
                left: 30.0,
                right: 40.5,
            }),
            page_size: None,
        })
    );
}

#[test]
fn change_page_size_reads_the_size_as_well() {
    let mut form = CropForm::default();
    form.apply(CropAction::ChangePageSize);
    let crop = request(form, &[0], 1, TYPED).expect("valid");
    assert_eq!(crop.page_size, Some((612.0, 792.0)));
    for bad in ["0.5", "wide", "-3"] {
        let typed = ["0", "0", "0", "0", bad, "100"];
        let error = request(form, &[0], 1, typed).expect_err(bad);
        assert!(error.contains("not a page size"), "{error}");
    }
    assert!(shows(form, TextField::CropWidth));
    form.apply(CropAction::RemoveWhiteMargins);
    assert!(
        !shows(form, TextField::CropHeight),
        "a fitted page takes no size"
    );
    assert_eq!(
        request(form, &[0], 1, TYPED).expect("valid").page_size,
        None
    );
}

#[test]
fn every_page_and_another_box_are_the_forms_to_choose() {
    let mut form = CropForm::default();
    form.apply(CropAction::SetScope(CropScope::All));
    form.apply(CropAction::SetBox(PageBox::Bleed));
    let crop = request(form, &[1], 3, TYPED).expect("valid");
    assert_eq!(crop.pages, [0, 1, 2]);
    assert_eq!(crop.which, PageBox::Bleed);
}

#[test]
fn remove_white_margins_ignores_what_is_typed() {
    let mut form = CropForm::default();
    form.apply(CropAction::RemoveWhiteMargins);
    assert!(form.remove_white);
    let crop = request(form, &[0], 1, ["nonsense", "", "", "", "", ""]).expect("valid");
    assert_eq!(crop.margins, None);
    form.apply(CropAction::RemoveWhiteMargins);
    assert!(!form.remove_white, "it toggles");
}

#[test]
fn a_margin_that_is_not_a_distance_is_named() {
    let form = CropForm::default();
    for bad in ["-1", "abc", "inf", ""] {
        let error = request(form, &[0], 1, ["0", "0", bad, "0", "1", "1"]).expect_err(bad);
        assert!(error.contains("not a margin"), "{error}");
    }
    assert!(request(form, &[], 1, TYPED).is_err(), "no pages");
    let unchanged = form;
    let mut applied = form;
    applied.apply(CropAction::Submit);
    applied.apply(CropAction::SetToZero);
    assert_eq!(applied, unchanged, "the frame's actions leave the form");
}

#[test]
fn a_margin_is_shown_without_trailing_zeros() {
    assert_eq!(shown(36.0), "36");
    assert_eq!(shown(12.5), "12.5");
    assert_eq!(shown(0.333), "0.33");
    assert_eq!(shown(0.0), "0");
}

#[test]
fn a_build_without_the_plugin_says_so() {
    let refusal = crop_refusal(None);
    if cfg!(feature = "tools-edit") {
        assert_eq!(refusal, None);
        assert_eq!(crop_refusal(Some("Encrypted")), Some("Encrypted"));
    } else {
        assert!(refusal.expect("refused").contains("Edit PDF"));
    }
}
