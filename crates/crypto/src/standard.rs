//! The standard security handler: from an `/Encrypt` dictionary and a
//! password to a handler that decrypts strings and streams.

use std::collections::BTreeMap;

use crate::algorithms::{
    file_key_r2_to_r4, file_key_r5_r6, object_key, owner_file_key, owner_password_matches,
    padded_user_password, unpadded, user_password_matches, KeyInputs,
};
use crate::encrypt::{encrypt, key_for};
use crate::filters::{decrypt, Method};
use crate::Error;

/// The `/Encrypt` entries the handler reads, as plain values.
///
/// `crypto` does not depend on `cos`, so `cos` reads its own dictionary into
/// this and hands it over. That keeps the dependency pointing the way the
/// crate docs say it points - `cos` uses `crypto` - and keeps this crate
/// testable without a PDF parser.
#[derive(Clone, Debug, Default)]
pub struct EncryptDict {
    /// `/Filter`. Only `Standard` is handled.
    pub filter: Vec<u8>,
    pub v: u8,
    pub r: u8,
    pub o: Vec<u8>,
    pub u: Vec<u8>,
    /// `/OE` and `/UE`, `/R` 5 and 6 only.
    pub oe: Vec<u8>,
    pub ue: Vec<u8>,
    /// `/Perms`, `/R` 6: the permissions encrypted under the file key.
    pub perms: Vec<u8>,
    pub p: i32,
    /// `/Length` in bits. Absent means 40.
    pub length: Option<u32>,
    /// `/EncryptMetadata`. Absent means true.
    pub encrypt_metadata: bool,
    /// `/CF`: crypt filter name to its `/CFM`.
    pub crypt_filters: BTreeMap<Vec<u8>, Vec<u8>>,
    /// `/StmF` and `/StrF`. Absent means `Identity`.
    pub stream_filter: Option<Vec<u8>>,
    pub string_filter: Option<Vec<u8>>,
}

/// The permission bits of `/P`, the ones a user interface names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions(pub i32);

impl Permissions {
    /// Bit 3.
    pub const PRINT: i32 = 1 << 2;
    /// Bit 4.
    pub const MODIFY: i32 = 1 << 3;
    /// Bit 5.
    pub const EXTRACT: i32 = 1 << 4;
    /// Bit 6.
    pub const ANNOTATE: i32 = 1 << 5;
    /// Bit 9.
    pub const FILL_FORMS: i32 = 1 << 8;
    /// Bit 10.
    pub const ACCESSIBILITY: i32 = 1 << 9;
    /// Bit 11.
    pub const ASSEMBLE: i32 = 1 << 10;
    /// Bit 12.
    pub const PRINT_HIGH: i32 = 1 << 11;
    /// Bits 7, 8 and 13 to 32, which ISO 32000 has a writer set.
    pub const RESERVED: i32 = !0xF3F;
    /// Everything allowed.
    pub const ALL: Permissions = Permissions(!0b11);

    /// Bits 9: fill in form fields and sign, even without bit 6.
    pub fn fill_forms(self) -> bool {
        self.bit(9)
    }
    /// Bit 10: extract text and graphics for accessibility.
    pub fn accessibility(self) -> bool {
        self.bit(10)
    }
    /// Bit 11: insert, rotate and delete pages, and make bookmarks and
    /// thumbnails, even without bit 4.
    pub fn assemble(self) -> bool {
        self.bit(11)
    }
    /// Bit 12: print at full quality. Without it, with bit 3, printing is
    /// low resolution.
    pub fn print_high(self) -> bool {
        self.bit(12)
    }

    /// Bit 3: print.
    pub fn print(self) -> bool {
        self.bit(3)
    }
    /// Bit 4: modify the contents - the one ruling A's residual turns on.
    pub fn modify(self) -> bool {
        self.bit(4)
    }
    /// Bit 5: copy or extract text and graphics.
    pub fn extract(self) -> bool {
        self.bit(5)
    }
    /// Bit 6: add or modify annotations and fill form fields.
    pub fn annotate(self) -> bool {
        self.bit(6)
    }

    fn bit(self, position: u32) -> bool {
        self.0 & (1 << (position - 1)) != 0
    }
}

/// Which password opened a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The user password, or none: the permissions apply.
    User,
    /// The owner password: every permission is granted.
    Owner,
}

/// A validated handler, ready to decrypt.
#[derive(Clone, Debug)]
pub struct SecurityHandler {
    file_key: Vec<u8>,
    revision: u8,
    streams: Method,
    strings: Method,
    encrypt_metadata: bool,
    permissions: Permissions,
}

impl SecurityHandler {
    /// Validate `password` as the user password and derive the file key.
    ///
    /// A document that opens with the empty password is one Acrobat opens
    /// without asking. A handler that opens a password-protected file with
    /// the empty password is the permissive bug the tests rule out.
    pub fn open(dict: &EncryptDict, file_id: &[u8], password: &[u8]) -> Result<Self, Error> {
        if dict.filter != b"Standard" {
            return Err(Error::Unsupported(
                "a security handler other than /Standard",
            ));
        }
        methods(dict)?;

        let file_key = match dict.r {
            2..=4 => {
                let length = match dict.r {
                    2 => 5,
                    _ => (dict.length.unwrap_or(40) / 8).clamp(5, 16) as usize,
                };
                let key = file_key_r2_to_r4(&KeyInputs {
                    password,
                    owner: &dict.o,
                    permissions: dict.p,
                    file_id,
                    revision: dict.r,
                    length,
                    encrypt_metadata: dict.encrypt_metadata,
                });
                if !user_password_matches(&key, &dict.u, file_id, dict.r) {
                    return Err(Error::WrongPassword);
                }
                key
            }
            5 | 6 => file_key_r5_r6(password, &dict.u, &dict.ue, dict.r)?,
            _ => return Err(Error::Unsupported("a /R outside 2 to 6")),
        };

        Self::from_key(dict, file_key)
    }

    /// Open with `password`, as the owner when it is the owner password and
    /// as a user otherwise. An owner is granted every permission.
    pub fn open_with(
        dict: &EncryptDict,
        file_id: &[u8],
        password: &[u8],
    ) -> Result<(Self, Access), Error> {
        let length = match dict.r {
            2 => 5,
            _ => (dict.length.unwrap_or(40) / 8).clamp(5, 16) as usize,
        };
        let inputs = KeyInputs {
            password,
            owner: &dict.o,
            permissions: dict.p,
            file_id,
            revision: dict.r,
            length,
            encrypt_metadata: dict.encrypt_metadata,
        };
        if dict.filter == b"Standard" {
            if let Some(key) = owner_file_key(&inputs, &dict.u, &dict.oe) {
                let mut handler = Self::from_key(dict, key)?;
                handler.permissions = Permissions::ALL;
                return Ok((handler, Access::Owner));
            }
        }
        Self::open(dict, file_id, password).map(|handler| (handler, Access::User))
    }

    /// A handler for a file key already derived: the one a document is
    /// being encrypted with.
    pub(crate) fn from_key(dict: &EncryptDict, file_key: Vec<u8>) -> Result<Self, Error> {
        let (streams, strings) = methods(dict)?;
        Ok(SecurityHandler {
            file_key,
            revision: dict.r,
            streams,
            strings,
            encrypt_metadata: dict.encrypt_metadata,
            permissions: Permissions(dict.p),
        })
    }

    pub fn permissions(&self) -> Permissions {
        self.permissions
    }

    /// Encrypt a string belonging to object `number`, with `iv` for AES.
    /// Like decryption, not for strings inside an object stream.
    pub fn encrypt_string(
        &self,
        number: u32,
        generation: u16,
        data: &[u8],
        iv: [u8; 16],
    ) -> Vec<u8> {
        let key = key_for(self.strings, &self.file_key, number, generation);
        encrypt(self.strings, &key, data, iv)
    }

    /// Encrypt a stream's data, after its filters are applied. The XMP
    /// metadata stream stays in the clear when `/EncryptMetadata` is false.
    pub fn encrypt_stream(
        &self,
        number: u32,
        generation: u16,
        data: &[u8],
        iv: [u8; 16],
        is_metadata: bool,
    ) -> Vec<u8> {
        if is_metadata && !self.encrypt_metadata {
            return data.to_vec();
        }
        let key = key_for(self.streams, &self.file_key, number, generation);
        encrypt(self.streams, &key, data, iv)
    }

    pub fn revision(&self) -> u8 {
        self.revision
    }

    /// How streams are encrypted, which is what a document's encryption
    /// level is named by.
    pub fn stream_method(&self) -> Method {
        self.streams
    }

    /// Whether the XMP metadata is encrypted.
    pub fn encrypts_metadata(&self) -> bool {
        self.encrypt_metadata
    }

    /// Decrypt a string belonging to object `number`.
    ///
    /// **Not for strings inside an object stream.** Those are not separately
    /// encrypted - the container already was - and decrypting them again
    /// corrupts every object-stream document. The caller knows which object a
    /// string came from; this function cannot, so the rule is the caller's.
    pub fn decrypt_string(
        &self,
        number: u32,
        generation: u16,
        data: &[u8],
    ) -> Result<Vec<u8>, Error> {
        self.run(self.strings, number, generation, data)
    }

    /// Decrypt a stream's data, before its filters are undone.
    ///
    /// `is_metadata` is for the catalog's `/Metadata` XMP stream: with
    /// `/EncryptMetadata false` it is written in the clear, and "decrypting"
    /// plaintext turns a readable document description into noise.
    pub fn decrypt_stream(
        &self,
        number: u32,
        generation: u16,
        data: &[u8],
        is_metadata: bool,
    ) -> Result<Vec<u8>, Error> {
        if is_metadata && !self.encrypt_metadata {
            return Ok(data.to_vec());
        }
        self.run(self.streams, number, generation, data)
    }

    fn run(
        &self,
        method: Method,
        number: u32,
        generation: u16,
        data: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let key = match method {
            Method::Identity => return Ok(data.to_vec()),
            // AES-256 uses the file key itself; the per-object derivation of
            // algorithm 1 is for `/R` 2 to 4 only.
            Method::Aes256 => self.file_key.clone(),
            Method::Rc4 | Method::Aes128 => {
                object_key(&self.file_key, number, generation, method.is_aes())
            }
        };
        decrypt(method, &key, data)
    }
}

/// For `/R` 2 to 4: the user password that `owner_password` unlocks, for a
/// reader that opens documents with the user password only. `None` when it
/// is not the owner password, and for `/R` 5 and 6, whose owner password
/// every reader takes as it is.
pub fn user_password_from_owner(
    dict: &EncryptDict,
    file_id: &[u8],
    owner_password: &[u8],
) -> Option<Vec<u8>> {
    if !(2..=4).contains(&dict.r) {
        return None;
    }
    let length = match dict.r {
        2 => 5,
        _ => (dict.length.unwrap_or(40) / 8).clamp(5, 16) as usize,
    };
    let inputs = KeyInputs {
        password: owner_password,
        owner: &dict.o,
        permissions: dict.p,
        file_id,
        revision: dict.r,
        length,
        encrypt_metadata: dict.encrypt_metadata,
    };
    owner_file_key(&inputs, &dict.u, &[])?;
    padded_user_password(&inputs).map(|padded| unpadded(&padded).to_vec())
}

/// Whether `password` is the owner password, for the measurement table.
///
/// Not part of opening a document: M3 opens with the user password and never
/// escalates to owner rights, which would be a way around the permission bits.
pub fn validates_owner_password(dict: &EncryptDict, file_id: &[u8], password: &[u8]) -> bool {
    let length = match dict.r {
        2 => 5,
        _ => (dict.length.unwrap_or(40) / 8).clamp(5, 16) as usize,
    };
    owner_password_matches(
        &KeyInputs {
            password,
            owner: &dict.o,
            permissions: dict.p,
            file_id,
            revision: dict.r,
            length,
            encrypt_metadata: dict.encrypt_metadata,
        },
        &dict.u,
    )
}

/// The methods for streams and strings, from `/V` and the crypt filters.
fn methods(dict: &EncryptDict) -> Result<(Method, Method), Error> {
    match dict.v {
        1 | 2 => Ok((Method::Rc4, Method::Rc4)),
        4 | 5 => {
            let pick = |name: &Option<Vec<u8>>| -> Result<Method, Error> {
                match name.as_deref() {
                    None | Some(b"Identity") => Ok(Method::Identity),
                    Some(filter) => {
                        let cfm = dict
                            .crypt_filters
                            .get(filter)
                            .ok_or(Error::Malformed("/StmF or /StrF names a filter /CF lacks"))?;
                        Method::from_cfm(cfm)
                    }
                }
            };
            Ok((pick(&dict.stream_filter)?, pick(&dict.string_filter)?))
        }
        _ => Err(Error::Unsupported("a /V outside 1, 2, 4 and 5")),
    }
}
