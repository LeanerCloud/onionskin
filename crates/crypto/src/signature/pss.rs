//! RSA-PSS verification with any salt length (RFC 8017 9.1.2).
//!
//! ring checks PSS only with a salt as long as the digest, and signers are
//! free to use another: pyHanko and OpenSSL's default use the longest the
//! key allows. Verification is the public-key operation and a decode, with
//! no secret in it, so it is written here over num-bigint rather than left
//! unsupported.

use der::asn1::UintRef;
use der::{Decode, Reader, SliceReader};
use num_bigint::BigUint;

use super::Digest;

/// The modulus and public exponent of a DER `RSAPublicKey`.
fn public_key(der: &[u8]) -> Option<(BigUint, BigUint)> {
    let mut reader = SliceReader::new(der).ok()?;
    reader
        .sequence(|inner| {
            let modulus = UintRef::decode(inner)?;
            let exponent = UintRef::decode(inner)?;
            Ok((
                BigUint::from_bytes_be(modulus.as_bytes()),
                BigUint::from_bytes_be(exponent.as_bytes()),
            ))
        })
        .ok()
}

/// MGF1 with `digest`, `length` bytes from `seed`.
fn mgf1(digest: Digest, seed: &[u8], length: usize) -> Vec<u8> {
    let mut mask = Vec::with_capacity(length + 64);
    let mut counter: u32 = 0;
    while mask.len() < length {
        let block = [seed, &counter.to_be_bytes()].concat();
        mask.extend(digest.hash(&block));
        counter += 1;
    }
    mask.truncate(length);
    mask
}

/// Whether `signature` is an RSA-PSS signature over `message` with `digest`
/// by the key `key_der`.
pub(crate) fn verify(key_der: &[u8], digest: Digest, message: &[u8], signature: &[u8]) -> bool {
    let Some((modulus, exponent)) = public_key(key_der) else {
        return false;
    };
    let signature = BigUint::from_bytes_be(signature);
    if signature >= modulus {
        return false;
    }
    let bits = modulus.bits() as usize;
    let em_bits = bits - 1;
    let em_length = em_bits.div_ceil(8);
    let decoded = signature.modpow(&exponent, &modulus).to_bytes_be();
    if decoded.len() > em_length {
        return false;
    }
    let mut encoded = vec![0u8; em_length - decoded.len()];
    encoded.extend(decoded);
    decode(&encoded, em_bits, digest, &digest.hash(message))
}

/// EMSA-PSS-VERIFY's decode of `encoded` against the message digest.
fn decode(encoded: &[u8], em_bits: usize, digest: Digest, message_hash: &[u8]) -> bool {
    let hash_length = message_hash.len();
    let em_length = encoded.len();
    if em_length < hash_length + 2 || encoded[em_length - 1] != 0xBC {
        return false;
    }
    let (masked, rest) = encoded.split_at(em_length - hash_length - 1);
    let hash = &rest[..hash_length];
    let zero_bits = 8 * em_length - em_bits;
    let top_mask = 0xFFu8.checked_shr(zero_bits as u32).unwrap_or(0);
    if masked[0] & !top_mask != 0 {
        return false;
    }
    let mut block: Vec<u8> = masked
        .iter()
        .zip(mgf1(digest, hash, masked.len()))
        .map(|(byte, mask)| byte ^ mask)
        .collect();
    block[0] &= top_mask;
    let Some(one) = block.iter().position(|byte| *byte != 0) else {
        return false;
    };
    if block[one] != 0x01 {
        return false;
    }
    let salt = &block[one + 1..];
    let prime = [&[0u8; 8][..], message_hash, salt].concat();
    digest.hash(&prime) == hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mgf1_is_as_long_as_asked_and_counts_blocks() {
        let mask = mgf1(Digest::Sha256, b"seed", 70);
        assert_eq!(mask.len(), 70);
        assert_eq!(&mask[..32], Digest::Sha256.hash(b"seed\0\0\0\0").as_slice());
        assert_eq!(
            &mask[32..64],
            Digest::Sha256.hash(b"seed\0\0\0\x01").as_slice()
        );
    }

    #[test]
    fn a_malformed_encoding_or_key_is_refused() {
        let hash = Digest::Sha256.hash(b"m");
        assert!(!decode(&[0u8; 10], 79, Digest::Sha256, &hash), "too short");
        assert!(!decode(&[0u8; 64], 511, Digest::Sha256, &hash), "no 0xBC");
        let mut top = vec![0u8; 64];
        top[0] = 0x80;
        top[63] = 0xBC;
        assert!(!decode(&top, 511, Digest::Sha256, &hash), "the top bit set");
        assert!(!verify(b"\x30\x00", Digest::Sha256, b"m", b"\x01"));
        assert!(!verify(b"not der", Digest::Sha256, b"m", b"\x01"));
    }
}
