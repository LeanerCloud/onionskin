//! Document metadata: the `/Info` dictionary, the XMP packet, and the
//! initial view.
//!
//! **Two copies of the description, kept in step.** Title, author, subject
//! and keywords live in both `/Info` and the catalog's `/Metadata` XMP
//! packet, and readers disagree about which they trust. [`write_properties`]
//! writes both in the one transaction, and the round trip is asserted with
//! the two independent readers here - `/Info` as a dictionary, XMP as a
//! parsed packet - so a writer that skipped either would be caught by the
//! reader of the one it skipped.
//!
//! **A document with no `/Info`** gets one: a new dictionary and the
//! trailer's `/Info` key, which is a trailer edit, undone with the rest of
//! the transaction.

pub mod criteria;
pub mod fonts;
pub mod view;
pub mod xmp;

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

pub use criteria::{matches_all, CriterionError, PropertyCriterion, PropertyField, PropertyTest};
pub use fonts::{document_fonts, FontEntry};
pub use view::{read_initial_view, write_initial_view, InitialView, OpenFit, PageLayout, PageMode};
pub use xmp::XmpFields;

use crate::annots::{pdf_date, text_string};
use crate::edit::Transaction;
use crate::pages::{dict_at, resolve};
use crate::{Error, Result};

/// The four fields the Description tab edits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Description {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
}

/// What `/Info` says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub description: Description,
    /// `/Creator`, the application the document was made in.
    pub creator: Option<String>,
    /// `/Producer`, the program that wrote the PDF.
    pub producer: Option<String>,
    /// `/CreationDate` and `/ModDate`, as the file writes them.
    pub created: Option<String>,
    pub modified: Option<String>,
    /// Every other key with a text value: the Custom tab.
    pub custom: Vec<(String, String)>,
}

/// The keys `/Info` defines, which the Custom tab does not list.
const STANDARD_KEYS: [&str; 9] = [
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Creator",
    "Producer",
    "CreationDate",
    "ModDate",
    "Trapped",
];

/// Read `/Info`. A document without one has an empty `Info`.
pub fn read_info(doc: &CosDocument) -> Info {
    let Some(dict) = info_dict(doc) else {
        return Info::default();
    };
    let text = |key: &str| text_value(doc, dict.get(key.as_bytes()));
    let mut custom: Vec<(String, String)> = dict
        .iter()
        .filter_map(|(key, value)| {
            let key = String::from_utf8_lossy(key.as_bytes()).into_owned();
            if STANDARD_KEYS.contains(&key.as_str()) {
                return None;
            }
            text_value(doc, Some(value)).map(|value| (key, value))
        })
        .collect();
    custom.sort();
    Info {
        description: Description {
            title: text("Title"),
            author: text("Author"),
            subject: text("Subject"),
            keywords: text("Keywords"),
        },
        creator: text("Creator"),
        producer: text("Producer"),
        created: text("CreationDate"),
        modified: text("ModDate"),
        custom,
    }
}

fn info_dict(doc: &CosDocument) -> Option<Dict> {
    let info = doc.trailer().get(b"Info")?.clone();
    match doc.resolve(&info).ok()? {
        Object::Dict(dict) => Some(dict),
        _ => None,
    }
}

fn text_value(doc: &CosDocument, value: Option<&Object>) -> Option<String> {
    match doc.resolve(value?).ok()? {
        Object::String(bytes) => Some(onionskin_content::pdf_text_string(&bytes)),
        _ => None,
    }
}

/// Read the catalog's XMP packet: `Ok(None)` when there is none, or it is
/// not a packet this reader can parse.
pub fn read_xmp(doc: &CosDocument) -> Result<Option<XmpFields>> {
    let catalog = doc.catalog()?;
    let Some(metadata) = catalog.get(b"Metadata") else {
        return Ok(None);
    };
    let Object::Stream(stream) = doc.resolve(metadata)? else {
        return Ok(None);
    };
    Ok(xmp::read(&doc.decode_stream(&stream)?))
}

/// What the Description and Custom tabs change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropertiesEdit {
    pub description: Description,
    /// The Custom tab's keys, all of them: a key missing here that `/Info`
    /// had is removed.
    pub custom: Vec<(String, String)>,
}

/// Why a custom key cannot be written.
fn check_custom_key(key: &str) -> Result<()> {
    let refused = key.trim().is_empty()
        || STANDARD_KEYS.contains(&key)
        || key.chars().any(|c| c.is_whitespace() || c.is_control());
    if refused {
        return Err(Error::InvalidMetadataKey(key.to_owned()));
    }
    Ok(())
}

/// Write the description and custom keys to `/Info`, and the description to
/// the XMP packet, stamping both with `now` as the modification date.
pub fn write_properties(tx: &mut Transaction<'_>, edit: &PropertiesEdit, now: i64) -> Result<()> {
    for (key, _) in &edit.custom {
        check_custom_key(key)?;
    }
    let modified = pdf_date(now);
    let info = write_info(tx, edit, &modified)?;
    write_xmp(tx, edit, &info, &modified)
}

/// The new `/Info`, returned for the XMP writer to take the dates from.
fn write_info(tx: &mut Transaction<'_>, edit: &PropertiesEdit, modified: &str) -> Result<Dict> {
    let existing = tx.trailer_value(b"Info");
    let mut dict = match resolve(tx, existing.as_ref())? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    };
    let description = &edit.description;
    for (key, value) in [
        ("Title", &description.title),
        ("Author", &description.author),
        ("Subject", &description.subject),
        ("Keywords", &description.keywords),
    ] {
        set_text(&mut dict, key, value.as_deref());
    }
    let stale: Vec<Name> = dict
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| !STANDARD_KEYS.contains(&String::from_utf8_lossy(key.as_bytes()).as_ref()))
        .collect();
    for key in stale {
        dict.remove(key.as_bytes());
    }
    for (key, value) in &edit.custom {
        set_text(&mut dict, key, Some(value));
    }
    dict.set(
        Name::new("ModDate"),
        Object::String(modified.as_bytes().to_vec()),
    );

    match existing {
        Some(Object::Ref(holder)) => {
            tx.put_object(holder.number, holder.generation, Object::Dict(dict.clone()))?;
        }
        _ => {
            // No `/Info`, or a direct one: a new object, and the trailer
            // pointed at it. The trailer key is part of the transaction, so
            // an undo removes it again.
            let number = tx.reserve();
            tx.put_object(number, 0, Object::Dict(dict.clone()))?;
            tx.set_trailer(Name::new("Info"), Some(Object::Ref(ObjRef::new(number, 0))))?;
        }
    }
    Ok(dict)
}

/// A field as both copies store it: trimmed, and absent when blank, so
/// `/Info` and XMP cannot disagree about a value made of spaces.
fn normalized(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn set_text(dict: &mut Dict, key: &str, value: Option<&str>) {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => dict.set(Name::new(key), text_string(value)),
        None => {
            dict.remove(key.as_bytes());
        }
    }
}

/// The catalog's `/Metadata`, rewritten from the description, keeping every
/// property of the old packet this module does not own.
fn write_xmp(
    tx: &mut Transaction<'_>,
    edit: &PropertiesEdit,
    info: &Dict,
    modified: &str,
) -> Result<()> {
    let catalog_ref = tx
        .trailer_value(b"Root")
        .and_then(|root| root.as_reference())
        .ok_or(Error::NoCatalog)?;
    let mut catalog = dict_at(tx, catalog_ref)?;
    let previous = match resolve(tx, catalog.get(b"Metadata"))? {
        Some(Object::Stream(stream)) => tx.base().decode_stream(&stream).ok(),
        _ => None,
    };
    let old = previous.as_deref().and_then(xmp::read).unwrap_or_default();
    let info_text = |key: &[u8]| match info.get(key) {
        Some(Object::String(bytes)) => Some(onionskin_content::pdf_text_string(bytes)),
        _ => None,
    };
    let description = &edit.description;
    let fields = XmpFields {
        title: normalized(&description.title),
        authors: description
            .author
            .iter()
            .flat_map(|author| author.split(';'))
            .map(str::trim)
            .filter(|author| !author.is_empty())
            .map(str::to_owned)
            .collect(),
        subject: normalized(&description.subject),
        keywords: normalized(&description.keywords),
        producer: info_text(b"Producer").or(old.producer),
        creator_tool: info_text(b"Creator").or(old.creator_tool),
        created: info_text(b"CreationDate")
            .and_then(|date| xmp::xmp_date(&date))
            .or(old.created),
        modified: xmp::xmp_date(modified),
    };
    let packet = xmp::write(&fields, previous.as_deref());

    // Uncompressed, as XMP is meant to be stored: a tool that scans a file
    // for its packet without parsing the PDF finds it.
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("Metadata"));
    dict.set(Name::new("Subtype"), Object::name("XML"));
    dict.set(Name::new("Length"), Object::Integer(packet.len() as i64));
    let number = tx.reserve();
    tx.put_object(number, 0, Object::Stream(Stream { dict, raw: packet }))?;
    catalog.set(Name::new("Metadata"), Object::Ref(ObjRef::new(number, 0)));
    tx.put_object(
        catalog_ref.number,
        catalog_ref.generation,
        Object::Dict(catalog),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_key_cannot_be_a_standard_one_or_hold_spaces() {
        for bad in ["", "Title", "Two words", "tab\tkey"] {
            assert!(
                matches!(check_custom_key(bad), Err(Error::InvalidMetadataKey(_))),
                "{bad:?}"
            );
        }
        assert!(check_custom_key("Department").is_ok());
    }
}
