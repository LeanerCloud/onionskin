//! Signature fields, as the signatures pane lists them, and their
//! validation ([`validate`]).
//!
//! The listing is what the form dictionary states about itself, unverified.
//! Validation checks each signature's bytes and key and what was changed
//! after it.

mod changes;
mod validate;

pub use changes::Change;
pub(crate) use validate::validate;
pub use validate::{Coverage, Validation, Verdict};

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document as CosDocument, Object};

use crate::{Error, Result};

/// A field tree can nest; a hostile one can nest forever.
const MAX_DEPTH: usize = 32;
const MAX_FIELDS: usize = 10_000;

/// One signature form field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureField {
    /// The fully qualified field name, parents first, joined with `.` the way
    /// ISO 32000-2 12.7.4.2 builds it.
    pub name: String,
    /// Whether the field carries a `/V`, which is the file saying it has been
    /// signed. Not a claim that the signature verifies.
    pub signed: bool,
    /// `/V /Name`, the name the signer's own dictionary states. Unverified.
    pub signer: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    /// `/V /M`, the signing time as the file wrote it, in PDF date syntax.
    pub signed_at: Option<String>,
}

/// Read `/AcroForm /Fields`, depth first.
///
/// An absent `/AcroForm` is no signature fields. A present one that is not a
/// dictionary is an error: the file claims a form the reader cannot produce.
pub(crate) fn read(doc: &CosDocument) -> Result<Vec<SignatureField>> {
    Ok(walk(doc)?.found)
}

/// Every signed field's name and signature dictionary.
pub(crate) fn read_with_values(doc: &CosDocument) -> Result<Vec<(String, Dict)>> {
    Ok(walk(doc)?.values)
}

fn walk(doc: &CosDocument) -> Result<Walk<'_>> {
    let mut walk = Walk {
        doc,
        seen: BTreeSet::new(),
        found: Vec::new(),
        values: Vec::new(),
    };
    let catalog = doc.catalog()?;
    let Some(form) = catalog.get(b"AcroForm") else {
        return Ok(walk);
    };
    let form = doc.resolve(form)?;
    if matches!(form, Object::Null) {
        return Ok(walk);
    }
    let form = form.as_dict().cloned().ok_or_else(|| {
        Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: "/AcroForm does not resolve to a dictionary".into(),
        })
    })?;
    let Some(fields) = form.get(b"Fields") else {
        return Ok(walk);
    };
    let fields = doc.resolve(fields)?;
    let Some(fields) = fields.as_array().map(<[Object]>::to_vec) else {
        return Ok(walk);
    };
    for field in fields {
        walk.field(&field, "", None, 0, true)?;
    }
    Ok(walk)
}

/// The field-tree walk's own state: the document it reads, the objects it
/// has already been through, and what it has found.
struct Walk<'a> {
    doc: &'a CosDocument,
    seen: BTreeSet<u32>,
    found: Vec<SignatureField>,
    /// Each signed field's name and signature dictionary.
    values: Vec<(String, Dict)>,
}

impl Walk<'_> {
    /// One node of the field tree. `inherited` carries `/FT` down, because a
    /// parent may declare the type its kids share.
    fn field(
        &mut self,
        field: &Object,
        prefix: &str,
        inherited: Option<&[u8]>,
        depth: usize,
        is_root: bool,
    ) -> Result<()> {
        let doc = self.doc;
        if depth >= MAX_DEPTH || self.found.len() >= MAX_FIELDS {
            return Ok(());
        }
        if let Some(reference) = field.as_reference() {
            if !self.seen.insert(reference.number) {
                return Ok(());
            }
        }
        let field = doc.resolve(field)?;
        let Some(dict) = field.as_dict().cloned() else {
            return Ok(());
        };

        let partial = text(doc, &dict, b"T")?;
        let name = match (prefix.is_empty(), partial) {
            (_, None) => prefix.to_owned(),
            (true, Some(partial)) => partial,
            (false, Some(partial)) => format!("{prefix}.{partial}"),
        };
        let owned_type = dict
            .get(b"FT")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes().to_vec());
        let field_type = owned_type.as_deref().or(inherited);

        let kids = match dict.get(b"Kids") {
            Some(kids) => doc
                .resolve(kids)?
                .as_array()
                .map(<[Object]>::to_vec)
                .unwrap_or_default(),
            None => Vec::new(),
        };
        // Only a terminal field is a signature field, and only a field is: a
        // node with child fields under it is the branch above them, and the
        // widget annotations a terminal field carries as kids are appearances
        // of it rather than fields of their own. A partial name is what tells
        // the two apart (ISO 32000-2 12.7.4.2); an entry of `/AcroForm
        // /Fields` is a field whether or not it named itself.
        let terminal = !kids
            .iter()
            .any(|kid| self.is_child_field(kid).unwrap_or(false));
        let is_field_node = is_root || dict.contains(b"T");
        if terminal && is_field_node && field_type == Some(b"Sig".as_slice()) {
            let found = signature(doc, &dict, name.clone())?;
            self.found.push(found);
            if let Some(value) = dict.get(b"V") {
                if let Some(value) = doc.resolve(value)?.as_dict() {
                    self.values.push((name.clone(), value.clone()));
                }
            }
        }

        for kid in kids {
            self.field(&kid, &name, field_type, depth + 1, false)?;
        }
        Ok(())
    }

    /// Whether a `/Kids` entry is a child field rather than one of the
    /// terminal field's own widget annotations. A partial name is what makes
    /// it one.
    fn is_child_field(&self, kid: &Object) -> Result<bool> {
        Ok(self
            .doc
            .resolve(kid)?
            .as_dict()
            .is_some_and(|kid| kid.contains(b"T")))
    }
}

fn signature(doc: &CosDocument, field: &Dict, name: String) -> Result<SignatureField> {
    let value = match field.get(b"V") {
        Some(value) => doc.resolve(value)?,
        None => Object::Null,
    };
    let Some(value) = value.as_dict() else {
        return Ok(SignatureField {
            name,
            signed: false,
            signer: None,
            reason: None,
            location: None,
            signed_at: None,
        });
    };
    Ok(SignatureField {
        name,
        signed: true,
        signer: text(doc, value, b"Name")?,
        reason: text(doc, value, b"Reason")?,
        location: text(doc, value, b"Location")?,
        signed_at: text(doc, value, b"M")?,
    })
}

fn text(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Result<Option<String>> {
    let Some(value) = dict.get(key) else {
        return Ok(None);
    };
    match doc.resolve(value)? {
        Object::String(bytes) => Ok(Some(onionskin_content::pdf_text_string(&bytes))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testpdf::{dict, pages, pdf};

    fn open(bytes: Vec<u8>) -> CosDocument {
        onionskin_cos::Document::open_repairing(Box::new(onionskin_cos::BytesSource::new(bytes)))
            .expect("the fixture opens")
            .0
    }

    fn document(catalog: &str, tail: &[&str]) -> Vec<u8> {
        let (tree, page_bodies) = pages(3, 1);
        let mut objects = vec![dict(catalog), tree];
        objects.extend(page_bodies);
        objects.extend(tail.iter().map(|body| dict(body)));
        pdf(&objects)
    }

    #[test]
    fn a_signed_field_reports_what_the_signature_dictionary_states() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            &[
                "<< /FT /Sig /T (Approval) /V 5 0 R >>",
                "<< /Type /Sig /Name (Ada Lovelace) /Reason (Reviewed) \
                 /Location (London) /M (D:20260101120000Z) >>",
            ],
        ));

        let fields = read(&doc).expect("the signature fields read");

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "Approval");
        assert!(fields[0].signed);
        assert_eq!(fields[0].signer.as_deref(), Some("Ada Lovelace"));
        assert_eq!(fields[0].reason.as_deref(), Some("Reviewed"));
        assert_eq!(fields[0].location.as_deref(), Some("London"));
        assert_eq!(fields[0].signed_at.as_deref(), Some("D:20260101120000Z"));
    }

    /// An unsigned signature field is a placeholder waiting for a signature.
    /// It has to list, and it has to be distinguishable from a signed one.
    #[test]
    fn an_unsigned_field_lists_as_unsigned_with_nothing_claimed_about_it() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            &["<< /FT /Sig /T (Countersignature) >>"],
        ));

        let fields = read(&doc).expect("the signature fields read");

        assert_eq!(fields.len(), 1);
        assert!(!fields[0].signed);
        assert_eq!(fields[0].signer, None);
        assert_eq!(fields[0].signed_at, None);
    }

    /// `/FT` is inheritable and the qualified name is built from the
    /// parents, but only the leaf is a field. A reader that listed every
    /// node would show two branches that are not signature fields beside the
    /// one that is, both of them "not signed".
    #[test]
    fn an_inherited_field_type_is_found_on_the_leaf_under_its_qualified_name() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            &[
                "<< /T (form) /FT /Sig /Kids [5 0 R] >>",
                "<< /T (inner) /Kids [6 0 R] >>",
                "<< /T (leaf) >>",
            ],
        ));

        let names: Vec<_> = read(&doc)
            .expect("the signature fields read")
            .into_iter()
            .map(|field| field.name)
            .collect();

        assert_eq!(names, ["form.inner.leaf"]);
    }

    /// A terminal field merges its widget annotation into itself, or carries
    /// several of them as kids without partial names. Those are appearances
    /// of one field, not fields, so the field is still listed once.
    #[test]
    fn a_field_whose_kids_are_widgets_is_still_one_field() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            &[
                "<< /T (Approval) /FT /Sig /V 7 0 R /Kids [5 0 R 6 0 R] >>",
                "<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] >>",
                "<< /Type /Annot /Subtype /Widget /Rect [20 0 30 10] >>",
                "<< /Type /Sig /Name (Ada Lovelace) >>",
            ],
        ));

        let fields = read(&doc).expect("the signature fields read");

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "Approval");
        assert!(fields[0].signed);
    }

    /// Only `/FT /Sig` fields belong in this pane. A text field under the
    /// same form must not be listed as a signature.
    #[test]
    fn a_non_signature_field_is_not_listed() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R] >> >>",
            &["<< /FT /Tx /T (Name) >>", "<< /FT /Sig /T (Signature) >>"],
        ));

        let fields = read(&doc).expect("the signature fields read");

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "Signature");
    }

    /// A `/Kids` chain that points back at an ancestor terminates, and the
    /// terminal field beside the cycle is still found.
    #[test]
    fn a_kids_cycle_terminates_without_losing_the_field_beside_it() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            &[
                "<< /T (root) /FT /Sig /Kids [5 0 R 6 0 R] >>",
                "<< /T (leaf) >>",
                "<< /T (loop) /Kids [4 0 R] >>",
            ],
        ));

        let names: Vec<_> = read(&doc)
            .expect("the signature fields read")
            .into_iter()
            .map(|field| field.name)
            .collect();

        assert_eq!(names, ["root.leaf"]);
    }

    #[test]
    fn a_document_without_a_form_lists_nothing() {
        let doc = open(document("<< /Type /Catalog /Pages 2 0 R >>", &[]));

        assert!(read(&doc).expect("the signature fields read").is_empty());
    }
}
