//! The M3 structure invariant: what has to be true of a tagged document after
//! any edit, callable from every editing package's tests.
//!
//! **This is not M5's checker.** It does not grade a document against PDF/UA.
//! It answers one narrower question: did the edit leave the structure tree
//! internally consistent with the document it describes. A file that was
//! already invalid before the edit can still satisfy it, and should: the
//! invariant is about what the edit did, not about what it inherited.
//!
//! **The clause that earns its place is `/Pg`.** A `/K`-only invariant passes
//! while a surviving element points at a page that is gone, because nothing in
//! `/K` references the removed page: the element's own `/Pg` does. The same
//! applies to `/IDTree`, which can name an element that has been removed
//! without any `/K` entry mentioning it. Both are checked here, and the
//! broken-fixture test in `tests/structure.rs` exists to prove the whole thing
//! is not vacuous.

use std::collections::BTreeSet;

use onionskin_cos::{Document as CosDocument, ObjRef};

use super::read::{Kid, ParentEntry, Structure, StructureTree};
use crate::Result;

/// One way the tree and the document disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    /// An element's `/Pg` names a page this document no longer has. The clause
    /// a `/K`-only invariant misses.
    ElementPageMissing { element: ObjRef, page: ObjRef },
    /// A `/K` entry names a page this document no longer has.
    KidPageMissing { element: ObjRef, page: ObjRef },
    /// A `/K` entry names an object that is not in the document.
    KidObjectMissing { element: ObjRef, object: ObjRef },
    /// An `/IDTree` entry names an element that is no longer in the tree.
    IdTreeDangling { id: Vec<u8>, element: ObjRef },
    /// A page or annotation carries a `/StructParent(s)` index the
    /// `/ParentTree` does not resolve.
    StructParentUnresolved { holder: ObjRef, key: i64 },
    /// A `/ParentTree` entry names an element that is not in the tree.
    ParentTreeDangling { key: i64, element: ObjRef },
    /// A page's `/ParentTree` entry is a single element rather than the array
    /// of marked-content slots a page's entry has to be.
    ParentTreeNotSlots { page: ObjRef, key: i64 },
    /// `/StructTreeRoot` is present while `/MarkInfo` `/Marked` is false or
    /// absent: the file contradicts itself. Reported rather than repaired.
    NotMarked,
}

/// Everything the invariant found. Empty is the passing result, and a caller
/// asserts on emptiness rather than on a boolean so a failure names itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub violations: Vec<Violation>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Check the tree against the document it describes.
///
/// An untagged document passes trivially, and `Report::is_clean` is true for a
/// reason the caller can assert on separately through [`Structure::is_tagged`].
pub(crate) fn check(doc: &CosDocument, structure: &Structure, page_count: usize) -> Result<Report> {
    let Some(tree) = structure.tree() else {
        return Ok(Report::default());
    };

    let pages = live_pages(doc, page_count)?;
    let mut report = Report::default();

    if !tree.marked {
        report.violations.push(Violation::NotMarked);
    }
    check_element_pages(tree, &pages, &mut report);
    check_kids(doc, tree, &pages, &mut report);
    check_id_tree(tree, &mut report);
    check_parent_tree(tree, &mut report);
    check_struct_parents(doc, tree, page_count, &mut report)?;

    Ok(report)
}

/// Object numbers of the pages this document still has, in no particular
/// order: the invariant asks about membership, never about position.
fn live_pages(doc: &CosDocument, page_count: usize) -> Result<BTreeSet<u32>> {
    let mut pages = BTreeSet::new();
    for index in 0..page_count {
        pages.insert(doc.page(index)?.objref.number);
    }
    Ok(pages)
}

/// The `/Pg` clause. Every surviving element's own page must still be here.
fn check_element_pages(tree: &StructureTree, pages: &BTreeSet<u32>, report: &mut Report) {
    for element in tree.elements.values() {
        let Some(page) = element.page else { continue };
        if !pages.contains(&page.number) {
            report.violations.push(Violation::ElementPageMissing {
                element: element.objref,
                page,
            });
        }
    }
}

fn check_kids(doc: &CosDocument, tree: &StructureTree, pages: &BTreeSet<u32>, report: &mut Report) {
    for element in tree.elements.values() {
        for kid in &element.kids {
            match kid {
                // A bare MCID hangs off the element's own /Pg, which
                // `check_element_pages` already covers.
                Kid::Mcid(_) | Kid::Element(_) => {}
                Kid::MarkedContent {
                    page: Some(page), ..
                } => {
                    if !pages.contains(&page.number) {
                        report.violations.push(Violation::KidPageMissing {
                            element: element.objref,
                            page: *page,
                        });
                    }
                }
                Kid::MarkedContent { page: None, .. } => {}
                Kid::Object { page, object } => {
                    if let Some(page) = page {
                        if !pages.contains(&page.number) {
                            report.violations.push(Violation::KidPageMissing {
                                element: element.objref,
                                page: *page,
                            });
                        }
                    }
                    if !object_is_live(doc, *object) {
                        report.violations.push(Violation::KidObjectMissing {
                            element: element.objref,
                            object: *object,
                        });
                    }
                }
            }
        }
    }
}

/// A freed or absent object number. `get` is the same path every other reader
/// uses, so an object this says is gone is one no edit can reach either.
fn object_is_live(doc: &CosDocument, objref: ObjRef) -> bool {
    doc.get(objref.number).is_ok()
}

/// The `/IDTree` clause. Every entry must still name an element in the tree.
fn check_id_tree(tree: &StructureTree, report: &mut Report) {
    for (id, element) in &tree.id_tree {
        if !tree.elements.contains_key(&element.number) {
            report.violations.push(Violation::IdTreeDangling {
                id: id.clone(),
                element: *element,
            });
        }
    }
}

/// Every `/ParentTree` value must name an element the tree still holds.
/// Well-formed and pointing at the wrong elements is the dangerous state, and
/// this is what rules out the "wrong" half of it.
fn check_parent_tree(tree: &StructureTree, report: &mut Report) {
    for (key, entry) in &tree.parent_tree {
        match entry {
            ParentEntry::Element(element) => {
                if !tree.elements.contains_key(&element.number) {
                    report.violations.push(Violation::ParentTreeDangling {
                        key: *key,
                        element: *element,
                    });
                }
            }
            ParentEntry::Slots(slots) => {
                for element in slots.iter().flatten() {
                    if !tree.elements.contains_key(&element.number) {
                        report.violations.push(Violation::ParentTreeDangling {
                            key: *key,
                            element: *element,
                        });
                    }
                }
            }
        }
    }
}

/// Every `/StructParents` on a page and every `/StructParent` on an annotation
/// must resolve through the `/ParentTree`, and a page's entry must be the array
/// form.
fn check_struct_parents(
    doc: &CosDocument,
    tree: &StructureTree,
    page_count: usize,
    report: &mut Report,
) -> Result<()> {
    let page_keys = super::read::page_struct_parents(doc, page_count)?;
    for (number, key) in &page_keys {
        let holder = ObjRef::new(*number, 0);
        match tree.parent_tree.get(key) {
            None => report
                .violations
                .push(Violation::StructParentUnresolved { holder, key: *key }),
            Some(ParentEntry::Element(_)) => {
                report.violations.push(Violation::ParentTreeNotSlots {
                    page: holder,
                    key: *key,
                })
            }
            Some(ParentEntry::Slots(_)) => {}
        }
    }

    let annotation_keys = super::read::annotation_struct_parents(doc, page_count)?;
    for (number, key) in &annotation_keys {
        if !tree.parent_tree.contains_key(key) {
            report.violations.push(Violation::StructParentUnresolved {
                holder: ObjRef::new(*number, 0),
                key: *key,
            });
        }
    }

    Ok(())
}
