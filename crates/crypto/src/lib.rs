//! Signature verification, PAdES signing later, and the encryption and
//! decryption handlers `cos` uses. Appending to an encrypted document
//! means encrypting new objects with the existing key material, so this
//! is a kernel concern from the start rather than a late addition.
//!
//! # The standard security handler
//!
//! **Read side:** `/V` 1, 2, 4 and 5; `/R` 2 through 6; RC4, AES-128 and
//! AES-256; crypt filters; `/EncryptMetadata`; opening with the user or the
//! owner password.
//!
//! **Write side:** Acrobat's two levels, 128-bit and 256-bit AES, from a
//! user password, an owner password and the permissions ([`protect`]), and
//! a handler that encrypts strings and streams with the key a document
//! already has, so a section appended to it is encrypted as the rest is.

mod algorithms;
mod encrypt;
mod filters;
pub mod signature;
mod standard;

pub use encrypt::{protect, system_random, Protection, Random, Strength};
pub use filters::Method;
pub use standard::{
    user_password_from_owner, validates_owner_password, Access, EncryptDict, Permissions,
    SecurityHandler,
};

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
