//! `core::pages`: every page-set operation as one transformation.
//!
//! Delete, reorder, insert and import are all the same thing, a new page
//! order, so there is one entry point, [`rewrite_page_tree`], and no separate
//! `delete_page` to get subtly different.
//!
//! # What it produces
//!
//! A single **flat** `/Pages` node reusing the original root's object number,
//! with each surviving page dict rewritten so that its inheritance is
//! materialized, its `/Parent` points at the new node, and everything else on
//! it is carried through untouched. That is the "unimplemented means untouched"
//! rule at the page level: a `/Tabs`, a `/Group`, a `/UserUnit` or a private
//! key this crate has never heard of survives a page-set change unchanged.
//!
//! # What happens to a removed page: nothing
//!
//! **A removed page is removed by not being listed, and nothing else happens to
//! it.** Its dict is not rewritten and not freed; neither are its annotations,
//! their appearance streams, their `/Popup` partners, its content streams, or
//! the internal `/Pages` nodes it hung under. They become one internally
//! consistent garbage subtree that nothing reaches.
//!
//! This is T5's free-nothing rule, and it is the reason **no `delete_object`
//! call appears anywhere in this module**. Freeing them is what manufactures
//! dangling references: an object number handed out again is a reference that
//! used to mean one thing and now means another, and every incremental section
//! ever appended to the file still names the old one.
//!
//! # The seven document-level fix-ups
//!
//! A page-set change breaks seven other parts of a document, and **each is its
//! own module with its own fixture and its own test**, because each walks a
//! different part of the file and no one of them finds another's case:
//!
//! | Module | What it repairs |
//! | --- | --- |
//! | [`labels`] | `/PageLabels`, a number tree keyed on page index |
//! | [`destinations`] | `/Dests` and the `/Names /Dests` **name tree** |
//! | [`outline`] | the bookmark chain: `/Prev`, `/Next`, `/First`, `/Last`, `/Count` |
//! | [`links`] | `/Link` annotations on surviving pages naming a removed one |
//! | [`fields`] | `/AcroForm /Fields` whose widgets were on a removed page |
//! | [`threads`] | article bead rings, re-linked through `/N` and `/V` |
//! | [`actions`] | `/OpenAction` and page-level `/AA` |
//!
//! The `/Count` on the new `/Pages` node is not one of the seven: it is the
//! length of the list.

mod actions;
mod assemble;
mod boxes;
mod destinations;
mod fields;
mod import;
mod inherit;
mod labels;
mod links;
mod marks;
mod ops;
mod outline;
mod print_form;
mod rewrite;

pub(crate) use rewrite::{catalog_ref, dict_at, resolve};

/// Every object number that is part of the document's structure: the catalog,
/// the page tree root, every intermediate `/Pages` node and every page.
///
/// The catalog is not reachable from the walk, so it is added here. A page-tree
/// failure is propagated rather than swallowed: a caller asking this question
/// is asking whether some other object number is structure, and an incomplete
/// answer would let a structural target through.
pub(crate) fn structural_numbers(
    tx: &crate::edit::Transaction<'_>,
) -> crate::Result<std::collections::BTreeSet<u32>> {
    let catalog = catalog_ref(tx)?;
    let mut resolve = rewrite::resolver(tx);
    let root = match resolve(catalog.number)? {
        Some(onionskin_cos::Object::Dict(dict)) => dict
            .get(b"Pages")
            .and_then(onionskin_cos::Object::as_reference),
        _ => None,
    }
    .ok_or(crate::Error::NoPageTree)?;
    let mut numbers = inherit::node_numbers(root, &mut resolve)?;
    numbers.insert(catalog.number);
    Ok(numbers)
}

/// The resources page `page` has, its own or inherited.
pub(crate) fn inherited_resources(
    tx: &crate::edit::Transaction<'_>,
    page: onionskin_cos::ObjRef,
) -> crate::Result<Option<onionskin_cos::Object>> {
    Ok(ops::leaves(tx)?
        .into_iter()
        .find(|leaf| leaf.objref == page)
        .and_then(|leaf| leaf.inherited.resources))
}

/// Every `/ParentTree` key a page's `/StructParents` or an annotation's
/// `/StructParent` holds: keys taken whether the tree lists them or not.
pub(crate) fn struct_parent_keys(
    tx: &crate::edit::Transaction<'_>,
) -> crate::Result<std::collections::BTreeSet<i64>> {
    use onionskin_cos::Object;
    let mut keys = std::collections::BTreeSet::new();
    for leaf in ops::leaves(tx)? {
        keys.extend(leaf.dict.get(b"StructParents").and_then(Object::as_integer));
        let annots = match leaf.dict.get(b"Annots") {
            Some(Object::Ref(objref)) => rewrite::object_at(tx, *objref)?,
            other => other.cloned(),
        };
        for annot in annots
            .as_ref()
            .and_then(Object::as_array)
            .into_iter()
            .flatten()
        {
            let dict = match annot {
                Object::Ref(objref) => rewrite::object_at(tx, *objref)?,
                other => Some(other.clone()),
            };
            keys.extend(
                dict.as_ref()
                    .and_then(Object::as_dict)
                    .and_then(|dict| dict.get(b"StructParent"))
                    .and_then(Object::as_integer),
            );
        }
    }
    Ok(keys)
}

/// Draw `content` after everything page dictionary `page` draws, as a
/// content stream of its own, with the page's own content guarded so its
/// leftover graphics state does not reach it.
pub(crate) fn append_content(
    tx: &mut crate::edit::Transaction<'_>,
    page: &mut onionskin_cos::Dict,
    content: Vec<u8>,
) -> crate::Result<()> {
    use onionskin_cos::{Dict, Name, ObjRef, Object, Stream};
    let mut parts = marks::contents::parts(tx, page.get(b"Contents"))?;
    let mut dict = Dict::new();
    dict.set(Name::new("Length"), Object::Integer(content.len() as i64));
    let number = tx.reserve();
    tx.put_object(number, 0, Object::Stream(Stream { dict, raw: content }))?;
    marks::contents::insert(tx, &mut parts, Object::Ref(ObjRef::new(number, 0)), false)?;
    page.set(Name::new("Contents"), Object::Array(parts));
    Ok(())
}
mod threads;
mod thumbs;
mod tree;

pub use assemble::{Assembled, Assembly, Tagging, Untagged};
pub use boxes::{
    boxed, resized, set_media_size, set_page_box, shown_margins, Margins, PageBox, MIN_BOX_SIZE,
};
pub use import::{extract_pages, import_page_as_form, import_pages};
pub use marks::{
    add_page_marks, mark_settings, marked_pages, page_marks, remove_page_marks, shown, MarkKind,
    PageMark, ShownSpace,
};
pub(crate) use ops::{current_page_count, page_ref, page_refs};
pub use ops::{
    delete_pages, insert_blank_pages, insert_pages_from, move_pages, page_count,
    replace_pages_from, rotate_pages, set_page_labels, LabelRange, LabelStyle,
};
pub use print_form::import_page_for_print;
pub use rewrite::{rewrite_page_tree, PageSource, Rewrite};
pub use thumbs::{embed_thumbnails, remove_thumbnails, THUMBNAIL_SIDE};
