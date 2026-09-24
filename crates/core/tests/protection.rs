//! `core::protection`: what a user may do with a document, from its
//! security.
//!
//! Asserted here over real encrypted documents written by qpdf
//! (`corpus/make-encrypted.py`): one whose permissions allow everything, one
//! that allows printing only, and one that allows comments and form filling.

use std::path::PathBuf;

use onionskin_core::protection::{self, EditKind, Refusal};
use onionskin_core::{Document, DocumentEdit, DocumentFile, EditSession, Error};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::{BytesSource, Document as CosDocument, Name, Object};

fn encrypted(name: &str) -> PathBuf {
    encrypted_fixture(name)
}

fn open_cos(path: &PathBuf, password: &[u8]) -> CosDocument {
    CosDocument::open_with_password(
        Box::new(BytesSource::new(std::fs::read(path).expect("readable"))),
        password,
    )
    .expect("opens")
}

const KINDS: [EditKind; 4] = [
    EditKind::Content,
    EditKind::Comments,
    EditKind::Forms,
    EditKind::Pages,
];

/// A plain document, and an encrypted one whose permissions allow
/// everything, allow everything.
#[test]
fn what_allows_everything_refuses_nothing() {
    for path in [
        seed("hello.pdf"),
        encrypted("r4-aes-128.pdf"),
        encrypted("r6-aes-256.pdf"),
    ] {
        let document = open_cos(&path, b"");
        assert_eq!(
            protection::read_out(&document),
            Ok(()),
            "{}",
            path.display()
        );
        for kind in KINDS {
            assert_eq!(
                protection::edit_as(&document, kind),
                Ok(()),
                "{}",
                path.display()
            );
        }
        assert_eq!(protection::notice(&document), None);
    }
}

/// Printing only: every kind of change is refused, and so is copying out.
#[test]
fn print_only_refuses_every_change() {
    let document = open_cos(&encrypted("r6-aes-256-print-only.pdf"), b"");
    assert_eq!(
        protection::read_out(&document),
        Err(Refusal::EncryptedSource)
    );
    for kind in KINDS {
        assert_eq!(
            protection::edit_as(&document, kind),
            Err(Refusal::Restricted(kind))
        );
    }
    assert_eq!(
        protection::edit(&document),
        Err(Refusal::Restricted(EditKind::Content))
    );
    let notice = protection::notice(&document).expect("a notice");
    assert!(
        notice.contains(
            "does not allow changes, comments, filling in forms, changing pages or copying content"
        ),
        "{notice}"
    );
    assert!(notice.contains("permissions password"), "{notice}");
}

/// Comments and form filling allowed, nothing else.
#[test]
fn comments_only_allows_comments_and_forms() {
    let document = open_cos(&encrypted("r6-aes-256-comments-only.pdf"), b"");
    assert_eq!(protection::edit_as(&document, EditKind::Comments), Ok(()));
    assert_eq!(protection::edit_as(&document, EditKind::Forms), Ok(()));
    assert_eq!(
        protection::edit_as(&document, EditKind::Content),
        Err(Refusal::Restricted(EditKind::Content))
    );
    assert_eq!(
        protection::edit_as(&document, EditKind::Pages),
        Err(Refusal::Restricted(EditKind::Pages))
    );
}

/// The owner password lifts every restriction.
#[test]
fn the_owner_password_lifts_the_restrictions() {
    let document = open_cos(&encrypted("r6-aes-256-print-only.pdf"), b"owner-password");
    assert_eq!(protection::read_out(&document), Ok(()));
    for kind in KINDS {
        assert_eq!(protection::edit_as(&document, kind), Ok(()));
    }
    assert_eq!(protection::notice(&document), None);
}

/// Refused at the one door every edit goes through, so no tool, command or
/// verb can begin one - and before the body runs, so nothing is half-written.
#[test]
fn no_refused_edit_can_begin() {
    let base = open_cos(&encrypted("r6-aes-256-comments-only.pdf"), b"");
    let mut edit = EditSession::for_base(&base);
    let mut ran = false;
    let outcome = edit.transact(&base, "Anything", |_| {
        ran = true;
        Ok(())
    });
    assert!(
        matches!(
            outcome,
            Err(Error::Protected(Refusal::Restricted(EditKind::Content)))
        ),
        "got {outcome:?}"
    );
    assert!(
        !ran,
        "the body must not run: refused at the start of the work"
    );
    assert!(edit.pending_edits().is_empty());

    let outcome = edit.transact_as(&base, "Comment", EditKind::Comments, |_| {
        ran = true;
        Ok(())
    });
    assert!(outcome.is_ok() && ran, "a comment is allowed");
}

/// The same gate reached through `core::Document`, which is what a tool holds,
/// and through a verb rather than a raw transaction.
#[test]
fn a_refused_verb_says_why() {
    let bytes = std::fs::read(encrypted("r6-aes-256-print-only.pdf")).expect("readable");
    let mut document = Document::open_bytes(bytes).expect("opens");
    assert_eq!(
        document.edit_refusal(),
        Some(Refusal::Restricted(EditKind::Content))
    );
    assert_eq!(
        document.edit_refusal_as(EditKind::Comments),
        Some(Refusal::Restricted(EditKind::Comments))
    );
    assert_eq!(document.read_out_refusal(), Some(Refusal::EncryptedSource));
    assert!(!document.permitted().modify());
    assert_eq!(document.access(), Some(onionskin_crypto::Access::User));

    let (edit, base) = document.edit_mut();
    let outcome = edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Title"),
            value: Some(Object::String(b"changed".to_vec())),
        },
    );
    let Err(error) = outcome else {
        panic!("a document that allows no changes was changed");
    };
    let message = error.to_string();
    assert!(message.contains("changes are not allowed"), "{message}");
    assert!(message.contains("permissions password"), "{message}");
    assert!(!document.is_dirty());
}

/// An encrypted document that allows changes takes them, saves them
/// encrypted, and reads them back after the save.
#[test]
fn an_encrypted_document_is_edited_and_saved_encrypted() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("secured.pdf");
    std::fs::copy(encrypted("r6-aes-256-user-password.pdf"), &path).expect("copies");
    assert!(Document::open_path(&path).is_err(), "it needs its password");
    let mut document = DocumentFile::open_with_password(&path, "secret").expect("opens");
    let (edit, base) = document.edit_mut();
    edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Title"),
            value: Some(Object::String(b"A secret title".to_vec())),
        },
    )
    .expect("allowed");
    assert_eq!(
        document.info().expect("reads").description.title.as_deref(),
        Some("A secret title"),
        "the preview reads the edit through the password"
    );
    document.save().expect("saves");
    let saved = std::fs::read(&path).expect("reads");
    assert!(
        !saved.windows(14).any(|w| w == b"A secret title"),
        "in the clear"
    );
    let mut reopened = Document::open_path_with_password(&path, "secret").expect("reopens");
    assert_eq!(
        reopened.info().expect("reads").description.title.as_deref(),
        Some("A secret title")
    );
    assert!(reopened.render_page_now(0, 1.0).is_ok(), "and renders");
}

/// A plain document is not touched by any of this.
#[test]
fn a_plain_document_edits_as_before() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    assert_eq!(document.edit_refusal(), None);
    assert_eq!(document.protection_notice(), None);
    assert_eq!(document.access(), None);
    let (edit, base) = document.edit_mut();
    edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Title"),
            value: Some(Object::String(b"changed".to_vec())),
        },
    )
    .expect("a plain document edits");
    assert!(document.is_dirty());
}

/// The reasons are words a disabled entry can show.
#[test]
fn each_refusal_has_its_reason() {
    let reasons: Vec<&str> = [
        Refusal::EncryptedSource,
        Refusal::Restricted(EditKind::Content),
        Refusal::Restricted(EditKind::Comments),
        Refusal::Restricted(EditKind::Forms),
        Refusal::Restricted(EditKind::Pages),
    ]
    .map(Refusal::reason)
    .to_vec();
    assert!(reasons
        .iter()
        .all(|reason| reason.starts_with("Security: ")));
    let unique: std::collections::BTreeSet<_> = reasons.iter().collect();
    assert_eq!(unique.len(), reasons.len());
}

/// An encrypted document still renders: the preview is `original ++ section`,
/// and an empty overlay has no section.
#[test]
fn an_encrypted_document_still_has_preview_bytes() {
    let bytes = std::fs::read(encrypted("r4-aes-128.pdf")).expect("readable");
    let length = bytes.len();
    let mut document = Document::open_bytes(bytes).expect("opens");
    let preview = document
        .preview_bytes(onionskin_core::AnnotationFilter::DocumentAndMarkups)
        .expect("an encrypted document previews");
    assert_eq!(preview.len(), length, "and the preview is the file itself");
}
