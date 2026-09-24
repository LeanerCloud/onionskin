//! The encrypted-source sweep (P1b's rule, asserted here in P20, the first
//! point at which commands, organize tools and codecs are all registered).
//!
//! Every way an M3 build can take an encrypted document's content somewhere
//! else is in one of three buckets:
//!
//! - **refused** on an encrypted document;
//! - **provably raster-only**: its output is pixels, checked by the bytes it
//!   writes;
//! - **reads into session state or the clipboard and writes no file**, named
//!   below with the reason it is safe.
//!
//! The walk covers the registry, every command and every codec, and
//! `core::Document`'s public methods that hand out bytes derived from the
//! object graph. An entry in none of the buckets fails this test by existing,
//! which is the only form of the rule that survives the next feature.

use onionskin_core::protection::Refusal;
use onionskin_core::{Document, Error};
use onionskin_corpus_testing::encrypted_fixture;

fn encrypted() -> Document {
    Document::open_path(&encrypted_fixture("r6-aes-256-print-only.pdf")).expect("the fixture opens")
}

fn refused_by_the_rule(error: &Error) -> bool {
    matches!(
        error,
        Error::Protected(Refusal::EncryptedSource | Refusal::Restricted(_))
    )
}

/// `core`'s own read-outs, which no registry walk reaches: attachment
/// extraction is a pane button going straight to `attachment_bytes`.
#[test]
fn core_read_outs_are_each_in_a_bucket() {
    let mut doc = encrypted();
    let refused = |result: Result<(), Error>, what: &str| match result {
        Err(error) => assert!(refused_by_the_rule(&error), "{what}: {error}"),
        Ok(()) => panic!("{what} read an encrypted document out"),
    };
    refused(doc.attachment_bytes(0).map(|_| ()), "attachment_bytes");
    refused(doc.page_svg(0).map(|_| ()), "page_svg");
    // Reads into session state: the text a selection, a search and the
    // clipboard use. It writes no file; the text codec that would is refused
    // above.
    assert!(
        doc.page_text(0).is_ok(),
        "page_text serves selection and search"
    );
    // Provably raster-only: the export snapshot feeds only the background
    // raster export, whose output the codec sweep checks by its bytes.
    assert!(doc.export_snapshot().is_ok());
}

/// The registry half, which needs every plugin that can read a document out.
#[cfg(all(
    feature = "commands-core",
    feature = "tools-organize",
    feature = "codecs-common"
))]
mod registry {
    use onionskin_app::build_registry;
    use onionskin_plugin_api::{
        CommandCtx, CommandEffect, ExportError, ExportRequest, PageRange, Requirement, Session,
    };

    use super::{encrypted, refused_by_the_rule};

    /// Commands that read the encrypted document into session state and write
    /// no file, each with why that is safe.
    const READS_INTO_SESSION: &[(&str, &str)] = &[
        (
            "edit.select-all",
            "puts the page's text into the session's text selection; a copy goes to the clipboard, not a file",
        ),
        ("edit.deselect-all", "clears the selection; reads nothing out"),
    ];

    /// Raster formats by the bytes they start with.
    const RASTER_MAGIC: &[(&str, &[u8])] = &[
        ("png", b"\x89PNG"),
        ("jpeg", b"\xFF\xD8\xFF"),
        ("tiff", b"II*\0"),
        ("tiff", b"MM\0*"),
    ];

    #[test]
    fn every_command_is_refused_or_named_as_reading_into_the_session() {
        let registry = build_registry();
        let mut doc = encrypted();
        let edit = doc.edit_refusal().map(|refusal| refusal.reason());
        let read_out = doc.read_out_refusal().map(|refusal| refusal.reason());
        assert!(
            edit.is_some() && read_out.is_some(),
            "the fixture is encrypted"
        );
        let session = Session {
            registry: &registry,
            has_text_selection: true,
            edit_refusal: edit,
            comment_refusal: edit,
            read_out_refusal: read_out,
        };
        assert!(
            !registry.commands().is_empty(),
            "the sweep would prove nothing"
        );
        let mut refusals = 0;
        for command in registry.commands() {
            let availability = Requirement::Command {
                id: command.id,
                reason: "not registered",
            }
            .availability(&session);
            match command.effect {
                CommandEffect::Edits => {
                    assert_eq!(availability.reason(), edit, "{} is offered", command.id);
                    // On every page, so a command that has nothing to do on the
                    // first (moving it earlier) still meets one it would edit.
                    for page in 0..doc.page_count() {
                        let before = doc.edit().epoch();
                        refusals += usize::from(
                            (command.run)(&mut CommandCtx {
                                doc: &mut doc,
                                page,
                            })
                            .is_err(),
                        );
                        assert_eq!(
                            doc.edit().epoch(),
                            before,
                            "{} changed an encrypted document on page {page}",
                            command.id
                        );
                    }
                }
                CommandEffect::ReadsOut => {
                    assert_eq!(availability.reason(), read_out, "{} is offered", command.id);
                }
                CommandEffect::Reads => {
                    assert!(
                        READS_INTO_SESSION.iter().any(|(id, _)| *id == command.id),
                        "{} reads the document and is in no bucket: refuse it, prove it \
                         raster-only, or name why it writes no file",
                        command.id
                    );
                }
            }
        }
        assert!(
            refusals > 0,
            "no editing command was ever refused, so none was tried"
        );
    }

    #[test]
    fn every_codec_is_refused_or_writes_only_pixels() {
        let registry = build_registry();
        let mut doc = encrypted();
        let request = ExportRequest {
            pages: PageRange::new(0, 0, doc.page_count()).expect("a page"),
            dpi: 36.0,
            quality: None,
        };
        let mut seen = 0;
        for codec in registry.codecs() {
            seen += 1;
            match codec.export_page(&mut doc, &request, 0, true) {
                Err(ExportError::Page { source, .. }) if refused_by_the_rule(&source) => {}
                Err(other) => panic!("{} failed for another reason: {other}", codec.id()),
                Ok(bytes) => assert!(
                    RASTER_MAGIC
                        .iter()
                        .any(|(_, magic)| bytes.starts_with(magic)),
                    "{} wrote something other than pixels from an encrypted document",
                    codec.id()
                ),
            }
        }
        assert!(
            seen >= 5,
            "PNG, JPEG, TIFF, SVG and text are all registered"
        );
    }
}
