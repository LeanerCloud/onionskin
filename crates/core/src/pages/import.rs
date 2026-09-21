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

use super::inherit::{walk, Leaf};
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

impl NewDocument {
    /// The number the next reservation will get.
    pub(crate) fn next_number(&self) -> u32 {
        self.next + 1
    }
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
    let leaves = source_leaves(source)?;
    let mut copier = Copier::new(source, &leaves, pages, None)?;
    let placed = copier.pages(sink, &leaves, pages)?;
    copier.drain(sink)?;
    Ok(placed)
}

/// Every page of `source`, in order, refusing an encrypted one first: nothing
/// is read out of a document the encrypted-source rule protects.
pub(crate) fn source_leaves(source: &CosDocument) -> Result<Vec<Leaf>> {
    crate::protection::read_out(source).map_err(Error::Protected)?;
    let root = source
        .catalog()?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .ok_or(Error::NoPageTree)?;
    let mut resolve = |number: u32| -> Result<Option<Object>> {
        Ok(source.get(number).ok().map(|parsed| parsed.object))
    };
    walk(root, &mut resolve)
}

/// Extract `pages` of `source` into a new document, returned as its bytes.
///
/// An [`super::Assembly`] of one part, so it is the same transitive copy as
/// [`import_pages`] and keeps the structure tree when every page comes.
/// Refuses an encrypted source: an extracted copy is exactly the silently
/// decrypted file the encrypted-source rule exists to prevent.
pub fn extract_pages(source: &CosDocument, pages: &[usize]) -> Result<Vec<u8>> {
    let mut assembly = super::Assembly::new();
    assembly.append(source, pages)?;
    Ok(assembly.finish()?.bytes)
}

/// One source document's copy into one destination: the number map, the
/// objects still to copy, and what to do with structure keys.
pub(crate) struct Copier<'s> {
    source: &'s CosDocument,
    /// Source object number to its number in the destination.
    map: BTreeMap<u32, ObjRef>,
    /// Source objects numbered but not yet copied.
    queue: VecDeque<u32>,
    /// The pages being imported. Any other page of the source is not followed.
    imported_pages: BTreeSet<u32>,
    copied: usize,
    /// `None`: `/StructParents` and `/StructParent` index the source's
    /// `/ParentTree`, which does not come, so they are dropped. `Some(offset)`:
    /// the source's structure tree is coming too, merged with others, and its
    /// keys move up by `offset` so they cannot collide.
    structure_offset: Option<i64>,
}

impl<'s> Copier<'s> {
    /// A copier for `pages` of `source`, whose leaves the caller already walked.
    pub(crate) fn new(
        source: &'s CosDocument,
        leaves: &[Leaf],
        pages: &[usize],
        structure_offset: Option<i64>,
    ) -> Result<Self> {
        for page in pages {
            if *page >= leaves.len() {
                return Err(Error::NoSuchPage {
                    page: *page,
                    count: leaves.len(),
                });
            }
        }
        Ok(Self {
            source,
            map: BTreeMap::new(),
            queue: VecDeque::new(),
            imported_pages: pages
                .iter()
                .map(|index| leaves[*index].objref.number)
                .collect(),
            copied: 0,
            structure_offset,
        })
    }

    /// Make every reference to the source's object `number` mean `target`
    /// instead, without copying it: a merged structure tree's root stands in
    /// for each source's own.
    pub(crate) fn redirect(&mut self, number: u32, target: ObjRef) {
        self.map.insert(number, target);
    }

    /// Copy the page dictionaries of `pages`, returning their new references
    /// in order. What they reach is queued; [`Copier::drain`] copies it.
    pub(crate) fn pages(
        &mut self,
        sink: &mut dyn Sink,
        leaves: &[Leaf],
        pages: &[usize],
    ) -> Result<Vec<ObjRef>> {
        // Numbered first, so a reference from one imported page's annotation
        // to another imported page resolves to the new page, not to null.
        let placed: Vec<ObjRef> = pages
            .iter()
            .map(|index| self.number_for(sink, leaves[*index].objref.number))
            .collect();
        for index in pages {
            let leaf = &leaves[*index];
            // Materialized now, on the source's tree, so the copy does not depend
            // on ancestors it will never have.
            let mut dict = leaf.materialized(leaf.objref);
            dict.remove(b"Parent");
            dict.remove(b"B");
            let dict = self.structure_keys(dict, b"StructParents");
            let rewritten = self.rewrite(sink, Object::Dict(dict))?;
            let target = self.map[&leaf.objref.number];
            sink.write(target.number, rewritten)?;
        }
        Ok(placed)
    }

    /// Copy everything queued, and everything that reaches, breadth first.
    pub(crate) fn drain(&mut self, sink: &mut dyn Sink) -> Result<()> {
        while let Some(number) = self.queue.pop_front() {
            self.copied += 1;
            if self.copied > MAX_OBJECTS {
                return Err(Error::PageTreeTooLarge {
                    depth: 0,
                    visits: self.copied,
                });
            }
            let target = self.map[&number];
            let object = match self.source.get(number) {
                Ok(parsed) => parsed.object,
                // A reference the source itself cannot resolve is copied as
                // null, which is what it already meant there.
                Err(_) => Object::Null,
            };
            let object = match object {
                Object::Dict(dict) => Object::Dict(self.structure_keys(dict, b"StructParent")),
                other => other,
            };
            let rewritten = self.rewrite(sink, object)?;
            sink.write(target.number, rewritten)?;
        }
        Ok(())
    }

    /// `/StructParents` or `/StructParent`, dropped or moved up by the offset.
    fn structure_keys(&self, mut dict: Dict, key: &[u8]) -> Dict {
        match (
            self.structure_offset,
            dict.get(key).and_then(Object::as_integer),
        ) {
            (Some(offset), Some(value)) => {
                dict.set(Name(key.to_vec()), Object::Integer(value + offset));
            }
            _ => {
                dict.remove(key);
            }
        }
        dict
    }

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
    pub(crate) fn rewrite(&mut self, sink: &mut dyn Sink, object: Object) -> Result<Object> {
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
