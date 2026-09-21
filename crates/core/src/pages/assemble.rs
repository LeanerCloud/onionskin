//! Page assembly: an ordered list of `(document, pages)` into one new document.
//!
//! The primitive behind Combine, Split and Extract, and behind P10's comment
//! summary and P14a's create-from-images when they land: each appends pages
//! from a source and asks for the bytes at the end. Every page arrives through
//! the same transitive [`Copier`] the importer uses, so an assembled page draws
//! exactly as it did in its source, and a document combined with itself gets
//! independent copies rather than aliases - each append is its own copy with
//! its own number map.
//!
//! # Sources are streamed, the output is not
//!
//! [`Assembly::append`] takes a borrowed source and is done with it when it
//! returns, so a caller combining a hundred files opens, appends and drops them
//! one at a time: at most one input is ever held. The output's objects are
//! held until [`Assembly::finish`] writes them, because `write_new` writes a
//! whole document at once.
//!
//! # What the output is
//!
//! - **Fresh metadata.** A new `/Info` naming the producer and nothing else.
//!   Inheriting the first input's title, author and dates would be a guess
//!   about which input the user thinks of as "the document", and a merge of
//!   several has no right answer; nothing is carried.
//! - **One flat page tree**, every page's inherited attributes materialized.
//! - **A structure tree only when every input had one and came whole.** Each
//!   input's tree is merged under one root - its top-level elements in input
//!   order, its `/ParentTree` keys moved past the previous inputs' so none
//!   collide, its role and class maps merged with the first definition winning.
//!   If any input is untagged, or only some of its pages came, the output is
//!   untagged, and [`Assembled::tagging`] says which input made it so: a
//!   half-tagged document is one whose reading order silently skips pages.
//! - **No outline, no forms, no named destinations.** Each belongs to a whole
//!   source document; links between pages that came together still resolve,
//!   and ones to pages that did not are null (see `import.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use super::import::{source_leaves, Copier, NewDocument, Sink};
use crate::structure::{read_structure, ParentEntry, StructureTree};
use crate::{Error, Result};

/// Pages from many sources, in order, becoming one document.
#[derive(Default)]
pub struct Assembly {
    document: NewDocument,
    pages: Vec<ObjRef>,
    inputs: usize,
    structure: Merged,
}

/// Whether the assembled document carries a structure tree, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tagging {
    Tagged,
    Untagged(Untagged),
}

/// Why an assembled document came out untagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Untagged {
    /// No input was tagged.
    NoInputTagged,
    /// This input (counting from 0) has no structure tree.
    InputUntagged { input: usize },
    /// This input came in part, and a structure tree cannot be cut to a page
    /// selection without losing what spans the cut.
    PartialInput { input: usize },
}

/// An assembled document.
pub struct Assembled {
    pub bytes: Vec<u8>,
    pub page_count: usize,
    pub tagging: Tagging,
}

impl Assembly {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pages so far.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Append `pages` of `source`, in the order given, returning where they
    /// landed in the output. Refuses an encrypted source before copying
    /// anything from it.
    pub fn append(&mut self, source: &CosDocument, pages: &[usize]) -> Result<Range<usize>> {
        let leaves = source_leaves(source)?;
        let input = self.inputs;
        let whole = pages.iter().copied().eq(0..leaves.len());
        let tree = read_structure(source)?.tree().cloned();
        let merge = match (&tree, whole) {
            (None, _) => {
                self.structure
                    .drop_because(Untagged::InputUntagged { input });
                None
            }
            (Some(_), false) => {
                self.structure
                    .drop_because(Untagged::PartialInput { input });
                None
            }
            (Some(tree), true) => self.structure.keeping().then_some(tree),
        };

        let offset = merge.map(|_| self.structure.next_key);
        let mut copier = Copier::new(source, &leaves, pages, offset)?;
        if let Some(tree) = merge {
            let root = self.structure.root(&mut self.document);
            copier.redirect(tree.root.number, root);
        }
        let placed = copier.pages(&mut self.document, &leaves, pages)?;
        copier.drain(&mut self.document)?;
        if let Some(tree) = merge {
            self.structure
                .merge(&mut self.document, &mut copier, source, tree)?;
        }

        let start = self.pages.len();
        self.pages.extend(placed);
        self.inputs += 1;
        Ok(start..self.pages.len())
    }

    /// The document's bytes. Refuses an assembly with no pages: a PDF with
    /// none is not a document a reader will open.
    pub fn finish(mut self) -> Result<Assembled> {
        if self.pages.is_empty() {
            return Err(Error::WouldLeaveNoPages);
        }
        let tagging = self.structure.tagging(self.inputs);
        let structure_root = match tagging {
            Tagging::Tagged => Some(self.structure.write_root(&mut self.document)?),
            Tagging::Untagged(_) => {
                self.structure.discard(&mut self.document);
                None
            }
        };

        let tree = ObjRef::new(self.document.reserve(), 0);
        let catalog = ObjRef::new(self.document.reserve(), 0);
        let info = ObjRef::new(self.document.reserve(), 0);
        let placed: BTreeSet<ObjRef> = self.pages.iter().copied().collect();
        for (objref, object) in &mut self.document.objects {
            if placed.contains(objref) {
                if let Object::Dict(page) = object {
                    page.set(Name::new("Parent"), Object::Ref(tree));
                }
            }
        }

        let mut pages_node = Dict::new();
        pages_node.set(Name::new("Type"), Object::name("Pages"));
        pages_node.set(Name::new("Count"), Object::Integer(self.pages.len() as i64));
        pages_node.set(
            Name::new("Kids"),
            Object::Array(self.pages.iter().map(|page| Object::Ref(*page)).collect()),
        );
        self.document.write(tree.number, Object::Dict(pages_node))?;

        let mut catalog_dict = Dict::new();
        catalog_dict.set(Name::new("Type"), Object::name("Catalog"));
        catalog_dict.set(Name::new("Pages"), Object::Ref(tree));
        if let Some(root) = structure_root {
            catalog_dict.set(Name::new("StructTreeRoot"), Object::Ref(root));
            let mut mark_info = Dict::new();
            mark_info.set(Name::new("Marked"), Object::Bool(true));
            catalog_dict.set(Name::new("MarkInfo"), Object::Dict(mark_info));
        }
        self.document
            .write(catalog.number, Object::Dict(catalog_dict))?;

        let mut info_dict = Dict::new();
        info_dict.set(
            Name::new("Producer"),
            Object::String(format!("Onionskin {}", env!("CARGO_PKG_VERSION")).into_bytes()),
        );
        self.document.write(info.number, Object::Dict(info_dict))?;

        let mut trailer = Dict::new();
        trailer.set(Name::new("Root"), Object::Ref(catalog));
        trailer.set(Name::new("Info"), Object::Ref(info));
        Ok(Assembled {
            bytes: CosDocument::write_new(&self.document.objects, trailer)?,
            page_count: self.pages.len(),
            tagging,
        })
    }
}

/// The structure trees merged so far.
#[derive(Default)]
struct Merged {
    /// Why the output will be untagged, once that is known. The first reason
    /// is kept: it names the input the user can do something about.
    dropped: Option<Untagged>,
    /// The merged root's number, reserved when the first tree arrives so every
    /// source's elements can name it as their parent.
    root: Option<ObjRef>,
    kids: Vec<Object>,
    parent_tree: Vec<(i64, Object)>,
    next_key: i64,
    role_map: Dict,
    class_map: Dict,
    ids: BTreeMap<Vec<u8>, Object>,
    /// Objects reached only through a structure tree: dropped if the output
    /// turns out untagged, because nothing else names them.
    objects: BTreeSet<u32>,
}

impl Merged {
    fn keeping(&self) -> bool {
        self.dropped.is_none()
    }

    fn drop_because(&mut self, reason: Untagged) {
        self.dropped.get_or_insert(reason);
    }

    fn tagging(&self, inputs: usize) -> Tagging {
        match (self.dropped, self.root) {
            (Some(reason), _) => Tagging::Untagged(reason),
            (None, Some(_)) if inputs > 0 => Tagging::Tagged,
            (None, _) => Tagging::Untagged(Untagged::NoInputTagged),
        }
    }

    fn root(&mut self, document: &mut NewDocument) -> ObjRef {
        *self
            .root
            .get_or_insert_with(|| ObjRef::new(document.reserve(), 0))
    }

    /// Bring one whole, tagged source's tree in, after its pages: its root's
    /// kids, its `/ParentTree` with keys moved past the previous inputs', and
    /// its role, class and ID maps. Everything first reached from here is
    /// structure-only, and remembered as such.
    fn merge(
        &mut self,
        document: &mut NewDocument,
        copier: &mut Copier<'_>,
        source: &CosDocument,
        tree: &StructureTree,
    ) -> Result<()> {
        let first_structure_number = document.next_number();
        let offset = self.next_key;
        let root = match source.get(tree.root.number)?.object {
            Object::Dict(dict) => dict,
            _ => Dict::new(),
        };

        match copier.rewrite(document, root.get(b"K").cloned().unwrap_or(Object::Null))? {
            Object::Array(kids) => self.kids.extend(kids),
            Object::Null => {}
            single => self.kids.push(single),
        }
        for (key, entry) in &tree.parent_tree {
            let value = copier.rewrite(document, parent_entry(entry))?;
            self.parent_tree.push((key + offset, value));
        }
        let maps: [(&mut Dict, &[u8]); 2] = [
            (&mut self.role_map, b"RoleMap"),
            (&mut self.class_map, b"ClassMap"),
        ];
        for (target, key) in maps {
            if let Some(Object::Dict(map)) = resolved(source, root.get(key)) {
                for (name, value) in map.iter() {
                    if target.get(name.as_bytes()).is_none() {
                        target.set(name.clone(), copier.rewrite(document, value.clone())?);
                    }
                }
            }
        }
        for (id, element) in &tree.id_tree {
            let element = copier.rewrite(document, Object::Ref(*element))?;
            self.ids.entry(id.clone()).or_insert(element);
        }
        copier.drain(document)?;

        self.objects
            .extend(first_structure_number..document.next_number());
        let used = tree
            .parent_tree
            .keys()
            .max()
            .map_or(0, |highest| highest + 1);
        self.next_key = offset + tree.parent_tree_next_key.unwrap_or(0).max(used);
        Ok(())
    }

    fn write_root(&mut self, document: &mut NewDocument) -> Result<ObjRef> {
        let root = self.root.expect("a tagged assembly reserved its root");
        let mut dict = Dict::new();
        dict.set(Name::new("Type"), Object::name("StructTreeRoot"));
        dict.set(
            Name::new("K"),
            Object::Array(std::mem::take(&mut self.kids)),
        );
        let mut nums = Vec::with_capacity(self.parent_tree.len() * 2);
        for (key, value) in std::mem::take(&mut self.parent_tree) {
            nums.push(Object::Integer(key));
            nums.push(value);
        }
        let mut parent_tree = Dict::new();
        parent_tree.set(Name::new("Nums"), Object::Array(nums));
        dict.set(Name::new("ParentTree"), Object::Dict(parent_tree));
        dict.set(
            Name::new("ParentTreeNextKey"),
            Object::Integer(self.next_key),
        );
        for (key, map) in [("RoleMap", &self.role_map), ("ClassMap", &self.class_map)] {
            if !map.is_empty() {
                dict.set(Name::new(key), Object::Dict(map.clone()));
            }
        }
        if !self.ids.is_empty() {
            let mut names = Vec::with_capacity(self.ids.len() * 2);
            for (id, element) in std::mem::take(&mut self.ids) {
                names.push(Object::String(id));
                names.push(element);
            }
            let mut id_tree = Dict::new();
            id_tree.set(Name::new("Names"), Object::Array(names));
            dict.set(Name::new("IDTree"), Object::Dict(id_tree));
        }
        document.write(root.number, Object::Dict(dict))?;
        Ok(root)
    }

    /// The output is untagged after all: drop the structure-only objects, and
    /// the keys on pages and annotations that indexed a tree nobody will write.
    fn discard(&self, document: &mut NewDocument) {
        document
            .objects
            .retain(|(objref, _)| !self.objects.contains(&objref.number));
        for (_, object) in &mut document.objects {
            if let Object::Dict(dict) = object {
                dict.remove(b"StructParents");
                dict.remove(b"StructParent");
            }
        }
    }
}

fn parent_entry(entry: &ParentEntry) -> Object {
    match entry {
        ParentEntry::Element(element) => Object::Ref(*element),
        ParentEntry::Slots(slots) => Object::Array(
            slots
                .iter()
                .map(|slot| slot.map_or(Object::Null, Object::Ref))
                .collect(),
        ),
    }
}

fn resolved(source: &CosDocument, value: Option<&Object>) -> Option<Object> {
    match value {
        Some(Object::Ref(objref)) => source.get(objref.number).ok().map(|parsed| parsed.object),
        other => other.cloned(),
    }
}
