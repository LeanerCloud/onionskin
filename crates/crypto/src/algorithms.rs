//! The key-derivation and password-validation algorithms of ISO 32000.
//!
//! Numbered as the standard numbers them, because a reviewer checking this
//! against the spec checks it algorithm by algorithm, and a function named for
//! what it does in this codebase would make that a translation exercise.
//!
//! - **Algorithm 1**, per-object key (`/R` 2-4): [`object_key`].
//! - **Algorithm 2**, file key from a password (`/R` 2-4): [`file_key_r2_to_r4`].
//! - **Algorithm 2.A**, file key (`/R` 5 and 6): [`file_key_r5_r6`].
//! - **Algorithm 2.B**, the hardened hash (`/R` 6): [`hash_r6`].
//! - **Algorithms 4 and 5**, validating a user password (`/R` 2, `/R` 3-4):
//!   [`user_password_matches`].
//!
//! **`/R` 5 and `/R` 6 are different algorithms**, which is the review risk the
//! plan names. `/R` 5 is Adobe's deprecated extension: one SHA-256. `/R` 6 is
//! ISO 32000-2's hardened form: a loop of at least 64 rounds mixing SHA-256,
//! SHA-384 and SHA-512 under AES-128. A handler that applies the `/R` 5 form to
//! an `/R` 6 file does not fail loudly; it derives the wrong key, and every
//! string and stream comes out as noise.

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

use crate::Error;

/// The 32-byte padding string of ISO 32000-1 7.6.3.3, appended to a short
/// password and used whole for an empty one.
pub(crate) const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// The inputs algorithm 2 hashes. Named, because the order is the algorithm
/// and a positional argument list of five byte slices is a list someone will
/// eventually pass in the wrong order.
#[derive(Clone, Copy)]
pub(crate) struct KeyInputs<'a> {
    pub(crate) password: &'a [u8],
    pub(crate) owner: &'a [u8],
    pub(crate) permissions: i32,
    pub(crate) file_id: &'a [u8],
    pub(crate) revision: u8,
    /// In bytes: 5 for `/R` 2, `/Length / 8` otherwise.
    pub(crate) length: usize,
    pub(crate) encrypt_metadata: bool,
}

/// Algorithm 2: the file encryption key, `/R` 2 to 4.
pub(crate) fn file_key_r2_to_r4(inputs: &KeyInputs<'_>) -> Vec<u8> {
    let mut hasher = Md5::new();
    hasher.update(padded(inputs.password));
    hasher.update(inputs.owner);
    hasher.update(inputs.permissions.to_le_bytes());
    hasher.update(inputs.file_id);
    // Step (f): only at `/R` 4 and above, and only when metadata is left in
    // the clear.
    if inputs.revision >= 4 && !inputs.encrypt_metadata {
        hasher.update([0xFF; 4]);
    }
    let mut digest = hasher.finalize().to_vec();

    // Step (g): fifty more rounds over the first `length` bytes, from `/R` 3.
    if inputs.revision >= 3 {
        for _ in 0..50 {
            digest = Md5::digest(&digest[..inputs.length]).to_vec();
        }
    }
    digest.truncate(inputs.length);
    digest
}

/// Algorithms 4 and 5: whether `password` is the user password.
///
/// Algorithm 4 (`/R` 2) encrypts the padding string with the key and compares
/// the whole of `/U`. Algorithm 5 (`/R` 3 and 4) hashes the padding with the
/// file identifier, encrypts it twenty times with a key varied by XOR, and
/// compares only the first 16 bytes, because the rest of `/U` is arbitrary.
pub(crate) fn user_password_matches(
    file_key: &[u8],
    user: &[u8],
    file_id: &[u8],
    revision: u8,
) -> bool {
    if revision == 2 {
        let mut expected = PAD;
        rc4_in_place(file_key, &mut expected);
        return user.len() >= 32 && user[..32] == expected;
    }

    let mut hasher = Md5::new();
    hasher.update(PAD);
    hasher.update(file_id);
    let mut value = hasher.finalize().to_vec();
    rc4_in_place(file_key, &mut value);
    for round in 1u8..=19 {
        let key: Vec<u8> = file_key.iter().map(|byte| byte ^ round).collect();
        rc4_in_place(&key, &mut value);
    }
    user.len() >= 16 && user[..16] == value[..16]
}

/// Algorithm 1: the key for one object, `/R` 2 to 4.
///
/// Salted with `sAlT` for AES, which is the one difference between the RC4
/// and AES-128 forms and the one a handler that shares the code path forgets.
pub(crate) fn object_key(file_key: &[u8], number: u32, generation: u16, aes: bool) -> Vec<u8> {
    let mut hasher = Md5::new();
    hasher.update(file_key);
    hasher.update(&number.to_le_bytes()[..3]);
    hasher.update(&generation.to_le_bytes()[..2]);
    if aes {
        hasher.update(b"sAlT");
    }
    let digest = hasher.finalize();
    let length = (file_key.len() + 5).min(16);
    digest[..length].to_vec()
}

/// Algorithm 2.A: validate a user password and derive the file key, `/R` 5
/// and 6.
///
/// `/U` is 48 bytes: a 32-byte hash, an 8-byte validation salt and an 8-byte
/// key salt. The password is validated against the first two; only then is the
/// key salt used to decrypt `/UE`, which holds the file key under AES-256-CBC
/// with a zero IV and no padding.
pub(crate) fn file_key_r5_r6(
    password: &[u8],
    user: &[u8],
    user_encrypted: &[u8],
    revision: u8,
) -> Result<Vec<u8>, Error> {
    if user.len() < 48 || user_encrypted.len() < 32 {
        return Err(Error::Malformed("/U or /UE is too short for /R 5 or 6"));
    }
    // ISO 32000-2 7.6.4.3.3: the password is SASLprepped UTF-8 truncated to
    // 127 bytes. An empty password needs neither, which is the only case M3
    // opens.
    let password = &password[..password.len().min(127)];
    let validation_salt = &user[32..40];
    let key_salt = &user[40..48];

    let check = hash_for(revision, password, validation_salt, &[]);
    if check[..] != user[..32] {
        return Err(Error::WrongPassword);
    }

    let intermediate = hash_for(revision, password, key_salt, &[]);
    let mut key = user_encrypted[..32].to_vec();
    cbc::Decryptor::<aes::Aes256>::new(intermediate[..32].into(), &[0u8; 16].into())
        .decrypt_padded_mut::<NoPadding>(&mut key)
        .map_err(|_| Error::Malformed("/UE does not decrypt to a whole number of blocks"))?;
    Ok(key)
}

/// Algorithms 7 and 12: whether `password` is the owner password.
///
/// Only the measurement asks: M3 opens with the user password and never
/// escalates. Algorithm 7 (`/R` 2-4) recovers the padded user password from
/// `/O` with a key derived from the owner password, then checks it as a user
/// password. Algorithm 12 (`/R` 5-6) hashes the owner password with `/O`'s
/// validation salt **and the whole of `/U`**, which is the one place the user
/// key enters an owner check and the step a short implementation drops.
///
/// `inputs.password` is the candidate owner password; `user` is `/U`.
pub(crate) fn owner_password_matches(inputs: &KeyInputs<'_>, user: &[u8]) -> bool {
    let KeyInputs {
        password,
        owner,
        permissions,
        file_id,
        revision,
        length,
        encrypt_metadata,
    } = *inputs;
    match revision {
        2..=4 => {
            let mut key = Md5::digest(padded(password)).to_vec();
            if revision >= 3 {
                for _ in 0..50 {
                    key = Md5::digest(&key[..length]).to_vec();
                }
            }
            key.truncate(if revision == 2 { 5 } else { length });
            if owner.len() < 32 {
                return false;
            }
            let mut recovered = owner[..32].to_vec();
            if revision == 2 {
                rc4_in_place(&key, &mut recovered);
            } else {
                for round in (0u8..=19).rev() {
                    let step: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
                    rc4_in_place(&step, &mut recovered);
                }
            }
            let file_key = file_key_r2_to_r4(&KeyInputs {
                password: &recovered,
                owner,
                permissions,
                file_id,
                revision,
                length: if revision == 2 { 5 } else { length },
                encrypt_metadata,
            });
            user_password_matches(&file_key, user, file_id, revision)
        }
        5 | 6 => {
            if owner.len() < 40 || user.len() < 48 {
                return false;
            }
            let password = &password[..password.len().min(127)];
            let check = hash_for(revision, password, &owner[32..40], &user[..48]);
            check[..] == owner[..32]
        }
        _ => false,
    }
}

/// The hash `/R` 5 or `/R` 6 uses, chosen by revision rather than by guessing.
fn hash_for(revision: u8, password: &[u8], salt: &[u8], user: &[u8]) -> Vec<u8> {
    match revision {
        5 => {
            let mut hasher = Sha256::new();
            hasher.update(password);
            hasher.update(salt);
            hasher.update(user);
            hasher.finalize().to_vec()
        }
        _ => hash_r6(password, salt, user),
    }
}

/// Algorithm 2.B: the hardened hash of ISO 32000-2, `/R` 6.
///
/// Starts from SHA-256 of password, salt and (for an owner password) the
/// user key, then loops: build `K1` as 64 copies of password-hash-user,
/// encrypt it with AES-128-CBC keyed and IVed from the current hash, and pick
/// the next hash function from the sum of the first 16 encrypted bytes modulo
/// 3. At least 64 rounds, and on until the last byte of the ciphertext is no
/// greater than the round number minus 32.
pub(crate) fn hash_r6(password: &[u8], salt: &[u8], user: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(password);
    hasher.update(salt);
    hasher.update(user);
    let mut hash = hasher.finalize().to_vec();

    let mut round: u32 = 0;
    loop {
        let mut block = Vec::with_capacity(64 * (password.len() + hash.len() + user.len()));
        for _ in 0..64 {
            block.extend_from_slice(password);
            block.extend_from_slice(&hash);
            block.extend_from_slice(user);
        }
        let length = block.len();
        let encrypted = cbc::Encryptor::<aes::Aes128>::new(hash[..16].into(), hash[16..32].into())
            .encrypt_padded_mut::<NoPadding>(&mut block, length)
            .expect("64 copies of a 16-byte-multiple block are a whole number of blocks")
            .to_vec();

        let selector: u32 = encrypted[..16]
            .iter()
            .map(|byte| u32::from(*byte))
            .sum::<u32>()
            % 3;
        hash = match selector {
            0 => Sha256::digest(&encrypted).to_vec(),
            1 => Sha384::digest(&encrypted).to_vec(),
            _ => Sha512::digest(&encrypted).to_vec(),
        };

        round += 1;
        let last = u32::from(*encrypted.last().expect("the ciphertext is not empty"));
        if round >= 64 && last <= round - 32 {
            break;
        }
    }
    hash.truncate(32);
    hash
}

/// A password padded or truncated to 32 bytes with [`PAD`].
fn padded(password: &[u8]) -> [u8; 32] {
    let mut out = PAD;
    let length = password.len().min(32);
    out[..length].copy_from_slice(&password[..length]);
    out[length..].copy_from_slice(&PAD[..32 - length]);
    out
}

/// RC4, which is its own inverse, so this both encrypts and decrypts.
pub(crate) fn rc4_in_place(key: &[u8], data: &mut [u8]) {
    use rc4::{consts::*, KeyInit, Rc4, StreamCipher};
    // The `rc4` crate is generic over key length; PDF keys are 5 to 16 bytes.
    macro_rules! run {
        ($size:ty) => {{
            let mut cipher = Rc4::<$size>::new_from_slice(key).expect("the key length matches");
            cipher.apply_keystream(data);
        }};
    }
    match key.len() {
        5 => run!(U5),
        6 => run!(U6),
        7 => run!(U7),
        8 => run!(U8),
        9 => run!(U9),
        10 => run!(U10),
        11 => run!(U11),
        12 => run!(U12),
        13 => run!(U13),
        14 => run!(U14),
        15 => run!(U15),
        16 => run!(U16),
        other => panic!("an RC4 key of {other} bytes is outside what ISO 32000 derives"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_password_pads_to_the_whole_padding_string() {
        assert_eq!(padded(b""), PAD);
    }

    #[test]
    fn a_short_password_is_padded_after_its_own_bytes() {
        let padded = padded(b"abc");
        assert_eq!(&padded[..3], b"abc");
        assert_eq!(&padded[3..], &PAD[..29]);
    }

    #[test]
    fn rc4_is_its_own_inverse() {
        let key = b"Key12";
        let mut data = b"Plaintext".to_vec();
        rc4_in_place(key, &mut data);
        assert_ne!(data, b"Plaintext");
        rc4_in_place(key, &mut data);
        assert_eq!(data, b"Plaintext");
    }

    /// The classic published RC4 vector: key "Secret", plaintext
    /// "Attack at dawn". A real known answer rather than a round trip, which
    /// any self-inverse function passes.
    #[test]
    fn rc4_matches_the_published_vector() {
        let mut data = b"Attack at dawn".to_vec();
        rc4_in_place(b"Secret", &mut data);
        assert_eq!(
            data,
            [0x45, 0xA0, 0x1F, 0x64, 0x5F, 0xC3, 0x5B, 0x38, 0x35, 0x52, 0x54, 0x4B, 0x9B, 0xF5]
        );
    }

    #[test]
    fn the_object_key_is_salted_for_aes_and_not_for_rc4() {
        let key = [7u8; 16];
        assert_ne!(
            object_key(&key, 12, 0, true),
            object_key(&key, 12, 0, false)
        );
        assert_eq!(object_key(&key, 12, 0, false).len(), 16);
        assert_eq!(object_key(&[7u8; 5], 12, 0, false).len(), 10);
    }
}
