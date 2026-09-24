//! Writing embedded files: the inverse of [`crate::attachments`].
//!
//! Two callers share it. Attach File as a comment points a `/FileAttachment`
//! annotation's `/FS` at the file specification [`embed_file`] writes, and the
//! Attachments pane (P13b) adds the same specification to the document's
//! `/Names /EmbeddedFiles` tree with [`add_to_attachments`].
//!
//! **A name is a file name, never a path.** `/UF` is a path in the format,
//! and a name carrying a directory - `../../.ssh/config` - is how a file
//! would suggest a place to write to a reader that trusts it. The reader side
//! strips separators from what it suggests; the writer refuses them, so this
//! program never puts such a name into a file.

use onionskin_cos::{flate_encode, Dict, Name, ObjRef, Object, Stream};

use crate::annots::{pdf_date, text_string};
use crate::edit::Transaction;
use crate::pages::{dict_at, page_ref, resolve};
use crate::{Error, Result};

/// How deep an existing name tree is followed when it is rewritten; the
/// same bound the reader walks with.
const MAX_DEPTH: usize = 32;

/// A file to embed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewAttachment<'a> {
    /// The file name, without any directory.
    pub name: &'a str,
    pub data: &'a [u8],
    /// The MIME type, written as the stream's `/Subtype`.
    pub mime: Option<&'a str>,
    pub description: Option<&'a str>,
}

/// Why a name cannot be an attachment's.
pub fn check_name(name: &str) -> Result<()> {
    let refused = name.trim().is_empty()
        || name.contains(['/', '\\', '\0'])
        || name.trim_matches('.').is_empty();
    if refused {
        return Err(Error::InvalidAttachmentName(name.to_owned()));
    }
    Ok(())
}

/// A MIME type from the file's extension, for the common ones; `None`
/// rather than a guess for the rest, which is what the format allows.
pub fn mime_for(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => return None,
    })
}

/// Write the embedded file stream and its file specification, returning the
/// specification. Nothing names it yet: the caller points an annotation or
/// the attachments tree at it, in the same transaction.
pub fn embed_file(tx: &mut Transaction<'_>, file: &NewAttachment<'_>, now: i64) -> Result<ObjRef> {
    check_name(file.name)?;
    let stream_number = tx.reserve();
    tx.put_object(stream_number, 0, Object::Stream(file_stream(file, now)))?;

    let stream = Object::Ref(ObjRef::new(stream_number, 0));
    let mut embedded = Dict::new();
    embedded.set(Name::new("F"), stream.clone());
    embedded.set(Name::new("UF"), stream);

    let mut spec = Dict::new();
    spec.set(Name::new("Type"), Object::name("Filespec"));
    spec.set(Name::new("F"), text_string(file.name));
    spec.set(Name::new("UF"), text_string(file.name));
    spec.set(Name::new("EF"), Object::Dict(embedded));
    if let Some(description) = file.description {
        spec.set(Name::new("Desc"), text_string(description));
    }
    let spec_number = tx.reserve();
    tx.put_object(spec_number, 0, Object::Dict(spec))?;
    Ok(ObjRef::new(spec_number, 0))
}

fn file_stream(file: &NewAttachment<'_>, now: i64) -> Stream {
    let mut params = Dict::new();
    params.set(Name::new("Size"), Object::Integer(file.data.len() as i64));
    params.set(
        Name::new("ModDate"),
        Object::String(pdf_date(now).into_bytes()),
    );
    let raw = flate_encode(file.data);
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("EmbeddedFile"));
    if let Some(mime) = file.mime {
        dict.set(Name::new("Subtype"), Object::Name(Name::new(mime)));
    }
    dict.set(Name::new("Params"), Object::Dict(params));
    dict.set(Name::new("Filter"), Object::name("FlateDecode"));
    dict.set(Name::new("Length"), Object::Integer(raw.len() as i64));
    Stream { dict, raw }
}

/// Name `spec` in the document's `/Names /EmbeddedFiles`, under `name` or,
/// if that key is taken, `name (2)`, `name (3)` and so on: two attachments
/// with one key would leave one of them unreachable. Returns the key used.
pub fn add_to_attachments(tx: &mut Transaction<'_>, name: &str, spec: ObjRef) -> Result<String> {
    check_name(name)?;
    let mut used = String::new();
    rewrite_attachments(tx, |entries| {
        used = unique_key(name, entries);
        entries.push((key_bytes(&used), Object::Ref(spec)));
        Ok(())
    })?;
    Ok(used)
}

/// Delete the embedded file whose stream is object `stream`: every
/// `/Names /EmbeddedFiles` entry naming it, and every file attachment comment
/// carrying it. Returns how many places named it; zero changes nothing.
///
/// The file specification and the stream stay in the file, unreferenced:
/// nothing is freed, and an undo puts the names back.
pub fn remove_attachment(tx: &mut Transaction<'_>, stream: u32) -> Result<usize> {
    let mut entries = Vec::new();
    read_entries(tx, &mut entries)?;
    let mut doomed = Vec::new();
    for entry in entries {
        if embeds(tx, &entry.1, stream)? {
            doomed.push(entry);
        }
    }
    if !doomed.is_empty() {
        rewrite_attachments(tx, |entries| {
            entries.retain(|entry| !doomed.contains(entry));
            Ok(())
        })?;
    }
    Ok(doomed.len() + remove_comments(tx, stream)?)
}

/// Every `/FileAttachment` comment carrying `stream`, taken off its page.
fn remove_comments(tx: &mut Transaction<'_>, stream: u32) -> Result<usize> {
    let mut removed = 0;
    for index in 0..crate::pages::page_count(tx)? {
        let page = page_ref(tx, index)?;
        let annots = match resolve(tx, dict_at(tx, page)?.get(b"Annots"))? {
            Some(Object::Array(annots)) => annots,
            _ => continue,
        };
        for annotation in annots.iter().filter_map(Object::as_reference) {
            let Ok(dict) = dict_at(tx, annotation) else {
                continue;
            };
            let is_attachment = dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|name| name.as_bytes() == b"FileAttachment");
            let carries = match dict.get(b"FS") {
                Some(spec) if is_attachment => embeds(tx, spec, stream)?,
                _ => false,
            };
            if carries && crate::annots::remove_annotation(tx, page, annotation)? {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Whether the file specification `spec` embeds object `stream`.
pub(crate) fn embeds(tx: &Transaction<'_>, spec: &Object, stream: u32) -> Result<bool> {
    let Some(Object::Dict(spec)) = resolve(tx, Some(spec))? else {
        return Ok(false);
    };
    let Some(Object::Dict(files)) = resolve(tx, spec.get(b"EF"))? else {
        return Ok(false);
    };
    Ok([b"UF".as_slice(), b"F"].into_iter().any(|key| {
        files
            .get(key)
            .and_then(Object::as_reference)
            .is_some_and(|objref| objref.number == stream)
    }))
}

/// The attachments name tree, read whole.
pub(crate) fn read_entries(
    tx: &Transaction<'_>,
    entries: &mut Vec<(Vec<u8>, Object)>,
) -> Result<()> {
    let catalog_ref = tx
        .trailer_value(b"Root")
        .and_then(|root| root.as_reference())
        .ok_or(Error::NoCatalog)?;
    let catalog = dict_at(tx, catalog_ref)?;
    let Some(Object::Dict(names)) = resolve(tx, catalog.get(b"Names"))? else {
        return Ok(());
    };
    if let Some(tree) = resolve(tx, names.get(b"EmbeddedFiles"))? {
        flatten(tx, &tree, 0, entries)?;
    }
    Ok(())
}

/// Read the attachments name tree whole, let `change` edit its entries, and
/// write it back as one sorted leaf. A tree built of `/Kids` is flattened in
/// the process, which changes its shape and nothing it resolves to.
pub(crate) fn rewrite_attachments(
    tx: &mut Transaction<'_>,
    change: impl FnOnce(&mut Vec<(Vec<u8>, Object)>) -> Result<()>,
) -> Result<()> {
    let catalog_ref = tx
        .trailer_value(b"Root")
        .and_then(|root| root.as_reference())
        .ok_or(Error::NoCatalog)?;
    let mut catalog = dict_at(tx, catalog_ref)?;
    let names_holder = catalog.get(b"Names").cloned();
    let mut names = match resolve(tx, names_holder.as_ref())? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    };
    let mut entries = Vec::new();
    read_entries(tx, &mut entries)?;
    change(&mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut leaf = Dict::new();
    leaf.set(
        Name::new("Names"),
        Object::Array(
            entries
                .into_iter()
                .flat_map(|(key, value)| [Object::String(key), value])
                .collect(),
        ),
    );
    let leaf_number = tx.reserve();
    tx.put_object(leaf_number, 0, Object::Dict(leaf))?;
    names.set(
        Name::new("EmbeddedFiles"),
        Object::Ref(ObjRef::new(leaf_number, 0)),
    );

    match names_holder {
        Some(Object::Ref(holder)) => {
            tx.put_object(holder.number, holder.generation, Object::Dict(names))?;
        }
        _ => {
            catalog.set(Name::new("Names"), Object::Dict(names));
            tx.put_object(
                catalog_ref.number,
                catalog_ref.generation,
                Object::Dict(catalog),
            )?;
        }
    }
    Ok(())
}

/// Every `(key, value)` of a name tree, leaves and kids alike.
fn flatten(
    tx: &Transaction<'_>,
    node: &Object,
    depth: usize,
    into: &mut Vec<(Vec<u8>, Object)>,
) -> Result<()> {
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    let Some(node) = node.as_dict() else {
        return Ok(());
    };
    if let Some(Object::Array(pairs)) = resolve(tx, node.get(b"Names"))? {
        for pair in pairs.as_chunks::<2>().0 {
            if let Object::String(key) = &pair[0] {
                into.push((key.clone(), pair[1].clone()));
            }
        }
    }
    if let Some(Object::Array(kids)) = resolve(tx, node.get(b"Kids"))? {
        for kid in &kids {
            if let Some(kid) = resolve(tx, Some(kid))? {
                flatten(tx, &kid, depth + 1, into)?;
            }
        }
    }
    Ok(())
}

/// A name tree key as the file stores it: a text string, so a name outside
/// ASCII reads back as itself.
fn key_bytes(key: &str) -> Vec<u8> {
    match text_string(key) {
        Object::String(bytes) => bytes,
        _ => unreachable!("a text string is a string"),
    }
}

/// `name`, or the first `name (n)` no entry has.
fn unique_key(name: &str, entries: &[(Vec<u8>, Object)]) -> String {
    let taken = |candidate: &str| {
        let encoded = key_bytes(candidate);
        entries.iter().any(|(key, _)| *key == encoded)
    };
    if !taken(name) {
        return name.to_owned();
    }
    (2..)
        .map(|index| format!("{name} ({index})"))
        .find(|candidate| !taken(candidate))
        .expect("an unbounded range has a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_a_path_is_refused() {
        for bad in [
            "",
            "  ",
            "..",
            ".",
            "a/b.txt",
            "..\\up.txt",
            "nul\0.txt",
            "../../x",
        ] {
            assert!(
                matches!(check_name(bad), Err(Error::InvalidAttachmentName(_))),
                "{bad:?}"
            );
        }
        for good in [
            "report.pdf",
            "Q3 figures (final).xlsx",
            ".profile",
            "naïve.txt",
        ] {
            assert!(check_name(good).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn a_mime_type_comes_from_a_known_extension_and_is_none_otherwise() {
        assert_eq!(mime_for("Report.PDF"), Some("application/pdf"));
        assert_eq!(mime_for("photo.jpeg"), Some("image/jpeg"));
        assert_eq!(mime_for("archive.7z"), None);
        assert_eq!(mime_for("README"), None);
    }

    #[test]
    fn a_taken_key_gets_the_next_free_number() {
        let entry = |key: &str| (key.as_bytes().to_vec(), Object::Null);
        assert_eq!(unique_key("a.txt", &[]), "a.txt");
        assert_eq!(
            unique_key("a.txt", &[entry("a.txt"), entry("a.txt (2)")]),
            "a.txt (3)"
        );
    }
}
