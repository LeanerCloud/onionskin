//! Embedded files, as the attachments pane lists them.
//!
//! Listing and reading only. Nothing here writes: extraction hands the
//! decoded bytes back and the caller decides where they go, so a reader
//! cannot put a file anywhere the user did not choose.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document as CosDocument, Object};

use crate::{Error, Result};

/// A name tree can nest; a hostile one can nest forever.
const MAX_DEPTH: usize = 32;
/// Enough for every real file; a bound so a crafted tree cannot make the
/// pane grow without end.
const MAX_ATTACHMENTS: usize = 10_000;

/// One embedded file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// The name the file gave it, from the name tree key or `/UF`/`/F`.
    pub name: String,
    pub description: Option<String>,
    /// `/Params /Size`, when the file states one. Absent means the file said
    /// nothing, not zero bytes.
    pub size: Option<u64>,
    /// `/Subtype` on the embedded file stream, the file's own MIME type.
    pub mime: Option<String>,
    /// The object the embedded file stream lives in, which is what
    /// [`read_bytes`] extracts. Public for the same reason every other
    /// object in this workspace keeps its provenance: a caller that has to
    /// say which object a file came from should not have to guess.
    pub stream: u32,
}

impl Attachment {
    /// The name with any directory separators removed, for a save dialog to
    /// suggest.
    ///
    /// `/UF` is a path, and a file naming its attachment `../../etc/passwd`
    /// must not be able to suggest that path to a dialog. Extraction writes
    /// wherever the caller says regardless; this only keeps the suggestion
    /// from carrying a traversal the user would have to notice and undo.
    pub fn file_name(&self) -> String {
        let last = self
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .trim_matches('.');
        if last.is_empty() {
            "attachment".to_owned()
        } else {
            last.to_owned()
        }
    }
}

/// Read `/Names /EmbeddedFiles` in name-tree order.
///
/// An absent tree is no attachments. A present one that is not a dictionary
/// is an error: the file claims attachments the reader cannot produce.
pub(crate) fn read(doc: &CosDocument) -> Result<Vec<Attachment>> {
    let catalog = doc.catalog()?;
    let Some(names) = catalog.get(b"Names") else {
        return Ok(Vec::new());
    };
    let names = doc.resolve(names)?;
    let Some(names) = names.as_dict() else {
        return Ok(Vec::new());
    };
    let Some(embedded) = names.get(b"EmbeddedFiles") else {
        return Ok(Vec::new());
    };
    let embedded = doc.resolve(embedded)?;
    if matches!(embedded, Object::Null) {
        return Ok(Vec::new());
    }
    if embedded.as_dict().is_none() {
        return Err(Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: "/Names /EmbeddedFiles does not resolve to a dictionary".into(),
        }));
    }

    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    walk(doc, &embedded, 0, &mut seen, &mut found)?;
    Ok(found)
}

/// The decoded bytes of one attachment.
///
/// Takes an [`Attachment`] the caller got from [`read`] rather than a name or
/// an object number, so nothing outside this module can ask for a stream the
/// list never showed.
pub(crate) fn read_bytes(doc: &CosDocument, attachment: &Attachment) -> Result<Vec<u8>> {
    let parsed = doc.get(attachment.stream)?;
    let stream = parsed.object.as_stream().ok_or_else(|| {
        Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: format!(
                "attachment {} is not an embedded file stream",
                attachment.name
            ),
        })
    })?;
    Ok(doc.decode_stream(stream)?)
}

fn walk(
    doc: &CosDocument,
    node: &Object,
    depth: usize,
    seen: &mut BTreeSet<u32>,
    found: &mut Vec<Attachment>,
) -> Result<()> {
    if depth >= MAX_DEPTH || found.len() >= MAX_ATTACHMENTS {
        return Ok(());
    }
    let Some(node) = node.as_dict().cloned() else {
        return Ok(());
    };

    if let Some(entries) = node.get(b"Names") {
        let entries = doc.resolve(entries)?;
        if let Some(entries) = entries.as_array() {
            for pair in entries.as_chunks::<2>().0 {
                if found.len() >= MAX_ATTACHMENTS {
                    return Ok(());
                }
                let key = match &pair[0] {
                    Object::String(name) => onionskin_content::pdf_text_string(name),
                    _ => continue,
                };
                let spec = doc.resolve(&pair[1])?;
                if let Some(attachment) = file_spec(doc, key, spec.as_dict())? {
                    found.push(attachment);
                }
            }
        }
    }

    let Some(kids) = node.get(b"Kids") else {
        return Ok(());
    };
    let kids = doc.resolve(kids)?;
    let Some(kids) = kids.as_array().map(<[Object]>::to_vec) else {
        return Ok(());
    };
    for kid in kids {
        // A kid listed twice, or a tree pointing back at an ancestor, would
        // otherwise repeat every attachment under it.
        if let Some(reference) = kid.as_reference() {
            if !seen.insert(reference.number) {
                continue;
            }
        }
        let kid = doc.resolve(&kid)?;
        walk(doc, &kid, depth + 1, seen, found)?;
    }
    Ok(())
}

/// One `/Filespec`, if it carries an embedded file stream. A spec naming only
/// an external path is not an attachment: there is nothing in this document
/// to extract.
fn file_spec(doc: &CosDocument, key: String, spec: Option<&Dict>) -> Result<Option<Attachment>> {
    let Some(spec) = spec else {
        return Ok(None);
    };
    let Some(ef) = spec.get(b"EF") else {
        return Ok(None);
    };
    let ef = doc.resolve(ef)?;
    let Some(ef) = ef.as_dict() else {
        return Ok(None);
    };
    // `/UF` is the Unicode name and takes precedence over `/F`, per
    // ISO 32000-2 7.11.3; the name tree key is the fallback.
    let Some(stream) = ef
        .get(b"UF")
        .or_else(|| ef.get(b"F"))
        .and_then(Object::as_reference)
    else {
        return Ok(None);
    };
    let name = text(doc, spec, b"UF")?
        .or(text(doc, spec, b"F")?)
        .unwrap_or(key);
    let description = text(doc, spec, b"Desc")?;

    let parsed = doc.get(stream.number)?;
    let Some(dict) = parsed.object.as_dict() else {
        return Ok(None);
    };
    let mime = dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned());
    let size = match dict.get(b"Params") {
        Some(params) => doc
            .resolve(params)?
            .as_dict()
            .and_then(|params| params.get(b"Size").cloned())
            .and_then(|size| doc.resolve(&size).ok())
            .and_then(|size| size.as_integer())
            .and_then(|size| u64::try_from(size).ok()),
        None => None,
    };

    Ok(Some(Attachment {
        name,
        description,
        size,
        mime,
        stream: stream.number,
    }))
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
    use crate::testpdf::{dict, pages, pdf, stream};

    fn open(bytes: Vec<u8>) -> CosDocument {
        onionskin_cos::Document::open_repairing(Box::new(onionskin_cos::BytesSource::new(bytes)))
            .expect("the fixture opens")
            .0
    }

    /// Objects 1 and 2 are the catalog and the page tree root, 3 is the one
    /// page, and the rest are written by the test.
    fn document(catalog: &str, tail: Vec<Vec<u8>>) -> Vec<u8> {
        let (tree, page_bodies) = pages(3, 1);
        let mut objects = vec![dict(catalog), tree];
        objects.extend(page_bodies);
        objects.extend(tail);
        pdf(&objects)
    }

    fn one_attachment_document() -> Vec<u8> {
        document(
            "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 4 0 R >> >>",
            vec![
                dict("<< /Names [(notes.txt) 5 0 R] >>"),
                dict(
                    "<< /Type /Filespec /F (notes.txt) /UF (notes.txt) \
                     /Desc (Reviewer notes) /EF << /F 6 0 R >> >>",
                ),
                stream(
                    "/Type /EmbeddedFile /Subtype /text#2Fplain /Params << /Size 11 >>",
                    b"hello world",
                ),
            ],
        )
    }

    #[test]
    fn an_embedded_file_lists_with_the_metadata_the_file_states() {
        let doc = open(one_attachment_document());

        let attachments = read(&doc).expect("the attachments read");

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "notes.txt");
        assert_eq!(
            attachments[0].description.as_deref(),
            Some("Reviewer notes")
        );
        assert_eq!(attachments[0].size, Some(11));
        assert_eq!(attachments[0].mime.as_deref(), Some("text/plain"));
    }

    #[test]
    fn extraction_returns_the_streams_decoded_bytes() {
        let doc = open(one_attachment_document());
        let attachments = read(&doc).expect("the attachments read");

        let bytes = read_bytes(&doc, &attachments[0]).expect("the attachment extracts");

        assert_eq!(bytes, b"hello world");
    }

    /// A spec with no `/EF` names a file on someone else's disk. Listing it
    /// would offer a Save that has nothing to save.
    #[test]
    fn a_file_spec_without_an_embedded_stream_is_not_an_attachment() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 4 0 R >> >>",
            vec![
                dict("<< /Names [(external.txt) 5 0 R (real.txt) 6 0 R] >>"),
                dict("<< /Type /Filespec /F (/tmp/external.txt) >>"),
                dict("<< /Type /Filespec /F (real.txt) /EF << /F 7 0 R >> >>"),
                stream("/Type /EmbeddedFile", b"data"),
            ],
        ));

        let attachments = read(&doc).expect("the attachments read");

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "real.txt");
    }

    #[test]
    fn a_name_tree_with_kids_lists_every_leaf_once_and_terminates_on_a_cycle() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 4 0 R >> >>",
            vec![
                // The root lists one kid twice and a second kid; the repeat
                // must not list its attachment twice.
                dict("<< /Kids [5 0 R 5 0 R 6 0 R] >>"),
                dict("<< /Names [(one.bin) 7 0 R] >>"),
                dict("<< /Names [(two.bin) 8 0 R] /Kids [4 0 R] >>"),
                dict("<< /Type /Filespec /F (one.bin) /EF << /F 9 0 R >> >>"),
                dict("<< /Type /Filespec /F (two.bin) /EF << /F 9 0 R >> >>"),
                stream("/Type /EmbeddedFile", b"shared"),
            ],
        ));

        let attachments = read(&doc).expect("the attachments read");

        assert_eq!(
            attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>(),
            ["one.bin", "two.bin"]
        );
    }

    /// A save dialog is offered the last path component, so a name carrying a
    /// traversal cannot suggest a destination outside the folder the user
    /// picked.
    #[test]
    fn a_traversing_name_suggests_only_its_last_component() {
        let traversing = Attachment {
            name: "../../etc/passwd".to_owned(),
            description: None,
            size: None,
            mime: None,
            stream: 1,
        };
        let dotted = Attachment {
            name: "..".to_owned(),
            ..traversing.clone()
        };
        let windows = Attachment {
            name: r"..\..\windows\system32\config".to_owned(),
            ..traversing.clone()
        };

        assert_eq!(traversing.file_name(), "passwd");
        assert_eq!(dotted.file_name(), "attachment");
        assert_eq!(windows.file_name(), "config");
    }

    #[test]
    fn a_document_without_embedded_files_lists_nothing() {
        let doc = open(document("<< /Type /Catalog /Pages 2 0 R >>", Vec::new()));

        assert!(read(&doc).expect("the attachments read").is_empty());
    }
}
