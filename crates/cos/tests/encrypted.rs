//! Encrypted documents: opening them with or without a password, and writing
//! them encrypted.
//!
//! # Where the known answers come from
//!
//! ISO 32000 publishes the algorithms and no test vectors. So every fixture
//! under `corpus/encrypted/` was written by **qpdf**, through pikepdf, by
//! `corpus/make-encrypted.py` - an independent implementation of the same
//! standard - and carries the same known plaintext in a content stream and in
//! an information-dictionary string. A decryptor that matches qpdf on all
//! eight is correct in the sense that matters: it reads what the most widely
//! used open PDF toolkit writes.
//!
//! One fixture per revision, because the revisions fail independently. The
//! mutation this file is built around - handing back the file key as every
//! object key - breaks `/R` 2 to 4, which derive a key per object, and leaves
//! `/R` 6 green, which does not. A suite with only an AES-256 fixture would
//! pass that mutation.

use std::path::PathBuf;

mod common;

use common::{corpus_dir, corpus_root};
use onionskin_cos::{BytesSource, Document, Error, Object};

/// The plaintext `make-encrypted.py` puts in every fixture.
const CONTENT_TEXT: &[u8] = b"Onionskin decrypts this sentence";
const INFO_TITLE: &[u8] = b"Onionskin encrypted fixture";
const XMP_TITLE: &[u8] = b"Onionskin plaintext metadata";

/// Every fixture that opens with the empty user password.
const OPENS: &[&str] = &[
    "r2-rc4-40.pdf",
    "r3-rc4-128.pdf",
    "r4-rc4-128.pdf",
    "r4-aes-128.pdf",
    "r4-aes-128-plain-metadata.pdf",
    "r4-aes-128-objstm.pdf",
    "r6-aes-256.pdf",
];

fn fixture(name: &str) -> PathBuf {
    corpus_root()
        .expect("the corpus root is found; encrypted fixtures are committed")
        .join("encrypted")
        .join(name)
}

fn open(name: &str) -> Result<Document, Error> {
    let bytes = std::fs::read(fixture(name)).expect("the fixture is readable");
    Document::open(Box::new(BytesSource::new(bytes)))
}

// ---------------------------------------------------------------------------
// The class that opens
// ---------------------------------------------------------------------------

#[test]
fn every_revision_opens_with_the_empty_user_password() {
    for name in OPENS {
        let document = open(name).unwrap_or_else(|error| panic!("{name} refused: {error}"));
        assert!(document.is_encrypted(), "{name} reports itself encrypted");
        assert_eq!(document.page_count().expect("pages"), 1, "{name}");
    }
}

/// The content stream: a **stream**, decrypted under `/StmF`.
///
/// Failures are collected by fixture rather than stopping at the first, so a
/// broken revision is named: the mutation that hands back the file key as
/// every object key has to show `/R` 2 to 4 failing and `/R` 6 passing, and a
/// test that stops at the first failure cannot show the second half.
#[test]
fn every_revision_decrypts_its_content_stream() {
    let mut failed = Vec::new();
    for name in OPENS {
        let Ok(document) = open(name) else {
            failed.push(format!("{name}: does not open"));
            continue;
        };
        let contents = document
            .page(0)
            .ok()
            .and_then(|page| page.dict.get(b"Contents").and_then(Object::as_reference));
        let text = contents.and_then(|objref| decoded(&document, objref.number).ok());
        if !text.is_some_and(|text| contains(&text, CONTENT_TEXT)) {
            failed.push(format!("{name}: the content stream does not decrypt"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The information dictionary: a **string**, decrypted under `/StrF`. A
/// handler that decrypts streams and not strings, or keys them alike when the
/// crypt filters differ, gets exactly one of these two tests wrong.
#[test]
fn every_revision_decrypts_its_strings() {
    for name in OPENS {
        let document = open(name).expect("opens");
        let info = document
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference)
            .expect("an indirect /Info");
        let dict = document
            .get(info.number)
            .expect("/Info parses")
            .object
            .as_dict()
            .cloned()
            .expect("a dictionary");
        let Some(Object::String(title)) = dict.get(b"Title") else {
            panic!("{name}: /Title is a string");
        };
        assert_eq!(
            title.as_slice(),
            INFO_TITLE,
            "{name}: the decrypted /Title does not match"
        );
    }
}

/// Strings inside an object stream are **not** separately encrypted: the
/// container already was. Decrypting them a second time corrupts every
/// object-stream document, and the plain `r4-aes-128` fixture cannot catch it
/// because its `/Info` is not in an object stream.
#[test]
fn a_string_inside_an_object_stream_is_decrypted_once_not_twice() {
    let document = open("r4-aes-128-objstm.pdf").expect("opens");
    let info = document
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("an indirect /Info");
    let parsed = document.get(info.number).expect("/Info parses");
    assert!(
        matches!(parsed.origin, onionskin_cos::Origin::ObjectStream { .. }),
        "the fixture has to put /Info in an object stream, or this proves nothing"
    );
    let Some(Object::String(title)) = parsed
        .object
        .as_dict()
        .and_then(|dict| dict.get(b"Title"))
        .cloned()
    else {
        panic!("/Title is a string");
    };
    assert_eq!(title.as_slice(), INFO_TITLE);
}

/// `/EncryptMetadata false`: the XMP stream is written in the clear, and
/// "decrypting" plaintext turns it into noise.
#[test]
fn plaintext_metadata_is_left_alone() {
    for name in ["r4-aes-128-plain-metadata.pdf", "r4-rc4-128.pdf"] {
        let document = open(name).expect("opens");
        let metadata = document
            .catalog()
            .expect("catalog")
            .get(b"Metadata")
            .and_then(Object::as_reference)
            .expect("a metadata stream");
        let decoded = decoded(&document, metadata.number).expect("decodes");
        assert!(
            contains(&decoded, XMP_TITLE),
            "{name}: the plaintext XMP was mangled"
        );
    }
}

/// And when metadata **is** encrypted, it is decrypted like any other stream.
#[test]
fn encrypted_metadata_is_decrypted() {
    for name in ["r4-aes-128.pdf", "r6-aes-256.pdf"] {
        let document = open(name).expect("opens");
        let metadata = document
            .catalog()
            .expect("catalog")
            .get(b"Metadata")
            .and_then(Object::as_reference)
            .expect("a metadata stream");
        let decoded = decoded(&document, metadata.number).expect("decodes");
        assert!(contains(&decoded, XMP_TITLE), "{name}");
    }
}

// ---------------------------------------------------------------------------
// The class that is refused
// ---------------------------------------------------------------------------

/// The permissive bug ruled out: an empty password must not open a document
/// that has a user password.
#[test]
fn a_document_with_a_user_password_is_refused() {
    assert!(
        matches!(open("r6-aes-256-user-password.pdf"), Err(Error::Encrypted)),
        "a password-protected document opened with the empty password"
    );
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Guarantee 1 holds for this class too, and for free: a no-op save writes
/// nothing, so there is nothing to refuse.
#[test]
fn a_no_op_save_of_an_encrypted_document_writes_nothing() {
    for name in OPENS {
        let document = open(name).expect("opens");
        assert_eq!(
            document
                .section_for(&Default::default(), &Default::default())
                .expect("an empty overlay is not refused"),
            None,
            "{name}"
        );
    }
}

/// Every fixture's `/Info`, with its `/Title` replaced.
fn retitled(
    document: &Document,
    title: &[u8],
) -> (
    u32,
    std::collections::BTreeMap<u32, onionskin_cos::PendingEdit>,
) {
    let info = document
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("an indirect /Info");
    let mut dict = document
        .get(info.number)
        .expect("/Info parses")
        .object
        .as_dict()
        .cloned()
        .expect("a dictionary");
    dict.set("Title", Object::String(title.to_vec()));
    let mut edits = std::collections::BTreeMap::new();
    edits.insert(
        info.number,
        onionskin_cos::PendingEdit::Set {
            generation: info.generation,
            object: Object::Dict(dict),
        },
    );
    (info.number, edits)
}

fn title(document: &Document, number: u32) -> Vec<u8> {
    match document
        .get(number)
        .expect("parses")
        .object
        .as_dict()
        .and_then(|dict| dict.get(b"Title"))
        .cloned()
    {
        Some(Object::String(title)) => title,
        other => panic!("/Title is a string: {other:?}"),
    }
}

/// A section appended to an encrypted document is encrypted with its key,
/// under every revision: nothing it adds is in the clear, and it reads back.
#[test]
fn an_edit_to_an_encrypted_document_is_written_encrypted() {
    const NEW_TITLE: &[u8] = b"Retitled in the clear nowhere";
    for name in OPENS {
        let original = std::fs::read(fixture(name)).expect("readable");
        let document = open(name).expect("opens");
        let (number, edits) = retitled(&document, NEW_TITLE);
        let section = document
            .section_for(&edits, &Default::default())
            .expect("an encrypted document takes a section")
            .expect("one is written");
        assert!(
            !contains(&section, NEW_TITLE),
            "{name}: the title is in the clear"
        );
        let mut saved = original;
        saved.extend_from_slice(&section);
        let reopened = Document::open(Box::new(BytesSource::new(saved))).expect("reopens");
        assert_eq!(title(&reopened, number), NEW_TITLE, "{name}");
    }
}

/// The security itself cannot be changed by a section: that is a new file.
#[test]
fn a_section_may_not_change_the_encryption() {
    let document = open("r6-aes-256.pdf").expect("opens");
    let edits: std::collections::BTreeMap<onionskin_cos::Name, Option<Object>> =
        [(onionskin_cos::Name::new("Encrypt"), None)]
            .into_iter()
            .collect();
    assert!(matches!(
        document.section_for(&Default::default(), &edits),
        Err(Error::EncryptedWrite)
    ));
}

/// A user password opens a document that needs one, and the owner password
/// opens it as its owner.
#[test]
fn the_user_and_owner_passwords_open_a_protected_document() {
    use onionskin_crypto::Access;
    let bytes = std::fs::read(fixture("r6-aes-256-user-password.pdf")).expect("readable");
    let with = |password: &[u8]| {
        Document::open_repairing_with_password(Box::new(BytesSource::new(bytes.clone())), password)
            .map(|(document, _)| document)
    };
    let user = with(b"secret").expect("the user password opens it");
    assert_eq!(user.access(), Some(Access::User));
    assert_eq!(user.page_count().expect("pages"), 1);
    let owner = with(b"owner-password").expect("the owner password opens it");
    assert_eq!(owner.access(), Some(Access::Owner));
    assert_eq!(
        owner.reader_password(b"owner-password"),
        b"owner-password",
        "R 6"
    );
    assert_eq!(user.reader_password(b"secret"), b"secret");
    let r4 = Document::open_with_password(
        Box::new(BytesSource::new(
            std::fs::read(fixture("r4-aes-128.pdf")).expect("reads"),
        )),
        b"owner-password",
    )
    .expect("opens as its owner");
    assert_eq!(
        r4.reader_password(b"owner-password"),
        b"",
        "its user password is empty"
    );
    assert!(owner.permissions().expect("encrypted").modify());
    assert!(matches!(with(b"guess"), Err(Error::Encrypted)));
    assert_eq!(
        open("r2-rc4-40.pdf").expect("opens").access(),
        Some(Access::User),
        "the empty password is the user's"
    );
}

/// Protecting, changing and lifting security each write the whole document
/// afresh, and what comes out opens as it should, under every level.
#[test]
fn a_document_is_rewritten_with_new_security_or_none() {
    use onionskin_crypto::{Access, Permissions, Protection, Strength};
    for strength in [Strength::Aes256, Strength::Aes128] {
        let document = open("r4-aes-128-objstm.pdf").expect("opens");
        let protection = Protection {
            strength,
            user_password: b"new user".to_vec(),
            owner_password: b"new owner".to_vec(),
            permissions: Permissions(Permissions::PRINT),
            encrypt_metadata: true,
        };
        let (number, edits) = retitled(&document, b"Rewritten title");
        let bytes = document
            .rewrite(&edits, &Default::default(), Some(&protection))
            .expect("rewrites");
        assert!(
            !contains(&bytes, b"Rewritten title"),
            "{strength:?}: in the clear"
        );
        assert!(
            !contains(&bytes, CONTENT_TEXT),
            "{strength:?}: content in the clear"
        );
        let source = || Box::new(BytesSource::new(bytes.clone()));
        assert!(
            matches!(Document::open(source()), Err(Error::Encrypted)),
            "needs its password"
        );
        let (user, _) =
            Document::open_repairing_with_password(source(), b"new user").expect("opens");
        assert_eq!(user.access(), Some(Access::User));
        assert!(!user.permissions().expect("encrypted").modify());
        let info = user
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference)
            .expect("/Info");
        assert_eq!(info.number, number);
        assert_eq!(title(&user, number), b"Rewritten title");
        let contents = user
            .page(0)
            .ok()
            .and_then(|page| page.dict.get(b"Contents").and_then(Object::as_reference))
            .expect("contents");
        assert!(contains(
            &decoded(&user, contents.number).expect("decodes"),
            CONTENT_TEXT
        ));
        let (owner, _) =
            Document::open_repairing_with_password(source(), b"new owner").expect("opens");
        assert_eq!(owner.access(), Some(Access::Owner));

        let plain = owner
            .rewrite(&Default::default(), &Default::default(), None)
            .expect("lifts");
        let plain = Document::open(Box::new(BytesSource::new(plain.clone())))
            .expect("opens with no password");
        assert!(!plain.is_encrypted());
        assert_eq!(title(&plain, number), b"Rewritten title");
        assert_eq!(plain.page_count().expect("pages"), 1);
    }
}

// ---------------------------------------------------------------------------
// The real corpus
// ---------------------------------------------------------------------------

/// Every encrypted file in the fetched corpus that qpdf opens with an empty
/// password opens here too, and its page count agrees.
#[test]
fn the_external_encrypted_files_open() {
    let Some(root) = corpus_dir("external") else {
        return;
    };
    let mut opened = 0;
    for path in walk(&root) {
        let bytes = std::fs::read(&path).expect("readable");
        if !contains(&bytes, b"/Encrypt") {
            continue;
        }
        let document = Document::open_repairing(Box::new(BytesSource::new(bytes)))
            .map(|(document, _)| document)
            .unwrap_or_else(|error| panic!("{} refused: {error}", path.display()));
        if !document.is_encrypted() {
            continue;
        }
        assert!(
            document.page_count().expect("pages") >= 1,
            "{}",
            path.display()
        );
        opened += 1;
    }
    assert!(opened >= 4, "only {opened} encrypted external files opened");
}

/// A stream object's data with its filters undone.
fn decoded(document: &Document, number: u32) -> Result<Vec<u8>, Error> {
    let parsed = document.get(number)?;
    let stream = parsed
        .object
        .as_stream()
        .cloned()
        .expect("the object is a stream");
    document.decode_stream(&stream)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn walk(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|kind| kind == "pdf") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// qpdf, an independent implementation, reads what is written: with each
/// password, with the permissions set, and not without one. Skipped, loudly,
/// where qpdf is not installed.
#[test]
fn qpdf_reads_what_is_written() {
    use onionskin_crypto::{Permissions, Protection, Strength};
    if std::process::Command::new("qpdf")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("qpdf is not installed: skipping the independent check");
        return;
    }
    let dir = std::env::temp_dir().join(format!("onionskin-qpdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    for (strength, bits) in [(Strength::Aes256, "256"), (Strength::Aes128, "128")] {
        let document = open("r6-aes-256.pdf").expect("opens");
        let protection = Protection {
            strength,
            user_password: b"u".to_vec(),
            owner_password: b"o".to_vec(),
            permissions: Permissions(Permissions::PRINT | Permissions::PRINT_HIGH),
            encrypt_metadata: true,
        };
        let bytes = document
            .rewrite(&Default::default(), &Default::default(), Some(&protection))
            .expect("rewrites");
        let path = dir.join(format!("aes-{bits}.pdf"));
        std::fs::write(&path, bytes).expect("writes");
        let qpdf = |args: &[&str]| {
            let output = std::process::Command::new("qpdf")
                .args(args)
                .arg(&path)
                .output()
                .expect("qpdf runs");
            (
                output.status.code(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )
        };
        assert_eq!(qpdf(&["--password=u", "--check"]).0, Some(0), "{bits}");
        let (_, shown) = qpdf(&["--password=o", "--show-encryption"]);
        assert!(
            shown.contains(&format!(
                "P = {}",
                Permissions::PRINT | Permissions::PRINT_HIGH | Permissions::RESERVED
            )),
            "{shown}"
        );
        assert!(shown.contains("print low resolution: allowed"), "{shown}");
        assert!(shown.contains("modify anything: not allowed"), "{shown}");
        assert!(
            shown.contains(if bits == "256" { "AESv3" } else { "AESv2" }),
            "{shown}"
        );
        assert_ne!(qpdf(&["--password=wrong", "--check"]).0, Some(0), "{bits}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
