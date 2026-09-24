//! Signature validation over the pyHanko fixtures (`corpus/make-signed.py`),
//! with poppler's pdfsig as the independent referee where it has an opinion.

use onionskin_core::signatures::{Change, Coverage, Verdict};
use onionskin_core::{add_annotation, Annotation, Document, DocumentFile, Rect, Subtype};
use onionskin_corpus_testing::signed_fixture;

fn validate(name: &str) -> Vec<onionskin_core::signatures::Validation> {
    let mut document = Document::open_path(&signed_fixture(name)).expect("opens");
    document.validate_signatures().expect("validates")
}

#[test]
fn every_algorithm_is_valid_over_the_whole_file() {
    for name in [
        "rsa-sha256.pdf",
        "rsa-pss.pdf",
        "ecdsa-p256.pdf",
        "ecdsa-p384.pdf",
        "rsa-sha1.pdf",
        "cades.pdf",
        "certified-p1.pdf",
        "certified-p2.pdf",
    ] {
        let found = validate(name);
        assert_eq!(found.len(), 1, "{name}");
        let validation = &found[0];
        assert_eq!(validation.verdict, Verdict::Valid, "{name}: {validation:?}");
        assert_eq!(validation.coverage, Coverage::WholeFile, "{name}");
        assert!(validation.changes.is_empty(), "{name}");
        assert_eq!(validation.is_weak(), name == "rsa-sha1.pdf", "{name}");
        assert!(
            validation.summary().contains("has not been modified"),
            "{name}"
        );
        assert!(!validation.summary().contains("identity"), "{name}");
    }
    assert_eq!(validate("certified-p1.pdf")[0].certification, Some(1));
    assert_eq!(validate("certified-p2.pdf")[0].certification, Some(2));
    assert_eq!(
        validate("cades.pdf")[0].sub_filter.as_deref(),
        Some("ETSI.CAdES.detached")
    );
}

#[test]
fn a_tampered_file_is_invalid_and_an_empty_field_is_not_listed() {
    let found = validate("tampered.pdf");
    assert!(
        matches!(&found[0].verdict, Verdict::Invalid(why) if why.contains("altered")),
        "{:?}",
        found[0].verdict
    );
    assert!(validate("unsigned-field.pdf").is_empty());
}

#[test]
fn a_signature_before_later_sections_covers_its_revision_and_names_the_changes() {
    let found = validate("two-signatures.pdf");
    assert_eq!(found.len(), 2);
    assert!(
        matches!(found[0].coverage, Coverage::Revision { later: 1, .. }),
        "{:?}",
        found[0].coverage
    );
    assert!(
        found[0].changes.contains(&Change::Signature),
        "{:?}",
        found[0].changes
    );
    assert!(
        !found[0].changes.contains(&Change::Other),
        "{:?}",
        found[0].changes
    );
    assert!(found
        .iter()
        .all(|validation| validation.verdict == Verdict::Valid));
    assert_eq!(found[1].coverage, Coverage::WholeFile);
    let mut document = Document::open_path(&signed_fixture("two-signatures.pdf")).expect("opens");
    let first = document.validate_signatures().expect("validates").remove(0);
    let signed = document.signed_version(&first).expect("a signed version");
    let whole = std::fs::read(signed_fixture("two-signatures.pdf")).expect("reads");
    assert!(signed.len() < whole.len() && whole.starts_with(&signed));
    let mut earlier = Document::open_bytes(signed).expect("the signed version opens");
    let alone = earlier.validate_signatures().expect("validates");
    assert_eq!(alone.len(), 1, "one signature then");
    assert_eq!(alone[0].coverage, Coverage::WholeFile);

    let annotated = &validate("annotated-after.pdf")[0];
    assert_eq!(annotated.verdict, Verdict::Valid);
    assert!(
        annotated.changes.contains(&Change::Annotation),
        "{:?}",
        annotated.changes
    );
    assert!(
        !annotated.changes.contains(&Change::Other),
        "{:?}",
        annotated.changes
    );
    assert!(annotated.summary().contains("changed after"));
}

/// Guarantee 4's core: a note added and saved by Onionskin leaves an
/// approval signature valid, and pdfsig says so too; a certification that
/// allows no changes is broken by it, as Acrobat would report.
#[test]
fn a_note_saved_here_keeps_an_approval_signature_valid() {
    for (name, still_valid) in [
        ("rsa-sha256.pdf", true),
        ("ecdsa-p256.pdf", true),
        ("certified-p2.pdf", false),
        ("certified-p1.pdf", false),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join(name);
        std::fs::copy(signed_fixture(name), &path).expect("copies");
        let mut file = DocumentFile::open(&path).expect("opens");
        let page = file
            .structure()
            .expect("reads")
            .page(0)
            .expect("a page")
            .objref;
        let note = Annotation::new(Subtype::Text, Rect::new(300.0, 300.0, 320.0, 320.0));
        file.edit_annotations("Add Note", |tx, structure| {
            add_annotation(tx, structure, page, &note, 0).map(|_| ())
        })
        .expect("annotates");
        file.save().expect("saves");

        let found = file.validate_signatures().expect("validates");
        let validation = &found[0];
        assert!(
            validation.changes.contains(&Change::Annotation),
            "{name}: {:?}",
            validation.changes
        );
        assert!(
            !validation.changes.contains(&Change::Other),
            "{name}: {:?}",
            validation.changes
        );
        assert_eq!(
            validation.verdict == Verdict::Valid,
            still_valid,
            "{name}: {:?}",
            validation.verdict
        );
        pdfsig_agrees(&path, true);
    }
}

/// pdfsig checks the cryptography only: every signature's bytes and key.
fn pdfsig_agrees(path: &std::path::Path, valid: bool) {
    let Ok(output) = std::process::Command::new("pdfsig")
        .arg("-nocert")
        .arg(path)
        .output()
    else {
        eprintln!("pdfsig is not installed: skipping the independent check");
        return;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.contains("Signature is Valid") && !text.contains("Digest Mismatch"),
        valid,
        "{text}"
    );
}

fn test_ca(for_certified: bool) -> onionskin_core::signatures::TrustAnchor {
    let pem = std::fs::read(signed_fixture("test-ca.pem")).expect("the test CA reads");
    onionskin_core::signatures::TrustAnchor {
        certificate: onionskin_core::signatures::Certificate::from_file_bytes(&pem).remove(0),
        for_signatures: true,
        for_certified,
    }
}

#[test]
fn a_signer_is_trusted_once_their_ca_is() {
    use onionskin_core::signatures::{identity, identity_sentence, Identity, VerificationTime};
    let now = std::time::SystemTime::now();
    let approval = &validate("rsa-sha256.pdf")[0];

    let unknown = identity(approval, &[], VerificationTime::Current, now);
    assert!(matches!(unknown, Identity::Unknown(_)));
    assert!(identity_sentence(&unknown).starts_with("The signer's identity is unknown: "));

    let trusted = identity(approval, &[test_ca(false)], VerificationTime::Current, now);
    let Identity::Valid { path } = &trusted else {
        panic!("the test CA issued the signer: {trusted:?}")
    };
    assert_eq!(path[0].display_name(), "Ada Signer");
    assert_eq!(
        identity_sentence(&trusted),
        "The signer's identity is valid."
    );

    // The signer's own clock: the certificates were valid when it signed.
    let at_creation = identity(approval, &[test_ca(false)], VerificationTime::Creation, now);
    assert!(matches!(at_creation, Identity::Valid { .. }));

    // Long after every certificate expired.
    let later = now + std::time::Duration::from_secs(60 * 60 * 24 * 365 * 20);
    let expired = identity(
        approval,
        &[test_ca(false)],
        VerificationTime::Current,
        later,
    );
    assert!(
        matches!(&expired, Identity::Invalid(reason) if reason.contains("expired")),
        "{expired:?}"
    );

    // A certification needs a certificate trusted for certified documents.
    let certified = &validate("certified-p2.pdf")[0];
    assert!(matches!(
        identity(certified, &[test_ca(false)], VerificationTime::Current, now),
        Identity::Unknown(_)
    ));
    assert!(matches!(
        identity(certified, &[test_ca(true)], VerificationTime::Current, now),
        Identity::Valid { .. }
    ));
}
