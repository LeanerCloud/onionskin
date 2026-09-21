//! Crypt filters: which cipher a string or a stream is decrypted with.
//!
//! `/V` 1 and 2 have one cipher for everything, RC4. `/V` 4 and 5 name crypt
//! filters in `/CF` and pick one for streams (`/StmF`) and one for strings
//! (`/StrF`) separately - so a document can encrypt its streams with AES and
//! leave its strings alone, and a handler that uses one method for both reads
//! half of such a file as noise.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

use crate::algorithms::rc4_in_place;
use crate::Error;

/// How a crypt filter decrypts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `/Identity`, or `/CFM /None`: the data is not encrypted.
    Identity,
    /// `/CFM /V2`, and the only method at `/V` 1 and 2.
    Rc4,
    /// `/CFM /AESV2`: AES-128-CBC with a per-object key.
    Aes128,
    /// `/CFM /AESV3`: AES-256-CBC with the file key itself.
    Aes256,
}

impl Method {
    pub(crate) fn from_cfm(cfm: &[u8]) -> Result<Method, Error> {
        match cfm {
            b"None" | b"Identity" => Ok(Method::Identity),
            b"V2" => Ok(Method::Rc4),
            b"AESV2" => Ok(Method::Aes128),
            b"AESV3" => Ok(Method::Aes256),
            _ => Err(Error::Unsupported(
                "a crypt filter method this handler does not know",
            )),
        }
    }

    pub(crate) fn is_aes(self) -> bool {
        matches!(self, Method::Aes128 | Method::Aes256)
    }
}

/// Decrypt `data` with `key` under `method`.
///
/// AES data begins with its 16-byte IV and ends in PKCS#7 padding. A string
/// shorter than one IV is not AES ciphertext at all: some producers write an
/// empty string unencrypted, and reading it as ciphertext is an error where
/// passing it through is what every reader does.
pub(crate) fn decrypt(method: Method, key: &[u8], data: &[u8]) -> Result<Vec<u8>, Error> {
    match method {
        Method::Identity => Ok(data.to_vec()),
        Method::Rc4 => {
            let mut out = data.to_vec();
            rc4_in_place(key, &mut out);
            Ok(out)
        }
        Method::Aes128 | Method::Aes256 => {
            if data.len() < 16 {
                return Ok(data.to_vec());
            }
            let (iv, body) = data.split_at(16);
            if body.is_empty() {
                return Ok(Vec::new());
            }
            if body.len() % 16 != 0 {
                return Err(Error::Malformed(
                    "AES ciphertext is not a whole number of blocks",
                ));
            }
            let mut buffer = body.to_vec();
            let plain = match method {
                Method::Aes128 => cbc::Decryptor::<aes::Aes128>::new_from_slices(key, iv)
                    .map_err(|_| Error::Malformed("an AES-128 key is not 16 bytes"))?
                    .decrypt_padded_mut::<Pkcs7>(&mut buffer),
                _ => cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
                    .map_err(|_| Error::Malformed("an AES-256 key is not 32 bytes"))?
                    .decrypt_padded_mut::<Pkcs7>(&mut buffer),
            }
            .map_err(|_| Error::Malformed("AES padding is invalid, so the key is wrong"))?;
            Ok(plain.to_vec())
        }
    }
}
