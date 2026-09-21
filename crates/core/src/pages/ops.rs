//! The page-organization operations, each a new page order handed to
//! [`rewrite_page_tree`], plus the two that change something other than order:
//! rotation and page labels.
//!
//! Every function here runs inside the caller's one transaction, so each is a
//! single undo entry however many objects it writes.
//!
//! The ones that change the page order take the document's structure tree,
//! **as the transaction sees it**: the session's current tree, read from its
//! preview, not the file's. P4's hooks rewrite structure elements from the
//! tree they are given, so a tree read from the file would put back what an
//! earlier edit in the same session removed. `core::Document::edit_pages`
//! reads the right one; a caller with a bare `EditSession` over an unedited
//! base can read it from the base, which is then the same thing. **Replace is one
//! transaction, not delete-then-insert**: undoing half a replacement leaves a
//! document with the old pages gone and the new ones not arrived, which is not
//! a state anyone chose.

use std::collections::BTreeSet;

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use super::import::import_pages;
use super::inherit::walk;
use super::rewrite::{catalog_ref, dict_at, resolver};
use super::tree::{self, Entry, Kind};
use super::{rewrite_page_tree, PageSource, Rewrite};
use crate::edit::{Overlay, TrailerState, Transaction};
use crate::structure::Structure;
use crate::{Error, Result};

/// How many pages the document has, read through the transaction's view so it
/// counts pages an earlier step in the same transaction inserted.
pub fn page_count(tx: &Transaction<'_>) -> Result<usize> {
    Ok(leaves(tx)?.len())
}

/// The page object at `index`, in the document as the transaction sees it.
pub(crate) fn page_ref(tx: &Transaction<'_>, index: usize) -> Result<ObjRef> {
    let leaves = leaves(tx)?;
    leaves
        .get(index)
        .map(|leaf| leaf.objref)
        .ok_or(Error::NoSuchPage {
            page: index,
            count: leaves.len(),
        })
}

fn leaves(tx: &Transaction<'_>) -> Result<Vec<super::inherit::Leaf>> {
    let catalog = catalog_ref(tx)?;
    let mut resolve = resolver(tx);
    leaves_from(catalog, &mut resolve)
}

/// How many pages `base` has with `overlay` applied, outside any transaction:
/// what the session reports as its page count while an edit is pending.
pub(crate) fn current_page_count(overlay: &Overlay, base: &CosDocument) -> Result<usize> {
    let catalog = match overlay.capture_trailer(base, &Name::new("Root")) {
        TrailerState::Set(object) => object.as_reference(),
        TrailerState::Cleared => None,
    }
    .ok_or(Error::NoPageTree)?;
    let mut resolve = |number: u32| -> Result<Option<Object>> {
        Ok(overlay
            .capture_object(base, number)?
            .map(|state| state.object))
    };
    Ok(leaves_from(catalog, &mut resolve)?.len())
}

/// The one walk both views share: catalog, then `/Pages`, then the leaves.
fn leaves_from(
    catalog: ObjRef,
    resolve: &mut dyn FnMut(u32) -> Result<Option<Object>>,
) -> Result<Vec<super::inherit::Leaf>> {
    let root = match resolve(catalog.number)? {
        Some(Object::Dict(dict)) => dict.get(b"Pages").and_then(Object::as_reference),
        _ => None,
    }
    .ok_or(Error::NoPageTree)?;
    walk(root, resolve)
}

/// Delete `pages`. Deleting every page is refused: a PDF with no pages is not
/// a document a reader will open, and "delete everything" is always a slip.
pub fn delete_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    pages: &[usize],
) -> Result<Rewrite> {
    let count = page_count(tx)?;
    let doomed: BTreeSet<usize> = pages.iter().copied().collect();
    check_indices(&doomed, count)?;
    let order: Vec<PageSource> = (0..count)
        .filter(|index| !doomed.contains(index))
        .map(PageSource::Existing)
        .collect();
    if order.is_empty() {
        return Err(Error::WouldLeaveNoPages);
    }
    rewrite_page_tree(tx, structure, &order)
}

/// Move `pages`, keeping their relative order, so they sit before the page
/// that was at `before` (or at the end, for `before == count`).
///
/// `before` names a position in the document **as it was**, which is what a
/// user dragging thumbnails means: "put these before that one".
pub fn move_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    pages: &[usize],
    before: usize,
) -> Result<Rewrite> {
    let count = page_count(tx)?;
    let moving: BTreeSet<usize> = pages.iter().copied().collect();
    check_indices(&moving, count)?;
    if before > count {
        return Err(Error::NoSuchPage {
            page: before,
            count,
        });
    }
    let staying: Vec<usize> = (0..count).filter(|index| !moving.contains(index)).collect();
    let insert_at = staying.iter().take_while(|index| **index < before).count();
    let mut order: Vec<usize> = staying[..insert_at].to_vec();
    order.extend(moving.iter().copied());
    order.extend(&staying[insert_at..]);
    let order: Vec<PageSource> = order.into_iter().map(PageSource::Existing).collect();
    rewrite_page_tree(tx, structure, &order)
}

/// Rotate `pages` by `quarter_turns` clockwise. Composes with what the page
/// already has, including rotation it inherited: 270 plus a quarter turn is 0,
/// never 360, and a negative turn wraps the same way.
pub fn rotate_pages(tx: &mut Transaction<'_>, pages: &[usize], quarter_turns: i32) -> Result<()> {
    let leaves = leaves(tx)?;
    let set: BTreeSet<usize> = pages.iter().copied().collect();
    check_indices(&set, leaves.len())?;
    for index in set {
        let leaf = &leaves[index];
        let current = match leaf.inherited.rotate.as_ref() {
            Some(Object::Integer(degrees)) => *degrees,
            Some(Object::Real(degrees)) => degrees.round() as i64,
            _ => 0,
        };
        let turned = (current + i64::from(quarter_turns) * 90).rem_euclid(360);
        let mut dict = dict_at(tx, leaf.objref)?;
        dict.set(Name::new("Rotate"), Object::Integer(turned));
        tx.put_object(
            leaf.objref.number,
            leaf.objref.generation,
            Object::Dict(dict),
        )?;
    }
    Ok(())
}

/// Insert `count` blank pages of `media_box` before position `at`.
pub fn insert_blank_pages(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    at: usize,
    count: usize,
    media_box: [f64; 4],
) -> Result<Rewrite> {
    let existing = page_count(tx)?;
    if at > existing {
        return Err(Error::NoSuchPage {
            page: at,
            count: existing,
        });
    }
    let mut blanks = Vec::with_capacity(count);
    for _ in 0..count {
        let number = tx.reserve();
        let mut page = Dict::new();
        page.set(Name::new("Type"), Object::name("Page"));
        page.set(
            Name::new("MediaBox"),
            Object::Array(media_box.iter().map(|value| Object::Real(*value)).collect()),
        );
        // An empty resource dictionary rather than none: a page with no
        // `/Resources` at all inherits from its parent, and the flat parent it
        // is about to get has none, which some readers report as damage.
        page.set(Name::new("Resources"), Object::Dict(Dict::new()));
        tx.put_object(number, 0, Object::Dict(page))?;
        blanks.push(PageSource::Imported(ObjRef::new(number, 0)));
    }
    let mut order: Vec<PageSource> = (0..at).map(PageSource::Existing).collect();
    order.extend(blanks);
    order.extend((at..existing).map(PageSource::Existing));
    rewrite_page_tree(tx, structure, &order)
}

/// Insert `pages` of `source` before position `at`, imported transitively.
///
/// Refuses an encrypted `source` - the encrypted-source rule, checked here at
/// execution because the source is a file the user picked after invoking the
/// command, which a session-scoped requirement cannot see.
pub fn insert_pages_from(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    source: &CosDocument,
    pages: &[usize],
    at: usize,
) -> Result<Rewrite> {
    let existing = page_count(tx)?;
    if at > existing {
        return Err(Error::NoSuchPage {
            page: at,
            count: existing,
        });
    }
    let imported = import_pages(tx, source, pages)?;
    let mut order: Vec<PageSource> = (0..at).map(PageSource::Existing).collect();
    order.extend(imported.into_iter().map(PageSource::Imported));
    order.extend((at..existing).map(PageSource::Existing));
    rewrite_page_tree(tx, structure, &order)
}

/// Replace `targets` with `pages` of `source`, one for one, in one transaction.
///
/// The encrypted-source check is the importer's, so it cannot be bypassed by a
/// replace that avoids calling insert: both go through [`import_pages`].
pub fn replace_pages_from(
    tx: &mut Transaction<'_>,
    structure: &Structure,
    source: &CosDocument,
    pages: &[usize],
    targets: &[usize],
) -> Result<Rewrite> {
    if pages.len() != targets.len() {
        return Err(Error::ReplacementCountMismatch {
            replacements: pages.len(),
            targets: targets.len(),
        });
    }
    let existing = page_count(tx)?;
    check_indices(&targets.iter().copied().collect(), existing)?;
    let imported = import_pages(tx, source, pages)?;
    let mut order: Vec<PageSource> = (0..existing).map(PageSource::Existing).collect();
    for (target, replacement) in targets.iter().zip(imported) {
        order[*target] = PageSource::Imported(replacement);
    }
    rewrite_page_tree(tx, structure, &order)
}

/// A numbering range for [`set_page_labels`].
#[derive(Clone, Debug, PartialEq)]
pub struct LabelRange {
    /// The page index the range starts at.
    pub start: usize,
    pub style: Option<LabelStyle>,
    pub prefix: Option<String>,
    /// The number the first page of the range shows. `1` in the usual case.
    pub first: i64,
}

/// The numbering styles of ISO 32000-1 table 159.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelStyle {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
}

impl LabelStyle {
    fn name(self) -> &'static str {
        match self {
            LabelStyle::Decimal => "D",
            LabelStyle::UpperRoman => "R",
            LabelStyle::LowerRoman => "r",
            LabelStyle::UpperLetters => "A",
            LabelStyle::LowerLetters => "a",
        }
    }
}

/// Replace the document's page labels with `ranges`. An empty list removes
/// them, which is how Renumber Pages goes back to plain page numbers.
pub fn set_page_labels(tx: &mut Transaction<'_>, ranges: &[LabelRange]) -> Result<()> {
    let count = page_count(tx)?;
    let mut entries = Vec::with_capacity(ranges.len());
    for range in ranges {
        if range.start >= count {
            return Err(Error::NoSuchPage {
                page: range.start,
                count,
            });
        }
        let mut dict = Dict::new();
        if let Some(style) = range.style {
            dict.set(Name::new("S"), Object::name(style.name()));
        }
        if let Some(prefix) = &range.prefix {
            dict.set(Name::new("P"), Object::String(prefix.as_bytes().to_vec()));
        }
        if range.first != 1 {
            dict.set(Name::new("St"), Object::Integer(range.first));
        }
        entries.push(Entry {
            key: Object::Integer(range.start as i64),
            value: Object::Dict(dict),
        });
    }
    entries.sort_by_key(|entry| match entry.key {
        Object::Integer(key) => key,
        _ => i64::MAX,
    });

    let catalog_ref = catalog_ref(tx)?;
    let mut catalog = dict_at(tx, catalog_ref)?;
    let existing = tree::root_ref(tx, catalog.get(b"PageLabels"))?;
    match tree::write(tx, existing, Kind::Number, &entries)? {
        Some(objref) => catalog.set(Name::new("PageLabels"), Object::Ref(objref)),
        None => {
            catalog.remove(b"PageLabels");
        }
    }
    tx.put_object(
        catalog_ref.number,
        catalog_ref.generation,
        Object::Dict(catalog),
    )
}

fn check_indices(pages: &BTreeSet<usize>, count: usize) -> Result<()> {
    match pages.iter().find(|index| **index >= count) {
        Some(index) => Err(Error::NoSuchPage {
            page: *index,
            count,
        }),
        None => Ok(()),
    }
}
