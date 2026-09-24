//! Guarantee test 4: annotating a signed document leaves its signature valid.
//!
//! This drives the real Sticky Note tool over each signed fixture pyHanko
//! wrote, saves through `core` the way the shell's Save does, and asserts on
//! the bytes that reach the disk: the signed bytes are untouched, our own
//! validator still calls every approval signature valid and reports the note
//! as the change made after signing, and poppler's `pdfsig`, an independent
//! validator, agrees the signature still verifies.
//!
//! A certification signature that allows no changes, or form filling only,
//! is the exception the guarantee's sentence leaves room for: a note breaks
//! it, and both validators must say so.

use std::path::PathBuf;
use std::process::Command;

use onionskin_core::signatures::{Change, Verdict};
use onionskin_core::{Document, DocumentFile, Modifiers, PagePoint};
use onionskin_corpus_testing::signed_fixture;
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};
use onionskin_tools_comment::NoteTool;

/// Signed with an approval signature: a note must leave each one valid.
const APPROVALS: [&str; 6] = [
    "rsa-sha256.pdf",
    "rsa-pss.pdf",
    "ecdsa-p256.pdf",
    "ecdsa-p384.pdf",
    "cades.pdf",
    "two-signatures.pdf",
];

#[test]
fn a_note_saved_on_a_signed_document_leaves_every_approval_signature_valid() {
    for name in APPROVALS {
        let (original, saved) = annotate_and_save(name);
        assert_eq!(
            &saved[..original.len()],
            &original[..],
            "the signed bytes must survive the note untouched ({name})"
        );

        let validations = validate(name, &saved);
        assert!(!validations.is_empty(), "{name} lists its signatures");
        for validation in &validations {
            assert_eq!(
                validation.verdict,
                Verdict::Valid,
                "every approval signature must stay valid after a note ({name}, {})",
                validation.field
            );
            assert!(
                validation.changes.contains(&Change::Annotation),
                "the note must be reported as a change made after signing ({name})"
            );
        }
        assert_pdfsig_agrees(name, &saved, true);
    }
}

#[test]
fn a_note_breaks_a_certification_that_does_not_allow_comments() {
    for name in ["certified-p1.pdf", "certified-p2.pdf"] {
        let (_, saved) = annotate_and_save(name);
        let validations = validate(name, &saved);
        assert!(
            matches!(validations[0].verdict, Verdict::Invalid(_)),
            "a certification that allows no comments must be broken by one ({name})"
        );
        assert!(validations[0].disallowed.contains(&Change::Annotation));
        // Cryptographically the signature still verifies: the note is
        // outside its byte range. The verdict is about what was allowed.
        assert_pdfsig_agrees(name, &saved, true);
    }
}

/// Validate `saved` the way the shell does: through a document opened on it.
fn validate(name: &str, saved: &[u8]) -> Vec<onionskin_core::signatures::Validation> {
    let path = temp_dir(&format!("{name}-validate")).join(name);
    std::fs::write(&path, saved).expect("written");
    DocumentFile::open(&path)
        .expect("the saved document reopens")
        .validate_signatures()
        .unwrap_or_else(|error| panic!("{name} validates: {error}"))
}

/// Copy the fixture, place one note with the tool, save, and read both
/// versions of the file back.
fn annotate_and_save(name: &str) -> (Vec<u8>, Vec<u8>) {
    let dir = temp_dir(name);
    let path = dir.join(name);
    std::fs::copy(signed_fixture(name), &path).expect("fixture copied");
    let original = std::fs::read(&path).expect("readable");

    let mut file = DocumentFile::open(&path).expect("opens");
    place_note(&mut file);
    assert!(
        file.is_dirty(),
        "the tool has to have added the note ({name})"
    );
    file.save().expect("saves");
    (original, std::fs::read(&path).expect("readable"))
}

/// Click once with the Sticky Note tool on page one.
fn place_note(file: &mut DocumentFile) {
    let mut tool = NoteTool::new();
    let document: &mut Document = file;
    let mut viewport = onionskin_core::Viewport::new(
        document.page_count(),
        onionskin_core::ViewSize {
            width: 800.0,
            height: 600.0,
        },
        12.0,
    )
    .expect("viewport is valid");
    let at = PagePoint {
        page: 0,
        x: 400.0,
        y: 500.0,
    };
    let input = PointerInput {
        at,
        pressure: 1.0,
        modifiers: Modifiers::default(),
        clicks: 1,
    };
    let mut ctx = ToolCtx {
        doc: document,
        viewport: &mut viewport,
    };
    tool.on_pointer_down(&mut ctx, input);
    tool.on_pointer_up(&mut ctx, input);
}

/// When poppler's `pdfsig` is installed, it must call each signature's
/// cryptography valid (or not) as expected. CI installs it; a machine
/// without it skips this half and says so.
fn assert_pdfsig_agrees(name: &str, saved: &[u8], valid: bool) {
    let dir = temp_dir(&format!("{name}-pdfsig"));
    let path = dir.join(name);
    std::fs::write(&path, saved).expect("written");
    let output = match Command::new("pdfsig").arg(&path).output() {
        Ok(output) => output,
        Err(_) => {
            eprintln!("pdfsig is not installed: the independent check was skipped");
            return;
        }
    };
    let report = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        report.contains("Signature Validation: Signature is Valid."),
        valid,
        "pdfsig must agree on {name}:\n{report}"
    );
    assert!(
        !report.contains("Digest Mismatch"),
        "pdfsig must find every signed digest intact in {name}:\n{report}"
    );
}

fn temp_dir(name: &str) -> PathBuf {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "onionskin-sig-guarantee-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}
