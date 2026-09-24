//! The certificates the user trusts to identify signers, as Acrobat's
//! Trusted Certificates list keeps them: each with what it is trusted for.
//!
//! Kept in `trusted-certificates.json` beside the preferences, owner-only,
//! each certificate as PEM so the file can be read and edited by hand.
//! Nothing here is trusted by default: a signer's identity is unknown until
//! the user adds their certificate, or one it chains to.

use std::path::Path;

use onionskin_core::signatures::{Certificate, TrustAnchor};
use serde::{Deserialize, Serialize};

/// One certificate as the file keeps it.
#[derive(Serialize, Deserialize)]
struct Stored {
    pem: String,
    signatures: bool,
    certified: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    certificates: Vec<Stored>,
}

/// The trusted certificates, in the order they were added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustedCertificates {
    anchors: Vec<TrustAnchor>,
}

impl TrustedCertificates {
    /// The list at `path`, and why it could not be read, when it could
    /// not. A missing file is an empty list.
    pub fn load(path: Option<&Path>) -> (Self, Option<String>) {
        let Some(path) = path else {
            return (Self::default(), None);
        };
        let unreadable = |error: String| {
            (
                Self::default(),
                Some(format!("{} could not be read: {error}", path.display())),
            )
        };
        match crate::config::read(path) {
            Ok(None) => (Self::default(), None),
            Ok(Some(source)) => match serde_json::from_str::<File>(&source) {
                Ok(file) => (Self::from_file(file), None),
                Err(error) => {
                    unreadable(format!("{error}{}", crate::config::keep_unreadable(path)))
                }
            },
            Err(error) => unreadable(error.to_string()),
        }
    }

    fn from_file(file: File) -> Self {
        let anchors = file
            .certificates
            .into_iter()
            .filter_map(|stored| {
                let certificate = Certificate::from_file_bytes(stored.pem.as_bytes())
                    .into_iter()
                    .next()?;
                Some(TrustAnchor {
                    certificate,
                    for_signatures: stored.signatures,
                    for_certified: stored.certified,
                })
            })
            .collect();
        Self { anchors }
    }

    /// Write the list to `path`, owner-only.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let file = File {
            certificates: self
                .anchors
                .iter()
                .map(|anchor| Stored {
                    pem: anchor.certificate.to_pem(),
                    signatures: anchor.for_signatures,
                    certified: anchor.for_certified,
                })
                .collect(),
        };
        let json = serde_json::to_string_pretty(&file).expect("strings and flags serialize");
        crate::config::write_private(path, &json)
            .map_err(|error| format!("{} could not be saved: {error}", path.display()))
    }

    pub fn anchors(&self) -> &[TrustAnchor] {
        &self.anchors
    }

    /// Trust each of `certificates` for approval signatures, as Acrobat's
    /// Import dialog does by default. One already in the list is left as
    /// it is. How many were new.
    pub fn add(&mut self, certificates: Vec<Certificate>) -> usize {
        let before = self.anchors.len();
        for certificate in certificates {
            if self
                .anchors
                .iter()
                .all(|anchor| anchor.certificate.der != certificate.der)
            {
                self.anchors.push(TrustAnchor {
                    certificate,
                    for_signatures: true,
                    for_certified: false,
                });
            }
        }
        self.anchors.len() - before
    }

    /// Stop trusting the certificate at `index`.
    pub fn remove(&mut self, index: usize) -> bool {
        (index < self.anchors.len())
            .then(|| self.anchors.remove(index))
            .is_some()
    }

    /// Turn trust for approval signatures, or with `certified` for
    /// certified documents, the other way. Trusting for certified
    /// documents trusts for signatures too, as Acrobat's checkboxes do.
    pub fn toggle(&mut self, index: usize, certified: bool) -> bool {
        let Some(anchor) = self.anchors.get_mut(index) else {
            return false;
        };
        if certified {
            anchor.for_certified = !anchor.for_certified;
            anchor.for_signatures |= anchor.for_certified;
        } else {
            anchor.for_signatures = !anchor.for_signatures;
            anchor.for_certified &= anchor.for_signatures;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ca() -> Certificate {
        let path = onionskin_corpus_testing::signed_fixture("test-ca.pem");
        Certificate::from_file_bytes(&std::fs::read(path).unwrap()).remove(0)
    }

    #[test]
    fn a_certificate_is_added_once_toggled_and_removed() {
        let mut trusted = TrustedCertificates::default();
        assert_eq!(trusted.add(vec![test_ca(), test_ca()]), 1);
        assert_eq!(trusted.add(vec![test_ca()]), 0, "already trusted");
        let anchor = &trusted.anchors()[0];
        assert!(anchor.for_signatures && !anchor.for_certified);

        assert!(trusted.toggle(0, true));
        assert!(trusted.anchors()[0].for_certified);
        assert!(trusted.toggle(0, false), "untrusting signatures");
        let anchor = &trusted.anchors()[0];
        assert!(!anchor.for_signatures && !anchor.for_certified);
        assert!(trusted.toggle(0, true), "certified brings signatures back");
        assert!(trusted.anchors()[0].for_signatures);
        assert!(!trusted.toggle(5, true));

        assert!(!trusted.remove(1));
        assert!(trusted.remove(0));
        assert!(trusted.anchors().is_empty());
    }

    #[test]
    fn the_list_saves_as_pem_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("onionskin-trusted-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trusted-certificates.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(TrustedCertificates::load(Some(&path)).0.anchors().len(), 0);
        assert_eq!(TrustedCertificates::load(None).1, None);

        let mut trusted = TrustedCertificates::default();
        trusted.add(vec![test_ca()]);
        trusted.toggle(0, true);
        trusted.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("-----BEGIN CERTIFICATE-----"));
        let (read, error) = TrustedCertificates::load(Some(&path));
        assert_eq!(error, None);
        assert_eq!(read, trusted);

        std::fs::write(&path, "not json").unwrap();
        let (read, error) = TrustedCertificates::load(Some(&path));
        assert!(read.anchors().is_empty());
        assert!(error.unwrap().contains("could not be read"));
    }
}
