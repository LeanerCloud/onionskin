//! The dialog's model, without a window: every choice becomes an argument
//! to `crates/print`, and every typed value is checked before it does.

use onionskin_core::AnnotationFilter;
use onionskin_print::{Duplex, NUpOrder, Orientation, PaperSize, Sizing, Subset};

use super::view::preview_label;
use super::*;

const LETTER: PageSize = (612.0, 792.0);

fn typed() -> Typed {
    Typed {
        copies: "1".into(),
        pages: "2-4, 7".into(),
        scale: "50".into(),
        poster_scale: "200".into(),
        poster_overlap: "0.25".into(),
        booklet_from: "1".into(),
        booklet_to: "3".into(),
    }
}

fn printed(pages: usize) -> Printed {
    Printed {
        page_sizes: vec![LETTER; pages],
        current_page: 2,
        image_only: None,
    }
}

fn job_for(settings: &PrintSettings, typed: &Typed) -> Result<PrintJob, String> {
    job(
        settings,
        PageSetup::default(),
        typed,
        &printed(10),
        &[
            Destination::SaveAsPdf,
            Destination::Printer("Office".into()),
        ],
    )
}

#[test]
fn the_defaults_print_every_page_fitted_once_with_markups() {
    let job = job_for(&PrintSettings::default(), &typed()).expect("a job");
    assert!(job.selection.ranges.is_empty());
    assert_eq!(job.sizing, Sizing::Fit);
    assert_eq!(job.copies, 1);
    assert!(job.collate);
    assert_eq!(job.comments, AnnotationFilter::DocumentAndMarkups);
    assert_eq!(job.printer, None, "Save as PDF names no printer");
    assert_eq!(job.paper, PaperSize::LETTER);
}

#[test]
fn every_choice_reaches_the_job() {
    let mut settings = PrintSettings::default();
    for action in [
        PrintAction::Destination(1),
        PrintAction::Collate,
        PrintAction::Pages(PagesChoice::Custom),
        PrintAction::Subset(Subset::Odd),
        PrintAction::Reverse,
        PrintAction::Sizing(SizingChoice::Custom),
        PrintAction::PerSheet(4),
        PrintAction::Order(NUpOrder::Vertical),
        PrintAction::Borders,
        PrintAction::Comments(AnnotationFilter::DocumentOnly),
        PrintAction::Duplex(Duplex::ShortEdge),
        PrintAction::PrintAsImage,
    ] {
        assert!(apply(&mut settings, action), "{action:?} is a setting");
    }
    let job = job_for(&settings, &typed()).expect("a job");
    assert_eq!(job.printer.as_deref(), Some("Office"));
    assert!(!job.collate);
    assert_eq!(job.selection.ranges, [(1, 3), (6, 6)]);
    assert_eq!(job.selection.subset, Subset::Odd);
    assert!(job.selection.reverse);
    assert_eq!(job.sizing, Sizing::Custom(50));
    assert_eq!(job.n_up.per_sheet, 4);
    assert_eq!(job.n_up.order, NUpOrder::Vertical);
    assert!(job.n_up.borders);
    assert_eq!(job.comments, AnnotationFilter::DocumentOnly);
    assert_eq!(job.duplex, Duplex::ShortEdge);
    assert!(job.print_as_image);
}

#[test]
fn page_setup_holds_paper_and_orientation_and_nothing_else() {
    let mut setup = PageSetup::default();
    assert!(apply_setup(&mut setup, PrintAction::Paper(2)));
    assert!(apply_setup(
        &mut setup,
        PrintAction::Orientation(Orientation::Landscape)
    ));
    assert!(!apply_setup(&mut setup, PrintAction::Collate));
    assert!(apply_setup(&mut setup, PrintAction::Paper(99)), "clamped");
    assert_eq!(setup.paper(), PaperSize::A4);
    let mut settings = PrintSettings::default();
    for action in [
        PrintAction::Paper(0),
        PrintAction::Orientation(Orientation::Portrait),
        PrintAction::PreviewNext,
        PrintAction::PreviewPrevious,
        PrintAction::Print,
        PrintAction::Cancel,
    ] {
        assert!(!apply(&mut settings, action), "{action:?} is not a setting");
    }
    assert_eq!(settings, PrintSettings::default());
    let job = job(
        &settings,
        setup,
        &typed(),
        &printed(1),
        &[Destination::SaveAsPdf],
    )
    .expect("a job");
    assert_eq!(
        (job.paper, job.orientation),
        (PaperSize::A4, Orientation::Landscape)
    );
}

#[test]
fn the_current_page_is_the_one_on_screen() {
    let settings = PrintSettings {
        pages: PagesChoice::Current,
        ..PrintSettings::default()
    };
    let job = job_for(&settings, &typed()).expect("a job");
    assert_eq!(job.selection.ranges, [(2, 2)]);
}

#[test]
fn a_backwards_range_bad_copies_and_a_bad_scale_are_each_refused_in_words() {
    let custom = PrintSettings {
        pages: PagesChoice::Custom,
        sizing: SizingChoice::Custom,
        ..PrintSettings::default()
    };
    let refused = |typed: Typed| job_for(&custom, &typed).expect_err("refused");
    let backwards = refused(Typed {
        pages: "7-2".into(),
        ..typed()
    });
    assert!(backwards.contains("backwards"), "{backwards}");
    assert!(refused(Typed {
        copies: "0".into(),
        ..typed()
    })
    .contains("Copies"));
    assert!(refused(Typed {
        scale: "1000".into(),
        ..typed()
    })
    .contains("Custom Scale"));
    // A percent sign is what people type.
    assert!(job_for(
        &custom,
        &Typed {
            scale: "75%".into(),
            ..typed()
        }
    )
    .is_ok());
}

#[test]
fn an_encrypted_document_prints_as_image_whatever_the_box_says() {
    let mut encrypted = printed(1);
    encrypted.image_only = Some("encrypted");
    let job = job(
        &PrintSettings::default(),
        PageSetup::default(),
        &typed(),
        &encrypted,
        &[Destination::SaveAsPdf],
    )
    .expect("a job");
    assert!(job.print_as_image);
}

#[test]
fn the_preview_is_imposition_over_the_documents_own_page_sizes() {
    let settings = PrintSettings {
        n_up: onionskin_print::NUp {
            per_sheet: 2,
            ..Default::default()
        },
        ..PrintSettings::default()
    };
    let job = job_for(&settings, &typed()).expect("a job");
    let preview = sheets(&job, &printed(10)).expect("valid preview");
    assert_eq!(
        preview,
        onionskin_print::impose(&job, &[LETTER; 10]).expect("valid job")
    );
    assert_eq!(preview.len(), 5);
    assert_eq!(
        preview_label(&preview, 0),
        "Preview: sheet 1 of 5, landscape, pages 1, 2"
    );
    assert_eq!(preview_label(&[], 0), "Nothing to print");
}

#[test]
fn destinations_are_named_as_the_dialog_lists_them() {
    assert_eq!(Destination::SaveAsPdf.label(), "Save as PDF");
    assert_eq!(Destination::Printer("Office".into()).label(), "Office");
}

/// Booklet and Poster (M4) reach the job, and their sheets are what the
/// preview shows: ten pages fold into three sheets, six sides; a poster at
/// 200% with no overlap or marks is four tiles a page.
#[test]
fn booklet_and_poster_choices_reach_the_job_and_the_preview() {
    let mut settings = PrintSettings::default();
    for action in [
        PrintAction::Handling(HandlingChoice::Booklet),
        PrintAction::Binding(Binding::Right),
        PrintAction::BookletSides(BookletSides::BothSides),
    ] {
        assert!(apply(&mut settings, action));
    }
    let job = job_for(&settings, &typed()).expect("a job");
    assert_eq!(
        job.handling,
        Handling::Booklet(Booklet {
            sides: BookletSides::BothSides,
            binding: Binding::Right,
            sheets: None,
        })
    );
    assert_eq!(sheets(&job, &printed(10)).expect("valid preview").len(), 6);

    for action in [
        PrintAction::Handling(HandlingChoice::Poster),
        PrintAction::CutMarks,
    ] {
        assert!(apply(&mut settings, action));
    }
    let mut poster_input = typed();
    poster_input.poster_overlap = "0".into();
    let job = job_for(&settings, &poster_input).expect("a job");
    assert_eq!(
        job.handling,
        Handling::Poster(Poster {
            scale: 200.0,
            overlap: 0.0,
            cut_marks: false,
        })
    );
    assert_eq!(sheets(&job, &printed(2)).expect("valid preview").len(), 8);
}

#[test]
fn booklet_sheet_range_validation_uses_selected_source_pages() {
    let settings = PrintSettings {
        handling: HandlingChoice::Booklet,
        pages: PagesChoice::Custom,
        ..PrintSettings::default()
    };
    let mut input = typed();
    input.pages = "1-8".into();
    input.booklet_from = "2".into();
    input.booklet_to = "2".into();
    let full_job = job(
        &settings,
        PageSetup::default(),
        &input,
        &printed(10),
        &[Destination::SaveAsPdf],
    )
    .expect("valid physical interval");
    assert_eq!(
        full_job.handling,
        Handling::Booklet(Booklet {
            sheets: Some((1, 1)),
            ..Booklet::default()
        })
    );

    for (from, to, expected) in [
        ("", "2", "Sheets from"),
        ("x", "2", "Sheets from"),
        ("1.5", "2", "Sheets from"),
        ("0", "2", "Sheets from"),
        ("1", "184467440737095516160", "Sheets To"),
        ("2", "1", "must not be after"),
        ("1", "3", "from 1 to 2"),
    ] {
        input.booklet_from = from.into();
        input.booklet_to = to.into();
        let error = job(
            &settings,
            PageSetup::default(),
            &input,
            &printed(10),
            &[Destination::SaveAsPdf],
        )
        .expect_err("invalid physical interval");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn booklet_sheet_range_uses_odd_page_subset_bounds() {
    let settings = PrintSettings {
        handling: HandlingChoice::Booklet,
        pages: PagesChoice::Custom,
        subset: Subset::Odd,
        ..PrintSettings::default()
    };
    let mut input = typed();
    input.pages = "1-10".into();
    input.booklet_from = "1".into();
    input.booklet_to = "2".into();
    let full_job = job(
        &settings,
        PageSetup::default(),
        &input,
        &printed(10),
        &[Destination::SaveAsPdf],
    )
    .expect("odd selection has two physical sheets");
    assert_eq!(full_job.handling, Handling::Booklet(Booklet::default()));
    input.booklet_to = "3".into();
    let error = job(
        &settings,
        PageSetup::default(),
        &input,
        &printed(10),
        &[Destination::SaveAsPdf],
    )
    .expect_err("odd selection rejects a third sheet");
    assert!(error.contains("from 1 to 2"), "{error}");
}

#[test]
fn booklet_sheet_range_full_selection_is_stored_as_none() {
    let settings = PrintSettings {
        handling: HandlingChoice::Booklet,
        ..PrintSettings::default()
    };
    let mut input = typed();
    input.booklet_from = "1".into();
    input.booklet_to = "3".into();
    let job = job_for(&settings, &input).expect("valid full interval");
    assert_eq!(job.handling, Handling::Booklet(Booklet::default()));
}

#[test]
fn booklet_sheet_range_is_ignored_for_pages_and_poster() {
    let input = Typed {
        booklet_from: "not a sheet".into(),
        booklet_to: "also not a sheet".into(),
        ..typed()
    };
    for handling in [HandlingChoice::Pages, HandlingChoice::Poster] {
        let settings = PrintSettings {
            handling,
            ..PrintSettings::default()
        };
        assert!(job_for(&settings, &input).is_ok(), "{handling:?}");
    }
}

#[test]
fn poster_controls_parse_ascii_scale_and_inches_with_explicit_defaults() {
    let settings = PrintSettings {
        handling: HandlingChoice::Poster,
        ..PrintSettings::default()
    };
    let mut input = typed();
    input.poster_scale = " 125.5% ".into();
    input.poster_overlap = ".125".into();
    let parsed_job = job_for(&settings, &input).expect("valid custom Poster controls");
    let Handling::Poster(poster) = parsed_job.handling else {
        panic!("poster handling")
    };
    assert_eq!(poster.scale, 125.5);
    assert_eq!(poster.overlap, 9.0);
    input.poster_scale = "200".into();
    input.poster_overlap = "0.25".into();
    let defaults = job_for(&settings, &input).expect("default controls");
    let Handling::Poster(poster) = defaults.handling else {
        panic!("poster handling")
    };
    assert_eq!(poster.scale, 200.0);
    assert_eq!(poster.overlap, 18.0);

    let tiny = Printed {
        page_sizes: vec![(0.01, 0.01)],
        current_page: 0,
        image_only: None,
    };
    for (scale, overlap) in [("1", "0"), ("9999", "2")] {
        let mut endpoint = typed();
        endpoint.poster_scale = scale.into();
        endpoint.poster_overlap = overlap.into();
        assert!(job(
            &settings,
            PageSetup::default(),
            &endpoint,
            &tiny,
            &[Destination::SaveAsPdf]
        )
        .is_ok());
    }
    input.poster_scale = "2. %".into();
    input.poster_overlap = "0".into();
    assert!(job_for(&settings, &input).is_ok());
}

#[test]
fn poster_controls_reject_non_ascii_decimal_grammar() {
    let settings = PrintSettings {
        handling: HandlingChoice::Poster,
        ..PrintSettings::default()
    };
    for bad in [
        "", ".", "x", "١", "．", "+1", "-0", "1e2", "1,5", "1 0", "NaN", "inf", "1.2.3", "0.125in",
        "9pt", "2mm", "100%%",
    ] {
        let mut input = typed();
        input.poster_scale = bad.into();
        assert!(
            job_for(&settings, &input).is_err(),
            "scale accepted {bad:?}"
        );
        input = typed();
        input.poster_overlap = bad.into();
        assert!(
            job_for(&settings, &input).is_err(),
            "overlap accepted {bad:?}"
        );
    }
    let overflow = "1".repeat(400);
    let mut input = typed();
    input.poster_scale = overflow.clone();
    assert!(
        job_for(&settings, &input).is_err(),
        "scale accepted 400 digits"
    );
    input = typed();
    input.poster_overlap = overflow;
    assert!(
        job_for(&settings, &input).is_err(),
        "overlap accepted 400 digits"
    );
}

#[test]
fn poster_controls_retain_values_when_handling_is_switched_away_and_back() {
    let mut settings = PrintSettings {
        handling: HandlingChoice::Poster,
        ..PrintSettings::default()
    };
    let mut input = typed();
    input.poster_scale = "125.5%".into();
    input.poster_overlap = ".125".into();
    let first = job_for(&settings, &input).expect("custom Poster values");
    settings.handling = HandlingChoice::Pages;
    assert!(job_for(&settings, &input).is_ok());
    settings.handling = HandlingChoice::Poster;
    let second = job_for(&settings, &input).expect("retained Poster values");
    assert_eq!(first.handling, second.handling);
}

#[test]
fn poster_controls_cut_marks_revalidate_the_real_sheet_cap_without_clamping_values() {
    let printed = Printed {
        page_sizes: vec![(19_584.0, 25_344.0)],
        current_page: 0,
        image_only: None,
    };
    let mut settings = PrintSettings {
        handling: HandlingChoice::Poster,
        ..PrintSettings::default()
    };
    settings.poster.cut_marks = false;
    let mut input = typed();
    input.poster_scale = "100".into();
    input.poster_overlap = "0".into();
    let destinations = [Destination::SaveAsPdf];
    let setup = PageSetup {
        paper: 0,
        orientation: Orientation::Portrait,
    };
    let valid = job(&settings, setup, &input, &printed, &destinations)
        .expect("32x32 tiles fit the 1024-sheet cap without marks");
    assert_eq!(
        valid.handling,
        Handling::Poster(Poster {
            scale: 100.0,
            overlap: 0.0,
            cut_marks: false
        })
    );
    assert!(apply(&mut settings, PrintAction::CutMarks));
    assert!(job(&settings, setup, &input, &printed, &destinations,).is_err());
    assert!(apply(&mut settings, PrintAction::CutMarks));
    let restored = job(&settings, setup, &input, &printed, &destinations)
        .expect("turning marks off restores the valid job");
    assert_eq!(restored.handling, valid.handling);
}

#[test]
fn poster_controls_hidden_invalid_values_do_not_block_other_handling() {
    let mut input = typed();
    input.poster_scale = "not a number".into();
    input.poster_overlap = "1e2".into();
    for handling in [HandlingChoice::Pages, HandlingChoice::Booklet] {
        let settings = PrintSettings {
            handling,
            ..PrintSettings::default()
        };
        assert!(job_for(&settings, &input).is_ok(), "{handling:?}");
    }
}

#[test]
fn booklet_sheet_range_revalidates_retained_endpoints_for_current_selection() {
    let settings = PrintSettings {
        handling: HandlingChoice::Booklet,
        pages: PagesChoice::Current,
        ..PrintSettings::default()
    };
    let mut input = typed();
    input.booklet_from = "2".into();
    input.booklet_to = "2".into();
    let error = job_for(&settings, &input).expect_err("current page has one sheet");
    assert!(error.contains("from 1 to 1"), "{error}");
    input.booklet_from = "1".into();
    input.booklet_to = "1".into();
    assert!(job_for(&settings, &input).is_ok());
}
