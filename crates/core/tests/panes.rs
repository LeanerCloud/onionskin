//! The navigation-pane readers against real files.
//!
//! The unit tests in `core` build their own fixtures, so they prove the rules
//! but not that the rules match what producers write. These run the same
//! readers over named files in `corpus/external/`, which is fetched rather
//! than committed: absent, they skip loudly, and `ONIONSKIN_CORPUS_REQUIRED`
//! turns that skip into a failure wherever the corpus is meant to be there.

use std::path::{Path, PathBuf};

use onionskin_core::Document;

/// PDF Association, PDF 2.0 example set. Three bookmarks whose destinations
/// all resolve, titles in UTF-8 including an emoji and a right-to-left mark,
/// and two optional content groups the file turns off by default.
const UTF8_TEST: &str = "external/pdf-association/pdf20examples/pdf20-utf8-test.pdf";
/// veraPDF PDF/A-3b suite. One embedded CSV with a stated size and MIME type.
const EMBEDDED_FILE: &str =
    "external/verapdf/PDF_A-3b/6.8 Embedded files/veraPDF test suite 6-8-t02-fail-b.pdf";
/// A CAD drawing with seven optional content groups and an `/Order` tree.
const SEVEN_LAYERS: &str = "external/hayro/pdfs/custom/issue175.pdf";
/// veraPDF PDF/A-4 suite. The one file in the corpus carrying a signature
/// that has actually been signed.
const SIGNED: &str =
    "external/verapdf/PDF_A-4/6.1 File structure/6.1.11 Permissions/veraPDF test suite 6-1-11-t01-pass-a.pdf";
/// PDF Association difference set: the outline lives in an object stream the
/// file encodes with a filter no reader implements.
const UNREADABLE_OUTLINE: &str =
    "external/pdf-association/pdf-differences/UnknownFilter/UnknownFilter-OutlineObjStm.pdf";

#[test]
fn a_pdf_association_outline_reads_its_titles_and_destinations() {
    let Some(mut doc) = open(UTF8_TEST) else {
        return;
    };

    let items = doc.outline().expect("the outline reads");

    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|item| item.children.is_empty()));
    assert_eq!(items[0].title, "PDF 2.0 with UTF-8 test file");
    // The file's point: titles are UTF-8, so a reader that assumed PDFDoc
    // encoding would mangle these two rather than fail.
    assert!(items[1].title.contains("test"));
    assert!(items[2].title.contains('\u{1F308}'), "the rainbow survives");
    assert!(
        items.iter().all(|item| item.page == Some(0)),
        "every destination in a one-page document resolves to that page"
    );
}

#[test]
fn a_real_embedded_file_lists_and_extracts_its_bytes() {
    let Some(mut doc) = open(EMBEDDED_FILE) else {
        return;
    };

    let attachments = doc.attachments().expect("the attachments read").to_vec();

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].name, "ChartDiagram.csv");
    assert_eq!(attachments[0].mime.as_deref(), Some("text/csv"));
    assert_eq!(attachments[0].size, Some(33));

    let bytes = doc.attachment_bytes(0).expect("the attachment extracts");

    // The decoded length has to match the size the file states; a decoder
    // that stopped early would still hand back plausible-looking CSV.
    assert_eq!(bytes.len() as u64, attachments[0].size.expect("stated"));
    assert!(bytes.is_ascii());
}

#[test]
fn real_layer_lists_follow_each_files_own_default_configuration() {
    if let Some(mut doc) = open(SEVEN_LAYERS) {
        let layers = doc.layers().expect("the layers read");

        assert_eq!(layers.len(), 7);
        assert_eq!(layers[0].name, "Visible");
        // The file's /OFF list is empty, so every group is on, including the
        // one whose name says otherwise. Reading the name instead of the
        // configuration is the mistake this pins.
        assert_eq!(layers[1].name, "Hidden");
        assert!(
            layers.iter().all(|layer| layer.visible),
            "an empty /OFF list turns nothing off"
        );
        assert!(layers.iter().all(|layer| !layer.locked));
    }

    let Some(mut doc) = open(UTF8_TEST) else {
        return;
    };
    let layers = doc.layers().expect("the layers read");

    assert_eq!(layers.len(), 2);
    assert!(
        layers.iter().all(|layer| !layer.visible),
        "this file's /OFF names both groups"
    );
}

/// Toggling reaches the rasterizer on a real file, not only on a fixture
/// built to make it easy.
#[test]
fn hiding_every_layer_of_a_real_drawing_changes_what_it_renders() {
    let Some(mut doc) = open(SEVEN_LAYERS) else {
        return;
    };
    let before = doc.render_page_now(0, 0.4).expect("the page renders");
    let ink_before = ink(&before);
    assert!(ink_before > 0, "the drawing has marks to hide");

    for layer in doc.layers().expect("the layers read").to_vec() {
        assert!(doc
            .set_layer_visible(layer.id, false)
            .expect("every group in this file is unlocked"));
    }
    let hidden = doc.render_page_now(0, 0.4).expect("the page renders again");

    assert_eq!(
        (hidden.raster.width(), hidden.raster.height()),
        (before.raster.width(), before.raster.height())
    );
    assert!(
        ink(&hidden) < ink_before,
        "hiding every layer has to remove marks, went from {ink_before} to {}",
        ink(&hidden)
    );
}

#[test]
fn a_real_signed_field_reports_the_signer_and_claims_nothing_about_validity() {
    let Some(mut doc) = open(SIGNED) else {
        return;
    };

    let fields = doc.signatures().expect("the signature fields read");

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "Signature2");
    assert!(fields[0].signed);
    assert_eq!(fields[0].signer.as_deref(), Some("veraPDF"));
    assert_eq!(
        fields[0].signed_at.as_deref(),
        Some("D:20171004151916+03'00'")
    );
    // Everything above is a string the file wrote about itself. Nothing on
    // the type can say whether it verifies, which is M6's answer to give.
    assert_eq!(fields[0].reason, None);
}

/// A file whose outline the reader genuinely cannot produce says so, rather
/// than showing an empty pane that reads as "this document has no bookmarks".
#[test]
fn an_outline_the_reader_cannot_decode_fails_loudly() {
    let Some(mut doc) = open(UNREADABLE_OUTLINE) else {
        return;
    };

    assert!(doc.outline().is_err());
}

fn ink(render: &onionskin_core::PageRender) -> usize {
    render
        .raster
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] < 200 || pixel[1] < 200 || pixel[2] < 200)
        .count()
}

/// `corpus/external` is gitignored, so it is absent from a fresh clone. Say
/// so loudly rather than reporting a pass that was never earned.
fn open(relative: &str) -> Option<Document> {
    let root = match std::env::var_os("ONIONSKIN_CORPUS") {
        Some(from_env) => PathBuf::from(from_env),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?
            .join("corpus"),
    };
    let path = root.join(relative);
    if !path.is_file() {
        if std::env::var_os("ONIONSKIN_CORPUS_REQUIRED").is_some() {
            panic!("corpus required but {} is absent", path.display());
        }
        eprintln!("SKIPPED: {} is absent (it is gitignored)", path.display());
        return None;
    }
    Some(Document::open_path(&path).expect("the corpus file opens"))
}
