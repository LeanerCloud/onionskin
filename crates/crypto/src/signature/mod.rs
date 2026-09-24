//! Checking a PDF signature: the CMS in `/Contents` against the bytes its
//! `/ByteRange` covers.
//!
//! What is checked is what the signature proves: that these bytes are the
//! ones signed, and that the key in the signer's certificate signed them.
//! Whether that certificate is one the user trusts is a separate question,
//! answered with the user's trusted certificates, not here.

mod certs;
mod cms;
mod digest;
mod keys;
mod pss;

pub use certs::Certificate;
pub use digest::Digest;
pub use keys::Algorithm;

/// Whether the signature itself verifies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureCheck {
    Valid,
    /// The key did not make it: the signature was altered or is not the
    /// signer's.
    Invalid,
    /// It could not be checked, and why.
    Unsupported(String),
}

/// What checking one signature found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CmsCheck {
    pub digest: Option<Digest>,
    /// Whether the signed bytes still hash to what was signed.
    pub digest_matches: bool,
    pub signature: SignatureCheck,
    pub algorithm: Option<Algorithm>,
    pub signer: Option<Certificate>,
    /// Every certificate the signature carries, the signer's among them.
    pub certificates: Vec<Certificate>,
    /// The signing time the signer's own attributes state. Unverified.
    pub signing_time: Option<String>,
    /// Whether a timestamp token is attached.
    pub has_timestamp: bool,
}

impl CmsCheck {
    /// The bytes are the signed ones and the signer's key signed them.
    pub fn is_valid(&self) -> bool {
        self.digest_matches && self.signature == SignatureCheck::Valid
    }

    /// Made with a digest collisions are practical for.
    pub fn is_weak(&self) -> bool {
        self.digest.is_some_and(Digest::is_weak)
    }
}

/// Why a signature could not be checked at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureError {
    Malformed(String),
    Unsupported(String),
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignatureError::Malformed(why) => write!(f, "the signature is malformed: {why}"),
            SignatureError::Unsupported(what) => write!(f, "{what} is not supported"),
        }
    }
}

impl std::error::Error for SignatureError {}

/// Check a CMS signature (`adbe.pkcs7.detached`, `ETSI.CAdES.detached`,
/// `adbe.pkcs7.sha1`) in `contents` over `data`.
pub fn verify_cms(contents: &[u8], data: &[u8]) -> Result<CmsCheck, SignatureError> {
    cms::check(contents, data)
}

/// Check an `adbe.x509.rsa_sha1` signature: `contents` is a DER OCTET
/// STRING holding a PKCS #1 signature over `data`'s SHA-1 digest, made
/// with the key of the first of `certificates` (`/Cert`).
pub fn verify_x509_rsa_sha1(
    contents: &[u8],
    certificates: &[Vec<u8>],
    data: &[u8],
) -> Result<CmsCheck, SignatureError> {
    use der::Decode;
    let contents = der_prefix(contents)?;
    let signature = der::asn1::OctetString::from_der(contents)
        .map_err(|error| SignatureError::Malformed(format!("the signature: {error}")))?;
    let parsed: Vec<x509_cert::Certificate> = certificates
        .iter()
        .filter_map(|der| x509_cert::Certificate::from_der(der).ok())
        .collect();
    let signer = parsed
        .first()
        .ok_or_else(|| SignatureError::Malformed("no /Cert".to_owned()))?;
    let key = &signer.tbs_certificate.subject_public_key_info;
    let valid = keys::verifier(
        der::asn1::ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.5"),
        key,
        Digest::Sha1,
    )
    .map(|verifier| keys::verify(verifier, key, data, signature.as_bytes()));
    let signature = match valid {
        Ok(true) => SignatureCheck::Valid,
        Ok(false) => SignatureCheck::Invalid,
        Err(unsupported) => SignatureCheck::Unsupported(unsupported),
    };
    // The signature is over the data's digest: with no separate digest to
    // compare, a signature that verifies is one whose digest matches.
    let digest_matches = signature == SignatureCheck::Valid;
    Ok(CmsCheck {
        digest: Some(Digest::Sha1),
        digest_matches,
        signature,
        algorithm: Some(Algorithm::RsaPkcs1),
        signer: Some(Certificate::from_x509(signer)),
        certificates: parsed.iter().map(Certificate::from_x509).collect(),
        signing_time: None,
        has_timestamp: false,
    })
}

/// The DER value at the start of `bytes`: `/Contents` is padded with zeros
/// past its end. A BER value of indefinite length is refused.
pub(crate) fn der_prefix(bytes: &[u8]) -> Result<&[u8], SignatureError> {
    let malformed = || SignatureError::Malformed("the signature is truncated".to_owned());
    let first = *bytes.get(1).ok_or_else(malformed)?;
    let (length, header) = match first {
        0x80 => {
            return Err(SignatureError::Unsupported(
                "a BER signature of indefinite length".to_owned(),
            ))
        }
        short if short < 0x80 => (usize::from(short), 2),
        long => {
            let count = usize::from(long & 0x7F);
            if count > 4 {
                return Err(malformed());
            }
            let digits = bytes.get(2..2 + count).ok_or_else(malformed)?;
            let length = digits
                .iter()
                .fold(0usize, |total, byte| (total << 8) | usize::from(*byte));
            (length, 2 + count)
        }
    };
    bytes.get(..header + length).ok_or_else(malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_der_value_is_cut_from_its_padding() {
        assert_eq!(
            der_prefix(&[0x04, 0x02, 1, 2, 0, 0]),
            Ok(&[0x04, 0x02, 1, 2][..])
        );
        let long = [&[0x30, 0x81, 0x03][..], &[7, 8, 9, 0, 0]].concat();
        assert_eq!(der_prefix(&long), Ok(&long[..6]));
        assert!(matches!(
            der_prefix(&[0x30, 0x80, 0]),
            Err(SignatureError::Unsupported(_))
        ));
        assert!(matches!(
            der_prefix(&[0x30, 0x05, 1]),
            Err(SignatureError::Malformed(_))
        ));
        assert!(matches!(
            der_prefix(&[0x30]),
            Err(SignatureError::Malformed(_))
        ));
        assert!(matches!(
            der_prefix(&[0x30, 0x85, 1, 1, 1, 1, 1]),
            Err(SignatureError::Malformed(_))
        ));
    }

    #[test]
    fn errors_say_what_went_wrong() {
        assert!(SignatureError::Malformed("x".into())
            .to_string()
            .contains("malformed"));
        assert!(SignatureError::Unsupported("ECDSA on P-521".into())
            .to_string()
            .ends_with("is not supported"));
        assert!(verify_cms(b"\x04\x01x", b"").is_err());
        assert!(verify_x509_rsa_sha1(b"\x04\x01x", &[], b"").is_err());
    }
}
