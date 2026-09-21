//! `core::protection`: what M3 lets a user do with an encrypted document.
//!
//! Two refusals, one predicate, both derived from the document rather than from
//! a flag. Asserted here at the depth this crate can reach - the predicate and
//! the editing gate over real encrypted documents - and at the app's depth by
//! the packages whose commands call it.

use std::path::PathBuf;

use onionskin_core::protection::{self, Refusal};
use onionskin_core::{Document, DocumentEdit, EditSession, Error};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::{BytesSource, Document as CosDocument, Name, Object};

fn encrypted(name: &str) -> PathBuf {
    encrypted_fixture(name)
}

fn open_cos(path: &PathBuf) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(
        std::fs::read(path).expect("readable"),
    )))
    .expect("opens")
}

/// The one predicate, over both kinds of document.
#[test]
fn the_predicate_refuses_an_encrypted_graph_and_allows_a_plain_one() {
    let plain = open_cos(&seed("hello.pdf"));
    assert_eq!(protection::read_out(&plain), Ok(()));

    let locked = open_cos(&encrypted("r4-aes-128.pdf"));
    assert_eq!(protection::read_out(&locked), Err(Refusal::EncryptedSource));
    assert_eq!(Refusal::EncryptedSource.milestone(), "M6");
}

/// The editing gate is the same answer, not a second function that can drift:
/// asserted by asking both over every encrypted fixture and a plain one.
#[test]
fn the_editing_gate_and_the_read_out_rule_agree_on_every_document() {
    let mut paths = vec![seed("hello.pdf"), seed("two-page.pdf")];
    for name in [
        "r2-rc4-40.pdf",
        "r3-rc4-128.pdf",
        "r4-rc4-128.pdf",
        "r4-aes-128.pdf",
        "r6-aes-256.pdf",
    ] {
        paths.push(encrypted(name));
    }
    for path in paths {
        let document = open_cos(&path);
        assert_eq!(
            protection::edit(&document),
            protection::read_out(&document),
            "{}",
            path.display()
        );
    }
}

/// Refused at the one door every edit goes through, so no tool, command or verb
/// can begin one - and before the body runs, so nothing is half-written.
#[test]
fn no_edit_can_begin_on_an_encrypted_document() {
    let base = open_cos(&encrypted("r4-aes-128.pdf"));
    let mut edit = EditSession::for_base(&base);
    let mut ran = false;
    let outcome = edit.transact(&base, "Anything", |_| {
        ran = true;
        Ok(())
    });
    assert!(
        matches!(outcome, Err(Error::Protected(Refusal::EncryptedSource))),
        "got {outcome:?}"
    );
    assert!(
        !ran,
        "the body must not run: refused at the start of the work"
    );
    assert!(edit.pending_edits().is_empty());
    assert!(!edit.is_dirty());
}

/// The same gate reached through `core::Document`, which is what a tool holds,
/// and through a verb rather than a raw transaction.
#[test]
fn a_verb_on_an_encrypted_document_is_refused_and_says_why() {
    let bytes = std::fs::read(encrypted("r6-aes-256.pdf")).expect("readable");
    let mut document = Document::open_bytes(bytes).expect("an encrypted document opens");
    assert_eq!(document.edit_refusal(), Some(Refusal::EncryptedSource));
    assert_eq!(document.read_out_refusal(), Some(Refusal::EncryptedSource));

    let (edit, base) = document.edit_mut();
    let outcome = edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Title"),
            value: Some(Object::String(b"changed".to_vec())),
        },
    );
    let Err(error) = outcome else {
        panic!("an encrypted document was edited");
    };
    let message = error.to_string();
    assert!(message.contains("encrypted"), "{message}");
    assert!(message.contains("M6"), "{message}");
    assert!(!document.is_dirty());
}

/// A plain document is not touched by any of this.
#[test]
fn a_plain_document_edits_as_before() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    assert_eq!(document.edit_refusal(), None);
    assert_eq!(document.protection_notice(), None);
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

/// The notice says what the user can and cannot do in words, and says the
/// uncomfortable part when the document's own permissions allow changes.
#[test]
fn the_open_time_notice_names_the_restriction_in_words() {
    let bytes = std::fs::read(encrypted("r4-aes-128.pdf")).expect("readable");
    let document = Document::open_bytes(bytes).expect("opens");
    let notice = document
        .protection_notice()
        .expect("an encrypted document has a notice");
    for words in ["encrypted", "read", "print", "editing is turned off", "M6"] {
        assert!(
            notice.contains(words),
            "the notice does not say {words:?}: {notice}"
        );
    }

    let permissions = open_cos(&encrypted("r4-aes-128.pdf"))
        .permissions()
        .expect("an encrypted document has permissions");
    assert_eq!(
        notice.contains("Its own permissions allow changes"),
        permissions.modify(),
        "the residual is stated exactly when /P bit 4 is set"
    );
}

/// An encrypted document still renders: the preview is `original ++ section`,
/// and an empty overlay has no section. Refusing unconditionally would make
/// every encrypted document unrenderable, which is the ordering the plan warns
/// about.
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
