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
    let preview = sheets(&job, &printed(10));
    assert_eq!(preview, onionskin_print::impose(&job, &[LETTER; 10]));
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
        })
    );
    assert_eq!(sheets(&job, &printed(10)).len(), 6);

    for action in [
        PrintAction::Handling(HandlingChoice::Poster),
        PrintAction::TileScale(200),
        PrintAction::Overlap(0),
        PrintAction::CutMarks,
    ] {
        assert!(apply(&mut settings, action));
    }
    let job = job_for(&settings, &typed()).expect("a job");
    assert_eq!(
        job.handling,
        Handling::Poster(Poster {
            scale: 200,
            overlap: 0.0,
            cut_marks: false,
        })
    );
    assert_eq!(sheets(&job, &printed(2)).len(), 8);
}
