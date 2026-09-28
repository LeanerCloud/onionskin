//! Bookmark authoring: the inverse of the reader, over the same chain walk.
//!
//! A bookmark is addressed by its **path**: its index among its siblings at
//! each level, from the top, which is exactly the shape
//! [`Document::outline`](crate::Document::outline) returns. The whole outline
//! is loaded through [`Walk`], changed as a tree in memory, and stored back
//! with every chain key (`/Parent`, `/Prev`, `/Next`, `/First`, `/Last`)
//! and every `/Count` derived from the tree. An item whose dictionary comes
//! out the same is not written, so an edit to one bookmark appends one
//! bookmark's worth of objects plus its neighbours, not the outline.
//!
//! Three rules decided here rather than left to fall out of the code:
//!
//! - **Destinations are explicit**: `[page /XYZ null null null]`, naming the
//!   page object. Not a named destination, because P5's page fix-ups follow
//!   an explicit destination to its page and drop the bookmark with the page,
//!   which is the behaviour a user expects when they delete it.
//! - **Deleting a bookmark deletes its children.** Acrobat does the same,
//!   and P5's fix-up applies the same rule when a page goes. The dictionaries
//!   stay in the file unreferenced; nothing is freed.
//! - **`/Count` keeps its sign.** A positive count is an open item, and a
//!   recount that wrote magnitudes would expand every collapsed bookmark. An
//!   item that gains its first child opens, so the new child is visible.

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::{Walk, MAX_DEPTH};
use crate::annots::text_string;
use crate::edit::Transaction;
use crate::pages::{dict_at, page_ref};
use crate::{Error, Result};

/// One item of the tree being edited. Index 0 is the `/Outlines` root.
struct Node {
    objref: ObjRef,
    dict: Dict,
    /// What the file held, to write only what changed. `None` for an item
    /// this edit created.
    original: Option<Dict>,
    children: Vec<usize>,
}

struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// The outline as the transaction sees it, with a new, empty root when
    /// the document has none. The catalog is pointed at a new root here.
    fn load(tx: &mut Transaction<'_>) -> Result<Self> {
        let catalog_ref = tx
            .trailer_value(b"Root")
            .and_then(|root| root.as_reference())
            .ok_or(Error::NoCatalog)?;
        let mut catalog = dict_at(tx, catalog_ref)?;
        let root = match catalog.get(b"Outlines").and_then(Object::as_reference) {
            Some(root) if dict_at(tx, root).is_ok() => root,
            _ => {
                let number = tx.reserve();
                let root = ObjRef::new(number, 0);
                let mut dict = Dict::new();
                dict.set(Name::new("Type"), Object::name("Outlines"));
                tx.put_object(number, 0, Object::Dict(dict))?;
                catalog.set(Name::new("Outlines"), Object::Ref(root));
                tx.put_object(
                    catalog_ref.number,
                    catalog_ref.generation,
                    Object::Dict(catalog),
                )?;
                root
            }
        };
        let dict = dict_at(tx, root)?;
        let mut tree = Tree {
            nodes: vec![Node {
                objref: root,
                original: Some(dict.clone()),
                dict,
                children: Vec::new(),
            }],
        };
        let mut walk = Walk::new();
        tree.load_children(tx, 0, &mut walk, 0)?;
        Ok(tree)
    }

    fn load_children(
        &mut self,
        tx: &Transaction<'_>,
        parent: usize,
        walk: &mut Walk,
        depth: usize,
    ) -> Result<()> {
        if depth >= MAX_DEPTH {
            return Ok(());
        }
        let parent_dict = self.nodes[parent].dict.clone();
        let chain = walk.children(&parent_dict, &mut |node| Ok(dict_at(tx, node).ok()))?;
        for (objref, dict) in chain {
            let index = self.nodes.len();
            self.nodes.push(Node {
                objref,
                original: Some(dict.clone()),
                dict,
                children: Vec::new(),
            });
            self.nodes[parent].children.push(index);
            self.load_children(tx, index, walk, depth + 1)?;
        }
        Ok(())
    }

    /// The node at `path`, or why there is none.
    fn find(&self, path: &[usize]) -> Result<usize> {
        let mut at = 0;
        for &step in path {
            at = *self.nodes[at]
                .children
                .get(step)
                .ok_or_else(|| Error::NoSuchBookmark(path.to_vec()))?;
        }
        Ok(at)
    }

    /// The parent of the node at a non-empty `path`, and its position there.
    fn parent_of(&self, path: &[usize]) -> Result<(usize, usize)> {
        let (&last, parent_path) = path
            .split_last()
            .ok_or_else(|| Error::NoSuchBookmark(Vec::new()))?;
        let parent = self.find(parent_path)?;
        if last >= self.nodes[parent].children.len() {
            return Err(Error::NoSuchBookmark(path.to_vec()));
        }
        Ok((parent, last))
    }

    /// Every chain key and count, from the tree; then every item whose
    /// dictionary changed, written.
    fn store(mut self, tx: &mut Transaction<'_>) -> Result<()> {
        self.relink(0);
        self.recount(0);
        let reachable = self.reachable();
        for (index, node) in self.nodes.iter().enumerate() {
            if node.original.as_ref() != Some(&node.dict) && reachable[index] {
                tx.put_object(
                    node.objref.number,
                    node.objref.generation,
                    Object::Dict(node.dict.clone()),
                )?;
            }
        }
        Ok(())
    }

    /// Which nodes are still in the tree. A removed item keeps its node here
    /// but is never written: its old dictionary stays as it was.
    fn reachable(&self) -> Vec<bool> {
        let mut reachable = vec![false; self.nodes.len()];
        let mut stack = vec![0];
        while let Some(at) = stack.pop() {
            reachable[at] = true;
            stack.extend(&self.nodes[at].children);
        }
        reachable
    }

    fn relink(&mut self, parent: usize) {
        let children = self.nodes[parent].children.clone();
        let parent_ref = self.nodes[parent].objref;
        let set = |dict: &mut Dict, key: &str, value: Option<ObjRef>| match value {
            Some(objref) => dict.set(Name::new(key), Object::Ref(objref)),
            None => {
                dict.remove(key.as_bytes());
            }
        };
        let refs: Vec<ObjRef> = children
            .iter()
            .map(|&child| self.nodes[child].objref)
            .collect();
        {
            let dict = &mut self.nodes[parent].dict;
            set(dict, "First", refs.first().copied());
            set(dict, "Last", refs.last().copied());
        }
        for (position, &child) in children.iter().enumerate() {
            let previous = position.checked_sub(1).map(|at| refs[at]);
            let next = refs.get(position + 1).copied();
            let dict = &mut self.nodes[child].dict;
            dict.set(Name::new("Parent"), Object::Ref(parent_ref));
            set(dict, "Prev", previous);
            set(dict, "Next", next);
            self.relink(child);
        }
    }

    /// Visible descendants of `at`, writing each `/Count` on the way.
    fn recount(&mut self, at: usize) -> i64 {
        let children = self.nodes[at].children.clone();
        let mut visible = 0;
        for &child in &children {
            let below = self.recount(child);
            visible += 1;
            if self.is_open(child) {
                visible += below;
            }
        }
        let is_root = at == 0;
        let open = self.is_open(at);
        let dict = &mut self.nodes[at].dict;
        if children.is_empty() && !is_root {
            dict.remove(b"Count");
        } else {
            let count = if open || is_root { visible } else { -visible };
            dict.set(Name::new("Count"), Object::Integer(count));
        }
        // What an ancestor counts through this node: all of it, when open.
        visible
    }

    /// Open unless the file closed it: a negative `/Count`. An item with no
    /// count yet opens, which is how a new child shows up under it.
    fn is_open(&self, at: usize) -> bool {
        self.nodes[at]
            .dict
            .get(b"Count")
            .and_then(Object::as_integer)
            .is_none_or(|count| count >= 0)
    }
}

/// An explicit destination to the top of `page`.
fn destination(tx: &Transaction<'_>, page: usize) -> Result<Object> {
    let target = page_ref(tx, page)?;
    Ok(Object::Array(vec![
        Object::Ref(target),
        Object::name("XYZ"),
        Object::Null,
        Object::Null,
        Object::Null,
    ]))
}

fn set_destination(tx: &Transaction<'_>, dict: &mut Dict, page: Option<usize>) -> Result<()> {
    dict.remove(b"A");
    match page {
        Some(page) => dict.set(Name::new("Dest"), destination(tx, page)?),
        None => {
            dict.remove(b"Dest");
        }
    }
    Ok(())
}

/// Add a bookmark titled `title` under the bookmark at `parent` (the top
/// level when empty), at `index` among its children or last. Returns the new
/// bookmark's path.
pub fn add_bookmark(
    tx: &mut Transaction<'_>,
    parent: &[usize],
    index: Option<usize>,
    title: &str,
    page: Option<usize>,
) -> Result<Vec<usize>> {
    let mut tree = Tree::load(tx)?;
    let at = tree.find(parent)?;
    let siblings = tree.nodes[at].children.len();
    let index = index.unwrap_or(siblings);
    if index > siblings {
        return Err(Error::NoSuchBookmark([parent, &[index]].concat()));
    }
    let mut dict = Dict::new();
    dict.set(Name::new("Title"), text_string(title));
    set_destination(tx, &mut dict, page)?;
    let number = tx.reserve();
    let node = tree.nodes.len();
    tree.nodes.push(Node {
        objref: ObjRef::new(number, 0),
        dict,
        original: None,
        children: Vec::new(),
    });
    tree.nodes[at].children.insert(index, node);
    tree.store(tx)?;
    Ok([parent, &[index]].concat())
}

/// Give the bookmark at `path` a new title. Its destination is untouched.
pub fn rename_bookmark(tx: &mut Transaction<'_>, path: &[usize], title: &str) -> Result<()> {
    let mut tree = Tree::load(tx)?;
    let at = tree.find(path)?;
    if at == 0 {
        return Err(Error::NoSuchBookmark(Vec::new()));
    }
    tree.nodes[at]
        .dict
        .set(Name::new("Title"), text_string(title));
    tree.store(tx)
}

/// Point the bookmark at `path` at the top of `page`, or at nothing. A
/// `/GoTo` or any other action it had is replaced.
pub fn set_bookmark_destination(
    tx: &mut Transaction<'_>,
    path: &[usize],
    page: Option<usize>,
) -> Result<()> {
    let mut tree = Tree::load(tx)?;
    let at = tree.find(path)?;
    if at == 0 {
        return Err(Error::NoSuchBookmark(Vec::new()));
    }
    let mut dict = tree.nodes[at].dict.clone();
    set_destination(tx, &mut dict, page)?;
    tree.nodes[at].dict = dict;
    tree.store(tx)
}

/// Set the bookmark at `path`'s style: `/F` for bold and italic, and `/C` for
/// the colour.
///
/// `/F` is ISO 32000-1 12.3.3's bit field, and the bits are worth rather than
/// flags: italic is 1, bold is 2, so both together are 3. Reading them as flags
/// would make "bold, not italic" mean italic instead.
///
/// A colour of `None` removes `/C`, which is how a bookmark goes back to the
/// colour its renderer would otherwise choose. Colours are 0..1 floats, which
/// is what `/C` carries.
pub fn set_bookmark_style(
    tx: &mut Transaction<'_>,
    path: &[usize],
    bold: bool,
    italic: bool,
    colour: Option<[f64; 3]>,
) -> Result<()> {
    let mut tree = Tree::load(tx)?;
    let at = tree.find(path)?;
    if at == 0 {
        return Err(Error::NoSuchBookmark(Vec::new()));
    }
    let mut flags = 0i64;
    if italic {
        flags |= 1;
    }
    if bold {
        flags |= 2;
    }
    let dict = &mut tree.nodes[at].dict;
    if flags == 0 {
        dict.remove(b"F");
    } else {
        dict.set(Name::new("F"), Object::Integer(flags));
    }
    match colour {
        Some(rgb) => dict.set(
            Name::new("C"),
            Object::Array(rgb.map(|channel| Object::Real(channel)).into()),
        ),
        None => {
            dict.remove(b"C");
        }
    }
    tree.store(tx)
}

/// Delete the bookmark at `path` and every bookmark under it.
pub fn delete_bookmark(tx: &mut Transaction<'_>, path: &[usize]) -> Result<()> {
    let mut tree = Tree::load(tx)?;
    let (parent, position) = tree.parent_of(path)?;
    tree.nodes[parent].children.remove(position);
    tree.store(tx)
}

/// Move the bookmark at `path`, with everything under it, to `index` among
/// the children of the bookmark at `new_parent` (the top level when empty).
/// `new_parent` is read before the move. Returns the bookmark's new path.
///
/// Nesting a bookmark under the one above it is this with that sibling as
/// the new parent. Moving a bookmark into its own subtree is refused: it
/// would take the subtree out of the outline altogether.
pub fn move_bookmark(
    tx: &mut Transaction<'_>,
    path: &[usize],
    new_parent: &[usize],
    index: usize,
) -> Result<Vec<usize>> {
    if new_parent.starts_with(path) {
        return Err(Error::NoSuchBookmark(new_parent.to_vec()));
    }
    let mut tree = Tree::load(tx)?;
    let (old_parent, position) = tree.parent_of(path)?;
    let target = tree.find(new_parent)?;
    let node = tree.nodes[old_parent].children.remove(position);
    let siblings = tree.nodes[target].children.len();
    if index > siblings {
        return Err(Error::NoSuchBookmark([new_parent, &[index]].concat()));
    }
    tree.nodes[target].children.insert(index, node);
    // The path the new parent has after the removal: an earlier sibling of
    // any of its ancestors that moved away shifts it up by one.
    let mut new_path = Vec::with_capacity(new_parent.len() + 1);
    locate(&tree, 0, tree.nodes[target].objref, &mut new_path);
    new_path.push(index);
    tree.store(tx)?;
    Ok(new_path)
}

/// The path to `objref` from `at`, into `path`. True when found.
fn locate(tree: &Tree, at: usize, objref: ObjRef, path: &mut Vec<usize>) -> bool {
    if tree.nodes[at].objref == objref {
        return true;
    }
    for (position, &child) in tree.nodes[at].children.iter().enumerate() {
        path.push(position);
        if locate(tree, child, objref, path) {
            return true;
        }
        path.pop();
    }
    false
}
