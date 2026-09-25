//! `core::pages` page organization: the nine operations and the importer.
//!
//! Every assertion reads the result through a **fresh parse** of the original
//! bytes with the edit's section appended, never through the overlay that
//! produced it. Page order is compared by **extracted text**, so a reorder that
//! keeps the count and shuffles nothing is caught; every output is audited for
//! dangling references.
//!
//! The importer's decisive test is the render comparison: an inserted page must
//! draw exactly as it drew in its source. A one-level-deep importer - the page
//! dictionary copied, its references left pointing at whatever the destination
//! happens to have at those numbers - passes every structural check here and
//! fails that one, which is why its destination is built with more objects than
//! the source has: every stale reference then resolves to *something*.

use std::collections::BTreeSet;

use onionskin_core::pages::{
    delete_pages, extract_pages, insert_blank_pages, insert_pages_from, move_pages,
    replace_pages_from, rotate_pages, set_page_labels, LabelRange, LabelStyle,
};
use onionskin_core::protection::Refusal;
use onionskin_core::{check, read_structure, Document, EditSession, Error, Structure, Transaction};
use onionskin_corpus_testing::{encrypted_fixture, organize_fixture};
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, ObjRef, Object};

mod common;
use common::{deep, flat, pdf, tagged};

// ---------------------------------------------------------------------------
// The operations, each on a flat tree and a deep one
// ---------------------------------------------------------------------------

#[test]
fn delete_drops_exactly_the_pages_asked_for() {
    for (name, original) in shapes() {
        let (saved, _) = apply(&original, |tx, s| delete_pages(tx, s, &[1])).expect("deletes");
        assert_pages(&saved, &["Page 1", "Page 3", "Page 4"], name);
    }
}

#[test]
fn deleting_every_page_is_refused_and_writes_nothing() {
    for (name, original) in shapes() {
        let refused = apply(&original, |tx, s| delete_pages(tx, s, &[0, 1, 2, 3]));
        assert!(
            matches!(refused, Err(Error::WouldLeaveNoPages)),
            "{name}: {:?}",
            refused.err()
        );
    }
}

#[test]
fn move_puts_pages_before_the_one_named_and_keeps_their_order() {
    let cases: [(&[usize], usize, [&str; 4]); 4] = [
        (&[3], 0, ["Page 4", "Page 1", "Page 2", "Page 3"]),
        (&[0, 1], 4, ["Page 3", "Page 4", "Page 1", "Page 2"]),
        (&[2], 1, ["Page 1", "Page 3", "Page 2", "Page 4"]),
        // Written out of order, moved in document order.
        (&[3, 0], 2, ["Page 2", "Page 1", "Page 4", "Page 3"]),
    ];
    for (name, original) in shapes() {
        for (pages, before, expected) in cases {
            let (saved, _) =
                apply(&original, |tx, s| move_pages(tx, s, pages, before)).expect("moves");
            assert_pages(
                &saved,
                &expected,
                &format!("{name}: {pages:?} before {before}"),
            );
        }
    }
}

#[test]
fn a_move_past_the_end_is_refused() {
    let refused = apply(&flat(4), |tx, s| move_pages(tx, s, &[0], 5));
    assert!(matches!(
        refused,
        Err(Error::NoSuchPage { page: 5, count: 4 })
    ));
}

/// On `/Rotate` in the file, read back raw from a fresh parse - not on the
/// view, which would pass every test that only looks at pixels on screen.
#[test]
fn rotation_is_written_to_the_page_and_composes_with_what_it_inherited() {
    // The deep tree's first two pages inherit /Rotate 270 from their parent.
    let (saved, _) = apply(&deep(), |tx, _| rotate_pages(tx, &[0], 1)).expect("rotates");
    assert_eq!(
        own_rotate(&saved, 0),
        Some(0),
        "270 plus a quarter turn is 0, never 360"
    );
    assert_eq!(own_rotate(&saved, 1), None, "and the sibling is untouched");
    assert_pages(&saved, &["Page 1", "Page 2", "Page 3", "Page 4"], "deep");

    let (saved, _) = apply(&flat(4), |tx, _| rotate_pages(tx, &[0, 2], -1)).expect("rotates");
    assert_eq!(
        own_rotate(&saved, 0),
        Some(270),
        "a negative turn wraps too"
    );
    assert_eq!(own_rotate(&saved, 2), Some(270));
    assert_eq!(own_rotate(&saved, 1), None);

    let (saved, _) = apply(&flat(4), |tx, _| {
        rotate_pages(tx, &[0], 2)?;
        rotate_pages(tx, &[0], 2)
    })
    .expect("rotates");
    assert_eq!(own_rotate(&saved, 0), Some(0), "two half turns are none");
}

#[test]
fn a_blank_page_lands_where_asked_at_the_size_asked() {
    for (name, original) in shapes() {
        let (saved, _) = apply(&original, |tx, s| {
            insert_blank_pages(tx, s, 2, 2, [0.0, 0.0, 300.0, 500.0])
        })
        .expect("inserts");
        assert_pages(
            &saved,
            &["Page 1", "Page 2", "", "", "Page 3", "Page 4"],
            name,
        );
        let after = open(&saved);
        let blank = after.page(2).expect("the blank page");
        let Some(Object::Array(media_box)) = blank.dict.get(b"MediaBox") else {
            panic!("{name}: the blank page has its own /MediaBox");
        };
        let media_box: Vec<f64> = media_box.iter().filter_map(number).collect();
        assert_eq!(media_box, [0.0, 0.0, 300.0, 500.0], "{name}");
    }
}

#[test]
fn inserted_pages_arrive_in_the_order_asked_for() {
    let source = embedded_font();
    for (name, original) in shapes() {
        let (saved, _) = apply(&original, |tx, s| {
            insert_pages_from(tx, s, &source, &[1, 0], 1)
        })
        .expect("inserts");
        assert_pages(
            &saved,
            &[
                "Page 1",
                "Second source page",
                "Imported glyphs",
                "Page 2",
                "Page 3",
                "Page 4",
            ],
            name,
        );
    }
}

#[test]
fn replace_swaps_pages_one_for_one() {
    let source = embedded_font();
    for (name, original) in shapes() {
        let (saved, _) = apply(&original, |tx, s| {
            replace_pages_from(tx, s, &source, &[1, 0], &[1, 2])
        })
        .expect("replaces");
        assert_pages(
            &saved,
            &["Page 1", "Second source page", "Imported glyphs", "Page 4"],
            name,
        );
    }
    let mismatched = apply(&flat(4), |tx, s| {
        replace_pages_from(tx, s, &source, &[0], &[1, 2])
    });
    assert!(matches!(
        mismatched,
        Err(Error::ReplacementCountMismatch {
            replacements: 1,
            targets: 2
        })
    ));
}

/// The review risk: replace built as delete-then-insert is two undo entries,
/// and undoing one of them leaves a document nobody chose.
#[test]
fn replace_is_one_undo_step() {
    let original = flat(4);
    let source = embedded_font();
    let base = open(&original);
    let structure = read_structure(&base).expect("structure");
    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Replace Pages", |tx| {
        replace_pages_from(tx, &structure, &source, &[0], &[3]).map(|_| ())
    })
    .expect("replaces");
    assert_eq!(edit.history().undo_label(), Some("Replace Pages"));

    assert!(edit.undo(&base).expect("undo runs"));
    assert!(
        edit.pending_edits().is_empty() && edit.trailer_edits().is_empty(),
        "one undo took the whole replacement back"
    );
    assert!(
        !edit.undo(&base).expect("undo runs"),
        "and there was one entry"
    );
}

#[test]
fn extract_writes_a_new_document_holding_exactly_those_pages() {
    for (name, original) in shapes() {
        let bytes = extract_pages(&open(&original), &[3, 1]).expect("extracts");
        assert_pages(&bytes, &["Page 4", "Page 2"], name);
        let extracted = open(&bytes);
        assert_eq!(
            extracted.sections().expect("sections").len(),
            1,
            "{name}: a new document, not a section over the old one"
        );
    }
    let empty = extract_pages(&open(&flat(2)), &[]);
    assert!(matches!(empty, Err(Error::WouldLeaveNoPages)));
}

/// Extracting is the importer too, so it carries fonts and images: the
/// extracted page draws exactly as the source page does.
#[test]
fn an_extracted_page_renders_exactly_as_it_did_in_its_source() {
    let path = organize_fixture("embedded-font.pdf");
    let bytes = extract_pages(&open(&std::fs::read(&path).expect("read")), &[0]).expect("extracts");
    assert_same_render(
        &render(&std::fs::read(&path).expect("read"), 0),
        &render(&bytes, 0),
    );
}

#[test]
fn page_labels_are_written_and_cleared() {
    for (name, original) in shapes() {
        let ranges = [
            LabelRange {
                start: 0,
                style: Some(LabelStyle::LowerRoman),
                prefix: None,
                first: 1,
            },
            LabelRange {
                start: 2,
                style: Some(LabelStyle::Decimal),
                prefix: Some("A-".into()),
                first: 5,
            },
        ];
        let (saved, _) = apply(&original, |tx, _| set_page_labels(tx, &ranges)).expect("labels");
        assert_eq!(
            label_entries(&saved),
            vec![
                (0, Some("r".to_owned()), None, None),
                (2, Some("D".to_owned()), Some("A-".to_owned()), Some(5)),
            ],
            "{name}"
        );

        let (cleared, _) = apply(&saved, |tx, _| set_page_labels(tx, &[])).expect("clears");
        assert!(
            catalog(&open(&cleared)).get(b"PageLabels").is_none(),
            "{name}: no ranges means plain page numbers"
        );
    }
    let refused = apply(&flat(4), |tx, _| {
        set_page_labels(
            tx,
            &[LabelRange {
                start: 4,
                style: None,
                prefix: None,
                first: 1,
            }],
        )
    });
    assert!(matches!(
        refused,
        Err(Error::NoSuchPage { page: 4, count: 4 })
    ));
}

/// Copying between two open documents reads the source as its session has
/// it, unsaved edits included - and moving leaves the source's undo able to
/// put its pages back.
#[test]
fn a_page_copied_between_open_documents_carries_the_sources_unsaved_edits() {
    let mut source = Document::open_bytes(flat(3)).expect("opens");
    source
        .edit_pages("Rotate", |tx, _| rotate_pages(tx, &[2], 1))
        .expect("rotates");
    let mut destination = Document::open_bytes(flat(2)).expect("opens");

    let from = source.structure().expect("the source's current state");
    destination
        .edit_pages("Insert Pages", |tx, s| {
            insert_pages_from(tx, s, from, &[2], 0).map(|_| ())
        })
        .expect("inserts");

    assert_eq!(
        destination.page_count(),
        3,
        "the session counts the new page"
    );
    assert_eq!(
        session_texts(&mut destination),
        ["Page 3", "Page 1", "Page 2"]
    );
    let current = destination.structure().expect("current");
    assert_eq!(
        current.page(0).expect("page").dict.get(b"Rotate"),
        Some(&Object::Integer(90)),
        "the source's unsaved rotation came with the page"
    );
}

// ---------------------------------------------------------------------------
// The importer
// ---------------------------------------------------------------------------

/// The decisive test. The source page's glyphs live in a `/FontFile2` three
/// references below the page, beside an image, a form XObject and an
/// annotation's appearance stream; the destination has more objects than the
/// source, so a shallow copy's stale references all resolve - to the wrong
/// objects - and only the pixels can tell.
#[test]
fn an_inserted_page_renders_exactly_as_it_did_in_its_source() {
    let source_bytes = std::fs::read(organize_fixture("embedded-font.pdf")).expect("read");
    let source = open(&source_bytes);
    let destination = flat(12);
    assert!(
        open(&destination).next_object_number() > source.next_object_number(),
        "the destination must have an object at every number the source uses, \
         or a shallow importer is caught by the audit instead of the render"
    );

    let (saved, _) = apply(&destination, |tx, s| {
        insert_pages_from(tx, s, &source, &[0], 1)
    })
    .expect("inserts");
    // Everything structural, which a shallow importer also passes: this is
    // why the pixels are the assertion that decides.
    let after = open(&saved);
    assert_eq!(after.page_count().expect("pages"), 13);
    assert_eq!(
        after.audit_references().expect("the audit runs"),
        Vec::new()
    );

    let expected = render(&source_bytes, 0);
    assert!(
        ink(&expected) > 5_000,
        "the source page draws something substantial, or equality proves nothing"
    );
    assert_same_render(&expected, &render(&saved, 1));
}

/// Import into a document whose numbers overlap the source's: every copied
/// object is renumbered, and the destination's own objects at those numbers
/// are exactly what they were.
#[test]
fn imported_objects_are_renumbered_and_the_destinations_own_are_untouched() {
    let original = flat(4);
    let base = open(&original);
    let source = embedded_font();
    let highest = base.next_object_number() - 1;
    let before: Vec<(u32, Object)> = (1..=highest)
        .filter_map(|number| base.get(number).ok().map(|parsed| (number, parsed.object)))
        .collect();

    let (saved, _) = apply(&original, |tx, s| {
        insert_pages_from(tx, s, &source, &[0], 4)
    })
    .expect("inserts");
    let after = open(&saved);
    let imported = [after.page(4).expect("the inserted page").objref];

    for (number, object) in &before {
        let now = after.get(*number).expect("still there").object;
        let page_tree_untouched = !is_page_or_pages(object);
        if page_tree_untouched {
            assert_eq!(
                &now, object,
                "object {number} was overwritten by the import"
            );
        }
    }
    assert!(
        imported.iter().all(|page| page.number > highest),
        "the imported page got a fresh number: {imported:?}"
    );
    let reached = reachable(&after, imported[0]);
    assert!(
        reached.len() >= 10,
        "the page, its resources, font, descriptor, font file, image, form and \
         annotation all came: only {} objects reached",
        reached.len()
    );
    assert!(
        reached.iter().all(|number| *number > highest),
        "and every one of them is new: {reached:?}"
    );
}

/// A form XObject whose resources name the form itself is legal and common.
/// The copier terminates on it, and the copy's cycle closes on the copy.
#[test]
fn a_cyclic_form_is_copied_once_and_still_names_itself() {
    let (saved, _) = apply(&flat(2), |tx, s| {
        insert_pages_from(tx, s, &embedded_font(), &[0], 2)
    })
    .expect("inserts");
    let after = open(&saved);
    let page = dict(&after, after.page(2).expect("the inserted page").objref);
    let resources = resolved_dict(&after, page.get(b"Resources"));
    let xobjects = resolved_dict(&after, resources.get(b"XObject"));
    let form = xobjects
        .get(b"Fm1")
        .and_then(Object::as_reference)
        .expect("the form is indirect");
    let form_dict = stream_dict(&after, form);
    let inner = resolved_dict(&after, form_dict.get(b"Resources"));
    let inner = resolved_dict(&after, inner.get(b"XObject"));
    assert_eq!(
        inner.get(b"Self").and_then(Object::as_reference),
        Some(form),
        "the copied form's /Self is the copied form, not the source's number"
    );
}

/// A link to a page that was not imported has nothing to mean here; one to a
/// page imported alongside it points at that page's copy.
#[test]
fn a_reference_to_another_source_page_is_nulled_unless_that_page_came_too() {
    let source = open(&linked_source());

    let (saved, _) =
        apply(&flat(2), |tx, s| insert_pages_from(tx, s, &source, &[0], 2)).expect("inserts");
    let after = open(&saved);
    let inserted = after.page(2).expect("the inserted page").objref;
    assert_eq!(
        link_target(&after, inserted),
        Some(Object::Null),
        "the page-2 destination was not followed into the source's page tree"
    );
    assert!(
        !reachable(&after, inserted).iter().any(|number| {
            after
                .get(*number)
                .is_ok_and(|parsed| is_page_or_pages(&parsed.object) && *number != inserted.number)
        }),
        "and no other page, or page-tree node, came with it"
    );

    let (saved, _) = apply(&flat(2), |tx, s| {
        insert_pages_from(tx, s, &source, &[0, 1], 2)
    })
    .expect("inserts");
    let after = open(&saved);
    assert_eq!(
        link_target(&after, after.page(2).expect("page").objref),
        Some(Object::Ref(after.page(3).expect("page").objref)),
        "imported together, the link follows its page"
    );
}

/// `/StructParents` indexes the source's `/ParentTree` and `/B` its threads;
/// neither comes with the page.
#[test]
fn an_imported_page_leaves_the_sources_structure_keys_behind() {
    let source = open(&tagged());
    let (saved, _) =
        apply(&flat(2), |tx, s| insert_pages_from(tx, s, &source, &[1], 2)).expect("inserts");
    let after = open(&saved);
    let page = dict(&after, after.page(2).expect("the inserted page").objref);
    assert!(page.get(b"StructParents").is_none());
    assert_eq!(
        page.get(b"Parent").and_then(Object::as_reference),
        catalog(&after).get(b"Pages").and_then(Object::as_reference),
        "its parent is this document's tree, never the source's"
    );
}

// ---------------------------------------------------------------------------
// The encrypted-source rule, in each shape this package has
// ---------------------------------------------------------------------------

#[test]
fn inserting_from_an_encrypted_file_is_refused_and_imports_nothing() {
    let source = encrypted();
    let original = flat(2);
    let outcome = apply_saving(&original, |tx, s| {
        insert_pages_from(tx, s, &source, &[0], 1).map(|_| ())
    });
    assert_refused_with_nothing_written(outcome, &original);
}

/// Asserted separately from insert: an implementation that routes replace
/// through insert passes one test, and one that does not passes none.
#[test]
fn replacing_from_an_encrypted_file_is_refused_and_imports_nothing() {
    let source = encrypted();
    let original = flat(2);
    let outcome = apply_saving(&original, |tx, s| {
        replace_pages_from(tx, s, &source, &[0], &[1]).map(|_| ())
    });
    assert_refused_with_nothing_written(outcome, &original);
}

#[test]
fn extracting_from_an_encrypted_document_is_refused() {
    assert!(matches!(
        extract_pages(&encrypted(), &[0]),
        Err(Error::Protected(Refusal::EncryptedSource))
    ));
    // The session-scoped shape: the same predicate, asked ahead of time.
    let session = Document::open_path(&encrypted_fixture("r6-aes-256-print-only.pdf"))
        .expect("opens read-only");
    assert_eq!(session.read_out_refusal(), Some(Refusal::EncryptedSource));
    assert!(Document::open_bytes(flat(1))
        .expect("opens")
        .read_out_refusal()
        .is_none());
}

// ---------------------------------------------------------------------------
// P4's invariant, through every operation on a tagged document
// ---------------------------------------------------------------------------

#[test]
fn a_tagged_document_stays_valid_through_every_operation() {
    let original = tagged();
    let source = embedded_font();
    let operations: Vec<(&str, Operation)> = vec![
        (
            "delete",
            Box::new(|tx, s| delete_pages(tx, s, &[1]).map(|_| ())),
        ),
        (
            "move",
            Box::new(|tx, s| move_pages(tx, s, &[2], 0).map(|_| ())),
        ),
        ("rotate", Box::new(|tx, _| rotate_pages(tx, &[0], 1))),
        (
            "blank",
            Box::new(|tx, s| insert_blank_pages(tx, s, 1, 1, [0.0, 0.0, 612.0, 792.0]).map(|_| ())),
        ),
        (
            "insert",
            Box::new(|tx, s| insert_pages_from(tx, s, &source, &[0], 3).map(|_| ())),
        ),
        (
            "replace",
            Box::new(|tx, s| replace_pages_from(tx, s, &source, &[0], &[1]).map(|_| ())),
        ),
        (
            "labels",
            Box::new(|tx, _| {
                set_page_labels(
                    tx,
                    &[LabelRange {
                        start: 0,
                        style: Some(LabelStyle::UpperLetters),
                        prefix: None,
                        first: 1,
                    }],
                )
            }),
        ),
    ];
    for (name, operation) in operations {
        let (saved, _) = apply(&original, |tx, s| operation(tx, s)).expect(name);
        let after = open(&saved);
        let structure = read_structure(&after).expect("the structure reads");
        let pages = after.page_count().expect("pages") as usize;
        let report = check(&after, &structure, pages).expect("check runs");
        assert!(
            report.violations.is_empty(),
            "{name} left the structure tree invalid: {:?}",
            report.violations
        );
        let live: BTreeSet<u32> = (0..pages)
            .map(|index| after.page(index).expect("page").objref.number)
            .collect();
        let order = reading_order(&after);
        assert_eq!(
            order.len(),
            root_kids(&after).len(),
            "{name}: every element in the reading order is on a page"
        );
        assert!(
            order.iter().all(|page| live.contains(page)),
            "{name}: and every one of those pages is still in the document"
        );
    }

    let (saved, _) = apply(&original, |tx, s| move_pages(tx, s, &[2], 0)).expect("moves");
    let after = open(&saved);
    let pages: Vec<u32> = (0..3)
        .map(|index| after.page(index).expect("page").objref.number)
        .collect();
    assert_eq!(
        reading_order(&after),
        pages,
        "the reading order follows the new page order"
    );
}

/// The reason `Document::edit_pages` reads the session's tree: two deletes in
/// a row on a tagged document, the second handed the tree as the first left
/// it. Handed the file's tree, the second would put back the element the
/// first removed.
#[test]
fn consecutive_page_edits_in_one_session_keep_the_structure_valid() {
    let mut document = Document::open_bytes(tagged()).expect("opens");
    for _ in 0..2 {
        document
            .edit_pages("Delete Pages", |tx, s| {
                delete_pages(tx, s, &[0]).map(|_| ())
            })
            .expect("deletes");
    }
    assert_eq!(document.page_count(), 1);
    assert_eq!(session_texts(&mut document), ["Page 3"]);
    let current = document.structure().expect("current");
    let structure = read_structure(current).expect("reads");
    let report = check(current, &structure, 1).expect("check runs");
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert_eq!(
        root_kids(current).len(),
        1,
        "one page, one element: the first delete's removal survived the second"
    );
}

#[test]
fn the_sessions_page_count_follows_edits_and_their_undo() {
    let mut document = Document::open_bytes(flat(4)).expect("opens");
    document
        .edit_pages("Delete Pages", |tx, s| {
            delete_pages(tx, s, &[0, 1]).map(|_| ())
        })
        .expect("deletes");
    assert_eq!(document.page_count(), 2);
    let (edit, base) = document.edit_mut();
    assert!(edit.undo(base).expect("undo"));
    assert_eq!(document.page_count(), 4, "the undo gave the pages back");
    let (edit, base) = document.edit_mut();
    assert!(edit.redo(base).expect("redo"));
    assert_eq!(document.page_count(), 2);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type Operation<'a> =
    Box<dyn Fn(&mut Transaction<'_>, &Structure) -> onionskin_core::Result<()> + 'a>;

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn embedded_font() -> CosDocument {
    open(&std::fs::read(organize_fixture("embedded-font.pdf")).expect("the fixture reads"))
}

fn encrypted() -> CosDocument {
    open(&std::fs::read(encrypted_fixture("r6-aes-256-print-only.pdf")).expect("the fixture reads"))
}

/// Run `body` in one transaction over `original` and append its section.
fn apply<T>(
    original: &[u8],
    body: impl FnOnce(&mut Transaction<'_>, &Structure) -> onionskin_core::Result<T>,
) -> onionskin_core::Result<(Vec<u8>, T)> {
    let base = open(original);
    let structure = read_structure(&base).expect("the structure reads");
    let mut edit = EditSession::for_base(&base);
    let value = edit.transact(&base, "Organize", |tx| body(tx, &structure))?;
    Ok((saved(original, &base, &edit), value))
}

/// The same, keeping what would be saved even when the body fails, so a
/// refusal can be asserted on the output rather than on the error alone.
fn apply_saving(
    original: &[u8],
    body: impl FnOnce(&mut Transaction<'_>, &Structure) -> onionskin_core::Result<()>,
) -> (onionskin_core::Result<()>, Vec<u8>) {
    let base = open(original);
    let structure = read_structure(&base).expect("the structure reads");
    let mut edit = EditSession::for_base(&base);
    let outcome = edit.transact(&base, "Organize", |tx| body(tx, &structure));
    (outcome, saved(original, &base, &edit))
}

fn saved(original: &[u8], base: &CosDocument, edit: &EditSession) -> Vec<u8> {
    let mut bytes = original.to_vec();
    if let Some(section) = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
    {
        bytes.extend_from_slice(&section);
    }
    bytes
}

fn assert_refused_with_nothing_written(
    (outcome, saved): (onionskin_core::Result<()>, Vec<u8>),
    original: &[u8],
) {
    assert!(
        matches!(outcome, Err(Error::Protected(Refusal::EncryptedSource))),
        "{:?}",
        outcome.err()
    );
    assert_eq!(
        saved, original,
        "the saved output is the original: no imported object, no section"
    );
}

/// Page count, page order by extracted text, and a clean reference audit, all
/// from a fresh parse.
fn assert_pages(bytes: &[u8], expected: &[&str], context: &str) {
    let parsed = open(bytes);
    assert_eq!(
        parsed.page_count().expect("pages") as usize,
        expected.len(),
        "{context}: page count"
    );
    assert_eq!(texts(bytes), expected, "{context}: page order");
    assert_eq!(
        parsed.audit_references().expect("the audit runs"),
        Vec::new(),
        "{context}: dangling references"
    );
}

fn texts(bytes: &[u8]) -> Vec<String> {
    session_texts(&mut Document::open_bytes(bytes.to_vec()).expect("opens"))
}

fn session_texts(document: &mut Document) -> Vec<String> {
    (0..document.page_count())
        .map(|index| {
            document
                .page_text(index)
                .expect("text extracts")
                .runs
                .iter()
                .map(|run| run.decoded_text.as_str())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect()
}

fn render(bytes: &[u8], page: usize) -> onionskin_core::PageRender {
    Document::open_bytes(bytes.to_vec())
        .expect("opens")
        .render_page_now(page, 1.0)
        .expect("renders")
}

/// Exact equality, which is the tolerance the render suite uses: the same
/// renderer over the same objects produces the same bytes.
fn assert_same_render(expected: &onionskin_core::PageRender, actual: &onionskin_core::PageRender) {
    assert!(expected.warnings.is_empty(), "{:?}", expected.warnings);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert_eq!(
        (actual.raster.width(), actual.raster.height()),
        (expected.raster.width(), expected.raster.height()),
        "the page's size changed"
    );
    let differing = expected
        .raster
        .rgba()
        .chunks(4)
        .zip(actual.raster.rgba().chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "{differing} pixels differ from the source's render"
    );
}

fn ink(render: &onionskin_core::PageRender) -> usize {
    render
        .raster
        .rgba()
        .chunks(4)
        .filter(|pixel| pixel[..3].iter().any(|channel| *channel < 200))
        .count()
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn own_rotate(bytes: &[u8], index: usize) -> Option<i64> {
    open(bytes)
        .page(index)
        .expect("page")
        .dict
        .get(b"Rotate")
        .and_then(Object::as_integer)
}

fn catalog(document: &CosDocument) -> Dict {
    document.catalog().expect("catalog")
}

fn dict(document: &CosDocument, objref: ObjRef) -> Dict {
    match document.get(objref.number).expect("present").object {
        Object::Dict(dict) => dict,
        other => panic!("{objref:?} is not a dictionary: {other:?}"),
    }
}

fn stream_dict(document: &CosDocument, objref: ObjRef) -> Dict {
    match document.get(objref.number).expect("present").object {
        Object::Stream(stream) => stream.dict,
        other => panic!("{objref:?} is not a stream: {other:?}"),
    }
}

fn resolved_dict(document: &CosDocument, value: Option<&Object>) -> Dict {
    match value {
        Some(Object::Dict(dict)) => dict.clone(),
        Some(Object::Ref(objref)) => dict(document, *objref),
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn is_page_or_pages(object: &Object) -> bool {
    let Object::Dict(dict) = object else {
        return false;
    };
    matches!(
        dict.get(b"Type")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes()),
        Some(b"Page") | Some(b"Pages") | Some(b"Catalog")
    )
}

/// Every object number reachable from `from`, not following `/Parent`: that
/// is the tree the page was placed in, not something it brought.
fn reachable(document: &CosDocument, from: ObjRef) -> BTreeSet<u32> {
    let mut seen = BTreeSet::new();
    let mut queue = vec![from.number];
    while let Some(number) = queue.pop() {
        if !seen.insert(number) {
            continue;
        }
        if let Ok(parsed) = document.get(number) {
            collect_refs(&parsed.object, &mut queue);
        }
    }
    seen
}

fn collect_refs(object: &Object, into: &mut Vec<u32>) {
    match object {
        Object::Ref(objref) => into.push(objref.number),
        Object::Array(items) => items.iter().for_each(|item| collect_refs(item, into)),
        Object::Dict(dict) => dict
            .iter()
            .filter(|(key, _)| key.as_bytes() != b"Parent")
            .for_each(|(_, value)| collect_refs(value, into)),
        Object::Stream(stream) => stream
            .dict
            .iter()
            .for_each(|(_, value)| collect_refs(value, into)),
        _ => {}
    }
}

fn link_target(document: &CosDocument, page: ObjRef) -> Option<Object> {
    let page = dict(document, page);
    let Some(Object::Array(annots)) = page.get(b"Annots") else {
        panic!("the page has its annotations");
    };
    let link = resolved_dict(document, annots.first());
    match link.get(b"Dest") {
        Some(Object::Array(destination)) => destination.first().cloned(),
        other => panic!("no /Dest array: {other:?}"),
    }
}

/// `(start, /S, /P, /St)` for each range in the number tree's root `/Nums`.
/// One label range as written: `(start, /S, /P, /St)`.
type LabelEntry = (i64, Option<String>, Option<String>, Option<i64>);

fn label_entries(bytes: &[u8]) -> Vec<LabelEntry> {
    let document = open(bytes);
    let root = resolved_dict(&document, catalog(&document).get(b"PageLabels"));
    let Some(Object::Array(nums)) = root.get(b"Nums") else {
        panic!("a flat /Nums");
    };
    nums.chunks(2)
        .map(|pair| {
            let label = resolved_dict(&document, pair.get(1));
            (
                pair[0].as_integer().expect("a key"),
                label
                    .get(b"S")
                    .and_then(Object::as_name)
                    .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned()),
                match label.get(b"P") {
                    Some(Object::String(prefix)) => {
                        Some(String::from_utf8_lossy(prefix).into_owned())
                    }
                    _ => None,
                },
                label.get(b"St").and_then(Object::as_integer),
            )
        })
        .collect()
}

/// The structure root's `/K`, as written.
fn root_kids(document: &CosDocument) -> Vec<Object> {
    let root = catalog(document)
        .get(b"StructTreeRoot")
        .and_then(Object::as_reference)
        .expect("a structure tree");
    match dict(document, root).get(b"K").cloned() {
        Some(Object::Array(kids)) => kids,
        Some(single) => vec![single],
        None => Vec::new(),
    }
}

fn reading_order(document: &CosDocument) -> Vec<u32> {
    root_kids(document)
        .iter()
        .filter_map(|kid| {
            resolved_dict(document, Some(kid))
                .get(b"Pg")
                .and_then(Object::as_reference)
                .map(|page| page.number)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn shapes() -> [(&'static str, Vec<u8>); 2] {
    [("flat", flat(4)), ("deep", deep())]
}

/// Two pages; the first carries a link whose destination is the second.
fn linked_source() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest [4 0 R /Fit] >>".to_vec(),
    ])
}
