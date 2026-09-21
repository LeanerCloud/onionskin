//! The encryption-class measurement: which encrypted documents M3 opens, and
//! what their permissions ask for.
//!
//! One row per encrypted file in `corpus/external`, with the columns
//! `docs/evidence/encryption-classes.md` records: `/V` and `/R`, whether the
//! **empty user password** validates (the permissions-only class Acrobat opens
//! without prompting), whether the **empty owner password** validates, and the
//! `/P` bits by name.
//!
//! **The tally is asserted, not printed.** A corpus refresh that changes the
//! picture fails this test, so the numbers in the evidence document and the
//! `known-issues.md` entry cannot go stale silently. When it fails, the right
//! response is to regenerate the table (run this with `--nocapture`, the rows
//! are printed) and update the constants below together.
//!
//! Three counts carry the ruling:
//!
//! - **permissions-only**: files M3 now opens;
//! - **of those, `/P` bit 4 set**: files whose own permissions allow changes,
//!   which ruling A's residual makes worse than Acrobat - Onionskin opens them
//!   and refuses to edit them;
//! - **encrypted and repaired**: files that open but cannot render, because
//!   the repair is a section and no section may be written into an encrypted
//!   document. The hole in "renders it" the plan names.

mod common;

use common::{corpus_dir, pdfs_in};
use onionskin_cos::{BytesSource, Document, Error, Object, Provenance};

/// Encrypted files in `external/`. Four, all veraPDF, with the `hayro` sets
/// unreachable from the network this was measured on; the spike's figure of
/// roughly 35 included them.
const ENCRYPTED: usize = 4;
/// Of those, the empty user password opens this many.
const PERMISSIONS_ONLY: usize = 4;
/// Of the permissions-only class, this many have `/P` bit 4 (modify) set.
const MODIFY_ALLOWED: usize = 1;
/// Encrypted and needing repair: open, but cannot render.
const ENCRYPTED_AND_REPAIRED: usize = 0;

/// The owner-password check the table's column rests on, asserted in the
/// positive case against every committed fixture: each was written by qpdf with
/// the owner password `owner-password`, which must validate, and an empty one,
/// which must not. A check that only ever answers "no" would pass the table's
/// column on every file but one.
#[test]
fn the_owner_password_check_agrees_with_qpdf_on_every_revision() {
    let Some(root) = common::corpus_root() else {
        return;
    };
    for name in [
        "r2-rc4-40.pdf",
        "r3-rc4-128.pdf",
        "r4-rc4-128.pdf",
        "r4-aes-128.pdf",
        "r6-aes-256.pdf",
        "r6-aes-256-user-password.pdf",
    ] {
        let bytes = std::fs::read(root.join("encrypted").join(name)).expect("readable");
        let (dict, file_id) = raw_encrypt_dict(&bytes).expect("the fixture has an /Encrypt");
        let encrypt = read(&dict);
        assert!(
            onionskin_crypto::validates_owner_password(&encrypt, &file_id, b"owner-password"),
            "{name}: the owner password qpdf wrote does not validate"
        );
        assert!(
            !onionskin_crypto::validates_owner_password(&encrypt, &file_id, b""),
            "{name}: an empty owner password validated"
        );
    }
}

/// `/Encrypt` and `/ID` straight from the trailer, for a document that may not
/// open with the empty user password.
fn raw_encrypt_dict(bytes: &[u8]) -> Option<(onionskin_cos::Dict, Vec<u8>)> {
    let document = Document::open(Box::new(BytesSource::new(bytes.to_vec())));
    match document {
        Ok(document) => encrypt_dict(&document),
        // The password-protected fixture: read its trailer through a copy with
        // `/Encrypt` renamed, so `cos` opens it without a handler and the
        // dictionary can be read as data.
        Err(Error::Encrypted) => {
            let renamed = replace(bytes, b"/Encrypt", b"/Encrypx");
            let document = Document::open(Box::new(BytesSource::new(renamed))).ok()?;
            let dict = match document.trailer().get(b"Encrypx")? {
                Object::Dict(dict) => dict.clone(),
                Object::Ref(objref) => document.get(objref.number).ok()?.object.as_dict()?.clone(),
                _ => return None,
            };
            let file_id = match document.trailer().get(b"ID") {
                Some(Object::Array(ids)) => match ids.first() {
                    Some(Object::String(first)) => first.clone(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
            Some((dict, file_id))
        }
        Err(_) => None,
    }
}

fn replace(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut index = 0;
    while index < haystack.len() {
        if haystack[index..].starts_with(from) {
            out.extend_from_slice(to);
            index += from.len();
        } else {
            out.push(haystack[index]);
            index += 1;
        }
    }
    out
}

#[derive(Debug)]
struct Row {
    path: String,
    v: i64,
    r: i64,
    user_empty: bool,
    owner_empty: bool,
    permissions: String,
    modify: bool,
    repaired: bool,
}

#[test]
fn the_encryption_classes_match_the_recorded_tally() {
    let Some(root) = corpus_dir("external") else {
        return;
    };
    let mut rows: Vec<Row> = Vec::new();
    for path in pdfs_in(&root) {
        let bytes = std::fs::read(&path).expect("readable");
        if !bytes.windows(8).any(|window| window == b"/Encrypt") {
            continue;
        }
        let Some(row) = measure(&path.display().to_string(), bytes) else {
            continue;
        };
        rows.push(row);
    }
    rows.sort_by(|left, right| left.path.cmp(&right.path));

    println!("| File | /V | /R | empty user | empty owner | /P | repaired |");
    println!("| --- | --- | --- | --- | --- | --- | --- |");
    for row in &rows {
        println!(
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            row.path.trim_start_matches(&format!("{}/", root.display())),
            row.v,
            row.r,
            yes(row.user_empty),
            yes(row.owner_empty),
            row.permissions,
            yes(row.repaired)
        );
    }

    let permissions_only: Vec<&Row> = rows.iter().filter(|row| row.user_empty).collect();
    assert_eq!(rows.len(), ENCRYPTED, "encrypted files in external/");
    assert_eq!(
        permissions_only.len(),
        PERMISSIONS_ONLY,
        "files the empty user password opens"
    );
    assert_eq!(
        permissions_only.iter().filter(|row| row.modify).count(),
        MODIFY_ALLOWED,
        "permissions-only files whose own /P allows modification"
    );
    assert_eq!(
        permissions_only.iter().filter(|row| row.repaired).count(),
        ENCRYPTED_AND_REPAIRED,
        "permissions-only files that also need repair, and so cannot render"
    );
}

/// One file's row, or `None` for a file whose trailer names `/Encrypt` only in
/// passing - a string, a comment - and is not encrypted.
fn measure(path: &str, bytes: Vec<u8>) -> Option<Row> {
    let (document, provenance) =
        match Document::open_repairing(Box::new(BytesSource::new(bytes.clone()))) {
            Ok(opened) => opened,
            // Refused: needs a password. Still a row, with the empty user
            // password failing; the dictionary is read from a scan below.
            Err(Error::Encrypted) => return refused_row(path, bytes),
            Err(_) => return None,
        };
    if !document.is_encrypted() {
        return None;
    }
    let (dict, file_id) = encrypt_dict(&document)?;
    let permissions = document.permissions()?;
    let encrypt = read(&dict);
    Some(Row {
        path: path.to_owned(),
        v: encrypt.v.into(),
        r: encrypt.r.into(),
        user_empty: true,
        owner_empty: onionskin_crypto::validates_owner_password(&encrypt, &file_id, b""),
        permissions: names(permissions),
        modify: permissions.modify(),
        repaired: !matches!(provenance, Provenance::Clean),
    })
}

/// A document that needs a password does not open, so its `/Encrypt` is read
/// from a document opened past it. Not reachable in the fetched corpus - every
/// encrypted file there opens - and kept so that a corpus which does contain
/// one produces a row rather than a silent skip.
fn refused_row(path: &str, _bytes: Vec<u8>) -> Option<Row> {
    Some(Row {
        path: path.to_owned(),
        v: 0,
        r: 0,
        user_empty: false,
        owner_empty: false,
        permissions: "unread: the document needs a password".to_owned(),
        modify: false,
        repaired: false,
    })
}

fn encrypt_dict(document: &Document) -> Option<(onionskin_cos::Dict, Vec<u8>)> {
    let dict = match document.trailer().get(b"Encrypt")? {
        Object::Dict(dict) => dict.clone(),
        Object::Ref(objref) => document.get(objref.number).ok()?.object.as_dict()?.clone(),
        _ => return None,
    };
    let file_id = match document.trailer().get(b"ID") {
        Some(Object::Array(ids)) => match ids.first() {
            Some(Object::String(first)) => first.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    Some((dict, file_id))
}

/// The same translation `cos` makes internally, repeated here because it is
/// private and this test reads the dictionary as data.
fn read(dict: &onionskin_cos::Dict) -> onionskin_crypto::EncryptDict {
    let bytes = |key: &[u8]| match dict.get(key) {
        Some(Object::String(value)) => value.clone(),
        _ => Vec::new(),
    };
    let integer = |key: &[u8]| dict.get(key).and_then(Object::as_integer);
    onionskin_crypto::EncryptDict {
        filter: dict
            .get(b"Filter")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes().to_vec())
            .unwrap_or_default(),
        v: integer(b"V").unwrap_or(0).clamp(0, 255) as u8,
        r: integer(b"R").unwrap_or(0).clamp(0, 255) as u8,
        o: bytes(b"O"),
        u: bytes(b"U"),
        oe: bytes(b"OE"),
        ue: bytes(b"UE"),
        p: integer(b"P").unwrap_or(0) as i32,
        length: integer(b"Length").and_then(|value| u32::try_from(value).ok()),
        encrypt_metadata: !matches!(dict.get(b"EncryptMetadata"), Some(Object::Bool(false))),
        ..Default::default()
    }
}

fn names(permissions: onionskin_crypto::Permissions) -> String {
    let mut out = Vec::new();
    for (allowed, name) in [
        (permissions.print(), "print"),
        (permissions.modify(), "modify"),
        (permissions.extract(), "extract"),
        (permissions.annotate(), "annotate"),
    ] {
        if allowed {
            out.push(name);
        }
    }
    if out.is_empty() {
        "none".to_owned()
    } else {
        out.join(", ")
    }
}

fn yes(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
