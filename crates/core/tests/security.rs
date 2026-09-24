//! Changing a document's security writes a new file, which opens as it
//! should: with the passwords set, under the permissions set, and the
//! session goes on from it.

use onionskin_core::protection::{EditKind, Refusal};
use onionskin_core::security::{Access, Permissions, Protection, Strength};
use onionskin_core::{DocumentEdit, DocumentFile};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::{Name, Object};

fn protection(strength: Strength) -> Protection {
    Protection {
        strength,
        user_password: b"reader".to_vec(),
        owner_password: b"author".to_vec(),
        permissions: Permissions(Permissions::PRINT | Permissions::ANNOTATE),
        encrypt_metadata: true,
    }
}

#[test]
fn a_plain_document_is_protected_and_unprotected() {
    for strength in [Strength::Aes256, Strength::Aes128] {
        let dir = tempfile::tempdir().expect("dir");
        let plain = dir.path().join("plain.pdf");
        std::fs::copy(seed("hello.pdf"), &plain).expect("copies");
        let mut file = DocumentFile::open(&plain).expect("opens");
        assert_eq!(file.security_facts().method, "No Security");
        assert_eq!(file.security_refusal(), None);
        let (edit, base) = file.document_mut().edit_mut();
        edit.apply(
            base,
            DocumentEdit::SetInfoField {
                key: Name::new("Title"),
                value: Some(Object::String(b"Unsaved title".to_vec())),
            },
        )
        .expect("edits");

        let protected = dir.path().join("protected.pdf");
        file.save_with_security(&protected, Some(&protection(strength)))
            .expect("protects");
        assert_eq!(file.path(), Some(protected.as_path()));
        assert!(!file.is_dirty(), "the unsaved change is in the new file");
        let facts = file.security_facts();
        assert_eq!(facts.method, "Password Security");
        let level = match strength {
            Strength::Aes256 => "256-bit AES",
            Strength::Aes128 => "128-bit AES",
        };
        assert_eq!(facts.level, Some(level));
        assert_eq!(
            facts.access,
            Some(Access::Owner),
            "the author goes on with full access"
        );
        assert_eq!(
            file.info().expect("info").description.title.as_deref(),
            Some("Unsaved title")
        );
        assert!(file.render_page_now(0, 1.0).is_ok());

        assert!(
            DocumentFile::open(&protected).is_err(),
            "it needs a password"
        );
        let reader = DocumentFile::open_with_password(&protected, "reader").expect("opens");
        assert_eq!(reader.security_facts().access, Some(Access::User));
        assert_eq!(reader.edit_refusal_as(EditKind::Comments), None);
        assert_eq!(
            reader.edit_refusal(),
            Some(Refusal::Restricted(EditKind::Content))
        );
        assert_eq!(reader.security_refusal(), Some(Refusal::ChangeSecurity));

        let unprotected = dir.path().join("unprotected.pdf");
        file.save_with_security(&unprotected, None)
            .expect("removes");
        let mut reopened = DocumentFile::open(&unprotected).expect("opens with no password");
        assert_eq!(reopened.security_facts().method, "No Security");
        assert_eq!(
            reopened.info().expect("info").description.title.as_deref(),
            Some("Unsaved title")
        );
    }
}

#[test]
fn only_the_permissions_password_changes_security() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("restricted.pdf");
    std::fs::copy(encrypted_fixture("r6-aes-256-print-only.pdf"), &path).expect("copies");
    let mut file = DocumentFile::open(&path).expect("opens");
    let facts = file.security_facts();
    assert_eq!(facts.level, Some("256-bit AES"));
    assert!(!facts.permitted.modify() && facts.permitted.print());
    let outcome = file.save_with_security(&path, None);
    assert!(
        matches!(
            outcome,
            Err(onionskin_core::Error::Protected(Refusal::ChangeSecurity))
        ),
        "{outcome:?}"
    );

    let mut owner = DocumentFile::open_with_password(&path, "owner-password").expect("opens");
    owner
        .save_with_security(&path, None)
        .expect("the owner removes it");
    assert_eq!(
        DocumentFile::open(&path)
            .expect("opens")
            .security_facts()
            .method,
        "No Security"
    );
}
