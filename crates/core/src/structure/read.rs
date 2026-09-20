//! Reading the tagged-PDF structure tree.
//!
//! [`crate::outline`] is the model for the shape of this: a cycle-guarded,
//! depth-capped walk over a cos object graph reached through `catalog()` and
//! `resolve`, reporting a file that claims something it cannot deliver as an
//! error rather than as emptiness.
//!
//! **Three states, not two.** A document with no `/StructTreeRoot` is
//! [`Structure::Untagged`], which is most documents and is not a failure. A
//! document whose `/StructTreeRoot` is present but is not a dictionary, or
//! whose tree cannot be walked, is an **error**. Collapsing those two into one
//! silent no-op is the defect this enum exists to prevent: an edit that skips
//! structure maintenance because the tree was unreadable would leave a tagged
//! document quietly broken.
//!
//! **What is read beyond `/K`.** A page removal reaches two things a `/K` walk
//! does not. A surviving element's own `/Pg` can name a removed page with no
//! `/K` entry pointing at it, and an `/IDTree` entry can name an element that is
//! gone. Both are read here so the invariant can check them.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use crate::{Error, Result};

/// A tagged document can carry tens of thousands of elements; the review risk
/// this cap answers is holding a 50k-element tree in memory. 200k is roughly
/// four times the largest real document anyone has reported and still bounds a
/// hostile file, which can otherwise describe an unbounded tree in a few
/// hundred bytes.
const MAX_ELEMENTS: usize = 200_000;

/// `/K` nesting past this is a producer bug rather than a structure a reader
/// has to serve. The same figure `outline.rs` uses, for the same reason.
const MAX_DEPTH: usize = 64;

/// Number and name trees are balanced by their producers; 32 levels is far past
/// any real fan-out and bounds a `/Kids` cycle that the seen-set alone does not,
/// because a node can chain without repeating on one path.
const MAX_TREE_DEPTH: usize = 32;

/// Whether this document has a structure tree, and if so, what is in it.
#[derive(Clone, Debug)]
pub enum Structure {
    /// No `/StructTreeRoot`. Every maintenance operation is a no-op, and says
    /// so rather than silently doing nothing.
    Untagged,
    Tagged(Box<StructureTree>),
}

impl Structure {
    pub fn is_tagged(&self) -> bool {
        matches!(self, Structure::Tagged(_))
    }

    pub fn tree(&self) -> Option<&StructureTree> {
        match self {
            Structure::Untagged => None,
            Structure::Tagged(tree) => Some(tree),
        }
    }

    pub fn tree_mut(&mut self) -> Option<&mut StructureTree> {
        match self {
            Structure::Untagged => None,
            Structure::Tagged(tree) => Some(tree),
        }
    }
}

/// One `/StructElem`, as read.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub objref: ObjRef,
    /// `/S`, the structure type, unresolved through `/RoleMap`. M3 needs the
    /// type only to report it; role resolution lands with the checker at M5.
    pub struct_type: Option<Name>,
    /// `/Pg`. Read separately from `/K` because a surviving element's `/Pg` can
    /// name a removed page with nothing in `/K` pointing at it.
    pub page: Option<ObjRef>,
    /// `/ID`, the key an `/IDTree` entry uses to find this element.
    pub id: Option<Vec<u8>>,
    /// `/P`, as the file states it. Not trusted as the parent link: the walk
    /// derives parentage from `/K`, and a `/P` that disagrees is the file's
    /// problem to report rather than the reader's to follow.
    pub stated_parent: Option<ObjRef>,
    pub kids: Vec<Kid>,
}

/// One entry in an element's `/K`.
#[derive(Clone, Debug, PartialEq)]
pub enum Kid {
    /// A bare integer: a marked-content id on this element's own `/Pg`.
    Mcid(i64),
    /// A child `/StructElem`, by object number.
    Element(u32),
    /// An `/MCR` marked-content reference, which names its own page.
    MarkedContent { page: Option<ObjRef>, mcid: i64 },
    /// An `/OBJR` object reference: an annotation or XObject that belongs to
    /// this element.
    Object {
        page: Option<ObjRef>,
        object: ObjRef,
    },
}

/// The tree, flattened by object number, plus the two indexes the invariant
/// checks against.
#[derive(Clone, Debug)]
pub struct StructureTree {
    pub root: ObjRef,
    /// Every element reached from the root's `/K`, by object number.
    pub elements: BTreeMap<u32, Element>,
    /// The root's own `/K`, in order. This is the sequence a page reorder
    /// rewrites.
    pub roots: Vec<Kid>,
    /// `/ParentTree`: marked-content parent index to the element or elements it
    /// names. A page's entry is an array, one slot per MCID; an annotation's is
    /// a single element.
    pub parent_tree: BTreeMap<i64, ParentEntry>,
    /// `/ParentTreeNextKey`, when the file states one. The next free index a
    /// new `/StructParent` may take.
    pub parent_tree_next_key: Option<i64>,
    /// `/IDTree`: element id to the object it names.
    pub id_tree: BTreeMap<Vec<u8>, ObjRef>,
    /// `/MarkInfo` `/Marked`. A tree with `/Marked false` is a file
    /// contradicting itself, which the invariant reports rather than repairs.
    pub marked: bool,
}

/// One `/ParentTree` value.
#[derive(Clone, Debug, PartialEq)]
pub enum ParentEntry {
    /// An annotation's entry: one element.
    Element(ObjRef),
    /// A page's entry: one slot per marked-content id, in MCID order. A slot
    /// may be null, which is a page whose MCID is not claimed by any element.
    Slots(Vec<Option<ObjRef>>),
}

/// Read the structure tree, or report that there is none.
pub(crate) fn read(doc: &CosDocument) -> Result<Structure> {
    let catalog = doc.catalog()?;
    let Some(root_object) = catalog.get(b"StructTreeRoot") else {
        return Ok(Structure::Untagged);
    };
    let root = match root_object {
        Object::Ref(objref) => *objref,
        // A direct /StructTreeRoot is legal but leaves nothing to address, and
        // every maintenance operation here rewrites the root by number.
        _ => return Err(malformed("/StructTreeRoot is not an indirect reference")),
    };
    let resolved = doc.resolve(root_object)?;
    if matches!(resolved, Object::Null) {
        // A dangling /StructTreeRoot is a claim the file cannot back. Untagged
        // would be the wrong answer: the file says it is tagged.
        return Err(malformed("/StructTreeRoot resolves to null"));
    }
    let root_dict = resolved
        .as_dict()
        .cloned()
        .ok_or_else(|| malformed("/StructTreeRoot does not resolve to a dictionary"))?;

    let mut reader = Reader {
        doc,
        seen: BTreeSet::new(),
        elements: BTreeMap::new(),
        budget: MAX_ELEMENTS,
    };
    let roots = reader.kids(&root_dict, 0)?;

    Ok(Structure::Tagged(Box::new(StructureTree {
        root,
        elements: reader.elements,
        roots,
        parent_tree: read_parent_tree(doc, &root_dict)?,
        parent_tree_next_key: root_dict
            .get(b"ParentTreeNextKey")
            .and_then(Object::as_integer),
        id_tree: read_id_tree(doc, &root_dict)?,
        marked: read_marked(doc, &catalog)?,
    })))
}

/// `/MarkInfo` `/Marked`, defaulting to false, which is what the spec says an
/// absent entry means.
fn read_marked(doc: &CosDocument, catalog: &Dict) -> Result<bool> {
    let Some(mark_info) = catalog.get(b"MarkInfo") else {
        return Ok(false);
    };
    let resolved = doc.resolve(mark_info)?;
    let Some(dict) = resolved.as_dict() else {
        return Ok(false);
    };
    Ok(matches!(dict.get(b"Marked"), Some(Object::Bool(true))))
}

struct Reader<'a> {
    doc: &'a CosDocument,
    /// Object numbers already turned into an element. A node reached twice is a
    /// cycle or a shared subtree; either way walking it again would not change
    /// what is in `elements` and might not terminate.
    seen: BTreeSet<u32>,
    elements: BTreeMap<u32, Element>,
    budget: usize,
}

impl Reader<'_> {
    /// One dictionary's `/K`, which may be absent, a single kid, or an array.
    fn kids(&mut self, dict: &Dict, depth: usize) -> Result<Vec<Kid>> {
        if depth > MAX_DEPTH {
            return Err(malformed("/K nests deeper than the reader will follow"));
        }
        let Some(k) = dict.get(b"K") else {
            return Ok(Vec::new());
        };
        // A direct array is the common case and must not be resolved as a
        // reference; an indirect one has to be.
        let resolved = self.doc.resolve(k)?;
        match resolved {
            Object::Null => Ok(Vec::new()),
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in &items {
                    if let Some(kid) = self.kid(item, depth)? {
                        out.push(kid);
                    }
                }
                Ok(out)
            }
            single => Ok(self
                .kid_from_resolved(k, single, depth)?
                .into_iter()
                .collect()),
        }
    }

    fn kid(&mut self, item: &Object, depth: usize) -> Result<Option<Kid>> {
        let resolved = self.doc.resolve(item)?;
        self.kid_from_resolved(item, resolved, depth)
    }

    /// `original` is kept because an element kid is addressed by the reference
    /// that named it, which the resolved value no longer carries.
    fn kid_from_resolved(
        &mut self,
        original: &Object,
        resolved: Object,
        depth: usize,
    ) -> Result<Option<Kid>> {
        match resolved {
            Object::Integer(mcid) => Ok(Some(Kid::Mcid(mcid))),
            Object::Null => Ok(None),
            Object::Dict(dict) => self.kid_dict(original, &dict, depth),
            // A /K entry that is neither an integer, a dictionary nor null says
            // nothing this reader can act on, and dropping it silently would
            // make the invariant's "no /K entry references a removed page"
            // claim weaker than it reads.
            _ => Err(malformed("/K holds something that is not a kid")),
        }
    }

    fn kid_dict(&mut self, original: &Object, dict: &Dict, depth: usize) -> Result<Option<Kid>> {
        match dict.get(b"Type").and_then(Object::as_name) {
            Some(name) if name.as_bytes() == b"MCR" => Ok(Some(Kid::MarkedContent {
                page: self.reference(dict.get(b"Pg")),
                mcid: dict.get(b"MCID").and_then(Object::as_integer).unwrap_or(0),
            })),
            Some(name) if name.as_bytes() == b"OBJR" => {
                let Some(object) = self.reference(dict.get(b"Obj")) else {
                    return Err(malformed("/OBJR names no object"));
                };
                Ok(Some(Kid::Object {
                    page: self.reference(dict.get(b"Pg")),
                    object,
                }))
            }
            // Everything else is a structure element. `/Type` is optional on a
            // `/StructElem`, so its absence is not a reason to reject one.
            _ => self.element(original, dict, depth),
        }
    }

    fn element(&mut self, original: &Object, dict: &Dict, depth: usize) -> Result<Option<Kid>> {
        let Some(objref) = self.reference(Some(original)) else {
            // A direct structure element cannot be addressed, so no edit can
            // maintain it and no /ParentTree entry can name it.
            return Err(malformed("a structure element is not an indirect object"));
        };
        if !self.seen.insert(objref.number) {
            return Ok(Some(Kid::Element(objref.number)));
        }
        if self.budget == 0 {
            return Err(malformed(
                "the structure tree holds more elements than the reader will hold",
            ));
        }
        self.budget -= 1;

        let kids = self.kids(dict, depth + 1)?;
        self.elements.insert(
            objref.number,
            Element {
                objref,
                struct_type: dict.get(b"S").and_then(Object::as_name).cloned(),
                page: self.reference(dict.get(b"Pg")),
                id: dict.get(b"ID").and_then(as_byte_string),
                stated_parent: self.reference(dict.get(b"P")),
                kids,
            },
        );
        Ok(Some(Kid::Element(objref.number)))
    }

    fn reference(&self, object: Option<&Object>) -> Option<ObjRef> {
        match object {
            Some(Object::Ref(objref)) => Some(*objref),
            _ => None,
        }
    }
}

fn as_byte_string(object: &Object) -> Option<Vec<u8>> {
    match object {
        Object::String(bytes) => Some(bytes.clone()),
        _ => None,
    }
}

/// The `/ParentTree` number tree, flattened.
fn read_parent_tree(doc: &CosDocument, root: &Dict) -> Result<BTreeMap<i64, ParentEntry>> {
    let Some(node) = root.get(b"ParentTree") else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    let mut seen = BTreeSet::new();
    walk_number_tree(doc, node, 0, &mut seen, &mut out)?;
    Ok(out)
}

/// `/Nums` is required to be sorted by key, and producers get that wrong. The
/// walk does not rely on the order: it collects every pair it finds into a map,
/// so an unsorted `/Nums` reads correctly rather than terminating early at the
/// first key that goes backwards.
fn walk_number_tree(
    doc: &CosDocument,
    node: &Object,
    depth: usize,
    seen: &mut BTreeSet<u32>,
    out: &mut BTreeMap<i64, ParentEntry>,
) -> Result<()> {
    if depth > MAX_TREE_DEPTH {
        return Err(malformed(
            "/ParentTree nests deeper than the reader will follow",
        ));
    }
    if let Object::Ref(objref) = node {
        if !seen.insert(objref.number) {
            return Ok(());
        }
    }
    let resolved = doc.resolve(node)?;
    let Some(dict) = resolved.as_dict() else {
        return Ok(());
    };

    if let Some(nums) = dict.get(b"Nums") {
        let nums = doc.resolve(nums)?;
        if let Object::Array(items) = nums {
            for pair in items.chunks(2) {
                let [key, value] = pair else { continue };
                let Some(key) = key.as_integer() else {
                    continue;
                };
                out.insert(key, parent_entry(doc, value)?);
            }
        }
    }
    if let Some(kids) = dict.get(b"Kids") {
        let kids = doc.resolve(kids)?;
        if let Object::Array(items) = kids {
            for kid in &items {
                walk_number_tree(doc, kid, depth + 1, seen, out)?;
            }
        }
    }
    Ok(())
}

/// A page's entry is an array of element references, one per marked-content id;
/// an annotation's is a single reference.
fn parent_entry(doc: &CosDocument, value: &Object) -> Result<ParentEntry> {
    match value {
        Object::Ref(objref) => {
            // An array reached by reference is still a page's entry.
            match doc.resolve(value)? {
                Object::Array(items) => Ok(ParentEntry::Slots(slots(&items))),
                _ => Ok(ParentEntry::Element(*objref)),
            }
        }
        Object::Array(items) => Ok(ParentEntry::Slots(slots(items))),
        _ => Ok(ParentEntry::Slots(Vec::new())),
    }
}

fn slots(items: &[Object]) -> Vec<Option<ObjRef>> {
    items
        .iter()
        .map(|item| match item {
            Object::Ref(objref) => Some(*objref),
            _ => None,
        })
        .collect()
}

/// The `/IDTree` name tree, flattened.
fn read_id_tree(doc: &CosDocument, root: &Dict) -> Result<BTreeMap<Vec<u8>, ObjRef>> {
    let Some(node) = root.get(b"IDTree") else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    let mut seen = BTreeSet::new();
    walk_name_tree(doc, node, 0, &mut seen, &mut out)?;
    Ok(out)
}

fn walk_name_tree(
    doc: &CosDocument,
    node: &Object,
    depth: usize,
    seen: &mut BTreeSet<u32>,
    out: &mut BTreeMap<Vec<u8>, ObjRef>,
) -> Result<()> {
    if depth > MAX_TREE_DEPTH {
        return Err(malformed(
            "/IDTree nests deeper than the reader will follow",
        ));
    }
    if let Object::Ref(objref) = node {
        if !seen.insert(objref.number) {
            return Ok(());
        }
    }
    let resolved = doc.resolve(node)?;
    let Some(dict) = resolved.as_dict() else {
        return Ok(());
    };

    if let Some(names) = dict.get(b"Names") {
        let names = doc.resolve(names)?;
        if let Object::Array(items) = names {
            for pair in items.chunks(2) {
                let [key, value] = pair else { continue };
                let Some(key) = as_byte_string(key) else {
                    continue;
                };
                if let Object::Ref(objref) = value {
                    out.insert(key, *objref);
                }
            }
        }
    }
    if let Some(kids) = dict.get(b"Kids") {
        let kids = doc.resolve(kids)?;
        if let Object::Array(items) = kids {
            for kid in &items {
                walk_name_tree(doc, kid, depth + 1, seen, out)?;
            }
        }
    }
    Ok(())
}

/// Per-page `/StructParents`, by page object number, in document order.
pub(crate) fn page_struct_parents(
    doc: &CosDocument,
    page_count: usize,
) -> Result<BTreeMap<u32, i64>> {
    let mut out = BTreeMap::new();
    for index in 0..page_count {
        let page = doc.page(index)?;
        if let Some(key) = page.dict.get(b"StructParents").and_then(Object::as_integer) {
            out.insert(page.objref.number, key);
        }
    }
    Ok(out)
}

/// Per-annotation `/StructParent`, by annotation object number.
pub(crate) fn annotation_struct_parents(
    doc: &CosDocument,
    page_count: usize,
) -> Result<BTreeMap<u32, i64>> {
    let mut out = BTreeMap::new();
    for index in 0..page_count {
        let page = doc.page(index)?;
        let Some(annots) = page.dict.get(b"Annots") else {
            continue;
        };
        let Object::Array(items) = doc.resolve(annots)? else {
            continue;
        };
        for item in &items {
            let Object::Ref(objref) = item else { continue };
            let resolved = doc.resolve(item)?;
            let Some(dict) = resolved.as_dict() else {
                continue;
            };
            if let Some(key) = dict.get(b"StructParent").and_then(Object::as_integer) {
                out.insert(objref.number, key);
            }
        }
    }
    Ok(out)
}

fn malformed(detail: &str) -> Error {
    Error::Cos(onionskin_cos::Error::Unrecoverable {
        detail: detail.into(),
    })
}
