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
use std::sync::Arc;

use onionskin_content::pdf_text_string;
use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use super::role;
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

/// Decoded `/Alt`, `/ActualText`, `/Lang`, `/T` and `/E` text across the whole
/// tree. An indirect string shared by many elements is decoded into each of
/// them, so without a total a small file can describe gigabytes. Real alt text
/// is kilobytes.
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// The namespace URI of the PDF 2.0 structure types (ISO 32000-2 14.7.4.2).
const PDF2_NAMESPACE: &[u8] = b"http://iso.org/pdf2/ssn";

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
    /// `/S`, the structure type as the producer wrote it. A custom type stays
    /// custom here; [`Element::standard_type`] is the one consumers dispatch on.
    pub struct_type: Option<Name>,
    /// `struct_type` followed through the root's `/RoleMap` to a standard type,
    /// or `None` when the chain cycles or ends on a type nothing defines. The
    /// PDF 2.0 types count as standard only for an element in the 2.0 namespace
    /// (`/NS`); the root's `/RoleMapNS` per-namespace maps are not read.
    pub standard_type: Option<Name>,
    /// `/Pg`. Read separately from `/K` because a surviving element's `/Pg` can
    /// name a removed page with nothing in `/K` pointing at it.
    pub page: Option<ObjRef>,
    /// `/ID`, the key an `/IDTree` entry uses to find this element.
    pub id: Option<Vec<u8>>,
    /// `/P`, as the file states it. Not trusted as the parent link: the walk
    /// derives parentage from `/K`, and a `/P` that disagrees is the file's
    /// problem to report rather than the reader's to follow.
    pub stated_parent: Option<ObjRef>,
    /// `/Alt`, the replacement text for a figure or other non-text content.
    pub alt: Option<String>,
    /// `/ActualText`, the text this element's content stands for.
    pub actual_text: Option<String>,
    /// `/Lang`, as stated. Not inherited here: a consumer that wants the
    /// effective language walks up the tree, the way it does for `/P`.
    pub lang: Option<String>,
    /// `/T`, the element's title.
    pub title: Option<String>,
    /// `/E`, the expansion of an abbreviation.
    pub expansion: Option<String>,
    /// `/A`, one entry per attribute object. The key is dict-or-array in the
    /// file; it is always a list here. Entries from every owner are kept, so a
    /// consumer that wants only `/O /Table` or `/O /Layout` must filter on
    /// [`Attribute::owner`] rather than assume the first one is its own.
    ///
    /// Not complete: revision numbers (`/R`) and `/C` attribute classes are not
    /// read, so a consumer cannot tell a stale attribute from a current one or
    /// see one a class supplies.
    pub attributes: Vec<Attribute>,
    pub kids: Vec<Kid>,
}

/// One attribute object from an element's `/A`.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// `/O`, the owner that defines what the entries mean.
    pub owner: Option<Name>,
    /// The whole dictionary, `/O` included. Shared between the elements that
    /// name the same indirect object.
    pub entries: Arc<Dict>,
}

/// One entry in an element's `/K`.
#[derive(Clone, Debug, PartialEq)]
pub enum Kid {
    /// A bare integer: a marked-content id on this element's own `/Pg`.
    Mcid(i64),
    /// A child `/StructElem`, by object number.
    Element(u32),
    /// An `/MCR` marked-content reference, which names its own page. With a
    /// `/Stm` the `mcid` numbers marked content in that form or appearance
    /// stream, not on the page.
    MarkedContent {
        page: Option<ObjRef>,
        mcid: i64,
        stream: Option<ObjRef>,
        /// `/StmOwn`, the object a `stream` that is an appearance stream
        /// belongs to.
        stream_owner: Option<ObjRef>,
    },
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
    /// `/RoleMap` on the root: a producer's custom types to the types they
    /// stand for.
    pub role_map: BTreeMap<Name, Name>,
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

    let role_map = read_role_map(doc, &root_dict);
    let mut reader = Reader {
        doc,
        role_map: &role_map,
        seen: BTreeSet::new(),
        elements: BTreeMap::new(),
        attribute_objects: BTreeMap::new(),
        attribute_lists: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        budget: MAX_ELEMENTS,
        text_budget: MAX_TEXT_BYTES,
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
        role_map,
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

/// `/RoleMap`, keeping only entries that map a name to a name. Anything else
/// in the dictionary says nothing a resolver can follow, and a reference that
/// does not resolve reads as null (ISO 32000-1 7.3.10), so a damaged entry
/// costs that entry and not the tree.
fn read_role_map(doc: &CosDocument, root: &Dict) -> BTreeMap<Name, Name> {
    let mut out = BTreeMap::new();
    let Some(Object::Dict(dict)) = root.get(b"RoleMap").and_then(|node| doc.resolve(node).ok())
    else {
        return out;
    };
    for (from, to) in dict.iter() {
        if let Ok(Object::Name(to)) = doc.resolve(to) {
            out.insert(from.clone(), to);
        }
    }
    out
}

struct Reader<'a> {
    doc: &'a CosDocument,
    role_map: &'a BTreeMap<Name, Name>,
    /// Attribute dictionaries already read, by object number, so an indirect
    /// one shared by many elements is held once.
    attribute_objects: BTreeMap<u32, Arc<Dict>>,
    /// Indirect `/A` values already read, by object number: one shared by many
    /// elements would otherwise be resolved and rebuilt for each.
    attribute_lists: BTreeMap<u32, Vec<Attribute>>,
    /// Whether a `/NS` object is the PDF 2.0 namespace, by object number.
    namespaces: BTreeMap<u32, bool>,
    text_budget: usize,
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
                stream: self.reference(dict.get(b"Stm")),
                stream_owner: self.reference(dict.get(b"StmOwn")),
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
        let struct_type = dict.get(b"S").and_then(Object::as_name).cloned();
        let standard_type = struct_type
            .as_ref()
            .and_then(|name| role::resolve(self.role_map, name, self.in_pdf2_namespace(dict)));
        let attributes = self.attributes(dict);
        let alt = self.text(dict, b"Alt")?;
        let actual_text = self.text(dict, b"ActualText")?;
        let lang = self.text(dict, b"Lang")?;
        let title = self.text(dict, b"T")?;
        let expansion = self.text(dict, b"E")?;
        self.elements.insert(
            objref.number,
            Element {
                objref,
                struct_type,
                standard_type,
                page: self.reference(dict.get(b"Pg")),
                id: dict.get(b"ID").and_then(as_byte_string),
                stated_parent: self.reference(dict.get(b"P")),
                alt,
                actual_text,
                lang,
                title,
                expansion,
                attributes,
                kids,
            },
        );
        Ok(Some(Kid::Element(objref.number)))
    }

    /// A text-string entry, resolved through a reference. A value that is not
    /// a string, or a reference that does not resolve, is absent, as `/ID` is.
    ///
    /// Only an indirect string is charged to the text budget. A direct one is in
    /// the file once and so bounded by its size; an indirect one can be named by
    /// every element.
    fn text(&mut self, dict: &Dict, key: &[u8]) -> Result<Option<String>> {
        let Some(value) = dict.get(key) else {
            return Ok(None);
        };
        let Some(Object::String(bytes)) = self.doc.resolve(value).ok() else {
            return Ok(None);
        };
        if matches!(value, Object::Ref(_)) {
            self.text_budget = self.text_budget.checked_sub(bytes.len()).ok_or_else(|| {
                malformed("the structure tree holds more text than the reader will hold")
            })?;
        }
        Ok(Some(pdf_text_string(&bytes)))
    }

    /// Whether this element's `/S` is in the PDF 2.0 namespace. ISO 32000-2
    /// 14.7.4.2 puts an element with no `/NS` in the default namespace, which is
    /// the PDF 1.7 one, whatever version the file declares; so `Title`, `Em`,
    /// `Hn` and the rest are standard only for an element that names the 2.0
    /// namespace.
    fn in_pdf2_namespace(&mut self, dict: &Dict) -> bool {
        let Some(ns) = dict.get(b"NS") else {
            return false;
        };
        let number = match ns {
            Object::Ref(objref) => Some(objref.number),
            _ => None,
        };
        if let Some(hit) = number.and_then(|number| self.namespaces.get(&number)) {
            return *hit;
        }
        let is_pdf2 = matches!(
            self.doc.resolve(ns).ok(),
            Some(Object::Dict(namespace))
                if matches!(
                    namespace.get(b"NS"),
                    Some(Object::String(uri)) if uri == PDF2_NAMESPACE
                )
        );
        if let Some(number) = number {
            self.namespaces.insert(number, is_pdf2);
        }
        is_pdf2
    }

    /// `/A`: an attribute object, or an array of them each of which may be
    /// followed by a revision number. An attribute object is a dictionary or a
    /// stream (ISO 32000-1 Table 323); anything else, and a reference that does
    /// not resolve, is skipped.
    fn attributes(&mut self, dict: &Dict) -> Vec<Attribute> {
        let Some(a) = dict.get(b"A") else {
            return Vec::new();
        };
        let number = match a {
            Object::Ref(objref) => Some(objref.number),
            _ => None,
        };
        if let Some(hit) = number.and_then(|number| self.attribute_lists.get(&number)) {
            return hit.clone();
        }
        let items = match self.doc.resolve(a) {
            Ok(Object::Array(items)) => items,
            _ => vec![a.clone()],
        };
        let list: Vec<Attribute> = items
            .iter()
            .filter_map(|item| self.attribute_object(item))
            .map(|entries| Attribute {
                owner: entries.get(b"O").and_then(Object::as_name).cloned(),
                entries,
            })
            .collect();
        if let Some(number) = number {
            self.attribute_lists.insert(number, list.clone());
        }
        list
    }

    fn attribute_object(&mut self, item: &Object) -> Option<Arc<Dict>> {
        let number = match item {
            Object::Ref(objref) => Some(objref.number),
            _ => None,
        };
        if let Some(hit) = number.and_then(|number| self.attribute_objects.get(&number)) {
            return Some(Arc::clone(hit));
        }
        let entries = match self.doc.resolve(item).ok()? {
            Object::Dict(entries) => entries,
            Object::Stream(stream) => stream.dict,
            _ => return None,
        };
        let entries = Arc::new(entries);
        if let Some(number) = number {
            self.attribute_objects.insert(number, Arc::clone(&entries));
        }
        Some(entries)
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
