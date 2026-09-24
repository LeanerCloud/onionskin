//! A CMS SignedData signature, as `adbe.pkcs7.detached`,
//! `ETSI.CAdES.detached` and `adbe.pkcs7.sha1` carry one in `/Contents`.
//!
//! Two checks, reported apart because they mean different things:
//!
//! - **the digest**: what the signer's `messageDigest` attribute says the
//!   signed bytes hash to is what they do hash to, or the document changed;
//! - **the signature**: the signer's key signed those attributes, or someone
//!   else wrote them.

use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerIdentifier, SignerInfo};
use der::asn1::{ObjectIdentifier, OctetString};
use der::{Decode, Encode};
use x509_cert::ext::pkix::SubjectKeyIdentifier;
use x509_cert::time::Time;

use super::keys::{verifier, verify};
use super::{CmsCheck, Digest, SignatureCheck, SignatureError};
use crate::signature::certs::Certificate;

const SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
const SIGNING_TIME: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.5");
const TIME_STAMP_TOKEN: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.14");
const SUBJECT_KEY_IDENTIFIER: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.14");

/// Check the SignedData in `contents` against `data`, the bytes its byte
/// range covers.
pub(crate) fn check(contents: &[u8], data: &[u8]) -> Result<CmsCheck, SignatureError> {
    let contents = super::der_prefix(contents)?;
    let info = ContentInfo::from_der(contents)
        .map_err(|error| SignatureError::Malformed(format!("the signature is not CMS: {error}")))?;
    if info.content_type != SIGNED_DATA {
        return Err(SignatureError::Unsupported(format!(
            "CMS content of type {}",
            info.content_type
        )));
    }
    let signed: SignedData = info
        .content
        .decode_as()
        .map_err(|error| SignatureError::Malformed(format!("SignedData: {error}")))?;
    let signer = signed
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or_else(|| SignatureError::Malformed("no signer".to_owned()))?;
    let certificates = certificates(&signed);
    let signer_certificate = find_signer(signer, &certificates);

    let digest = Digest::from_oid(signer.digest_alg.oid).ok_or_else(|| {
        SignatureError::Unsupported(format!("the digest {}", signer.digest_alg.oid))
    })?;
    let (content, content_matches) = encapsulated(&signed, data, digest);
    let digest_matches = content_matches && message_digest_matches(signer, digest, &content);
    let (algorithm, signature) = match signer_certificate {
        None => (
            None,
            SignatureCheck::Unsupported("the signer's certificate is missing".to_owned()),
        ),
        Some(certificate) => signature_check(signer, certificate, digest, &content),
    };

    Ok(CmsCheck {
        digest: Some(digest),
        digest_matches,
        signature,
        algorithm,
        signer: signer_certificate.map(Certificate::from_x509),
        certificates: certificates.iter().map(Certificate::from_x509).collect(),
        signing_time: signing_time(signer),
        has_timestamp: unsigned_attribute(signer, TIME_STAMP_TOKEN),
    })
}

fn certificates(signed: &SignedData) -> Vec<x509_cert::Certificate> {
    signed
        .certificates
        .iter()
        .flat_map(|set| set.0.iter())
        .filter_map(|choice| match choice {
            CertificateChoices::Certificate(certificate) => Some(certificate.clone()),
            CertificateChoices::Other(_) => None,
        })
        .collect()
}

/// The certificate `signer` names, by issuer and serial number or by key
/// identifier.
fn find_signer<'a>(
    signer: &SignerInfo,
    certificates: &'a [x509_cert::Certificate],
) -> Option<&'a x509_cert::Certificate> {
    certificates.iter().find(|certificate| {
        let tbs = &certificate.tbs_certificate;
        match &signer.sid {
            SignerIdentifier::IssuerAndSerialNumber(named) => {
                tbs.issuer == named.issuer && tbs.serial_number == named.serial_number
            }
            SignerIdentifier::SubjectKeyIdentifier(identifier) => tbs
                .extensions
                .iter()
                .flatten()
                .filter(|extension| extension.extn_id == SUBJECT_KEY_IDENTIFIER)
                .filter_map(|extension| {
                    SubjectKeyIdentifier::from_der(extension.extn_value.as_bytes()).ok()
                })
                .any(|found| found == *identifier),
        }
    })
}

/// What the signature is over, and whether it matches `data`: `data`
/// itself for a detached signature, or the digest of it that
/// `adbe.pkcs7.sha1` encapsulates.
fn encapsulated(signed: &SignedData, data: &[u8], digest: Digest) -> (Vec<u8>, bool) {
    let content = signed
        .encap_content_info
        .econtent
        .as_ref()
        .and_then(|content| content.decode_as::<OctetString>().ok())
        .map(|content| content.as_bytes().to_vec());
    match content {
        None => (data.to_vec(), true),
        Some(content) => {
            let matches = content == Digest::Sha1.hash(data) || content == digest.hash(data);
            (content, matches)
        }
    }
}

fn attribute_values(
    attributes: Option<&x509_cert::attr::Attributes>,
    oid: ObjectIdentifier,
) -> impl Iterator<Item = &der::Any> {
    attributes
        .into_iter()
        .flat_map(|set| set.iter())
        .filter(move |attribute| attribute.oid == oid)
        .flat_map(|attribute| attribute.values.iter())
}

/// Whether the signed `messageDigest` is `content`'s digest. With no signed
/// attributes the signature is over the content itself, and the signature
/// check is the digest check.
fn message_digest_matches(signer: &SignerInfo, digest: Digest, content: &[u8]) -> bool {
    if signer.signed_attrs.is_none() {
        return true;
    }
    attribute_values(signer.signed_attrs.as_ref(), MESSAGE_DIGEST)
        .filter_map(|value| value.decode_as::<OctetString>().ok())
        .any(|stated| stated.as_bytes() == digest.hash(content))
}

fn signature_check(
    signer: &SignerInfo,
    certificate: &x509_cert::Certificate,
    digest: Digest,
    content: &[u8],
) -> (Option<super::Algorithm>, SignatureCheck) {
    let key = &certificate.tbs_certificate.subject_public_key_info;
    let verifier = match verifier(signer.signature_algorithm.oid, key, digest) {
        Ok(found) => found,
        Err(unsupported) => return (None, SignatureCheck::Unsupported(unsupported)),
    };
    let algorithm = verifier.algorithm();
    let message = match &signer.signed_attrs {
        // Signed as a SET OF, not as the [0] IMPLICIT it is stored as
        // (RFC 5652 5.4).
        Some(attributes) => match attributes.to_der() {
            Ok(der) => der,
            Err(error) => {
                return (
                    Some(algorithm),
                    SignatureCheck::Unsupported(format!("its signed attributes: {error}")),
                )
            }
        },
        None => content.to_vec(),
    };
    let valid = verify(verifier, key, &message, signer.signature.as_bytes());
    let check = if valid {
        SignatureCheck::Valid
    } else {
        SignatureCheck::Invalid
    };
    (Some(algorithm), check)
}

fn signing_time(signer: &SignerInfo) -> Option<String> {
    attribute_values(signer.signed_attrs.as_ref(), SIGNING_TIME)
        .find_map(|value| {
            // A CHOICE, so read from the value's own encoding.
            let der = value.to_der().ok()?;
            Time::from_der(&der).ok()
        })
        .map(|time| time.to_string())
}

fn unsigned_attribute(signer: &SignerInfo, oid: ObjectIdentifier) -> bool {
    attribute_values(signer.unsigned_attrs.as_ref(), oid)
        .next()
        .is_some()
}
