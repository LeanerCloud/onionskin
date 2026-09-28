//! The facts the Description tab shows about the file, which are read rather
//! than edited: the version, the page size, whether the document is tagged, and
//! whether it is linearized for fast web view.
//!
//! These are all things the file states and the user cannot change from this
//! tab, so they are read from the base rather than from the pending edits. A
//! catalog's `/Version` raises the effective version above the header's, which
//! is why both are read.

use onionskin_cos::Object;

/// What the Description tab reports about the document itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentFacts {
    /// The effective PDF version, from the catalog's `/Version` when it states
    /// one and the `%PDF-` header otherwise.
    pub version: Option<String>,
    /// The first page's size, as written.
    pub page_size: Option<String>,
    /// `/MarkInfo /Marked`: the document has a tag tree.
    pub tagged: bool,
    /// The first object carries `/Linearized`: fast web view.
    pub linearized: bool,
}

/// Reads the facts from a COS document. Every one is a best-effort read: a
/// document that states none of them still reports, it just reports less.
pub fn document_facts(doc: &onionskin_cos::Document) -> DocumentFacts {
    DocumentFacts {
        version: effective_version(doc),
        page_size: first_page_size(doc),
        tagged: is_tagged(doc),
        linearized: is_linearized(doc),
    }
}

/// A catalog's `/Version` outranks the header: ISO 32000-1 7.5.2 says the
/// catalog version applies to the whole file when the two disagree.
fn effective_version(doc: &onionskin_cos::Document) -> Option<String> {
    let declared = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Version").cloned())
        .and_then(|value| match value {
            Object::Name(name) => Some(String::from_utf8_lossy(&name.0).into_owned()),
            _ => None,
        });
    declared.or_else(|| doc.header_version())
}

fn first_page_size(doc: &onionskin_cos::Document) -> Option<String> {
    let box_ = doc.page(0).ok()?.media_box?;
    Some(format!(
        "{:.0} x {:.0}",
        box_[2] - box_[0],
        box_[3] - box_[1]
    ))
}

fn is_tagged(doc: &onionskin_cos::Document) -> bool {
    doc.catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"MarkInfo").cloned())
        .is_some_and(|mark| match mark {
            Object::Dict(dict) => matches!(dict.get(b"Marked"), Some(Object::Bool(true))),
            _ => false,
        })
}

/// Linearization puts its dictionary in the file's FIRST object, so this reads
/// object 1 rather than the catalog: a linearized file's first object is the
/// linearization dictionary, not the catalog.
fn is_linearized(doc: &onionskin_cos::Document) -> bool {
    doc.get(1).ok().is_some_and(|parsed| match &parsed.object {
        Object::Dict(dict) => dict.get(b"Linearized").is_some(),
        _ => false,
    })
}
