//! Crop Pages through the plugin surface: the dialog's function, the
//! registered command, and the margins measured off a rendered page.
//!
//! How a box is computed and written is `core::pages`' and tested there.
//! This suite asserts what this crate adds: a crop is one undo step, the
//! page the viewer draws takes the new size, and "fit to content" lands on
//! the marks the page draws, whatever its rotation or existing crop.

use onionskin_core::pages::{Margins, PageBox};
use onionskin_core::protection::Refusal;
use onionskin_core::{Document, Error};
use onionskin_corpus_testing::encrypted_fixture;
use onionskin_plugin_api::command_ids::CROP_PAGES;
use onionskin_plugin_api::{
    Availability, CommandCtx, CommandEffect, CommandError, PluginManifest, PluginRegistry,
    Requirement, Session,
};
use onionskin_tools_edit::{
    crop_pages, crop_to_content, white_margins, CropPages, EditToolsPlugin,
};

/// A classic-xref PDF whose object `n` is `objects[n - 1]`.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// Two Letter pages. The first has a black square from (100, 300) to
/// (200, 500) and whatever extra `page` entries are given; the second is
/// blank.
fn marked(page: &str) -> Vec<u8> {
    let ink = "0 g 100 300 100 200 re f";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page} >>").into_bytes(),
        format!("<< /Length {} >>\nstream\n{ink}\nendstream", ink.len()).into_bytes(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
    ])
}

fn open(bytes: Vec<u8>) -> Document {
    Document::open_bytes(bytes).expect("opens")
}

fn crop_box(document: &mut Document, page: usize) -> Option<[f64; 4]> {
    document.page_geometry(page).expect("geometry").crop_box
}

fn registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    EditToolsPlugin.register(&mut registry);
    registry
}

fn run_command(document: &mut Document, page: usize) -> Result<(), CommandError> {
    let registry = registry();
    let command = registry
        .commands()
        .iter()
        .find(|command| command.id == CROP_PAGES)
        .expect("registered");
    (command.run)(&mut CommandCtx {
        doc: document,
        page,
    })
}

#[test]
fn the_plugin_registers_crop_as_an_edit() {
    let registry = registry();
    let ids: Vec<_> = registry.commands().iter().map(|c| c.id).collect();
    assert_eq!(ids[0], CROP_PAGES);
    assert_eq!(ids.len(), 5, "crop, and the image commands: {ids:?}");
    assert!(registry
        .commands()
        .iter()
        .all(|command| command.effect == CommandEffect::Edits));
    let tools: Vec<_> = registry.tools().map(|tool| tool.id()).collect();
    assert_eq!(
        tools,
        [
            "crop-pages",
            "link",
            "edit-image",
            "add-image",
            "edit-text",
            "add-text"
        ],
        "and the tools that draw"
    );
    assert_eq!(EditToolsPlugin.id(), "onionskin.tools-edit");
    assert_eq!(EditToolsPlugin.name(), "Edit PDF");

    let refusal = Refusal::EncryptedSource.reason();
    let requirement = Requirement::Command {
        id: CROP_PAGES,
        reason: "not installed",
    };
    let session = |edit_refusal| Session {
        registry: &registry,
        has_text_selection: false,
        edit_refusal,
        comment_refusal: edit_refusal,
        read_out_refusal: None,
    };
    assert_eq!(
        requirement.availability(&session(None)),
        Availability::Enabled
    );
    assert_eq!(
        requirement.availability(&session(Some(refusal))),
        Availability::Disabled(refusal)
    );
}

#[test]
fn a_dialog_crop_is_one_undo_step_and_the_viewer_draws_the_new_size() {
    let mut document = open(marked(""));
    let crop = CropPages {
        pages: vec![0, 1],
        which: PageBox::Crop,
        margins: Margins {
            top: 72.0,
            bottom: 72.0,
            left: 36.0,
            right: 36.0,
        },
        page_size: None,
    };
    crop_pages(&mut document, &crop).expect("crops");
    for page in 0..2 {
        assert_eq!(
            crop_box(&mut document, page),
            Some([36.0, 72.0, 576.0, 720.0])
        );
        assert_eq!(
            document.page_geometry(page).expect("geometry").render_size,
            (540.0, 648.0)
        );
    }
    assert!(document.undo().expect("undoes"));
    assert_eq!(
        crop_box(&mut document, 0),
        None,
        "one step undoes both pages"
    );
    assert!(!document.undo().expect("nothing more"));
}

/// Change Page Size and a crop in one step: the margins are measured from
/// the new, larger media box, centred on the old one.
#[test]
fn a_new_page_size_and_its_crop_are_one_undo_step() {
    let mut document = open(marked(""));
    let crop = CropPages {
        pages: vec![0],
        which: PageBox::Crop,
        margins: Margins {
            top: 10.0,
            bottom: 10.0,
            left: 10.0,
            right: 10.0,
        },
        page_size: Some((812.0, 992.0)),
    };
    crop_pages(&mut document, &crop).expect("resizes and crops");
    let geometry = document.page_geometry(0).expect("geometry").clone();
    assert_eq!(geometry.media_box, [-100.0, -100.0, 712.0, 892.0]);
    assert_eq!(geometry.crop_box, Some([-90.0, -90.0, 702.0, 882.0]));
    assert!(document.undo().expect("undoes"));
    assert_eq!(
        document.page_geometry(0).expect("geometry").media_box,
        [0.0, 0.0, 612.0, 792.0]
    );
}

#[test]
fn a_refused_crop_says_which_edit_and_why() {
    let mut document = open(marked(""));
    let error = crop_pages(
        &mut document,
        &CropPages {
            pages: vec![0],
            which: PageBox::Crop,
            margins: Margins {
                top: 400.0,
                bottom: 400.0,
                ..Margins::default()
            },
            page_size: None,
        },
    )
    .expect_err("nothing is left");
    assert!(matches!(
        error,
        CommandError::Edit {
            label: "Crop Pages",
            source: Error::PageBoxTooSmall { page: 0, .. }
        }
    ));
    assert_eq!(crop_box(&mut document, 0), None);
}

#[test]
fn white_margins_measure_to_the_marks_the_page_draws() {
    let mut document = open(marked(""));
    assert_eq!(
        white_margins(&mut document, 0).expect("measures"),
        Some(Margins {
            top: 292.0,
            bottom: 300.0,
            left: 100.0,
            right: 412.0,
        })
    );
    assert_eq!(white_margins(&mut document, 1).expect("measures"), None);
    assert!(matches!(
        white_margins(&mut document, 2),
        Err(CommandError::Page { page: 2, .. })
    ));
}

/// Turned a quarter clockwise, the page's left edge is at the top of the
/// screen: the margins come back as shown, which is what a crop takes.
#[test]
fn white_margins_are_as_shown_on_a_turned_page() {
    let mut document = open(marked("/Rotate 90"));
    assert_eq!(
        white_margins(&mut document, 0).expect("measures"),
        Some(Margins {
            top: 100.0,
            right: 292.0,
            bottom: 412.0,
            left: 300.0,
        })
    );
}

/// A page already cropped is measured from its media box all the same, so
/// cropping to content twice changes nothing the second time.
#[test]
fn crop_to_content_fits_the_marks_and_is_stable() {
    let mut document = open(marked("/CropBox [50 250 400 600]"));
    crop_to_content(&mut document, &[0, 1], PageBox::Crop).expect("crops");
    assert_eq!(
        crop_box(&mut document, 0),
        Some([100.0, 300.0, 200.0, 500.0])
    );
    assert_eq!(
        crop_box(&mut document, 1),
        None,
        "a blank page keeps its crop"
    );
    run_command(&mut document, 0).expect("runs again");
    assert_eq!(
        crop_box(&mut document, 0),
        Some([100.0, 300.0, 200.0, 500.0])
    );
}

#[test]
fn content_can_set_any_box() {
    let mut document = open(marked(""));
    crop_to_content(&mut document, &[0], PageBox::Trim).expect("sets the trim box");
    assert_eq!(
        crop_box(&mut document, 0),
        None,
        "the crop box is not the one asked for"
    );
    assert!(document.undo().expect("undoes"), "and it was one edit");
}

#[test]
fn a_crop_on_an_encrypted_document_is_refused_by_core() {
    let mut document = Document::open_path(&encrypted_fixture("r6-aes-256-print-only.pdf"))
        .expect("opens read-only");
    let crop = CropPages {
        pages: vec![0],
        which: PageBox::Art,
        margins: Margins::default(),
        page_size: None,
    };
    assert!(matches!(
        crop_pages(&mut document, &crop),
        Err(CommandError::Edit {
            source: Error::Protected(_),
            ..
        })
    ));
}
