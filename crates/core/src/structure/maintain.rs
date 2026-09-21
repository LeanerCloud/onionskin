//! The maintenance hook every `DocumentEdit` passes through.
//!
//! Three operations, which is what M3's edits need and no more: remove a page's
//! elements and their `/ParentTree` entries, reorder the root's `/K` to match a
//! new page order, and attach an `/Annot` element with a fresh
//! `/StructParent`.
//!
//! **One choke point, not one per tool.** Putting structure maintenance behind
//! the closed `DocumentEdit` enum is what makes "every edit maintains the tree"
//! a property that can be checked rather than a convention each new tool has to
//! remember.
//!
//! **The untagged path says so.** Every operation returns [`Maintenance`],
//! which reports whether it was a no-op because the document carries no
//! structure tree. A caller that cannot tell "nothing to do" from "done"
//! cannot assert the path was taken, and the P4 verification list requires
//! exactly that assertion.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Name, ObjRef, Object};

use super::read::{Kid, ParentEntry, Structure, StructureTree};
use crate::edit::Transaction;
use crate::Result;

/// What an operation did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Maintenance {
    /// The document carries no structure tree. Nothing was written, and that
    /// is the correct outcome rather than a failure.
    Untagged,
    /// The tree was read and needed no change for this edit.
    NoChange,
    /// The tree was rewritten.
    Changed,
}

impl Maintenance {
    pub fn wrote(self) -> bool {
        matches!(self, Maintenance::Changed)
    }

    pub fn was_untagged(self) -> bool {
        matches!(self, Maintenance::Untagged)
    }
}

/// Remove every element whose content lived on `page`, and the `/ParentTree`
/// entries that named them.
///
/// An element is removed when its own `/Pg` names the page, or when every one
/// of its kids did. An element with kids on other pages survives with the
/// page's kids dropped, because removing it would take surviving content with
/// it.
pub(crate) fn remove_page(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
) -> Result<Maintenance> {
    remove_pages(tx, structure, &[page])
}

/// [`remove_page`] for several pages at once.
///
/// **One call, not one per page.** Each removal rewrites the root's `/K` and
/// the `/ParentTree` from the tree it was handed; handed the same tree twice,
/// the second removal writes back the element the first one took out.
pub(crate) fn remove_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    pages: &[ObjRef],
) -> Result<Maintenance> {
    let Some(tree) = structure.tree() else {
        return Ok(Maintenance::Untagged);
    };

    let pages: BTreeSet<u32> = pages.iter().map(|page| page.number).collect();
    let doomed = elements_on_pages(tree, &pages);
    if doomed.is_empty() {
        return Ok(Maintenance::NoChange);
    }

    for number in &doomed {
        let Some(element) = tree.elements.get(number) else {
            continue;
        };
        // M3 frees no object number, so a removal is a rewrite of the referrer
        // rather than a delete. The element becomes an empty structure element
        // with no page and no kids, and every reference to it is cleared below.
        let mut dict = Dict::new();
        dict.set(Name::new("Type"), Object::name("StructElem"));
        if let Some(struct_type) = &element.struct_type {
            dict.set(Name::new("S"), Object::Name(struct_type.clone()));
        }
        tx.put_object(*number, element.objref.generation, Object::Dict(dict))?;
    }

    rewrite_kids_without(tx, tree, &doomed, &pages)?;
    rewrite_parent_tree_without(tx, tree, &doomed)?;
    rewrite_id_tree_without(tx, tree, &doomed)?;
    Ok(Maintenance::Changed)
}

/// Elements whose surviving content is entirely on the removed pages.
fn elements_on_pages(tree: &StructureTree, pages: &BTreeSet<u32>) -> BTreeSet<u32> {
    let mut doomed = BTreeSet::new();
    for (number, element) in &tree.elements {
        let own_page = element.page.is_some_and(|pg| pages.contains(&pg.number));
        let kids_elsewhere = element.kids.iter().any(|kid| match kid {
            Kid::MarkedContent { page: Some(pg), .. } | Kid::Object { page: Some(pg), .. } => {
                !pages.contains(&pg.number)
            }
            // A child element is judged on its own account, so a parent is not
            // kept alive by one.
            Kid::Element(_) => false,
            Kid::Mcid(_) => false,
            Kid::MarkedContent { page: None, .. } | Kid::Object { page: None, .. } => false,
        });
        if own_page && !kids_elsewhere {
            doomed.insert(*number);
        }
    }
    doomed
}

/// Drop removed elements and page-bound kids from every surviving element's
/// `/K`, and from the root's.
fn rewrite_kids_without(
    tx: &mut Transaction<'_>,
    tree: &StructureTree,
    doomed: &BTreeSet<u32>,
    pages: &BTreeSet<u32>,
) -> Result<()> {
    for (number, element) in &tree.elements {
        if doomed.contains(number) {
            continue;
        }
        let kept: Vec<&Kid> = element
            .kids
            .iter()
            .filter(|kid| keeps(kid, doomed, pages))
            .collect();
        if kept.len() == element.kids.len() {
            continue;
        }
        let Some(state) = tx.object(*number)? else {
            continue;
        };
        let Some(dict) = state.object.as_dict() else {
            continue;
        };
        let mut dict = dict.clone();
        dict.set(Name::new("K"), kids_array(&kept, tree));
        tx.put_object(*number, state.generation, Object::Dict(dict))?;
    }

    let kept: Vec<&Kid> = tree
        .roots
        .iter()
        .filter(|kid| keeps(kid, doomed, pages))
        .collect();
    if kept.len() != tree.roots.len() {
        set_root_kids(tx, tree, &kept)?;
    }
    Ok(())
}

fn keeps(kid: &Kid, doomed: &BTreeSet<u32>, pages: &BTreeSet<u32>) -> bool {
    match kid {
        Kid::Element(number) => !doomed.contains(number),
        Kid::MarkedContent { page: Some(pg), .. } | Kid::Object { page: Some(pg), .. } => {
            !pages.contains(&pg.number)
        }
        Kid::Mcid(_) => false,
        Kid::MarkedContent { page: None, .. } | Kid::Object { page: None, .. } => true,
    }
}

/// Rebuild a `/K` array from kids the reader flattened.
fn kids_array(kids: &[&Kid], tree: &StructureTree) -> Object {
    Object::Array(kids.iter().map(|kid| kid_object(kid, tree)).collect())
}

fn kid_object(kid: &Kid, tree: &StructureTree) -> Object {
    match kid {
        Kid::Mcid(mcid) => Object::Integer(*mcid),
        Kid::Element(number) => {
            let generation = tree
                .elements
                .get(number)
                .map_or(0, |element| element.objref.generation);
            Object::Ref(ObjRef::new(*number, generation))
        }
        Kid::MarkedContent { page, mcid } => {
            let mut dict = Dict::new();
            dict.set(Name::new("Type"), Object::name("MCR"));
            if let Some(page) = page {
                dict.set(Name::new("Pg"), Object::Ref(*page));
            }
            dict.set(Name::new("MCID"), Object::Integer(*mcid));
            Object::Dict(dict)
        }
        Kid::Object { page, object } => {
            let mut dict = Dict::new();
            dict.set(Name::new("Type"), Object::name("OBJR"));
            if let Some(page) = page {
                dict.set(Name::new("Pg"), Object::Ref(*page));
            }
            dict.set(Name::new("Obj"), Object::Ref(*object));
            Object::Dict(dict)
        }
    }
}

/// Clear the slots that named removed elements, keeping the key space intact.
///
/// The keys are not renumbered. Renumbering would have to rewrite every
/// `/StructParents` and `/StructParent` in the document in the same
/// transaction, and a mapping that is well-formed but off by one is precisely
/// the "well-formed and wrong" state this package's review risk names. Leaving
/// a key with an empty slot list is well-formed and right.
fn rewrite_parent_tree_without(
    tx: &mut Transaction<'_>,
    tree: &StructureTree,
    doomed: &BTreeSet<u32>,
) -> Result<()> {
    let mut nums: Vec<Object> = Vec::new();
    let mut changed = false;
    for (key, entry) in &tree.parent_tree {
        let (value, entry_changed) = parent_entry_without(entry, doomed);
        changed |= entry_changed;
        nums.push(Object::Integer(*key));
        nums.push(value);
    }
    if !changed {
        return Ok(());
    }

    let Some(state) = tx.object(tree.root.number)? else {
        return Ok(());
    };
    let Some(root_dict) = state.object.as_dict() else {
        return Ok(());
    };
    let mut parent_tree = Dict::new();
    parent_tree.set(Name::new("Nums"), Object::Array(nums));
    let mut root_dict = root_dict.clone();
    // The rebuilt tree is a single flat node, which is a legal number tree and
    // the shape the reader hands back. A balanced rebuild would be a
    // performance choice with no correctness content at M3's sizes.
    root_dict.set(Name::new("ParentTree"), Object::Dict(parent_tree));
    tx.put_object(tree.root.number, state.generation, Object::Dict(root_dict))?;
    Ok(())
}

fn parent_entry_without(entry: &ParentEntry, doomed: &BTreeSet<u32>) -> (Object, bool) {
    match entry {
        ParentEntry::Element(element) => {
            if doomed.contains(&element.number) {
                (Object::Null, true)
            } else {
                (Object::Ref(*element), false)
            }
        }
        ParentEntry::Slots(slots) => {
            let mut changed = false;
            let items = slots
                .iter()
                .map(|slot| match slot {
                    Some(element) if doomed.contains(&element.number) => {
                        changed = true;
                        Object::Null
                    }
                    Some(element) => Object::Ref(*element),
                    None => Object::Null,
                })
                .collect();
            (Object::Array(items), changed)
        }
    }
}

/// Drop `/IDTree` entries naming removed elements, so the invariant's
/// `IdTreeDangling` clause cannot fire on our own work.
fn rewrite_id_tree_without(
    tx: &mut Transaction<'_>,
    tree: &StructureTree,
    doomed: &BTreeSet<u32>,
) -> Result<()> {
    let kept: BTreeMap<&Vec<u8>, &ObjRef> = tree
        .id_tree
        .iter()
        .filter(|(_, element)| !doomed.contains(&element.number))
        .collect();
    if kept.len() == tree.id_tree.len() {
        return Ok(());
    }

    let mut names = Vec::with_capacity(kept.len() * 2);
    for (id, element) in kept {
        names.push(Object::String(id.clone()));
        names.push(Object::Ref(*element));
    }
    let Some(state) = tx.object(tree.root.number)? else {
        return Ok(());
    };
    let Some(root_dict) = state.object.as_dict() else {
        return Ok(());
    };
    let mut id_tree = Dict::new();
    id_tree.set(Name::new("Names"), Object::Array(names));
    let mut root_dict = root_dict.clone();
    root_dict.set(Name::new("IDTree"), Object::Dict(id_tree));
    tx.put_object(tree.root.number, state.generation, Object::Dict(root_dict))?;
    Ok(())
}

/// Reorder the root's `/K` to match a new page order.
///
/// `order` names the pages in their new order. Root kids are grouped by the
/// page their subtree sits on and emitted in that order; a kid whose page
/// cannot be determined keeps its position relative to the others, at the end,
/// because moving it would be a guess.
pub(crate) fn reorder_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    order: &[ObjRef],
) -> Result<Maintenance> {
    let Some(tree) = structure.tree() else {
        return Ok(Maintenance::Untagged);
    };

    let mut position: BTreeMap<u32, usize> = BTreeMap::new();
    for (index, page) in order.iter().enumerate() {
        position.insert(page.number, index);
    }

    // A kid on a page that is not in the order sits on a removed page: its
    // element was emptied by the removal, and carrying it over from the tree
    // as it was before would put it back in the reading order.
    let mut keyed: Vec<(usize, usize, &Kid)> = tree
        .roots
        .iter()
        .enumerate()
        .filter_map(|(original, kid)| {
            let rank = match page_of(kid, tree) {
                Some(page) => *position.get(&page.number)?,
                None => usize::MAX,
            };
            Some((rank, original, kid))
        })
        .collect();
    keyed.sort_by_key(|(rank, original, _)| (*rank, *original));

    let reordered: Vec<&Kid> = keyed.iter().map(|(_, _, kid)| *kid).collect();
    if reordered.len() == tree.roots.len()
        && reordered
            .iter()
            .zip(tree.roots.iter())
            .all(|(left, right)| *left == right)
    {
        return Ok(Maintenance::NoChange);
    }
    set_root_kids(tx, tree, &reordered)?;
    Ok(Maintenance::Changed)
}

/// The page a root kid's subtree sits on: its own if it names one, else the
/// first page any descendant names.
fn page_of(kid: &Kid, tree: &StructureTree) -> Option<ObjRef> {
    match kid {
        Kid::MarkedContent { page, .. } | Kid::Object { page, .. } => *page,
        Kid::Mcid(_) => None,
        Kid::Element(number) => descendant_page(tree, *number, 0),
    }
}

fn descendant_page(tree: &StructureTree, number: u32, depth: usize) -> Option<ObjRef> {
    if depth > 64 {
        return None;
    }
    let element = tree.elements.get(&number)?;
    if let Some(page) = element.page {
        return Some(page);
    }
    element.kids.iter().find_map(|kid| match kid {
        Kid::MarkedContent { page, .. } | Kid::Object { page, .. } => *page,
        Kid::Element(child) => descendant_page(tree, *child, depth + 1),
        Kid::Mcid(_) => None,
    })
}

fn set_root_kids(tx: &mut Transaction<'_>, tree: &StructureTree, kids: &[&Kid]) -> Result<()> {
    let Some(state) = tx.object(tree.root.number)? else {
        return Ok(());
    };
    let Some(root_dict) = state.object.as_dict() else {
        return Ok(());
    };
    let mut root_dict = root_dict.clone();
    root_dict.set(Name::new("K"), kids_array(kids, tree));
    tx.put_object(tree.root.number, state.generation, Object::Dict(root_dict))
}

/// Attach an `/Annot` structure element for an annotation, with a fresh
/// `/StructParent` index and the matching `/ParentTree` entry.
///
/// Returns the index written to the annotation's `/StructParent`, so the caller
/// can put it on the annotation in the same transaction.
pub(crate) fn attach_annotation(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    page: ObjRef,
    annotation: ObjRef,
) -> Result<(Maintenance, Option<i64>)> {
    let Some(tree) = structure.tree() else {
        return Ok((Maintenance::Untagged, None));
    };

    let key = next_parent_key(tree);
    let number = tx.reserve();
    let element = ObjRef::new(number, 0);

    let mut object_reference = Dict::new();
    object_reference.set(Name::new("Type"), Object::name("OBJR"));
    object_reference.set(Name::new("Pg"), Object::Ref(page));
    object_reference.set(Name::new("Obj"), Object::Ref(annotation));

    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("StructElem"));
    dict.set(Name::new("S"), Object::name("Annot"));
    dict.set(Name::new("P"), Object::Ref(tree.root));
    dict.set(Name::new("Pg"), Object::Ref(page));
    dict.set(Name::new("K"), Object::Dict(object_reference));
    tx.put_object(number, 0, Object::Dict(dict))?;

    let mut roots: Vec<Kid> = tree.roots.clone();
    roots.push(Kid::Element(number));
    let root_kids: Vec<&Kid> = roots.iter().collect();

    let mut nums: Vec<Object> = Vec::new();
    for (existing, entry) in &tree.parent_tree {
        nums.push(Object::Integer(*existing));
        nums.push(match entry {
            ParentEntry::Element(element) => Object::Ref(*element),
            ParentEntry::Slots(slots) => Object::Array(
                slots
                    .iter()
                    .map(|slot| slot.map_or(Object::Null, Object::Ref))
                    .collect(),
            ),
        });
    }
    nums.push(Object::Integer(key));
    nums.push(Object::Ref(element));

    let Some(state) = tx.object(tree.root.number)? else {
        return Ok((Maintenance::NoChange, None));
    };
    let Some(root_dict) = state.object.as_dict() else {
        return Ok((Maintenance::NoChange, None));
    };
    let mut root_dict = root_dict.clone();
    let mut parent_tree = Dict::new();
    parent_tree.set(Name::new("Nums"), Object::Array(nums));
    root_dict.set(Name::new("ParentTree"), Object::Dict(parent_tree));
    root_dict.set(Name::new("K"), kids_array(&root_kids, tree));
    root_dict.set(Name::new("ParentTreeNextKey"), Object::Integer(key + 1));
    tx.put_object(tree.root.number, state.generation, Object::Dict(root_dict))?;

    Ok((Maintenance::Changed, Some(key)))
}

/// The next free `/ParentTree` key: the file's own `/ParentTreeNextKey` when it
/// states one past every key in use, else one past the highest key in use.
/// Trusting a stated key that is already taken would produce a tree that is
/// well-formed and points at the wrong element.
fn next_parent_key(tree: &StructureTree) -> i64 {
    let highest = tree.parent_tree.keys().copied().max().unwrap_or(-1);
    match tree.parent_tree_next_key {
        Some(stated) if stated > highest => stated,
        _ => highest + 1,
    }
}
