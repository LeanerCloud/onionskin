//! The standard security handler, write side: from two passwords and the
//! permissions to an `/Encrypt` dictionary and a handler that encrypts.
//!
//! Acrobat's two levels:
//!
//! - **256-bit AES**, `/V` 5 `/R` 6, ISO 32000-2's handler. Algorithms 8, 9
//!   and 10 make `/U` and `/UE`, `/O` and `/OE`, and `/Perms`.
//! - **128-bit AES**, `/V` 4 `/R` 4 with the `/AESV2` crypt filter, for
//!   readers that predate PDF 2.0. Algorithms 3 and 5 make `/O` and `/U`.
//!
//! RC4 is not offered: Acrobat has not offered it for new documents since
//! version 9, and a file encrypted with it is not protected.
//!
//! **Randomness is passed in.** The file key, the salts and every AES IV come
//! from a function the caller supplies: [`system_random`] for a real save,
//! a fixed one in a test that needs repeatable bytes.

use aes::cipher::{
    block_padding::{NoPadding, Pkcs7},
    BlockEncrypt, BlockEncryptMut, KeyInit, KeyIvInit,
};
use md5::{Digest, Md5};

use crate::algorithms::{
    file_key_r2_to_r4, hash_r6, object_key, padded, rc4_in_place, KeyInputs, PAD,
};
use crate::filters::Method;
use crate::standard::{EncryptDict, Permissions, SecurityHandler};
use crate::Error;

/// Which of Acrobat's encryption levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    /// 128-bit AES, readable by Acrobat 7 and later.
    Aes128,
    /// 256-bit AES, readable by Acrobat X and later.
    Aes256,
}

/// What a document is protected with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protection {
    pub strength: Strength,
    /// The password that opens the document. Empty opens it without asking.
    pub user_password: Vec<u8>,
    /// The password that lifts the permissions. Empty means the user
    /// password does, which is what a document protected only against
    /// opening needs.
    pub owner_password: Vec<u8>,
    pub permissions: Permissions,
    /// Whether the XMP metadata is encrypted too.
    pub encrypt_metadata: bool,
}

/// Fills a buffer with random bytes.
pub type Random<'a> = &'a mut dyn FnMut(&mut [u8]);

/// The operating system's random numbers, which is what a real save uses.
///
/// # Panics
///
/// When the operating system has none to give, which leaves nothing safe to
/// encrypt with.
pub fn system_random(buffer: &mut [u8]) {
    getrandom::fill(buffer).expect("the operating system gives random bytes");
}

/// The `/Encrypt` dictionary `protection` is written as, for a document
/// whose first `/ID` string is `file_id`, and the handler that encrypts it.
pub fn protect(
    protection: &Protection,
    file_id: &[u8],
    random: Random<'_>,
) -> Result<(EncryptDict, SecurityHandler), Error> {
    let permissions = Permissions(protection.permissions.0 | Permissions::RESERVED);
    let owner = if protection.owner_password.is_empty() {
        &protection.user_password
    } else {
        &protection.owner_password
    };
    let (dict, file_key) = match protection.strength {
        Strength::Aes256 => r6(protection, owner, permissions, random),
        Strength::Aes128 => r4(protection, owner, permissions, file_id),
    };
    let handler = SecurityHandler::from_key(&dict, file_key)?;
    Ok((dict, handler))
}

fn crypt_filter(dict: &mut EncryptDict, method: &[u8]) {
    dict.filter = b"Standard".to_vec();
    dict.crypt_filters
        .insert(b"StdCF".to_vec(), method.to_vec());
    dict.stream_filter = Some(b"StdCF".to_vec());
    dict.string_filter = Some(b"StdCF".to_vec());
}

/// Algorithms 8, 9 and 10: `/R` 6.
fn r6(
    protection: &Protection,
    owner: &[u8],
    permissions: Permissions,
    random: Random<'_>,
) -> (EncryptDict, Vec<u8>) {
    let user = truncated(&protection.user_password);
    let owner = truncated(owner);
    let mut file_key = vec![0u8; 32];
    random(&mut file_key);

    // Algorithm 8: /U, a hash and two salts, then /UE, the file key under a
    // key made from the password and the key salt.
    let mut salts = [0u8; 16];
    random(&mut salts);
    let (validation, key_salt) = salts.split_at(8);
    let mut u = hash_r6(user, validation, &[]);
    u.extend_from_slice(validation);
    u.extend_from_slice(key_salt);
    let ue = wrap(&hash_r6(user, key_salt, &[]), &file_key);

    // Algorithm 9: the same for the owner, over the whole of /U.
    random(&mut salts);
    let (validation, key_salt) = salts.split_at(8);
    let mut o = hash_r6(owner, validation, &u);
    o.extend_from_slice(validation);
    o.extend_from_slice(key_salt);
    let oe = wrap(&hash_r6(owner, key_salt, &u), &file_key);

    // Algorithm 10: /Perms, the permissions under the file key, so a reader
    // can tell they were not altered.
    let mut perms = [0u8; 16];
    perms[..4].copy_from_slice(&permissions.0.to_le_bytes());
    perms[4..8].copy_from_slice(&[0xFF; 4]);
    perms[8] = if protection.encrypt_metadata {
        b'T'
    } else {
        b'F'
    };
    perms[9..12].copy_from_slice(b"adb");
    random(&mut perms[12..16]);
    let mut block = aes::Block::from(perms);
    aes::Aes256::new(file_key[..].into()).encrypt_block(&mut block);
    let perms = block.to_vec();

    let mut dict = EncryptDict {
        v: 5,
        r: 6,
        o,
        u,
        oe,
        ue,
        perms,
        p: permissions.0,
        length: Some(256),
        encrypt_metadata: protection.encrypt_metadata,
        ..EncryptDict::default()
    };
    crypt_filter(&mut dict, b"AESV3");
    (dict, file_key)
}

/// A password as `/R` 6 hashes it: at most 127 bytes.
fn truncated(password: &[u8]) -> &[u8] {
    &password[..password.len().min(127)]
}

/// `data` under AES-256-CBC with `key`, a zero IV and no padding: how `/UE`
/// and `/OE` hold the file key.
fn wrap(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut buffer = data.to_vec();
    let length = buffer.len();
    cbc::Encryptor::<aes::Aes256>::new(key[..32].into(), &[0u8; 16].into())
        .encrypt_padded_mut::<NoPadding>(&mut buffer, length)
        .expect("a 32-byte key is two whole blocks")
        .to_vec()
}

/// Algorithms 3, 2 and 5: `/R` 4 with AES-128.
fn r4(
    protection: &Protection,
    owner: &[u8],
    permissions: Permissions,
    file_id: &[u8],
) -> (EncryptDict, Vec<u8>) {
    const LENGTH: usize = 16;
    // Algorithm 3: /O, the padded user password under keys from the owner's.
    let mut key = Md5::digest(padded(owner)).to_vec();
    for _ in 0..50 {
        key = Md5::digest(&key[..LENGTH]).to_vec();
    }
    key.truncate(LENGTH);
    let mut o = padded(&protection.user_password).to_vec();
    for round in 0u8..=19 {
        let step: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
        rc4_in_place(&step, &mut o);
    }

    // Algorithm 2: the file key.
    let file_key = file_key_r2_to_r4(&KeyInputs {
        password: &protection.user_password,
        owner: &o,
        permissions: permissions.0,
        file_id,
        revision: 4,
        length: LENGTH,
        encrypt_metadata: protection.encrypt_metadata,
    });

    // Algorithm 5: /U, the padding and file identifier hashed and encrypted
    // twenty times, then padded out to 32 bytes.
    let mut hasher = Md5::new();
    hasher.update(PAD);
    hasher.update(file_id);
    let mut u = hasher.finalize().to_vec();
    for round in 0u8..=19 {
        let step: Vec<u8> = file_key.iter().map(|byte| byte ^ round).collect();
        rc4_in_place(&step, &mut u);
    }
    u.resize(32, 0);

    let mut dict = EncryptDict {
        v: 4,
        r: 4,
        o,
        u,
        p: permissions.0,
        length: Some(128),
        encrypt_metadata: protection.encrypt_metadata,
        ..EncryptDict::default()
    };
    crypt_filter(&mut dict, b"AESV2");
    (dict, file_key)
}

/// `data` encrypted under `method` with `key`: RC4 as is, AES in CBC with
/// the `iv` written in front and PKCS#7 padding.
pub(crate) fn encrypt(method: Method, key: &[u8], data: &[u8], iv: [u8; 16]) -> Vec<u8> {
    match method {
        Method::Identity => data.to_vec(),
        Method::Rc4 => {
            let mut out = data.to_vec();
            rc4_in_place(key, &mut out);
            out
        }
        Method::Aes128 | Method::Aes256 => {
            let mut buffer = data.to_vec();
            let length = buffer.len();
            buffer.resize(length + 16 - length % 16, 0);
            let body = match method {
                Method::Aes128 => cbc::Encryptor::<aes::Aes128>::new(key.into(), &iv.into())
                    .encrypt_padded_mut::<Pkcs7>(&mut buffer, length)
                    .map(<[u8]>::to_vec),
                _ => cbc::Encryptor::<aes::Aes256>::new(key.into(), &iv.into())
                    .encrypt_padded_mut::<Pkcs7>(&mut buffer, length)
                    .map(<[u8]>::to_vec),
            }
            .expect("the buffer has room for the padding");
            let mut out = iv.to_vec();
            out.extend_from_slice(&body);
            out
        }
    }
}

/// The key `method` encrypts object `number` with.
pub(crate) fn key_for(method: Method, file_key: &[u8], number: u32, generation: u16) -> Vec<u8> {
    match method {
        Method::Aes256 | Method::Identity => file_key.to_vec(),
        Method::Rc4 | Method::Aes128 => object_key(file_key, number, generation, method.is_aes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts up from one byte to the next: repeatable, and never the same
    /// salt twice.
    fn counting() -> impl FnMut(&mut [u8]) {
        let mut next = 0u8;
        move |buffer: &mut [u8]| {
            for byte in buffer {
                *byte = next;
                next = next.wrapping_add(1);
            }
        }
    }

    fn protection(strength: Strength) -> Protection {
        Protection {
            strength,
            user_password: b"open sesame".to_vec(),
            owner_password: b"owner".to_vec(),
            permissions: Permissions(Permissions::PRINT),
            encrypt_metadata: true,
        }
    }

    #[test]
    fn each_level_opens_with_either_password_and_not_without() {
        for strength in [Strength::Aes256, Strength::Aes128] {
            let id = b"0123456789abcdef";
            let (dict, handler) =
                protect(&protection(strength), id, &mut counting()).expect("protects");
            let (user, access) =
                SecurityHandler::open_with(&dict, id, b"open sesame").expect("the user's");
            assert_eq!(access, crate::Access::User, "{strength:?}");
            assert!(user.permissions().print());
            assert!(!user.permissions().modify());
            let (owner, access) =
                SecurityHandler::open_with(&dict, id, b"owner").expect("the owner's");
            assert_eq!(access, crate::Access::Owner, "{strength:?}");
            assert!(owner.permissions().modify(), "the owner may do everything");
            assert_eq!(
                SecurityHandler::open_with(&dict, id, b"").err(),
                Some(Error::WrongPassword),
                "{strength:?}"
            );

            let secret = b"the quarterly figures".to_vec();
            let sealed = handler.encrypt_string(7, 0, &secret, [9u8; 16]);
            assert_ne!(sealed, secret);
            assert_eq!(user.decrypt_string(7, 0, &sealed), Ok(secret.clone()));
            let stream = handler.encrypt_stream(8, 0, &secret, [3u8; 16], false);
            assert_eq!(owner.decrypt_stream(8, 0, &stream, false), Ok(secret));
        }
    }

    #[test]
    fn the_dictionary_is_the_one_each_level_names() {
        let id = b"id";
        let (dict, _) = protect(&protection(Strength::Aes256), id, &mut counting()).expect("ok");
        assert_eq!((dict.v, dict.r, dict.length), (5, 6, Some(256)));
        assert_eq!(
            (dict.u.len(), dict.ue.len(), dict.o.len(), dict.oe.len()),
            (48, 32, 48, 32)
        );
        assert_eq!(dict.perms.len(), 16);
        assert_eq!(
            dict.crypt_filters.get(&b"StdCF"[..]).map(Vec::as_slice),
            Some(&b"AESV3"[..])
        );
        assert_eq!(dict.p & Permissions::RESERVED, Permissions::RESERVED);

        let (dict, _) = protect(&protection(Strength::Aes128), id, &mut counting()).expect("ok");
        assert_eq!((dict.v, dict.r, dict.length), (4, 4, Some(128)));
        assert_eq!((dict.u.len(), dict.o.len()), (32, 32));
        assert!(dict.perms.is_empty() && dict.oe.is_empty());
        assert_eq!(
            dict.crypt_filters.get(&b"StdCF"[..]).map(Vec::as_slice),
            Some(&b"AESV2"[..])
        );
    }

    #[test]
    fn without_an_owner_password_the_user_password_lifts_the_permissions() {
        let id = b"id";
        let mut only_open = protection(Strength::Aes256);
        only_open.owner_password.clear();
        let (dict, _) = protect(&only_open, id, &mut counting()).expect("ok");
        let (_, access) = SecurityHandler::open_with(&dict, id, b"open sesame").expect("opens");
        assert_eq!(access, crate::Access::Owner);

        let mut only_permissions = protection(Strength::Aes128);
        only_permissions.user_password.clear();
        let (dict, _) = protect(&only_permissions, id, &mut counting()).expect("ok");
        let (handler, access) = SecurityHandler::open_with(&dict, id, b"").expect("no password");
        assert_eq!(access, crate::Access::User);
        assert!(!handler.permissions().modify());
    }

    #[test]
    fn the_owner_password_gives_the_user_password_below_r5() {
        let id = b"id";
        let (dict, _) = protect(&protection(Strength::Aes128), id, &mut counting()).expect("ok");
        assert_eq!(
            crate::user_password_from_owner(&dict, id, b"owner"),
            Some(b"open sesame".to_vec())
        );
        assert_eq!(
            crate::user_password_from_owner(&dict, id, b"open sesame"),
            None
        );
        let mut blank = protection(Strength::Aes128);
        blank.user_password.clear();
        let (dict, _) = protect(&blank, id, &mut counting()).expect("ok");
        assert_eq!(
            crate::user_password_from_owner(&dict, id, b"owner"),
            Some(Vec::new())
        );
        let (dict, _) = protect(&protection(Strength::Aes256), id, &mut counting()).expect("ok");
        assert_eq!(
            crate::user_password_from_owner(&dict, id, b"owner"),
            None,
            "R 6 needs none"
        );
        assert_eq!(
            crate::algorithms::unpadded(&crate::algorithms::padded(&[b'x'; 32])),
            [b'x'; 32]
        );
    }

    #[test]
    fn aes_output_carries_its_iv_and_whole_blocks() {
        let key = [1u8; 16];
        let sealed = encrypt(Method::Aes128, &key, b"", [5u8; 16]);
        assert_eq!(sealed.len(), 32, "an empty string is one block of padding");
        assert_eq!(&sealed[..16], &[5u8; 16]);
        let sealed = encrypt(Method::Aes128, &key, &[0u8; 16], [5u8; 16]);
        assert_eq!(sealed.len(), 48);
        assert_eq!(encrypt(Method::Identity, &key, b"plain", [0; 16]), b"plain");
        let rc4 = encrypt(Method::Rc4, &key[..5], b"plain", [0; 16]);
        assert_ne!(rc4, b"plain");
        assert_eq!(key_for(Method::Aes256, &[2u8; 32], 1, 0), [2u8; 32]);
        assert_eq!(key_for(Method::Rc4, &[2u8; 5], 1, 0).len(), 10);
    }

    #[test]
    fn the_system_gives_different_bytes_each_time() {
        let (mut first, mut second) = ([0u8; 16], [0u8; 16]);
        system_random(&mut first);
        system_random(&mut second);
        assert_ne!(first, second);
    }
}
