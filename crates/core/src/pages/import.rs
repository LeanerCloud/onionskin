//! Copying pages in from another document, transitively.
//!
//! **The one genuinely new piece of machinery in page organization**, and the
//! place a shallow implementation hides: copy the page dictionary and not its
//! content streams, fonts and images, and every structural check passes - the
//! page is there, its `/Parent` is right, nothing dangles, because the copied
//! references still point at numbers that exist in the destination - while it
//! renders someone else's objects or nothing at all. Only a render comparison
//! against the source catches that, which is what the importer's test does.
//!
//! # What is copied
//!
//! The page dictionary with its inheritance materialized, and **everything it
//! reaches**: content streams, resources, fonts and their programs, images,
//! form XObjects, annotations and their appearance streams. Each gets a fresh
//! number from the destination's overlay, and every reference inside every
//! copied object is rewritten through the same map. A number is reserved on
//! first sight and before its object is read, so a cycle - a form XObject whose
//! resources name the form itself, which is legal and common - terminates.
//!
//! # What is not
//!
//! - **Other pages of the source.** A link on an imported page pointing at a
//!   page that was not imported, or an annotation's `/P` naming one, becomes
//!   `null` rather than pulling the source's whole page tree in behind it.
//! - **The source's page tree.** `/Parent` is replaced by whoever places the
//!   page; it is never followed.
//! - **Article beads** (`/B`) and **structure keys** (`/StructParents`,
//!   `/StructParent`): both index into structures of the source document -
//!   its threads and its `/ParentTree` - that do not come with the page.
//!
//! # Where it writes
//!
//! **The overlay, never `cos`'s own edit map.** Every object is written with
//! [`Transaction::put_object`], inside the caller's one transaction, so an
//! import is one undo entry that takes every object back, and `section_for` sees
//! all of them. `cos::Document::add_object` would have put them where the save
//! path does not look.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use super::inherit::walk;
use crate::edit::Transaction;
use crate::{Error, Result};

/// Where copied objects go: a transaction's overlay when pages are imported
/// into an open document, a fresh object list when they are extracted into a
/// new one. One copier for both, so extract cannot be shallower than import.
pub(crate) trait Sink {
    fn reserve(&mut self) -> u32;
    fn write(&mut self, number: u32, object: Object) -> Result<()>;
}

impl Sink for Transaction<'_> {
    fn reserve(&mut self) -> u32 {
        Transaction::reserve(self)
    }
    fn write(&mut self, number: u32, object: Object) -> Result<()> {
        self.put_object(number, 0, object)
    }
}

/// A new document's objects, numbered from 1.
#[derive(Default)]
pub(crate) struct NewDocument {
    pub(crate) objects: Vec<(ObjRef, Object)>,
    next: u32,
}

impl Sink for NewDocument {
    fn reserve(&mut self) -> u32 {
        self.next += 1;
        self.next
    }
    fn write(&mut self, number: u32, object: Object) -> Result<()> {
        self.objects.push((ObjRef::new(number, 0), object));
        Ok(())
    }
}

/// How many objects one import may copy. A page that reaches more than this is
/// a page whose resources are the whole document, which is the symptom of a
/// reference this module should have refused to follow.
const MAX_OBJECTS: usize = 200_000;

/// Import `pages` of `source` into the transaction's document.
///
/// Returns the new page references in the order asked for, ready to be placed
/// with [`super::PageSource::Imported`]. Refuses an encrypted source with the
/// encrypted-source rule: its objects cannot be read out into another document.
pub fn import_pages(
    tx: &mut Transaction<'_>,
    source: &CosDocument,
    pages: &[usize],
) -> Result<Vec<ObjRef>> {
    copy_pages(tx, source, pages)
}

/// The transitive copy, into any [`Sink`].
pub(crate) fn copy_pages(
    sink: &mut dyn Sink,
    source: &CosDocument,
    pages: &[usize],
) -> Result<Vec<ObjRef>> {
    crate::protection::read_out(source).map_err(Error::Protected)?;

    let root = source
        .catalog()?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .ok_or(Error::NoPageTree)?;
    let leaves = {
        let mut resolve = |number: u32| -> Result<Option<Object>> {
            Ok(source.get(number).ok().map(|parsed| parsed.object))
        };
        walk(root, &mut resolve)?
    };
    for page in pages {
        if *page >= leaves.len() {
            return Err(Error::NoSuchPage {
                page: *page,
                count: leaves.len(),
            });
        }
    }

    let mut copier = Copier {
        source,
        map: BTreeMap::new(),
        queue: VecDeque::new(),
        imported_pages: pages
            .iter()
            .map(|index| leaves[*index].objref.number)
            .collect(),
        copied: 0,
    };

    // The pages first, so a reference from one imported page's annotation to
    // another imported page resolves to the new page rather than to null.
    let mut placed = Vec::with_capacity(pages.len());
    for index in pages {
        let number = leaves[*index].objref.number;
        placed.push(copier.number_for(sink, number));
    }
    for index in pages {
        let leaf = &leaves[*index];
        // Materialized now, on the source's tree, so the copy does not depend
        // on ancestors it will never have.
        let mut dict = leaf.materialized(leaf.objref);
        for key in [b"Parent".as_slice(), b"B", b"StructParents"] {
            dict.remove(key);
        }
        let rewritten = copier.rewrite(sink, Object::Dict(dict))?;
        let target = copier.map[&leaf.objref.number];
        sink.write(target.number, rewritten)?;
    }

    // Everything the pages reach, breadth first.
    while let Some(number) = copier.queue.pop_front() {
        copier.copied += 1;
        if copier.copied > MAX_OBJECTS {
            return Err(Error::PageTreeTooLarge {
                depth: 0,
                visits: copier.copied,
            });
        }
        let target = copier.map[&number];
        let object = match source.get(number) {
            Ok(parsed) => parsed.object,
            // A reference the source itself cannot resolve is copied as null,
            // which is what it already meant there.
            Err(_) => Object::Null,
        };
        let object = strip_structure_keys(object);
        let rewritten = copier.rewrite(sink, object)?;
        sink.write(target.number, rewritten)?;
    }
    Ok(placed)
}

/// Extract `pages` of `source` into a new document, returned as its bytes.
///
/// The same transitive copy as [`import_pages`], written into a fresh object
/// list and serialized by `cos::Document::write_new`. Refuses an encrypted
/// source: an extracted copy is exactly the silently decrypted file the
/// encrypted-source rule exists to prevent.
pub fn extract_pages(source: &CosDocument, pages: &[usize]) -> Result<Vec<u8>> {
    if pages.is_empty() {
        return Err(Error::WouldLeaveNoPages);
    }
    let mut document = NewDocument::default();
    let placed = copy_pages(&mut document, source, pages)?;

    let tree = ObjRef::new(document.reserve(), 0);
    let catalog = ObjRef::new(document.reserve(), 0);
    for (objref, object) in &mut document.objects {
        if placed.contains(objref) {
            if let Object::Dict(page) = object {
                page.set(Name::new("Parent"), Object::Ref(tree));
            }
        }
    }
    let mut pages_node = Dict::new();
    pages_node.set(Name::new("Type"), Object::name("Pages"));
    pages_node.set(Name::new("Count"), Object::Integer(placed.len() as i64));
    pages_node.set(
        Name::new("Kids"),
        Object::Array(placed.iter().map(|objref| Object::Ref(*objref)).collect()),
    );
    document.write(tree.number, Object::Dict(pages_node))?;
    let mut catalog_dict = Dict::new();
    catalog_dict.set(Name::new("Type"), Object::name("Catalog"));
    catalog_dict.set(Name::new("Pages"), Object::Ref(tree));
    document.write(catalog.number, Object::Dict(catalog_dict))?;

    let mut trailer = Dict::new();
    trailer.set(Name::new("Root"), Object::Ref(catalog));
    Ok(CosDocument::write_new(&document.objects, trailer)?)
}

struct Copier<'s> {
    source: &'s CosDocument,
    /// Source object number to its number in the destination.
    map: BTreeMap<u32, ObjRef>,
    /// Source objects numbered but not yet copied.
    queue: VecDeque<u32>,
    /// The pages being imported. Any other page of the source is not followed.
    imported_pages: BTreeSet<u32>,
    copied: usize,
}

impl Copier<'_> {
    /// The destination number for a source object, reserving one on first
    /// sight. Reserved **before** the object is read, which is what makes a
    /// cycle terminate: the second time round, the number is already there.
    fn number_for(&mut self, sink: &mut dyn Sink, number: u32) -> ObjRef {
        if let Some(existing) = self.map.get(&number) {
            return *existing;
        }
        let target = ObjRef::new(sink.reserve(), 0);
        self.map.insert(number, target);
        target
    }

    /// Every reference inside `object`, rewritten into the destination, and
    /// every newly seen target queued for copying.
    fn rewrite(&mut self, sink: &mut dyn Sink, object: Object) -> Result<Object> {
        Ok(match object {
            Object::Ref(objref) => {
                if let Some(existing) = self.map.get(&objref.number) {
                    return Ok(Object::Ref(*existing));
                }
                if self.is_foreign_page(objref.number) {
                    // A page of the source that is not being imported: the
                    // reference has nothing to mean in the destination.
                    return Ok(Object::Null);
                }
                let target = self.number_for(sink, objref.number);
                self.queue.push_back(objref.number);
                Object::Ref(target)
            }
            Object::Array(items) => Object::Array(
                items
                    .into_iter()
                    .map(|item| self.rewrite(sink, item))
                    .collect::<Result<_>>()?,
            ),
            Object::Dict(dict) => Object::Dict(self.rewrite_dict(sink, dict)?),
            Object::Stream(mut stream) => {
                stream.dict = self.rewrite_dict(sink, stream.dict)?;
                Object::Stream(stream)
            }
            other => other,
        })
    }

    fn rewrite_dict(&mut self, sink: &mut dyn Sink, dict: Dict) -> Result<Dict> {
        let mut out = Dict::new();
        for (key, value) in dict.iter() {
            out.set(key.clone(), self.rewrite(sink, value.clone())?);
        }
        Ok(out)
    }

    /// Whether a source object is a page, or a page-tree node, that is not
    /// being imported. Read to find out, because a reference does not say what
    /// it points at.
    fn is_foreign_page(&self, number: u32) -> bool {
        if self.imported_pages.contains(&number) {
            return false;
        }
        let Ok(parsed) = self.source.get(number) else {
            return false;
        };
        let Some(dict) = parsed.object.as_dict() else {
            return false;
        };
        matches!(
            dict.get(b"Type")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            Some(b"Page") | Some(b"Pages")
        )
    }
}

/// An annotation's `/StructParent` indexes the source's `/ParentTree`, which
/// does not come with it.
fn strip_structure_keys(object: Object) -> Object {
    match object {
        Object::Dict(mut dict) => {
            dict.remove(b"StructParent");
            Object::Dict(dict)
        }
        other => other,
    }
}
