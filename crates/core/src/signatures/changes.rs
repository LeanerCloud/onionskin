//! What changed after a signature: every object a later section added,
//! changed or freed, sorted into the kinds of change a certification
//! signature's permissions are written in (ISO 32000-2 12.8.2.2).

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document as CosDocument, Object, XrefEntry};

/// One kind of change made after a signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    /// A comment added, changed or deleted, or a page's list of them.
    Annotation,
    /// A form field filled in, or the form's list of fields.
    FormField,
    /// Another signature, or a document timestamp.
    Signature,
    /// Validation data kept for later (`/DSS`), which no permission forbids.
    ValidationData,
    /// The document information dictionary, the XMP metadata, or the PDF
    /// version the catalog declares.
    Metadata,
    /// A new appearance's fonts and other resources, which go with the
    /// comment, field or signature that draws them.
    Appearance,
    /// Anything else: content, pages, structure.
    Other,
}

impl Change {
    pub fn label(self) -> &'static str {
        match self {
            Change::Annotation => "comments",
            Change::FormField => "form fields",
            Change::Signature => "signatures",
            Change::ValidationData => "validation data",
            Change::Metadata => "document information",
            Change::Appearance => "appearances",
            Change::Other => "other changes",
        }
    }

    /// Whether a certification signature with `/P` `permission` allows it:
    /// 1 nothing but validation data, 2 also form filling and signing, 3
    /// also comments. Metadata travels with whatever else is allowed.
    pub fn allowed_by(self, permission: u8) -> bool {
        match self {
            Change::ValidationData => true,
            Change::FormField | Change::Signature | Change::Metadata | Change::Appearance => {
                permission >= 2
            }
            Change::Annotation => permission >= 3,
            Change::Other => false,
        }
    }
}

/// The kinds of change between `signed`, the file as it was signed, and
/// `current`, the file as it is now.
pub(crate) fn changes(signed: &CosDocument, current: &CosDocument) -> BTreeSet<Change> {
    let info = current
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .map(|info| info.number);
    let root = current
        .trailer()
        .get(b"Root")
        .and_then(Object::as_reference)
        .map(|root| root.number);
    let mut found = BTreeSet::new();
    for (number, entry) in current.xref().iter() {
        if number == 0 || signed.xref().get(number) == Some(entry) {
            continue;
        }
        let before = signed.get(number).ok().map(|parsed| parsed.object);
        let after = match entry {
            XrefEntry::Free { .. } => None,
            _ => current.get(number).ok().map(|parsed| parsed.object),
        };
        let change = if Some(number) == info {
            Some(Change::Metadata)
        } else if Some(number) == root {
            catalog_change(before.as_ref(), after.as_ref())
        } else {
            classify(before.as_ref(), after.as_ref())
        };
        if change == Some(Change::Other) {
            eprintln!("OTHER {number}: {:?} -> {:?}", before, after);
        }
        found.extend(change);
    }
    found
}

/// What an object's change is, from what it was and what it is.
fn classify(before: Option<&Object>, after: Option<&Object>) -> Option<Change> {
    let object = after.or(before)?;
    let dict = match object {
        Object::Dict(dict) => dict,
        Object::Stream(stream) => &stream.dict,
        // A bare array or number is part of something: the field list, a
        // page's comments, a stream's length.
        _ => return is_reference_list(object).then_some(Change::Annotation),
    };
    Some(dict_change(dict, before, after))
}

fn is_type(dict: &Dict, name: &[u8]) -> bool {
    dict.get(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|found| found.as_bytes() == name)
}

fn subtype(dict: &Dict) -> Option<&[u8]> {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .map(|found| found.as_bytes())
}

fn dict_change(dict: &Dict, before: Option<&Object>, after: Option<&Object>) -> Change {
    if is_type(dict, b"Sig") || is_type(dict, b"DocTimeStamp") {
        return Change::Signature;
    }
    if is_type(dict, b"XRef") || is_type(dict, b"ObjStm") {
        return Change::ValidationData;
    }
    let is_field = dict.contains(b"FT") || dict.contains(b"Fields");
    if is_field || subtype(dict) == Some(b"Widget") {
        let signs = dict
            .get(b"FT")
            .and_then(Object::as_name)
            .is_some_and(|found| found.as_bytes() == b"Sig");
        return if signs {
            Change::Signature
        } else {
            Change::FormField
        };
    }
    if is_type(dict, b"Annot") || (dict.contains(b"Rect") && subtype(dict).is_some()) {
        return Change::Annotation;
    }
    // A form XObject new in the section is an appearance: of a comment or a
    // field, which the object that uses it is counted as.
    if subtype(dict) == Some(b"Form") && before.is_none() {
        return Change::Annotation;
    }
    if is_type(dict, b"Page") {
        return page_change(before, after);
    }
    if is_type(dict, b"Metadata") {
        return Change::Metadata;
    }
    if before.is_none() && is_resource(dict) {
        return Change::Appearance;
    }
    if dict.contains(b"Certs") || dict.contains(b"VRI") || dict.contains(b"OCSPs") {
        return Change::ValidationData;
    }
    Change::Other
}

/// A font, a font's parts, an image or a graphics state: what a new
/// appearance draws with.
fn is_resource(dict: &Dict) -> bool {
    [
        &b"Font"[..],
        b"FontDescriptor",
        b"XObject",
        b"ExtGState",
        b"Encoding",
    ]
    .iter()
    .any(|kind| is_type(dict, kind))
        || dict.contains(b"Length1")
        || subtype(dict) == Some(b"Image")
}

/// A page whose only change is its `/Annots` is a change to its comments.
fn page_change(before: Option<&Object>, after: Option<&Object>) -> Change {
    let (Some(Object::Dict(before)), Some(Object::Dict(after))) = (before, after) else {
        return Change::Other;
    };
    if without(before, &[b"Annots"]) == without(after, &[b"Annots"]) {
        Change::Annotation
    } else {
        Change::Other
    }
}

/// The catalog, by the keys that changed: the form, validation data, a
/// certification's permissions, the metadata and the declared version each
/// count as themselves, anything else as a change of its own.
fn catalog_change(before: Option<&Object>, after: Option<&Object>) -> Option<Change> {
    let (Some(Object::Dict(before)), Some(Object::Dict(after))) = (before, after) else {
        return Some(Change::Other);
    };
    const KEYS: [(&[u8], Change); 6] = [
        (b"AcroForm", Change::FormField),
        (b"DSS", Change::ValidationData),
        (b"Perms", Change::Signature),
        (b"Metadata", Change::Metadata),
        (b"Version", Change::Metadata),
        (b"Extensions", Change::Metadata),
    ];
    let keys: Vec<&[u8]> = KEYS.iter().map(|(key, _)| *key).collect();
    if without(before, &keys) != without(after, &keys) {
        return Some(Change::Other);
    }
    // The strictest of the changes it made stands for them all.
    KEYS.iter()
        .filter(|(key, _)| before.get(key) != after.get(key))
        .map(|(_, change)| *change)
        .max_by_key(|change| strictness(*change))
}

/// How few permissions allow a change: a catalog change is judged by the
/// one that needs the most.
fn strictness(change: Change) -> u8 {
    (1..=3)
        .find(|permission| change.allowed_by(*permission))
        .unwrap_or(4)
}

fn without(dict: &Dict, keys: &[&[u8]]) -> Vec<(Vec<u8>, Object)> {
    dict.iter()
        .filter(|(key, _)| !keys.contains(&key.as_bytes()))
        .map(|(key, value)| (key.as_bytes().to_vec(), value.clone()))
        .collect()
}

fn is_reference_list(object: &Object) -> bool {
    match object {
        Object::Array(items) => items.iter().all(|item| item.as_reference().is_some()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_cos::Name;

    fn dict(entries: &[(&str, Object)]) -> Object {
        let mut dict = Dict::new();
        for (key, value) in entries {
            dict.set(Name::new(key), value.clone());
        }
        Object::Dict(dict)
    }

    #[test]
    fn each_kind_of_object_is_its_kind_of_change() {
        let name = Object::name;
        let cases = [
            (dict(&[("Type", name("Sig"))]), Change::Signature),
            (dict(&[("FT", name("Sig"))]), Change::Signature),
            (dict(&[("FT", name("Tx"))]), Change::FormField),
            (dict(&[("Subtype", name("Widget"))]), Change::FormField),
            (
                dict(&[("Type", name("Annot")), ("Subtype", name("Text"))]),
                Change::Annotation,
            ),
            (
                dict(&[("Certs", Object::Array(vec![]))]),
                Change::ValidationData,
            ),
            (dict(&[("Type", name("XRef"))]), Change::ValidationData),
            (dict(&[("Length", Object::Integer(3))]), Change::Other),
        ];
        for (object, change) in cases {
            assert_eq!(classify(None, Some(&object)), Some(change), "{object:?}");
        }
        for resource in [
            dict(&[("Type", name("FontDescriptor"))]),
            dict(&[("Type", name("Font"))]),
            dict(&[("Length1", Object::Integer(9))]),
            dict(&[("Subtype", name("Image"))]),
        ] {
            assert_eq!(classify(None, Some(&resource)), Some(Change::Appearance));
            assert_eq!(
                classify(Some(&resource), Some(&resource)),
                Some(Change::Other)
            );
        }
        let metadata = dict(&[("Type", name("Metadata"))]);
        assert_eq!(classify(None, Some(&metadata)), Some(Change::Metadata));
        let form = dict(&[("Subtype", name("Form"))]);
        assert_eq!(classify(None, Some(&form)), Some(Change::Annotation));
        assert_eq!(classify(Some(&form), Some(&form)), Some(Change::Other));
        let refs = Object::Array(vec![Object::Ref(onionskin_cos::ObjRef::new(4, 0))]);
        assert_eq!(classify(None, Some(&refs)), Some(Change::Annotation));
        assert_eq!(classify(None, Some(&Object::Integer(3))), None);
        assert_eq!(classify(None, None), None);
    }

    #[test]
    fn a_page_is_a_comment_change_only_when_its_comments_are_all_that_changed() {
        let page = |annots: bool, rotate: i64| {
            let mut entries = vec![
                ("Type", Object::name("Page")),
                ("Rotate", Object::Integer(rotate)),
            ];
            if annots {
                entries.push(("Annots", Object::Array(vec![])));
            }
            dict(&entries)
        };
        assert_eq!(
            page_change(Some(&page(false, 0)), Some(&page(true, 0))),
            Change::Annotation
        );
        assert_eq!(
            page_change(Some(&page(false, 0)), Some(&page(false, 90))),
            Change::Other
        );
        assert_eq!(page_change(None, Some(&page(true, 0))), Change::Other);
    }

    #[test]
    fn the_catalog_changes_by_the_keys_that_changed() {
        let catalog = |form: i64, dss: i64, other: i64| {
            dict(&[
                ("AcroForm", Object::Integer(form)),
                ("DSS", Object::Integer(dss)),
                ("PageMode", Object::Integer(other)),
            ])
        };
        let base = catalog(0, 0, 0);
        assert_eq!(
            catalog_change(Some(&base), Some(&catalog(1, 0, 0))),
            Some(Change::FormField)
        );
        assert_eq!(
            catalog_change(Some(&base), Some(&catalog(0, 1, 0))),
            Some(Change::ValidationData)
        );
        assert_eq!(
            catalog_change(Some(&base), Some(&catalog(0, 0, 1))),
            Some(Change::Other)
        );
        assert_eq!(catalog_change(Some(&base), Some(&base)), None);
        let mut versioned = match catalog(1, 1, 0) {
            Object::Dict(dict) => dict,
            _ => unreachable!(),
        };
        versioned.set(Name::new("Version"), Object::name("2.0"));
        let judged = catalog_change(Some(&base), Some(&Object::Dict(versioned)));
        assert_eq!(
            judged.map(strictness),
            Some(2),
            "the strictest of form fields, validation data and the version: {judged:?}"
        );
        assert_eq!(catalog_change(None, Some(&base)), Some(Change::Other));
    }

    #[test]
    fn certification_permissions_allow_what_iso_32000_says() {
        assert!(Change::ValidationData.allowed_by(1));
        assert!(!Change::FormField.allowed_by(1));
        assert!(Change::FormField.allowed_by(2) && Change::Signature.allowed_by(2));
        assert!(!Change::Annotation.allowed_by(2) && Change::Annotation.allowed_by(3));
        assert!(!Change::Other.allowed_by(3));
        assert!(Change::Appearance.allowed_by(2) && !Change::Appearance.allowed_by(1));
        assert_eq!(strictness(Change::Annotation), 3);
        assert_eq!(strictness(Change::Other), 4);
        assert_eq!(Change::Metadata.label(), "document information");
    }
}
