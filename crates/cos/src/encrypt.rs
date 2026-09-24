//! Encryption at write time: what [`crate::decrypt`] undoes, done.
//!
//! Every string and every stream of an object written into an encrypted
//! document is encrypted with the document's key, except what decryption
//! leaves alone: the `/Encrypt` dictionary, a signature's `/Contents`, a
//! cross-reference stream, and the XMP metadata stream when
//! `/EncryptMetadata` is false. The same rules, so what is written reads
//! back as it was.

use onionskin_crypto::{EncryptDict, SecurityHandler};

use crate::object::{Dict, Name, Object};

/// Fills a buffer with random bytes: where each AES IV comes from.
pub(crate) type Random<'a> = &'a mut dyn FnMut(&mut [u8]);

/// Encrypt one object in place, as object `number` of generation
/// `generation`.
pub(crate) fn object(
    handler: &SecurityHandler,
    number: u32,
    generation: u16,
    object: &mut Object,
    random: Random<'_>,
) {
    match object {
        Object::String(bytes) => {
            *bytes = handler.encrypt_string(number, generation, bytes, iv(random));
        }
        Object::Array(items) => {
            for item in items {
                self::object(handler, number, generation, item, random);
            }
        }
        Object::Dict(dict) => self::dict(handler, number, generation, dict, random),
        Object::Stream(stream) => {
            self::dict(handler, number, generation, &mut stream.dict, random);
            if !is_type(&stream.dict, b"XRef") {
                let metadata = is_type(&stream.dict, b"Metadata");
                stream.raw =
                    handler.encrypt_stream(number, generation, &stream.raw, iv(random), metadata);
            }
        }
        _ => {}
    }
}

fn dict(
    handler: &SecurityHandler,
    number: u32,
    generation: u16,
    dict: &mut Dict,
    random: Random<'_>,
) {
    let signature = dict.contains(b"ByteRange");
    for (key, value) in dict.values_mut() {
        if signature && key.as_bytes() == b"Contents" {
            continue;
        }
        object(handler, number, generation, value, random);
    }
}

fn is_type(dict: &Dict, name: &[u8]) -> bool {
    dict.get(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|value| value.as_bytes() == name)
}

fn iv(random: Random<'_>) -> [u8; 16] {
    let mut iv = [0u8; 16];
    random(&mut iv);
    iv
}

/// The `/Encrypt` dictionary `crypto` described, as a file writes it.
pub(crate) fn write_dict(encrypt: &EncryptDict) -> Dict {
    let mut dict = Dict::new();
    let string = |bytes: &[u8]| Object::String(bytes.to_vec());
    let name = |bytes: &[u8]| Object::Name(Name(bytes.to_vec()));
    dict.set("Filter", name(&encrypt.filter));
    dict.set("V", Object::Integer(i64::from(encrypt.v)));
    dict.set("R", Object::Integer(i64::from(encrypt.r)));
    if let Some(length) = encrypt.length {
        dict.set("Length", Object::Integer(i64::from(length)));
    }
    dict.set("O", string(&encrypt.o));
    dict.set("U", string(&encrypt.u));
    for (key, value) in [
        ("OE", &encrypt.oe),
        ("UE", &encrypt.ue),
        ("Perms", &encrypt.perms),
    ] {
        if !value.is_empty() {
            dict.set(key, string(value));
        }
    }
    dict.set("P", Object::Integer(i64::from(encrypt.p)));
    if !encrypt.encrypt_metadata {
        dict.set("EncryptMetadata", Object::Bool(false));
    }
    if !encrypt.crypt_filters.is_empty() {
        let mut filters = Dict::new();
        for (filter, method) in &encrypt.crypt_filters {
            let mut entry = Dict::new();
            entry.set("Type", Object::name("CryptFilter"));
            entry.set("CFM", name(method));
            entry.set("AuthEvent", Object::name("DocOpen"));
            let bytes = if method.as_slice() == b"AESV3" {
                32
            } else {
                16
            };
            entry.set("Length", Object::Integer(bytes));
            filters.set(Name(filter.clone()), Object::Dict(entry));
        }
        dict.set("CF", Object::Dict(filters));
    }
    if let Some(filter) = &encrypt.stream_filter {
        dict.set("StmF", name(filter));
    }
    if let Some(filter) = &encrypt.string_filter {
        dict.set("StrF", name(filter));
    }
    dict
}
