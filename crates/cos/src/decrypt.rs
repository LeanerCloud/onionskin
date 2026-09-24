//! Decryption at parse time.
//!
//! **What is decrypted, and what is not.** Every string and every stream in an
//! object read from the file, except:
//!
//! - **objects inside an object stream**, which arrive already decrypted
//!   because their container was. Decrypting them again is the bug that
//!   corrupts every object-stream document, and it cannot happen here because
//!   this runs only on objects parsed straight from the file;
//! - **the `/Encrypt` dictionary itself**, whose `/O`, `/U`, `/OE` and `/UE`
//!   are the handler's inputs and were never encrypted;
//! - **cross-reference streams**, which ISO 32000-1 7.5.8.4 says are not
//!   encrypted - they are read before the handler exists anyway;
//! - **a signature's `/Contents`**, which 7.6.2 leaves in the clear so a
//!   signature can be checked without the key;
//! - **the document's XMP metadata stream** when `/EncryptMetadata` is false,
//!   which `SecurityHandler::decrypt_stream` handles.
//!
//! A string that will not decrypt is kept as it was. Some producers write
//! strings unencrypted into encrypted documents and every reader passes them
//! through; failing the whole object over one of them would refuse a page for
//! a producer's bug. A **stream** that will not decrypt is an error, because
//! passing ciphertext on as content draws noise and says nothing.

use onionskin_crypto::{EncryptDict, SecurityHandler};

use crate::error::{Error, Result};
use crate::object::{Dict, Object};

/// Decrypt one object in place.
pub(crate) fn object(
    handler: &SecurityHandler,
    number: u32,
    generation: u16,
    object: &mut Object,
) -> Result<()> {
    match object {
        Object::String(bytes) => {
            if let Ok(plain) = handler.decrypt_string(number, generation, bytes) {
                *bytes = plain;
            }
        }
        Object::Array(items) => {
            for item in items {
                self::object(handler, number, generation, item)?;
            }
        }
        Object::Dict(dict) => self::dict(handler, number, generation, dict)?,
        Object::Stream(stream) => {
            self::dict(handler, number, generation, &mut stream.dict)?;
            if !is_type(&stream.dict, b"XRef") {
                let metadata = is_type(&stream.dict, b"Metadata");
                stream.raw = handler
                    .decrypt_stream(number, generation, &stream.raw, metadata)
                    .map_err(|error| Error::Syntax {
                        offset: 0,
                        detail: format!("object {number} does not decrypt: {error}"),
                    })?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn dict(handler: &SecurityHandler, number: u32, generation: u16, dict: &mut Dict) -> Result<()> {
    // A signature dictionary is recognised by `/ByteRange`, which only a
    // signature has, rather than by `/Type /Sig`, which is optional.
    let signature = dict.contains(b"ByteRange");
    for (key, value) in dict.values_mut() {
        if signature && key.as_bytes() == b"Contents" {
            continue;
        }
        object(handler, number, generation, value)?;
    }
    Ok(())
}

fn is_type(dict: &Dict, name: &[u8]) -> bool {
    dict.get(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|value| value.as_bytes() == name)
}

/// The `/Encrypt` dictionary's entries, read into the plain form `crypto`
/// takes. `crypto` does not depend on `cos`, so the translation lives here.
pub(crate) fn read_dict(dict: &Dict) -> EncryptDict {
    let bytes = |key: &[u8]| match dict.get(key) {
        Some(Object::String(value)) => value.clone(),
        _ => Vec::new(),
    };
    let name = |key: &[u8]| {
        dict.get(key)
            .and_then(Object::as_name)
            .map(|value| value.as_bytes().to_vec())
    };
    let integer = |key: &[u8]| dict.get(key).and_then(Object::as_integer);

    let mut crypt_filters = std::collections::BTreeMap::new();
    if let Some(Object::Dict(filters)) = dict.get(b"CF") {
        for (filter, value) in filters.iter() {
            if let Object::Dict(entry) = value {
                if let Some(cfm) = entry.get(b"CFM").and_then(Object::as_name) {
                    crypt_filters.insert(filter.as_bytes().to_vec(), cfm.as_bytes().to_vec());
                }
            }
        }
    }

    EncryptDict {
        filter: name(b"Filter").unwrap_or_default(),
        v: integer(b"V").unwrap_or(0).clamp(0, 255) as u8,
        r: integer(b"R").unwrap_or(0).clamp(0, 255) as u8,
        o: bytes(b"O"),
        u: bytes(b"U"),
        oe: bytes(b"OE"),
        ue: bytes(b"UE"),
        perms: bytes(b"Perms"),
        // `/P` is a signed 32-bit integer in the file, written as a negative
        // number whenever the high bits are set, which they always are.
        p: integer(b"P").unwrap_or(0) as i32,
        length: integer(b"Length").and_then(|value| u32::try_from(value).ok()),
        encrypt_metadata: !matches!(dict.get(b"EncryptMetadata"), Some(Object::Bool(false))),
        crypt_filters,
        stream_filter: name(b"StmF"),
        string_filter: name(b"StrF"),
    }
}
