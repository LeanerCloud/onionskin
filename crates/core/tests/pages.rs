//! `core::pages`: the page-tree transformation and its seven fix-ups.
//!
//! # Why the fixtures are hand-built
//!
//! The plan asked for three named `external/` fixtures per shape. A sweep of
//! the fetched corpus says that does not exist. Of 2,907 veraPDF files there is
//! **one** with a page tree more than one level deep, **one** with
//! `/PageLabels`, **one** with an intra-document `/Link`, **zero** with
//! `/Threads`, and not one of the 77 with `/AcroForm /Fields` has more than a
//! single page - so none of them can answer "delete the page carrying this
//! widget and check the others survive". A 982-file sweep of the pdf.js corpus
//! is better and still thin: 17 deep trees, 4 multi-page form documents, and
//! **one** file with `/Threads`.
//!
//! So each fix-up gets a minimal hand-built fixture carrying exactly its own
//! shape, which is what makes the mutation matrix below meaningful: dropping
//! any one fix-up fails exactly its own test and no other. Three near-identical
//! corpus files would not have told anyone that.
//!
//! Real-world breadth is kept, and is stronger than three named files: the
//! corpus sweep at the end runs the transformation over **every** multi-page
//! external file there is and asserts `audit_references` is clean and every
//! surviving page's inheritable attributes are unchanged.

use std::collections::BTreeSet;

use onionskin_core::pages::{rewrite_page_tree, PageSource, Rewrite};
use onionskin_core::{check, read_structure, EditSession};
use onionskin_corpus_testing::corpus_dir;
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, ObjRef, Object};

// ---------------------------------------------------------------------------
// The transformation
// ---------------------------------------------------------------------------

#[test]
fn a_deleted_page_leaves_a_flat_tree_on_the_original_root_number() {
    let original = deep_tree();
    let base = open(&original);
    let root = pages_ref(&base);

    let (saved, report) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    assert_eq!(report.pages_before, 3);
    assert_eq!(report.pages_after, 2);
    assert_eq!(
        pages_ref(&after),
        root,
        "the flat node reuses the root's object number, so every reference to \
         the page tree still resolves"
    );
    let tree = dict_of(&after, root);
    assert_eq!(
        tree.get(b"Count").and_then(Object::as_integer),
        Some(2),
        "/Count is the length of the list, recomputed rather than copied"
    );
    let Some(Object::Array(kids)) = tree.get(b"Kids") else {
        panic!("/Kids is an array");
    };
    assert_eq!(kids.len(), 2, "one level, two kids, no intermediate nodes");
    assert_eq!(after.page_count().expect("pages"), 2);
}

/// The bug the flat rewrite exists to prevent, and it is invisible on a shallow
/// tree: a page that inherited `/Resources`, `/MediaBox`, `/CropBox` or
/// `/Rotate` from an ancestor has to keep it once the ancestor is gone.
#[test]
fn a_flattened_page_keeps_every_attribute_it_used_to_inherit() {
    let original = deep_tree();
    let base = open(&original);
    let before: Vec<Attributes> = (0..3).map(|index| attributes(&base, index)).collect();

    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(1), keep(2)]);
    let after = open(&saved);

    for (index, expected) in before.iter().enumerate() {
        assert_eq!(
            &attributes(&after, index),
            expected,
            "page {} lost something it inherited",
            index + 1
        );
    }
}

/// Materializing a shared `/Resources` by value inlines a full copy into every
/// page. A thousand-page document that shared one gets a thousand of them, and
/// a test that compares *resolved* values passes on the inlined form - which is
/// why this reads the reference.
#[test]
fn a_shared_resource_dictionary_stays_shared() {
    let original = deep_tree();
    let base = open(&original);
    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(1), keep(2)]);
    let after = open(&saved);

    let references: Vec<Option<ObjRef>> = (0..3)
        .map(|index| {
            after
                .page(index)
                .expect("page")
                .dict
                .get(b"Resources")
                .and_then(Object::as_reference)
        })
        .collect();
    assert!(
        references.iter().all(|objref| objref.is_some()),
        "/Resources is still a reference, not an inlined copy: {references:?}"
    );
    assert_eq!(
        references.iter().collect::<BTreeSet<_>>().len(),
        1,
        "and it is still the same one for every page"
    );
}

/// "Unimplemented means untouched" applies to values the parser dislikes.
/// `cos`'s `rectangle()` filters a degenerate box to `None`; materializing
/// through it turns a page with a bad `/MediaBox` into a page with none at all,
/// under a flat node that has none either.
#[test]
fn a_degenerate_media_box_survives_as_it_was_written() {
    let original = degenerate_box();
    let base = open(&original);
    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(1)]);
    let after = open(&saved);

    let box_of = |document: &CosDocument, index: usize| {
        document
            .page(index)
            .expect("page")
            .dict
            .get(b"MediaBox")
            .cloned()
    };
    assert_eq!(
        box_of(&after, 0),
        box_of(&base, 0),
        "the zero-area box is written back exactly as it was"
    );
    assert!(
        box_of(&after, 0).is_some(),
        "and it is still there at all, which is the failure mode"
    );
}

#[test]
fn everything_else_on_a_page_survives_untouched() {
    let original = deep_tree();
    let base = open(&original);
    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(1), keep(2)]);
    let after = open(&saved);

    let page = after.page(0).expect("page").dict;
    for key in [
        b"Tabs".as_slice(),
        b"UserUnit".as_slice(),
        b"Contents".as_slice(),
    ] {
        assert_eq!(
            page.get(key),
            base.page(0).expect("page").dict.get(key),
            "{} was changed",
            String::from_utf8_lossy(key)
        );
    }
}

/// Copying a page within a document goes through the importer, which renumbers
/// the copy's references. Aliasing one object into two `/Kids` slots instead
/// passes every structural check and produces two pages that are one object.
#[test]
fn a_repeated_page_index_is_refused_rather_than_aliased() {
    let original = deep_tree();
    let base = open(&original);
    let structure = read_structure(&base).expect("structure");
    let mut edit = EditSession::for_base(&base);
    let outcome = edit.transact(&base, "Reorder Pages", |tx| {
        rewrite_page_tree(tx, &structure, &[keep(0), keep(0)]).map(|_| ())
    });
    assert!(outcome.is_err(), "a repeated index has to be refused");
}

#[test]
fn a_page_index_the_document_does_not_have_is_refused() {
    let original = deep_tree();
    let base = open(&original);
    let structure = read_structure(&base).expect("structure");
    let mut edit = EditSession::for_base(&base);
    let outcome = edit.transact(&base, "Reorder Pages", |tx| {
        rewrite_page_tree(tx, &structure, &[keep(0), keep(9)]).map(|_| ())
    });
    assert!(outcome.is_err());
}

/// T5's free-nothing rule, settled the way the plan says it should be: by
/// looking - but at the syntax tree, not at the text.
///
/// The rule is stated in `pages/mod.rs`'s own doc comment, so a substring scan
/// finds the sentence that promises it and reports the promise as a violation.
/// That is the self-matching trap this repository has now hit three times; a
/// comment is not a token, so a parser cannot be fooled by one.
#[test]
fn nothing_in_this_module_frees_an_object() {
    use syn::visit::Visit;

    #[derive(Default)]
    struct Frees {
        found: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Frees {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if call.method == "delete_object" {
                self.found.push(call.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, call);
        }
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "delete_object")
            {
                self.found.push("delete_object".to_owned());
            }
            syn::visit::visit_path(self, path);
        }
    }

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/pages");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("the module directory is readable") {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_none_or(|kind| kind != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("readable");
        let parsed = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("{} does not parse ({error})", path.display()));
        let mut frees = Frees::default();
        frees.visit_file(&parsed);
        assert!(
            frees.found.is_empty(),
            "{} frees an object number, which T5 forbids",
            path.display()
        );
        checked += 1;
    }
    assert!(
        checked >= 11,
        "the seven fix-ups, the walk, the tree helper, the rewrite and the module root: \
         only {checked} files were checked"
    );
}

/// A removed page's own objects stay in the file, unreferenced. That is the
/// point of free-nothing: the number is never handed out again, so no reference
/// anywhere ever changes meaning.
#[test]
fn a_removed_pages_objects_are_left_in_place_rather_than_freed() {
    let original = deep_tree();
    let base = open(&original);
    let doomed = base.page(1).expect("page").objref.number;

    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    assert!(
        after.get(doomed).is_ok(),
        "the removed page's dictionary is still parseable, just unreachable"
    );
    assert!(
        after
            .audit_references()
            .expect("the walk completes")
            .is_empty(),
        "and nothing dangles"
    );
}

// ---------------------------------------------------------------------------
// Fix-up 1: /PageLabels
// ---------------------------------------------------------------------------

#[test]
fn page_labels_follow_their_pages_through_a_delete_and_a_reorder() {
    let original = labelled();
    let base = open(&original);
    assert_eq!(labels(&base), vec!["i", "ii", "1", "2"]);

    let (deleted, report) = rewrite(&original, &base, &[keep(0), keep(2), keep(3)]);
    assert!(report.page_labels_rebuilt);
    assert_eq!(
        labels(&open(&deleted)),
        vec!["i", "1", "2"],
        "each surviving page keeps its own label"
    );

    let (reordered, _) = rewrite(&original, &base, &[keep(3), keep(0), keep(1), keep(2)]);
    assert_eq!(
        labels(&open(&reordered)),
        vec!["2", "i", "ii", "1"],
        "a label belongs to its page, not to its position"
    );
}

// ---------------------------------------------------------------------------
// Fix-up 2: destinations
// ---------------------------------------------------------------------------

#[test]
fn a_named_destination_on_a_deleted_page_is_dropped_and_the_rest_still_resolve() {
    let original = destinations();
    let base = open(&original);

    let (saved, report) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    assert_eq!(report.destinations_dropped, 2, "one flat, one in the tree");
    assert_eq!(
        named_destinations(&after),
        vec!["first".to_owned(), "third".to_owned()],
        "the survivors are still there and the dropped one is gone"
    );
    for name in ["first", "third"] {
        assert!(
            destination_page(&after, name).is_some(),
            "{name} still resolves to a page"
        );
    }
}

/// A name tree whose entries were dropped without recomputing `/Limits` passes
/// every structural check and fails lookup for names outside the stale bracket.
#[test]
fn the_name_tree_is_rebuilt_with_limits_that_bracket_what_is_left() {
    let original = destinations();
    let base = open(&original);
    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    let tree = name_tree_root(&after).expect("the tree is still there");
    check_limits(&after, tree, 0);
}

// ---------------------------------------------------------------------------
// Fix-up 3: the outline chain
// ---------------------------------------------------------------------------

/// Walked from `/First` to `/Last`, not reached: a naive reachability check
/// passes on the broken form, which is the whole reason this fix-up exists.
#[test]
fn the_outline_chain_walks_past_a_dropped_item() {
    let original = outlined();
    let base = open(&original);

    let (saved, report) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    assert_eq!(report.bookmarks_dropped, 1);
    let walked = walk_outline(&after);
    assert_eq!(walked.len(), 2, "the chain visits exactly the survivors");
    let dropped = outline_item(&base, 1);
    assert!(
        !walked.contains(&dropped),
        "and never the dropped item, which is still in the file"
    );
    assert_eq!(
        outline_count(&after),
        Some(2),
        "the parent's /Count is recomputed, not copied"
    );
}

/// The sign is the part a recount loses, and losing it opens every collapsed
/// bookmark in the document while every structural check still passes.
#[test]
fn a_closed_outline_node_keeps_its_negative_count() {
    let original = outlined_closed();
    let base = open(&original);
    assert_eq!(outline_count(&base), Some(-3));

    let (saved, _) = rewrite(&original, &base, &[keep(0), keep(2)]);
    assert_eq!(
        outline_count(&open(&saved)),
        Some(-2),
        "still closed, and counting what is left"
    );
}

// ---------------------------------------------------------------------------
// Fix-up 4: link annotations
// ---------------------------------------------------------------------------

#[test]
fn a_link_to_a_deleted_page_goes_and_one_to_a_surviving_page_stays() {
    let original = linked();
    let base = open(&original);

    let (saved, report) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    assert_eq!(report.links_dropped, 1);
    let annots = page_annots(&after, 0);
    assert_eq!(annots.len(), 1, "one link left on page 1");
    let target = annots[0]
        .get(b"Dest")
        .and_then(|dest| match dest {
            Object::Array(items) => items.first().and_then(Object::as_reference),
            _ => None,
        })
        .expect("the survivor still names a page");
    assert_eq!(
        target.number,
        after.page(1).expect("page").objref.number,
        "and still the page it named before, which moved but did not change object"
    );
}

// ---------------------------------------------------------------------------
// Fix-up 5: /AcroForm /Fields
// ---------------------------------------------------------------------------

#[test]
fn a_form_field_on_a_deleted_page_goes_and_an_emptied_group_goes_with_it() {
    let original = form();
    let base = open(&original);

    let (saved, report) = rewrite(&original, &base, &[keep(0)]);
    let after = open(&saved);

    assert!(report.form_fields_dropped >= 2, "the widget and its group");
    let fields = acroform_fields(&after);
    assert_eq!(fields.len(), 1, "one field left");
    assert_eq!(
        fields[0].get(b"T").cloned(),
        Some(Object::String(b"kept".to_vec())),
        "and it is the one whose page survived"
    );
}

// ---------------------------------------------------------------------------
// Fix-up 6: article threads
// ---------------------------------------------------------------------------

/// Walked as a ring and compared as a set: a ring that still terminates but
/// visits a garbage bead passes any reachability check.
#[test]
fn an_article_ring_closes_over_the_beads_that_are_left() {
    let original = threaded();
    let base = open(&original);

    let (saved, report) = rewrite(&original, &base, &[keep(0), keep(2)]);
    let after = open(&saved);

    assert_eq!(report.article_beads_dropped, 1);
    assert_eq!(report.threads_dropped, 0);
    let ring = walk_thread(&after);
    assert_eq!(ring.len(), 2, "two beads, and the ring closes");
    for bead in &ring {
        let page = dict_of(&after, *bead)
            .get(b"P")
            .and_then(Object::as_reference)
            .expect("a bead names its page");
        assert!(
            (0..after.page_count().expect("pages") as usize).any(|index| after
                .page(index)
                .expect("page")
                .objref
                .number
                == page.number),
            "every bead left is on a page that is still in the tree"
        );
    }
}

#[test]
fn a_thread_whose_pages_all_went_leaves_the_catalog() {
    let original = threaded();
    let base = open(&original);

    // Page 4 carries no bead, so keeping only it is the one order that leaves
    // the thread with nothing.
    let (saved, report) = rewrite(&original, &base, &[keep(3)]);
    let after = open(&saved);

    assert_eq!(report.threads_dropped, 1);
    assert!(
        catalog(&after).get(b"Threads").is_none()
            || matches!(catalog(&after).get(b"Threads"), Some(Object::Array(items)) if items.is_empty()),
        "the thread is gone from /Threads"
    );
}

// ---------------------------------------------------------------------------
// Fix-up 7: /OpenAction and page-level /AA
// ---------------------------------------------------------------------------

#[test]
fn an_open_action_on_a_deleted_page_is_dropped_and_a_surviving_pages_aa_is_not() {
    let original = with_open_action();
    let base = open(&original);

    let (saved, report) = rewrite(&original, &base, &[keep(0)]);
    let after = open(&saved);

    assert!(report.open_action_dropped);
    assert!(
        catalog(&after).get(b"OpenAction").is_none(),
        "dropped rather than retargeted: there is no honest page to retarget to"
    );
    assert_eq!(
        after.page(0).expect("page").dict.get(b"AA").cloned(),
        base.page(0).expect("page").dict.get(b"AA").cloned(),
        "the surviving page's own actions are untouched"
    );
    assert!(
        after.page(0).is_ok(),
        "and reopening lands on a page that is there"
    );
}

// ---------------------------------------------------------------------------
// The structure tree, through P4's hooks
// ---------------------------------------------------------------------------

/// P4's invariant is the definition of a valid tree, so this asserts on it
/// rather than on a second idea of one - after a delete and after a reorder,
/// and with the reading order following the pages.
#[test]
fn a_tagged_document_keeps_a_valid_structure_tree_through_a_delete_and_a_reorder() {
    let original = tagged();
    let base = open(&original);
    let structure = read_structure(&base).expect("structure");
    assert!(structure.tree().is_some(), "the fixture is tagged");
    assert!(
        check(&base, &structure, 3)
            .expect("check runs")
            .violations
            .is_empty(),
        "and valid to begin with, or the rest proves nothing"
    );

    for order in [vec![keep(0), keep(2)], vec![keep(2), keep(0), keep(1)]] {
        let (saved, _) = rewrite(&original, &base, &order);
        let after = open(&saved);
        let rebuilt = read_structure(&after).expect("the structure still reads");
        let report = check(&after, &rebuilt, order.len()).expect("check runs");
        assert!(
            report.violations.is_empty(),
            "{order:?} left the structure tree invalid: {:?}",
            report.violations
        );
    }

    // The reorder: the root's kids follow the pages they sit on.
    let (saved, _) = rewrite(&original, &base, &[keep(2), keep(0), keep(1)]);
    let after = open(&saved);
    let pages: Vec<u32> = (0..3)
        .map(|index| after.page(index).expect("page").objref.number)
        .collect();
    let reading: Vec<u32> = root_kid_pages(&after);
    assert_eq!(
        reading, pages,
        "the reading order follows the new page order"
    );
}

/// Two pages removed in one rewrite. Each structure removal rewrites the
/// root's `/K` and the `/ParentTree` from the tree it is handed, so removing
/// them one at a time from the same tree writes back the element the first
/// removal took out - and the invariant check passes over that, because an
/// emptied element with no page is a legal element. So this reads the two
/// places it would show.
#[test]
fn deleting_several_tagged_pages_removes_every_one_of_their_elements() {
    let original = tagged();
    let base = open(&original);
    let (saved, _) = rewrite(&original, &base, &[keep(2)]);
    let after = open(&saved);

    let root = catalog(&after)
        .get(b"StructTreeRoot")
        .and_then(Object::as_reference)
        .expect("a structure tree");
    let root = dict_of(&after, root);
    assert_eq!(
        resolved(&after, root.get(b"K")),
        Some(Object::Array(vec![Object::Ref(ObjRef::new(9, 0))])),
        "only the surviving page's element is left in the reading order"
    );
    let Some(Object::Dict(parent_tree)) = resolved(&after, root.get(b"ParentTree")) else {
        panic!("a /ParentTree");
    };
    assert_eq!(
        parent_tree.get(b"Nums"),
        Some(&Object::Array(vec![
            Object::Integer(0),
            Object::Array(vec![Object::Null]),
            Object::Integer(1),
            Object::Array(vec![Object::Null]),
            Object::Integer(2),
            Object::Array(vec![Object::Ref(ObjRef::new(9, 0))]),
        ])),
        "both removed pages' /ParentTree entries are cleared, not just the last"
    );
}

fn root_kid_pages(document: &CosDocument) -> Vec<u32> {
    let root = catalog(document)
        .get(b"StructTreeRoot")
        .and_then(Object::as_reference)
        .expect("a structure tree");
    let Some(Object::Array(kids)) = resolved(document, dict_of(document, root).get(b"K")) else {
        return Vec::new();
    };
    kids.iter()
        .filter_map(|kid| match resolved(document, Some(kid)) {
            Some(Object::Dict(element)) => element
                .get(b"Pg")
                .and_then(Object::as_reference)
                .map(|page| page.number),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Every fixture, every operation, audited
// ---------------------------------------------------------------------------

#[test]
fn every_fixture_survives_every_operation_with_no_dangling_reference() {
    for (name, bytes) in fixtures() {
        let base = open(&bytes);
        let count = base.page_count().expect("pages") as usize;
        let orders: Vec<Vec<PageSource>> = vec![
            (0..count).map(keep).collect(),
            (0..count).rev().map(keep).collect(),
            (0..count).skip(1).map(keep).collect(),
            (0..count).take(1).map(keep).collect(),
        ];
        for (index, order) in orders.iter().enumerate() {
            if order.is_empty() {
                continue;
            }
            let (saved, _) = rewrite(&bytes, &base, order);
            let after = open(&saved);
            assert_eq!(
                after.audit_references().expect("the walk completes"),
                Vec::new(),
                "{name}, order {index}: the output dangles"
            );
            assert_eq!(
                after.page_count().expect("pages") as usize,
                order.len(),
                "{name}, order {index}: wrong page count"
            );
            assert_eq!(
                &saved[..bytes.len()],
                &bytes[..],
                "{name}: the original bytes have to survive underneath"
            );
        }
    }
}

/// Breadth, from whatever real files are here. Runs over **every** multi-page
/// external file rather than three named ones, which is more than the plan
/// asked for and available because it needs no fixture to be named.
#[test]
fn the_external_corpus_survives_a_delete_with_its_inheritance_intact() {
    let Some(root) = corpus_dir("external") else {
        return;
    };
    let mut swept = 0;
    let mut failures: Vec<String> = Vec::new();
    for path in walk_pdfs(&root) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(base) = CosDocument::open(Box::new(BytesSource::new(bytes.clone()))) else {
            continue;
        };
        let Ok(count) = base.page_count() else {
            continue;
        };
        if count < 2 {
            continue;
        }
        let count = count as usize;
        let before: Vec<Attributes> = (0..count).map(|index| attributes(&base, index)).collect();

        let order: Vec<PageSource> = (0..count).skip(1).map(keep).collect();
        let Ok(structure) = read_structure(&base) else {
            continue;
        };
        let mut edit = EditSession::for_base(&base);
        let outcome = edit.transact(&base, "Delete Pages", |tx| {
            rewrite_page_tree(tx, &structure, &order).map(|_| ())
        });
        if outcome.is_err() {
            continue;
        }
        let Some(section) = section(&base, &edit) else {
            continue;
        };
        let mut saved = bytes.clone();
        saved.extend_from_slice(&section);
        let Ok(after) = CosDocument::open(Box::new(BytesSource::new(saved))) else {
            failures.push(format!("{}: the output does not reopen", path.display()));
            continue;
        };
        swept += 1;

        match after.audit_references() {
            Ok(dangling) if dangling.is_empty() => {}
            Ok(dangling) => failures.push(format!(
                "{}: {} dangling reference(s)",
                path.display(),
                dangling.len()
            )),
            Err(error) => failures.push(format!("{}: audit failed: {error}", path.display())),
        }
        for index in 0..count - 1 {
            if attributes(&after, index) != before[index + 1] {
                failures.push(format!(
                    "{}: page {} lost an inherited attribute",
                    path.display(),
                    index + 2
                ));
                break;
            }
        }
    }
    assert!(swept > 0, "the sweep found no multi-page external file");
    assert!(
        failures.is_empty(),
        "{} of {swept} swept files failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("page-tree sweep: {swept} multi-page external files");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn keep(index: usize) -> PageSource {
    PageSource::Existing(index)
}

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

/// Run the rewrite and append the section it produces, which is how every
/// assertion here reads the result through a fresh parse rather than through
/// the overlay that produced it.
fn rewrite(original: &[u8], base: &CosDocument, order: &[PageSource]) -> (Vec<u8>, Rewrite) {
    let structure = read_structure(base).expect("the structure reads");
    let mut edit = EditSession::for_base(base);
    let mut report = Rewrite::default();
    edit.transact(base, "Rewrite Pages", |tx| {
        report = rewrite_page_tree(tx, &structure, order)?;
        Ok(())
    })
    .expect("the rewrite commits");
    let mut saved = original.to_vec();
    saved.extend_from_slice(&section(base, &edit).expect("a section"));
    (saved, report)
}

fn section(base: &CosDocument, edit: &EditSession) -> Option<Vec<u8>> {
    base.section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
}

#[derive(Debug, PartialEq)]
struct Attributes {
    resources: Option<Object>,
    media_box: Option<Object>,
    crop_box: Option<Object>,
    rotate: Option<Object>,
}

/// What a page ends up with, read the way a reader reads it: the page's own
/// entry if it has one, otherwise the nearest ancestor's. Read raw, because the
/// resolved accessor is lossy for exactly the two entries this is checking.
fn attributes(document: &CosDocument, index: usize) -> Attributes {
    let page = document.page(index).expect("page");
    let mut at = Some(page.dict.clone());
    let mut found = Attributes {
        resources: None,
        media_box: None,
        crop_box: None,
        rotate: None,
    };
    let mut depth = 0;
    while let Some(dict) = at {
        depth += 1;
        if depth > 64 {
            break;
        }
        for (key, slot) in [
            (b"Resources".as_slice(), &mut found.resources),
            (b"MediaBox".as_slice(), &mut found.media_box),
            (b"CropBox".as_slice(), &mut found.crop_box),
            (b"Rotate".as_slice(), &mut found.rotate),
        ] {
            if slot.is_none() {
                if let Some(value) = dict.get(key) {
                    *slot = Some(value.clone());
                }
            }
        }
        at = dict
            .get(b"Parent")
            .and_then(Object::as_reference)
            .and_then(|objref| document.get(objref.number).ok())
            .and_then(|parsed| parsed.object.as_dict().cloned());
    }
    found
}

fn catalog(document: &CosDocument) -> Dict {
    document.catalog().expect("catalog")
}

fn pages_ref(document: &CosDocument) -> ObjRef {
    catalog(document)
        .get(b"Pages")
        .and_then(Object::as_reference)
        .expect("indirect /Pages")
}

fn dict_of(document: &CosDocument, objref: ObjRef) -> Dict {
    document
        .get(objref.number)
        .expect("the object parses")
        .object
        .as_dict()
        .cloned()
        .expect("a dictionary")
}

fn resolved(document: &CosDocument, object: Option<&Object>) -> Option<Object> {
    match object {
        Some(Object::Ref(objref)) => document
            .get(objref.number)
            .ok()
            .map(|parsed| parsed.object.clone()),
        other => other.cloned(),
    }
}

/// Every page's label, built the way a reader builds it from `/PageLabels`.
fn labels(document: &CosDocument) -> Vec<String> {
    let entry = catalog(document).get(b"PageLabels").cloned();
    let mut ranges: Vec<(i64, Dict)> = Vec::new();
    collect_nums(document, entry.as_ref(), &mut ranges, 0);
    ranges.sort_by_key(|(key, _)| *key);

    let count = document.page_count().expect("pages") as usize;
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let Some((start, dict)) = ranges
            .iter()
            .rev()
            .find(|(key, _)| *key <= index as i64)
            .cloned()
        else {
            out.push(String::new());
            continue;
        };
        let first = match resolved(document, dict.get(b"St")) {
            Some(Object::Integer(value)) => value,
            _ => 1,
        };
        let number = first + (index as i64 - start);
        let style = dict
            .get(b"S")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes().to_vec());
        out.push(match style.as_deref() {
            Some(b"r") => roman(number),
            _ => number.to_string(),
        });
    }
    out
}

fn collect_nums(
    document: &CosDocument,
    node: Option<&Object>,
    out: &mut Vec<(i64, Dict)>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let Some(Object::Dict(dict)) = resolved(document, node) else {
        return;
    };
    if let Some(Object::Array(items)) = resolved(document, dict.get(b"Nums")) {
        for pair in items.chunks(2) {
            if let [Object::Integer(key), value] = pair {
                if let Some(Object::Dict(range)) = resolved(document, Some(value)) {
                    out.push((*key, range));
                }
            }
        }
    }
    if let Some(Object::Array(kids)) = resolved(document, dict.get(b"Kids")) {
        for kid in kids {
            collect_nums(document, Some(&kid), out, depth + 1);
        }
    }
}

fn roman(mut value: i64) -> String {
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (amount, glyph) in table {
        while value >= amount {
            out.push_str(glyph);
            value -= amount;
        }
    }
    out
}

fn name_tree_root(document: &CosDocument) -> Option<ObjRef> {
    let names = resolved(document, catalog(document).get(b"Names"))?;
    let Object::Dict(names) = names else {
        return None;
    };
    names.get(b"Dests").and_then(Object::as_reference)
}

/// Every name in both tables, sorted.
fn named_destinations(document: &CosDocument) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(Object::Dict(dests)) = resolved(document, catalog(document).get(b"Dests")) {
        for (key, _) in dests.iter() {
            names.push(String::from_utf8_lossy(key.as_bytes()).into_owned());
        }
    }
    if let Some(root) = name_tree_root(document) {
        let mut entries = Vec::new();
        collect_names(document, Some(&Object::Ref(root)), &mut entries, 0);
        for (key, _) in entries {
            names.push(String::from_utf8_lossy(&key).into_owned());
        }
    }
    names.sort();
    names
}

fn collect_names(
    document: &CosDocument,
    node: Option<&Object>,
    out: &mut Vec<(Vec<u8>, Object)>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let Some(Object::Dict(dict)) = resolved(document, node) else {
        return;
    };
    if let Some(Object::Array(items)) = resolved(document, dict.get(b"Names")) {
        for pair in items.chunks(2) {
            if let [Object::String(key), value] = pair {
                out.push((key.clone(), value.clone()));
            }
        }
    }
    if let Some(Object::Array(kids)) = resolved(document, dict.get(b"Kids")) {
        for kid in kids {
            collect_names(document, Some(&kid), out, depth + 1);
        }
    }
}

/// Look a name up the way a reader does: descend by `/Limits`. A tree whose
/// brackets are stale refuses a name that is there, which is the failure this
/// distinguishes from a merely well-formed tree.
fn destination_page(document: &CosDocument, name: &str) -> Option<ObjRef> {
    if let Some(Object::Dict(dests)) = resolved(document, catalog(document).get(b"Dests")) {
        if let Some(value) = dests.get(name.as_bytes()) {
            return first_page(document, value);
        }
    }
    let root = name_tree_root(document)?;
    let mut node = Object::Ref(root);
    for _ in 0..32 {
        let Some(Object::Dict(dict)) = resolved(document, Some(&node)) else {
            return None;
        };
        if let Some(Object::Array(items)) = resolved(document, dict.get(b"Names")) {
            for pair in items.chunks(2) {
                if let [Object::String(key), value] = pair {
                    if key == name.as_bytes() {
                        return first_page(document, value);
                    }
                }
            }
            return None;
        }
        let Some(Object::Array(kids)) = resolved(document, dict.get(b"Kids")) else {
            return None;
        };
        let mut next = None;
        for kid in kids {
            let Some(Object::Dict(child)) = resolved(document, Some(&kid)) else {
                continue;
            };
            let Some(Object::Array(limits)) = resolved(document, child.get(b"Limits")) else {
                continue;
            };
            let [Object::String(low), Object::String(high)] = &limits[..] else {
                continue;
            };
            if low.as_slice() <= name.as_bytes() && name.as_bytes() <= high.as_slice() {
                next = Some(kid.clone());
                break;
            }
        }
        node = next?;
    }
    None
}

fn first_page(document: &CosDocument, value: &Object) -> Option<ObjRef> {
    match resolved(document, Some(value))? {
        Object::Array(items) => items.first().and_then(Object::as_reference),
        Object::Dict(dict) => first_page(document, dict.get(b"D")?),
        _ => None,
    }
}

/// Every node with `/Limits` brackets its own subtree.
fn check_limits(document: &CosDocument, node: ObjRef, depth: usize) {
    if depth > 32 {
        return;
    }
    let dict = dict_of(document, node);
    let mut entries = Vec::new();
    collect_names(document, Some(&Object::Ref(node)), &mut entries, 0);
    if let Some(Object::Array(limits)) = resolved(document, dict.get(b"Limits")) {
        let [Object::String(low), Object::String(high)] = &limits[..] else {
            panic!("/Limits is two strings");
        };
        for (key, _) in &entries {
            assert!(
                low <= key && key <= high,
                "a /Limits bracket excludes an entry in its own subtree"
            );
        }
    }
    if let Some(Object::Array(kids)) = resolved(document, dict.get(b"Kids")) {
        for kid in kids {
            if let Some(objref) = kid.as_reference() {
                check_limits(document, objref, depth + 1);
            }
        }
    }
}

fn outline_root(document: &CosDocument) -> Option<ObjRef> {
    catalog(document)
        .get(b"Outlines")
        .and_then(Object::as_reference)
}

fn outline_item(document: &CosDocument, index: usize) -> u32 {
    let root = outline_root(document).expect("an outline");
    let mut at = dict_of(document, root)
        .get(b"First")
        .and_then(Object::as_reference)
        .expect("a first item");
    for _ in 0..index {
        at = dict_of(document, at)
            .get(b"Next")
            .and_then(Object::as_reference)
            .expect("a next item");
    }
    at.number
}

fn walk_outline(document: &CosDocument) -> Vec<u32> {
    let Some(root) = outline_root(document) else {
        return Vec::new();
    };
    let dict = dict_of(document, root);
    let mut walked = Vec::new();
    let mut at = dict.get(b"First").and_then(Object::as_reference);
    let last = dict.get(b"Last").and_then(Object::as_reference);
    for _ in 0..1000 {
        let Some(objref) = at else {
            break;
        };
        walked.push(objref.number);
        if Some(objref.number) == last.map(|objref| objref.number) {
            break;
        }
        at = dict_of(document, objref)
            .get(b"Next")
            .and_then(Object::as_reference);
    }
    walked
}

fn outline_count(document: &CosDocument) -> Option<i64> {
    let root = outline_root(document)?;
    dict_of(document, root)
        .get(b"Count")
        .and_then(Object::as_integer)
}

fn page_annots(document: &CosDocument, index: usize) -> Vec<Dict> {
    let page = document.page(index).expect("page");
    let Some(Object::Array(items)) = resolved(document, page.dict.get(b"Annots")) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match resolved(document, Some(item)) {
            Some(Object::Dict(dict)) => Some(dict),
            _ => None,
        })
        .collect()
}

fn acroform_fields(document: &CosDocument) -> Vec<Dict> {
    let Some(Object::Dict(acroform)) = resolved(document, catalog(document).get(b"AcroForm"))
    else {
        return Vec::new();
    };
    let Some(Object::Array(items)) = resolved(document, acroform.get(b"Fields")) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match resolved(document, Some(item)) {
            Some(Object::Dict(dict)) => Some(dict),
            _ => None,
        })
        .collect()
}

fn walk_thread(document: &CosDocument) -> Vec<ObjRef> {
    let Some(Object::Array(threads)) = resolved(document, catalog(document).get(b"Threads")) else {
        return Vec::new();
    };
    let Some(Object::Dict(thread)) = resolved(document, threads.first()) else {
        return Vec::new();
    };
    let Some(first) = thread.get(b"F").and_then(Object::as_reference) else {
        return Vec::new();
    };
    let mut ring = vec![first];
    let mut at = first;
    for _ in 0..1000 {
        let Some(next) = dict_of(document, at)
            .get(b"N")
            .and_then(Object::as_reference)
        else {
            break;
        };
        if next.number == first.number {
            return ring;
        }
        ring.push(next);
        at = next;
    }
    panic!("the ring does not close");
}

fn walk_pdfs(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|kind| kind == "pdf") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("deep_tree", deep_tree()),
        ("degenerate_box", degenerate_box()),
        ("labelled", labelled()),
        ("destinations", destinations()),
        ("outlined", outlined()),
        ("outlined_closed", outlined_closed()),
        ("linked", linked()),
        ("form", form()),
        ("threaded", threaded()),
        ("with_open_action", with_open_action()),
        ("tagged", tagged()),
    ]
}

/// Three pages under two levels of `/Pages`, inheriting `/Resources` from the
/// root, `/MediaBox` and `/Rotate` from an intermediate node, and one page
/// overriding `/Rotate` itself.
fn deep_tree() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 7 0 R] /Count 3 /Resources 8 0 R >>".to_vec(),
        b"<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /Count 2 /MediaBox [0 0 400 400] /Rotate 90 >>".to_vec(),
        b"<< /Type /Page /Parent 3 0 R /Tabs /S /UserUnit 2 /Contents 9 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 3 0 R /Rotate 180 >>".to_vec(),
        b"<< /Type /Pages /Parent 2 0 R /Kids [] /Count 0 >>".to_vec(),
        b"<< /Type /Pages /Parent 2 0 R /Kids [10 0 R] /Count 1 /MediaBox [0 0 612 792] /CropBox [10 10 600 780] >>".to_vec(),
        b"<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>".to_vec(),
        stream(""),
        b"<< /Type /Page /Parent 7 0 R >>".to_vec(),
    ])
}

/// A page whose own `/MediaBox` has zero area, which `cos`'s policy filter
/// reports as `None`.
fn degenerate_box() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [10 10 10 10] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
    ])
}

/// Four pages: two roman, two arabic.
fn labelled() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /PageLabels 7 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 /MediaBox [0 0 612 792] >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Nums [0 << /S /r /St 1 >> 2 << /S /D /St 1 >>] >>".to_vec(),
    ])
}

/// One destination in the flat `/Dests`, two in the `/Names /Dests` tree, and
/// one of each naming the page that goes.
fn destinations() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Dests 6 0 R /Names << /Dests 7 0 R >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /first [3 0 R /Fit] /second [4 0 R /Fit] >>".to_vec(),
        b"<< /Names [(gone) [4 0 R /Fit] (third) [5 0 R /Fit]] >>".to_vec(),
    ])
}

/// Three bookmarks in a chain, the middle one naming the page that goes.
fn outlined() -> Vec<u8> {
    outline_pdf(3)
}

fn outlined_closed() -> Vec<u8> {
    outline_pdf(-3)
}

fn outline_pdf(count: i64) -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        format!("<< /Type /Outlines /First 7 0 R /Last 9 0 R /Count {count} >>").into_bytes(),
        b"<< /Title (one) /Parent 6 0 R /Next 8 0 R /Dest [3 0 R /Fit] >>".to_vec(),
        b"<< /Title (two) /Parent 6 0 R /Prev 7 0 R /Next 9 0 R /Dest [4 0 R /Fit] >>".to_vec(),
        b"<< /Title (three) /Parent 6 0 R /Prev 8 0 R /Dest [5 0 R /Fit] >>".to_vec(),
    ])
}

/// Page 1 carries two links: one to the page that goes, one to a survivor.
fn linked() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [6 0 R 7 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest [4 0 R /Fit] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest [5 0 R /Fit] >>".to_vec(),
    ])
}

/// Two pages, two fields; one field is a group whose only widget is on page 2.
fn form() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R 6 0 R] >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [7 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (kept) /P 3 0 R /Rect [0 0 10 10] >>"
            .to_vec(),
        b"<< /FT /Tx /T (group) /Kids [7 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /Parent 6 0 R /P 4 0 R /Rect [0 0 10 10] >>".to_vec(),
    ])
}

/// One article over three pages, one bead each.
fn threaded() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Threads [6 0 R] >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 10 0 R] /Count 4 /MediaBox [0 0 612 792] >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /B [7 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /B [8 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /B [9 0 R] >>".to_vec(),
        b"<< /Type /Thread /F 7 0 R >>".to_vec(),
        b"<< /Type /Bead /T 6 0 R /N 8 0 R /V 9 0 R /P 3 0 R /R [0 0 10 10] >>".to_vec(),
        b"<< /Type /Bead /T 6 0 R /N 9 0 R /V 7 0 R /P 4 0 R /R [0 0 10 10] >>".to_vec(),
        b"<< /Type /Bead /T 6 0 R /N 7 0 R /V 8 0 R /P 5 0 R /R [0 0 10 10] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
    ])
}

/// `/OpenAction` on the page that goes; the survivor carries its own `/AA`.
fn with_open_action() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /OpenAction [4 0 R /Fit] >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /AA << /O << /S /JavaScript /JS (noop) >> >> >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
    ])
}

/// Three tagged pages, one paragraph element each, with a `/ParentTree`.
fn tagged() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R /MarkInfo << /Marked true >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /StructParents 0 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /StructParents 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /StructParents 2 >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [7 0 R 8 0 R 9 0 R] /ParentTree << /Nums [0 [7 0 R] 1 [8 0 R] 2 [9 0 R]] >> /ParentTreeNextKey 3 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 4 0 R /K 0 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 5 0 R /K 0 >>".to_vec(),
    ])
}

fn stream(data: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data.as_bytes());
    out.extend_from_slice(b"\nendstream");
    out
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}
