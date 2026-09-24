//! The public-key check: a signature over a message with a certificate's
//! key, in the algorithm the signer names. ring does the arithmetic.

use der::asn1::ObjectIdentifier;
use ring::signature::{self as ring_signature, UnparsedPublicKey, VerificationAlgorithm};
use x509_cert::spki::SubjectPublicKeyInfoOwned;

use super::Digest;

const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const RSA_SHA1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.5");
const RSA_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");
const RSA_SHA384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.12");
const RSA_SHA512: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.13");
const RSA_PSS: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.10");
const EC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const ECDSA_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");
const ECDSA_SHA384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.3");
const ECDSA_SHA512: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.4");
const P256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const P384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");

/// A signature algorithm, as a report names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    RsaPkcs1,
    RsaPss,
    EcdsaP256,
    EcdsaP384,
}

impl Algorithm {
    pub fn name(self) -> &'static str {
        match self {
            Algorithm::RsaPkcs1 => "RSA (PKCS #1 v1.5)",
            Algorithm::RsaPss => "RSA-PSS",
            Algorithm::EcdsaP256 => "ECDSA P-256",
            Algorithm::EcdsaP384 => "ECDSA P-384",
        }
    }
}

/// Which algorithm `signature_oid` and the key make, and ring's verifier for
/// it with `digest`. `Err` names what is not supported.
/// How a signature is checked: by ring, or by this crate's PSS decode.
#[derive(Clone, Copy)]
pub(crate) enum Verifier {
    Ring(Algorithm, &'static dyn VerificationAlgorithm),
    Pss(Digest),
}

impl Verifier {
    pub(crate) fn algorithm(self) -> Algorithm {
        match self {
            Verifier::Ring(algorithm, _) => algorithm,
            Verifier::Pss(_) => Algorithm::RsaPss,
        }
    }
}

pub(crate) fn verifier(
    signature_oid: ObjectIdentifier,
    key: &SubjectPublicKeyInfoOwned,
    digest: Digest,
) -> Result<Verifier, String> {
    let key_oid = key.algorithm.oid;
    match signature_oid {
        RSA | RSA_SHA1 | RSA_SHA256 | RSA_SHA384 | RSA_SHA512 if key_oid == RSA => {
            let digest = match signature_oid {
                RSA_SHA1 => Digest::Sha1,
                RSA_SHA256 => Digest::Sha256,
                RSA_SHA384 => Digest::Sha384,
                RSA_SHA512 => Digest::Sha512,
                _ => digest,
            };
            Ok(Verifier::Ring(Algorithm::RsaPkcs1, rsa_pkcs1(digest)))
        }
        RSA_PSS if key_oid == RSA || key_oid == RSA_PSS => Ok(Verifier::Pss(digest)),
        EC_KEY | ECDSA_SHA256 | ECDSA_SHA384 | ECDSA_SHA512 if key_oid == EC_KEY => {
            let digest = match signature_oid {
                ECDSA_SHA256 => Digest::Sha256,
                ECDSA_SHA384 => Digest::Sha384,
                ECDSA_SHA512 => Digest::Sha512,
                _ => digest,
            };
            ecdsa(curve(key)?, digest).map(|(algorithm, found)| Verifier::Ring(algorithm, found))
        }
        other => Err(format!("the signature algorithm {other}")),
    }
}

fn rsa_pkcs1(digest: Digest) -> &'static dyn VerificationAlgorithm {
    // The legacy variants take keys from 1024 bits, which old signatures
    // were made with; a report says when a key is that short.
    match digest {
        Digest::Sha1 => &ring_signature::RSA_PKCS1_1024_8192_SHA1_FOR_LEGACY_USE_ONLY,
        Digest::Sha256 => &ring_signature::RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY,
        Digest::Sha384 => &ring_signature::RSA_PKCS1_2048_8192_SHA384,
        Digest::Sha512 => &ring_signature::RSA_PKCS1_1024_8192_SHA512_FOR_LEGACY_USE_ONLY,
    }
}

fn curve(key: &SubjectPublicKeyInfoOwned) -> Result<ObjectIdentifier, String> {
    key.algorithm
        .parameters
        .as_ref()
        .and_then(|parameters| parameters.decode_as::<ObjectIdentifier>().ok())
        .ok_or_else(|| "an elliptic-curve key that names no curve".to_owned())
}

fn ecdsa(
    curve: ObjectIdentifier,
    digest: Digest,
) -> Result<(Algorithm, &'static dyn VerificationAlgorithm), String> {
    Ok(match (curve, digest) {
        (P256, Digest::Sha256) => (
            Algorithm::EcdsaP256,
            &ring_signature::ECDSA_P256_SHA256_ASN1,
        ),
        (P256, Digest::Sha384) => (
            Algorithm::EcdsaP256,
            &ring_signature::ECDSA_P256_SHA384_ASN1,
        ),
        (P384, Digest::Sha256) => (
            Algorithm::EcdsaP384,
            &ring_signature::ECDSA_P384_SHA256_ASN1,
        ),
        (P384, Digest::Sha384) => (
            Algorithm::EcdsaP384,
            &ring_signature::ECDSA_P384_SHA384_ASN1,
        ),
        (curve, digest) => {
            return Err(format!("ECDSA on the curve {curve} with {}", digest.name()))
        }
    })
}

/// Whether `signature` over `message` verifies with `key`.
pub(crate) fn verify(
    verifier: Verifier,
    key: &SubjectPublicKeyInfoOwned,
    message: &[u8],
    signature: &[u8],
) -> bool {
    let key = key.subject_public_key.raw_bytes();
    match verifier {
        Verifier::Ring(_, algorithm) => UnparsedPublicKey::new(algorithm, key)
            .verify(message, signature)
            .is_ok(),
        Verifier::Pss(digest) => super::pss::verify(key, digest, message, signature),
    }
}
