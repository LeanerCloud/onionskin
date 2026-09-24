//! Whether a signer is who their certificate says: a path from the
//! signer's certificate to one the user trusts.
//!
//! The path is built by name and checked by signature: each certificate's
//! issuer is one whose subject is its issuer name and whose key signed it,
//! found among the certificates the signature carries and the trusted
//! ones. Once a path reaches a trusted certificate, each certificate on it
//! must be valid at the chosen time, each issuer must be a CA allowed to
//! sign certificates, and the signer's key must be allowed to sign
//! documents. No path means the identity is unknown; a path that fails a
//! check means it is invalid. OpenSSL's `openssl verify -purpose
//! smimesign` is the reference the tests compare with.

use der::{Decode, Encode};
use x509_cert::ext::pkix::{BasicConstraints, KeyUsage, KeyUsages};
use x509_cert::Certificate as X509;

use super::certs::Certificate;
use super::{keys, Digest};

/// The longest path followed. Real document-signing chains are three or
/// four certificates; a loop stops here too.
const MAX_DEPTH: usize = 8;

/// A certificate the user trusts, and what for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustAnchor {
    pub certificate: Certificate,
    /// Trusted for approval signatures.
    pub for_signatures: bool,
    /// Trusted for certification signatures too.
    pub for_certified: bool,
}

/// What the signature is: an approval, or a certification, which needs a
/// certificate trusted for certified documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Approval,
    Certification,
}

/// The signer's identity, as Acrobat words the three answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identity {
    /// A path reaches a trusted certificate and every check on it passes.
    /// The path runs from the signer to the trusted certificate.
    Valid { path: Vec<Certificate> },
    /// No path reaches a certificate trusted for this purpose.
    Unknown(String),
    /// A path reaches a trusted certificate, and this check on it fails.
    Invalid(String),
}

/// The signer's identity, at `at` (RFC 3339, as `Certificate` writes its
/// dates), given the certificates the signature carries and the trusted
/// ones.
pub fn identity(
    signer: &Certificate,
    carried: &[Certificate],
    anchors: &[TrustAnchor],
    at: &str,
    purpose: Purpose,
) -> Identity {
    let trusted: Vec<&Certificate> = anchors
        .iter()
        .filter(|anchor| match purpose {
            Purpose::Approval => anchor.for_signatures,
            Purpose::Certification => anchor.for_certified,
        })
        .map(|anchor| &anchor.certificate)
        .collect();
    let Some(path) = path_to(signer, carried, &trusted) else {
        return Identity::Unknown(unknown_reason(anchors, &trusted, purpose));
    };
    match check(&path, at) {
        Ok(()) => Identity::Valid { path },
        Err(reason) => Identity::Invalid(reason),
    }
}

fn unknown_reason(anchors: &[TrustAnchor], trusted: &[&Certificate], purpose: Purpose) -> String {
    if trusted.is_empty() && !anchors.is_empty() && purpose == Purpose::Certification {
        return "none of your trusted certificates is trusted for certified documents".to_owned();
    }
    "neither the signer's certificate nor any certificate it chains to is in your trusted certificates".to_owned()
}

/// The path from `signer` to a trusted certificate, by name and signature.
fn path_to(
    signer: &Certificate,
    carried: &[Certificate],
    trusted: &[&Certificate],
) -> Option<Vec<Certificate>> {
    let mut path = vec![signer.clone()];
    for _ in 0..MAX_DEPTH {
        let current = path.last().expect("the path starts with the signer");
        if trusted.iter().any(|anchor| anchor.der == current.der) {
            return Some(path);
        }
        if current.subject == current.issuer {
            // Self-signed and not trusted: the path ends here.
            return None;
        }
        let issuer = trusted
            .iter()
            .copied()
            .chain(carried)
            .find(|candidate| issued(candidate, current))?;
        path.push(issuer.clone());
    }
    None
}

/// Whether `issuer` issued `child`: the names match and its key signed it.
fn issued(issuer: &Certificate, child: &Certificate) -> bool {
    if issuer.subject != child.issuer || issuer.der == child.der {
        return false;
    }
    let (Ok(issuer), Ok(child)) = (X509::from_der(&issuer.der), X509::from_der(&child.der)) else {
        return false;
    };
    let Ok(tbs) = child.tbs_certificate.to_der() else {
        return false;
    };
    let key = &issuer.tbs_certificate.subject_public_key_info;
    keys::verifier(child.signature_algorithm.oid, key, Digest::Sha256)
        .is_ok_and(|verifier| keys::verify(verifier, key, &tbs, child.signature.raw_bytes()))
}

/// Every check on a path that reached a trusted certificate.
fn check(path: &[Certificate], at: &str) -> Result<(), String> {
    for (depth, certificate) in path.iter().enumerate() {
        let name = certificate.display_name();
        if at < certificate.not_before.as_str() {
            return Err(format!(
                "{name}'s certificate is not valid until {}",
                certificate.not_before
            ));
        }
        if at > certificate.not_after.as_str() {
            return Err(format!(
                "{name}'s certificate expired on {}",
                certificate.not_after
            ));
        }
        let parsed = X509::from_der(&certificate.der)
            .map_err(|error| format!("{name}'s certificate is unreadable: {error}"))?;
        let usage = key_usage(&parsed);
        if depth == 0 {
            let signs = usage.is_none_or(|usage| {
                usage.0.contains(KeyUsages::DigitalSignature)
                    || usage.0.contains(KeyUsages::NonRepudiation)
            });
            if !signs {
                return Err(format!("{name}'s certificate is not for signing documents"));
            }
        } else {
            if !is_ca(&parsed) {
                return Err(format!(
                    "{name}'s certificate is not a certificate authority's"
                ));
            }
            if usage.is_some_and(|usage| !usage.0.contains(KeyUsages::KeyCertSign)) {
                return Err(format!("{name}'s certificate may not issue certificates"));
            }
        }
    }
    Ok(())
}

fn extension<T: for<'a> Decode<'a>>(certificate: &X509, oid: &str) -> Option<T> {
    let oid = der::asn1::ObjectIdentifier::new(oid).ok()?;
    certificate
        .tbs_certificate
        .extensions
        .as_ref()?
        .iter()
        .find(|extension| extension.extn_id == oid)
        .and_then(|extension| T::from_der(extension.extn_value.as_bytes()).ok())
}

fn key_usage(certificate: &X509) -> Option<KeyUsage> {
    extension(certificate, "2.5.29.15")
}

fn is_ca(certificate: &X509) -> bool {
    // A self-signed version 1 certificate has no extensions and is a CA by
    // being trusted; anything else must say so.
    extension::<BasicConstraints>(certificate, "2.5.29.19")
        .is_some_and(|constraints| constraints.ca)
        || certificate.tbs_certificate.extensions.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(certificate: &Certificate, for_certified: bool) -> TrustAnchor {
        TrustAnchor {
            certificate: certificate.clone(),
            for_signatures: true,
            for_certified,
        }
    }

    #[test]
    fn an_untrusted_self_signed_certificate_ends_the_path() {
        let certificate = Certificate {
            der: vec![1],
            subject: "CN=Me".into(),
            common_name: Some("Me".into()),
            issuer: "CN=Me".into(),
            serial: "01".into(),
            not_before: "2024-01-01T00:00:00Z".into(),
            not_after: "2039-01-01T00:00:00Z".into(),
        };
        let found = identity(
            &certificate,
            &[],
            &[],
            "2030-01-01T00:00:00Z",
            Purpose::Approval,
        );
        assert!(matches!(found, Identity::Unknown(_)));

        let trusted = [anchor(&certificate, false)];
        let certified = identity(
            &certificate,
            &[],
            &trusted,
            "2030-01-01T00:00:00Z",
            Purpose::Certification,
        );
        assert_eq!(
            certified,
            Identity::Unknown(
                "none of your trusted certificates is trusted for certified documents".into()
            )
        );
        // Trusted directly: the path is the certificate itself, and an
        // unreadable DER is invalid rather than trusted.
        let direct = identity(
            &certificate,
            &[],
            &trusted,
            "2030-01-01T00:00:00Z",
            Purpose::Approval,
        );
        assert!(matches!(direct, Identity::Invalid(reason) if reason.contains("unreadable")));
    }
}
