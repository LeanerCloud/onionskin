//! A certificate, as a signature report shows it.

use der::asn1::{Ia5StringRef, ObjectIdentifier, PrintableStringRef, Utf8StringRef};
use der::{Decode, Encode};
use x509_cert::name::Name;

/// A certificate's facts, read once from its DER.
#[derive(Clone, PartialEq, Eq)]
pub struct Certificate {
    pub der: Vec<u8>,
    /// The subject's distinguished name, as RFC 4514 writes it.
    pub subject: String,
    /// The subject's common name, which is the name a report leads with.
    pub common_name: Option<String>,
    pub issuer: String,
    /// The serial number, in hexadecimal.
    pub serial: String,
    /// When it is valid from and to, as RFC 3339 dates.
    pub not_before: String,
    pub not_after: String,
}

impl std::fmt::Debug for Certificate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Certificate")
            .field("subject", &self.subject)
            .field("issuer", &self.issuer)
            .field("serial", &self.serial)
            .finish_non_exhaustive()
    }
}

const COMMON_NAME: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.4.3");

impl Certificate {
    pub(crate) fn from_x509(certificate: &x509_cert::Certificate) -> Certificate {
        let tbs = &certificate.tbs_certificate;
        Certificate {
            der: certificate.to_der().unwrap_or_default(),
            subject: tbs.subject.to_string(),
            common_name: common_name(&tbs.subject),
            issuer: tbs.issuer.to_string(),
            serial: tbs
                .serial_number
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect(),
            not_before: tbs.validity.not_before.to_string(),
            not_after: tbs.validity.not_after.to_string(),
        }
    }

    /// A certificate from its DER, as a PDF's `/Cert` holds one.
    pub fn from_der(der: &[u8]) -> Option<Certificate> {
        x509_cert::Certificate::from_der(der)
            .ok()
            .map(|certificate| Certificate::from_x509(&certificate))
    }

    /// Who signed, in words: the common name, or the whole subject.
    pub fn display_name(&self) -> &str {
        self.common_name.as_deref().unwrap_or(&self.subject)
    }
}

/// The first common name in `name`, in whichever string type it is written.
fn common_name(name: &Name) -> Option<String> {
    name.0
        .iter()
        .flat_map(|rdn| rdn.0.iter())
        .filter(|attribute| attribute.oid == COMMON_NAME)
        .find_map(|attribute| {
            let value = &attribute.value;
            value
                .decode_as::<Utf8StringRef<'_>>()
                .map(|text| text.to_string())
                .or_else(|_| {
                    value
                        .decode_as::<PrintableStringRef<'_>>()
                        .map(|text| text.to_string())
                })
                .or_else(|_| {
                    value
                        .decode_as::<Ia5StringRef<'_>>()
                        .map(|text| text.to_string())
                })
                .ok()
        })
}
