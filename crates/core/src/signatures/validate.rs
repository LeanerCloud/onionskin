//! Validating the document's signatures, as Acrobat's Signatures pane does:
//! whether the signed bytes are unchanged and the signer's key signed them,
//! which revision each signature covers, what was changed after it, and
//! whether a certification signature allows those changes.
//!
//! **Identity is not decided here.** Whether the signer is who they say is
//! a question for the user's trusted certificates, which [`super::identity`]
//! asks.

use std::collections::BTreeSet;
use std::sync::Arc;

use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Object};
use onionskin_crypto::signature::{verify_cms, verify_x509_rsa_sha1, CmsCheck};

use super::changes::{changes, Change};

/// Which bytes a signature's byte range covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// The whole file as it is.
    WholeFile,
    /// The file as it was when revision `revision` (counting from 0) ended;
    /// later sections were added after it.
    Revision { revision: usize, later: usize },
    /// The byte range is not one a signature may have, and why.
    Invalid(String),
}

/// A signature's verdict, as the pane leads with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Unchanged since signing, or changed only as allowed, and signed by
    /// the certificate's key. Whether the signer is trusted is unknown.
    Valid,
    /// Changed after signing, or not made by the key it names.
    Invalid(String),
    /// It could not be checked, and why.
    Unknown(String),
}

/// One signed field, validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    pub field: String,
    /// `/SubFilter`: the signature's format.
    pub sub_filter: Option<String>,
    pub check: Result<CmsCheck, String>,
    pub coverage: Coverage,
    /// What later sections changed, when the signature covers an earlier
    /// revision.
    pub changes: BTreeSet<Change>,
    /// A certification signature's `/P`: 1, 2 or 3.
    pub certification: Option<u8>,
    /// How many bytes of the file the signature covers: the signed version
    /// is the file cut there.
    pub signed_length: Option<usize>,
    /// The changes a certification signature in the document forbids.
    pub disallowed: BTreeSet<Change>,
    pub verdict: Verdict,
}

impl Validation {
    /// Whether the signature is SHA-1, and so weak.
    pub fn is_weak(&self) -> bool {
        self.check.as_ref().is_ok_and(CmsCheck::is_weak)
    }

    /// The pane's one-line summary.
    pub fn summary(&self) -> String {
        let signer = self
            .check
            .as_ref()
            .ok()
            .and_then(|check| check.signer.as_ref())
            .map_or_else(
                || "an unknown signer".to_owned(),
                |signer| signer.display_name().to_owned(),
            );
        match &self.verdict {
            Verdict::Valid if self.changes.is_empty() => format!(
                "Signed by {signer}. The document has not been modified since this signature was applied."
            ),
            Verdict::Valid => format!(
                "Signed by {signer}. The document was changed after this signature was applied ({}); the signed version is unchanged.",
                labels(&self.changes)
            ),
            Verdict::Invalid(why) => format!("Signed by {signer}. The signature is invalid: {why}."),
            Verdict::Unknown(why) => format!("Signed by {signer}. Its validity is unknown: {why}."),
        }
    }
}

fn labels(changes: &BTreeSet<Change>) -> String {
    changes
        .iter()
        .map(|change| change.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A signature dictionary's own facts.
struct Signed<'a> {
    field: String,
    value: &'a Dict,
}

/// Validate every signed field in `bytes`, the document as saved.
pub(crate) fn validate(bytes: &Arc<Vec<u8>>, password: &str) -> crate::Result<Vec<Validation>> {
    let current = open(bytes, bytes.len(), password)?;
    let sections = current.sections()?;
    let values = signed_values(&current)?;
    let certification = certification(&current, &values);
    Ok(values
        .iter()
        .map(|signed| {
            let signed = Signed {
                field: signed.0.clone(),
                value: &signed.1,
            };
            one(bytes, password, &current, &sections, &signed, certification)
        })
        .collect())
}

fn open(bytes: &Arc<Vec<u8>>, length: usize, password: &str) -> crate::Result<CosDocument> {
    let source = BytesSource::prefix(Arc::clone(bytes), length);
    Ok(CosDocument::open_repairing_with_password(Box::new(source), password.as_bytes())?.0)
}

/// Every signed field's name and signature dictionary.
fn signed_values(doc: &CosDocument) -> crate::Result<Vec<(String, Dict)>> {
    let fields = super::read_with_values(doc)?;
    Ok(fields)
}

/// The certification signature's `/P`, when the document has one: named by
/// the catalog's `/Perms /DocMDP`, or a signature with a DocMDP reference.
fn certification(doc: &CosDocument, values: &[(String, Dict)]) -> Option<u8> {
    values.iter().find_map(|(_, value)| docmdp(doc, value))
}

fn docmdp(doc: &CosDocument, value: &Dict) -> Option<u8> {
    let references = doc.resolve(value.get(b"Reference")?).ok()?;
    references.as_array()?.iter().find_map(|reference| {
        let reference = doc.resolve(reference).ok()?;
        let reference = reference.as_dict()?;
        let method = reference.get(b"TransformMethod")?.as_name()?;
        if method.as_bytes() != b"DocMDP" {
            return None;
        }
        let permission = reference
            .get(b"TransformParams")
            .and_then(|params| doc.resolve(params).ok())
            .and_then(|params| params.as_dict()?.get(b"P")?.as_integer())
            .unwrap_or(2);
        Some(permission.clamp(1, 3) as u8)
    })
}

fn one(
    bytes: &Arc<Vec<u8>>,
    password: &str,
    current: &CosDocument,
    sections: &[onionskin_cos::Section],
    signed: &Signed<'_>,
    certification: Option<u8>,
) -> Validation {
    let value = signed.value;
    let sub_filter = value
        .get(b"SubFilter")
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned());
    let range = byte_range(current, value);
    let contents = match value.get(b"Contents") {
        Some(Object::String(contents)) => contents.clone(),
        _ => Vec::new(),
    };
    let coverage = match &range {
        Ok(range) => coverage(bytes, range, sections),
        Err(why) => Coverage::Invalid(why.clone()),
    };
    let check = match &range {
        Ok(range) => {
            let data = [
                &bytes[range[0]..range[0] + range[1]],
                &bytes[range[2]..range[2] + range[3]],
            ]
            .concat();
            check(current, value, sub_filter.as_deref(), &contents, &data)
        }
        Err(why) => Err(why.clone()),
    };
    let changes = match (&range, &coverage) {
        (Ok(range), Coverage::Revision { .. }) => open(bytes, range[2] + range[3], password)
            .map(|at_signing| changes(&at_signing, current))
            .unwrap_or_else(|_| BTreeSet::from([Change::Other])),
        _ => BTreeSet::new(),
    };
    let own = docmdp(current, value);
    let permission = own.or(certification);
    let disallowed: BTreeSet<Change> = match permission {
        Some(permission) => changes
            .iter()
            .copied()
            .filter(|change| !change.allowed_by(permission))
            .collect(),
        None => BTreeSet::new(),
    };
    let verdict = verdict(&check, &coverage, &disallowed);
    Validation {
        field: signed.field.clone(),
        sub_filter,
        check,
        coverage,
        changes,
        certification: own,
        signed_length: range.as_ref().ok().map(|range| range[2] + range[3]),
        disallowed,
        verdict,
    }
}

/// `/ByteRange`, checked to be one a signature may have: two ranges, the
/// first from the start of the file, around nothing but `/Contents`.
fn byte_range(doc: &CosDocument, value: &Dict) -> Result<[usize; 4], String> {
    let range = value
        .get(b"ByteRange")
        .and_then(|range| doc.resolve(range).ok())
        .and_then(|range| {
            let numbers: Vec<usize> = range
                .as_array()?
                .iter()
                .filter_map(|number| usize::try_from(number.as_integer()?).ok())
                .collect();
            <[usize; 4]>::try_from(numbers).ok()
        })
        .ok_or_else(|| "its byte range is missing or malformed".to_owned())?;
    if range[0] != 0 || range[2] < range[0] + range[1] {
        return Err(
            "its byte range does not start the file and skip only the signature".to_owned(),
        );
    }
    Ok(range)
}

fn coverage(bytes: &[u8], range: &[usize; 4], sections: &[onionskin_cos::Section]) -> Coverage {
    let end = range[2] + range[3];
    if end > bytes.len() {
        return Coverage::Invalid("its byte range runs past the end of the file".to_owned());
    }
    let gap = &bytes[range[1]..range[2]];
    if gap.first() != Some(&b'<') || gap.last() != Some(&b'>') {
        return Coverage::Invalid("its byte range skips more than the signature".to_owned());
    }
    if end == bytes.len() {
        return Coverage::WholeFile;
    }
    // A section's end is where its %%EOF line ends; a signed revision may
    // end a line earlier or later, so the nearest end at or past it counts.
    match sections
        .iter()
        .position(|section| section.end as usize >= end && section.start as usize <= end)
    {
        Some(revision) => Coverage::Revision {
            revision,
            later: sections.len() - revision - 1,
        },
        None => {
            Coverage::Invalid("its byte range ends inside the file, not at a revision".to_owned())
        }
    }
}

fn check(
    doc: &CosDocument,
    value: &Dict,
    sub_filter: Option<&str>,
    contents: &[u8],
    data: &[u8],
) -> Result<CmsCheck, String> {
    match sub_filter {
        Some("adbe.x509.rsa_sha1") => {
            let certificates = certificates(doc, value);
            verify_x509_rsa_sha1(contents, &certificates, data)
        }
        _ => verify_cms(contents, data),
    }
    .map_err(|error| error.to_string())
}

/// `/Cert`: one certificate, or an array of them.
fn certificates(doc: &CosDocument, value: &Dict) -> Vec<Vec<u8>> {
    match value.get(b"Cert").and_then(|cert| doc.resolve(cert).ok()) {
        Some(Object::String(der)) => vec![der],
        Some(Object::Array(items)) => items
            .iter()
            .filter_map(|item| match doc.resolve(item).ok()? {
                Object::String(der) => Some(der),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn verdict(
    check: &Result<CmsCheck, String>,
    coverage: &Coverage,
    disallowed: &BTreeSet<Change>,
) -> Verdict {
    let check = match check {
        Ok(check) => check,
        Err(why) => return Verdict::Unknown(why.clone()),
    };
    if let Coverage::Invalid(why) = coverage {
        return Verdict::Invalid(why.clone());
    }
    if !check.digest_matches {
        return Verdict::Invalid("the document was altered after it was signed".to_owned());
    }
    match &check.signature {
        onionskin_crypto::signature::SignatureCheck::Invalid => {
            return Verdict::Invalid("the signer's key did not make it".to_owned())
        }
        onionskin_crypto::signature::SignatureCheck::Unsupported(what) => {
            return Verdict::Unknown(format!("{what} cannot be checked"))
        }
        onionskin_crypto::signature::SignatureCheck::Valid => {}
    }
    if !disallowed.is_empty() {
        return Verdict::Invalid(format!(
            "the certification does not allow the changes made after it ({})",
            labels(disallowed)
        ));
    }
    Verdict::Valid
}
