//! Signature verification, PAdES signing later, and the encryption and
//! decryption handlers `cos` uses. Appending to an encrypted document
//! means encrypting new objects with the existing key material, so this
//! is a kernel concern from the start rather than a late addition.
//!
//! # What exists at M3: the standard security handler, read side
//!
//! `/V` 1, 2, 4 and 5; `/R` 2 through 6; RC4, AES-128 and AES-256; crypt
//! filters; `/EncryptMetadata`. Enough to open every document Acrobat opens
//! without asking for a password, and to refuse - with a typed error - every
//! document that needs one.
//!
//! **Write side is M6.** Nothing here encrypts, which is why `cos` refuses to
//! emit a section into an encrypted document at all rather than emitting one
//! in plaintext under a trailer that still names `/Encrypt`.

mod algorithms;
mod filters;
mod standard;

pub use filters::Method;
pub use standard::{EncryptDict, Permissions, SecurityHandler};

use std::fmt;

/// Why a security handler could not be opened, or could not decrypt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The password is not the user password. With the empty password M3
    /// tries, this is "the document needs a password", which stays a refusal
    /// naming M6.
    WrongPassword,
    /// A handler, revision or crypt filter method outside what M3 reads.
    Unsupported(&'static str),
    /// The `/Encrypt` dictionary, or the ciphertext, is not well formed.
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WrongPassword => write!(f, "the document needs a password"),
            Error::Unsupported(what) => write!(f, "unsupported encryption: {what}"),
            Error::Malformed(what) => write!(f, "malformed encryption: {what}"),
        }
    }
}

impl std::error::Error for Error {}
