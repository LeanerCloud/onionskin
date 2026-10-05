//! The edit graph, from the outside.
//!
//! Every test here is named after a bullet in P2's verification list, because
//! this package has a defect that passes every obvious test: capturing a
//! change's `before` by reading the base document rather than the overlay makes
//! the *second* edit of an object un-undoable, and the property test, the
//! capture test and the round-trip test are all green against it. The test that
//! catches it is `second_edit_undone_restores_the_first_edit_not_the_base`, and
//! it is the reason this file exists in the shape it does.

use std::collections::BTreeMap;

mod common;

use onionskin_core::{
    AnnotationFilter, Change, Document, DocumentEdit, DocumentFile, EditSession, Error, History,
    ObjectState,
};
use onionskin_corpus_testing::seed;
use onionskin_cos::{
    BytesSource, Dict, Document as CosDocument, Name, ObjRef, Object, PendingEdit, XrefEntry,
};

/// `minimal.pdf` is a four-object document with a catalog, a page tree, one
/// page and **no `/Info`**, which is what makes it the fixture for every
/// trailer test below. It is tracked, so these run everywhere.
fn base() -> CosDocument {
    CosDocument::open_path(&seed("minimal.pdf")).expect("the minimal seed opens")
}

fn original_bytes() -> Vec<u8> {
    std::fs::read(seed("minimal.pdf")).expect("the minimal seed is readable")
}

fn session() -> (CosDocument, EditSession) {
    let base = base();
    let edit = EditSession::for_base(&base);
    (base, edit)
}

/// What a save would append, without writing a file.
fn section(base: &CosDocument, edit: &EditSession) -> Option<Vec<u8>> {
    base.section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
}

fn string(text: &str) -> Object {
    Object::String(text.as_bytes().to_vec())
}

fn set_description(edit: &mut EditSession, base: &CosDocument, text: &str) {
    edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Description"),
            value: Some(string(text)),
        },
    )
    .expect("the description is set");
}

/// A dictionary with one integer under `/N`, so that distinct edits to the same
/// object are distinguishable by value.
fn marker(n: i64) -> Object {
    let mut dict = Dict::new();
    dict.set(Name::new("N"), Object::Integer(n));
    Object::Dict(dict)
}

fn overlay_object(edit: &EditSession, number: u32) -> Option<ObjectState> {
    edit.overlay().object(number).cloned()
}

// ---------------------------------------------------------------------------
// The base-capture rule
// ---------------------------------------------------------------------------

/// Asserted on the `Change` itself and not inferred from undo working. A test
/// that only checks undo passes against an implementation that reads the base
/// at undo time, which stops working the moment the base is reopened.
#[test]
fn before_is_captured_from_the_base_by_value_at_edit_time() {
    let (base, mut edit) = session();
    let catalog_before = base.get(1).expect("the catalog parses").object;

    edit.transact(&base, "Marker", |tx| tx.put_object(1, 0, marker(1)))
        .expect("the transaction commits");

    assert_eq!(edit.history().reach(), 1, "one transaction makes one entry");
    let changes = last_changes(&edit);
    match &changes[0] {
        Change::Object {
            number,
            before,
            after,
        } => {
            assert_eq!(*number, 1);
            assert_eq!(
                before.as_ref().map(|state| &state.object),
                Some(&catalog_before),
                "before holds the base object by value"
            );
            assert_eq!(after.as_ref().map(|state| &state.object), Some(&marker(1)));
        }
        other => panic!("expected an object change, got {other:?}"),
    }
}

/// **The precedence, asserted on the intermediate state.** Every other test in
/// this file passes against the wrong implementation: the property test asserts
/// the end state, which holds either way because undoing edit 1 writes the base
/// value back and the collapse rule then drops it, and the capture test asserts
/// that `before` holds the base object, which is exactly what the wrong
/// implementation produces. Without this one the suite is green and `Ctrl+Z`
/// loses two edits.
#[test]
fn second_edit_undone_restores_the_first_edit_not_the_base() {
    let (base, mut edit) = session();

    edit.transact(&base, "First", |tx| tx.put_object(1, 0, marker(1)))
        .expect("the first edit commits");
    edit.transact(&base, "Second", |tx| tx.put_object(1, 0, marker(2)))
        .expect("the second edit commits");

    assert!(edit.undo(&base).expect("undo runs"), "there is a step back");

    assert_eq!(
        overlay_object(&edit, 1).map(|state| state.object),
        Some(marker(1)),
        "one undo leaves the FIRST edit in the overlay, not the base value"
    );
}

/// The invariant `None` rests on, stated as a check: the reservation counter is
/// the only thing that produces it.
#[test]
fn no_change_carries_before_none_for_a_number_the_base_has() {
    let (base, mut edit) = session();
    let sequence = generated_sequence(200);

    for (number, value) in &sequence {
        edit.transact(&base, "Marker", |tx| {
            tx.put_object(*number, 0, marker(*value))
        })
        .expect("the edit commits");
    }

    for number in sequence.iter().map(|(number, _)| *number) {
        for change in every_change(&edit) {
            let Change::Object {
                number: changed,
                before,
                ..
            } = change
            else {
                continue;
            };
            if changed == number && base_has(&base, number) {
                assert!(
                    before.is_some(),
                    "object {number} exists in the base, so no change may carry before: None"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Undo and redo
// ---------------------------------------------------------------------------

/// Apply N edits then undo N leaves the overlay byte-identical to empty,
/// including sequences that overwrite the same object repeatedly. That repeated
/// overwrite is the case a "drop the last overlay node" undo gets wrong.
#[test]
fn undoing_every_edit_leaves_the_overlay_empty() {
    let (base, mut edit) = session();
    let sequence = generated_sequence(300);

    for (number, value) in &sequence {
        edit.transact(&base, "Marker", |tx| {
            tx.put_object(*number, 0, marker(*value))
        })
        .expect("the edit commits");
    }
    assert!(!edit.overlay().is_empty(), "the edits reached the overlay");

    while edit.undo(&base).expect("undo runs") {}

    assert!(
        edit.overlay().is_empty(),
        "undoing everything leaves nothing behind"
    );
    assert_eq!(edit.history().reach(), 0);
}

#[test]
fn redo_after_undo_restores_exactly() {
    let (base, mut edit) = session();
    edit.transact(&base, "First", |tx| tx.put_object(1, 0, marker(1)))
        .expect("the first edit commits");
    edit.transact(&base, "Second", |tx| tx.put_object(1, 0, marker(2)))
        .expect("the second edit commits");
    let after_both = edit.overlay().clone();

    assert!(edit.undo(&base).expect("undo runs"));
    assert!(edit.redo(&base).expect("redo runs"));

    assert_eq!(
        edit.overlay(),
        &after_both,
        "redo restores the overlay exactly"
    );
}

#[test]
fn an_edit_after_an_undo_truncates_the_redo_tail() {
    let (base, mut edit) = session();
    edit.transact(&base, "First", |tx| tx.put_object(1, 0, marker(1)))
        .expect("the first edit commits");
    edit.transact(&base, "Second", |tx| tx.put_object(1, 0, marker(2)))
        .expect("the second edit commits");

    assert!(edit.undo(&base).expect("undo runs"));
    assert_eq!(edit.history().redo_reach(), 1);

    edit.transact(&base, "Third", |tx| tx.put_object(1, 0, marker(3)))
        .expect("the third edit commits");

    assert_eq!(
        edit.history().redo_reach(),
        0,
        "choosing a different future drops the one that was undone"
    );
    assert!(!edit.redo(&base).expect("redo runs"), "nothing to redo");
}

#[test]
fn the_saved_mark_survives_undo_and_redo_and_moves_only_on_save() {
    let (base, mut edit) = session();
    assert!(
        edit.history().is_at_saved_mark(),
        "a fresh session is clean"
    );

    // A real verb rather than a marker dictionary: this test saves, and a
    // section whose /Root is not a catalog is one cos refuses to write.
    set_description(&mut edit, &base, "a description");
    assert!(edit.is_dirty(), "an edit dirties the document");

    assert!(edit.undo(&base).expect("undo runs"));
    assert!(
        edit.history().is_at_saved_mark(),
        "undoing back to the mark is clean again"
    );

    assert!(edit.redo(&base).expect("redo runs"));
    assert!(edit.is_dirty(), "redoing past the mark is dirty again");

    let saved = open(&append(
        &original_bytes(),
        section(&base, &edit).expect("the save appends"),
    ));
    edit.rebase(&saved);
    assert!(
        edit.history().is_at_saved_mark(),
        "only a save moves the mark"
    );
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

/// Two producers, one entry, one change per address, and the same shape in
/// either order. That is what makes the ordering rule a rule rather than a
/// description of today's code.
#[test]
fn two_producers_in_one_transaction_make_one_change_per_object() {
    for (first, second) in [(1, 2), (2, 1)] {
        let (base, mut edit) = session();
        let catalog_before = base.get(1).expect("the catalog parses").object;

        edit.transact(&base, "Two producers", |tx| {
            tx.put_object(1, 0, marker(first))?;
            tx.put_object(1, 0, marker(second))
        })
        .expect("the transaction commits");

        let changes = last_changes(&edit);
        assert_eq!(
            changes.len(),
            1,
            "two writes to one object coalesce into one change (order {first} then {second})"
        );
        match &changes[0] {
            Change::Object { before, after, .. } => {
                assert_eq!(
                    before.as_ref().map(|state| &state.object),
                    Some(&catalog_before),
                    "before is the state before the transaction, not before the second write"
                );
                assert_eq!(
                    after.as_ref().map(|state| &state.object),
                    Some(&marker(second)),
                    "after is the last write, whichever producer ran last"
                );
            }
            other => panic!("expected an object change, got {other:?}"),
        }
    }
}

/// An abort cannot leak an object number, and cannot leave half a change behind.
#[test]
fn an_aborted_transaction_leaves_the_overlay_and_the_counter_unchanged() {
    let (base, mut edit) = session();
    edit.transact(&base, "First", |tx| tx.put_object(1, 0, marker(1)))
        .expect("the first edit commits");
    let before = edit.overlay().clone();

    let outcome: Result<(), _> = edit.transact(&base, "Aborted", |tx| {
        tx.put_object(1, 0, marker(9))?;
        let reserved = tx.reserve();
        tx.put_object(reserved, 0, marker(10))?;
        Err(onionskin_core::Error::NoCatalog)
    });
    assert!(outcome.is_err(), "the transaction aborted");

    assert_eq!(
        edit.overlay(),
        &before,
        "an aborted transaction leaves the overlay untouched, including reserved numbers"
    );
    assert_eq!(
        edit.history().reach(),
        1,
        "an aborted transaction records no entry"
    );
}

#[test]
fn closure_errors_preserve_an_applied_overlay_and_its_redo_tail() {
    let original = original_bytes();
    let (base, mut edit) = session();
    set_description(&mut edit, &base, "A");
    set_description(&mut edit, &base, "B");
    let b_overlay = edit.overlay().clone();
    let b_section = section(&base, &edit).expect("B has a section");

    assert!(edit.undo(&base).expect("undo B"));
    assert_eq!(edit.history().reach(), 1);
    assert_eq!(edit.history().redo_reach(), 1);
    let info = edit
        .trailer_edits()
        .get(&onionskin_cos::Name::new("Info"))
        .and_then(Option::as_ref)
        .and_then(Object::as_reference)
        .expect("A names Info");
    assert_eq!(
        edit.overlay()
            .object(info.number)
            .and_then(|state| state.object.as_dict())
            .and_then(|dict| dict.get(b"Description")),
        Some(&string("A"))
    );
    let a_section = section(&base, &edit).expect("A has a section");
    let projected_a = open(&append(&original, a_section));
    let info = projected_a
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("A names Info in the projection");
    assert_eq!(
        projected_a
            .get(info.number)
            .expect("A Info object")
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Description")),
        Some(&string("A"))
    );

    let before_overlay = edit.overlay().clone();
    let before_epoch = edit.epoch();
    let before_section = section(&base, &edit);
    let before_history = edit.history().clone();
    let before_trailer = edit.trailer_edits();
    let before_dirty = edit.is_dirty();
    let outcome: Result<(), Error> = edit.transact(&base, "Aborted", |tx| {
        let info = tx
            .trailer_value(b"Info")
            .and_then(|value| value.as_reference())
            .expect("Info remains reachable");
        let state = tx.object(info.number)?.expect("the Info object");
        let mut dict = state.object.as_dict().cloned().expect("Info dictionary");
        dict.set(onionskin_cos::Name::new("Description"), string("discarded"));
        tx.put_object(info.number, state.generation, Object::Dict(dict))?;
        let reserved = tx.reserve();
        tx.put_object(reserved, 0, marker(10))?;
        Err(Error::Io(std::io::Error::other("closure failed")))
    });
    match outcome {
        Err(Error::Io(error)) => assert_eq!(error.to_string(), "closure failed"),
        other => panic!("expected the closure error, got {other:?}"),
    }

    assert_eq!(edit.overlay(), &before_overlay);
    assert_eq!(edit.epoch(), before_epoch);
    assert_eq!(section(&base, &edit), before_section);
    assert_eq!(edit.trailer_edits(), before_trailer);
    assert_eq!(edit.is_dirty(), before_dirty);
    let history = edit.history();
    assert_eq!(history.reach(), before_history.reach());
    assert_eq!(history.redo_reach(), before_history.redo_reach());
    assert_eq!(history.can_undo(), before_history.can_undo());
    assert_eq!(history.can_redo(), before_history.can_redo());
    assert_eq!(history.resident_bytes(), before_history.resident_bytes());
    assert_eq!(history.forgotten(), before_history.forgotten());
    assert_eq!(
        history.forgot_saved_mark(),
        before_history.forgot_saved_mark()
    );
    assert_eq!(
        history.is_at_saved_mark(),
        before_history.is_at_saved_mark()
    );
    assert_eq!(history.undo_label(), before_history.undo_label());
    assert_eq!(history.redo_label(), before_history.redo_label());
    for index in 0..history.reach() + history.redo_reach() {
        assert_eq!(history.entry(index), before_history.entry(index));
    }

    assert!(edit.redo(&base).expect("redo B"));
    assert_eq!(edit.overlay(), &b_overlay);
    assert_eq!(section(&base, &edit), Some(b_section.clone()));
    assert_eq!(edit.history().reach(), 2);
    assert_eq!(edit.history().redo_reach(), 0);
    let reopened = open(&append(&original, b_section));
    let info = reopened
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("B names Info");
    assert_eq!(
        reopened
            .get(info.number)
            .expect("B Info object")
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Description")),
        Some(&string("B"))
    );
    assert!((0..edit.history().reach())
        .all(|index| { edit.history().entry(index).expect("entry").label() != "Aborted" }));
}

#[test]
fn closure_errors_preserve_document_projection_and_redo_through_save() {
    let original = original_bytes();
    let mut document = Document::open_bytes(original.clone()).expect("opens");
    assert!(open(&original).trailer().get(b"Info").is_none());

    let b_ref = document
        .edit_document("Set B", |tx| {
            let number = tx.reserve();
            let mut dict = Dict::new();
            dict.set(Name::new("Title"), string("B"));
            tx.put_object(number, 0, Object::Dict(dict))?;
            tx.set_trailer(
                Name::new("Info"),
                Some(Object::Ref(onionskin_core::ObjRef::new(number, 0))),
            )?;
            Ok(onionskin_core::ObjRef::new(number, 0))
        })
        .expect("set B");
    let b_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("B preview")
        .to_vec();
    let b_overlay = document.edit().overlay().clone();
    let b_entry = document.edit().history().entry(0).expect("B entry").clone();
    assert!(document.undo().expect("undo B"));
    assert_eq!(document.edit().history().reach(), 0);
    assert_eq!(document.edit().history().redo_reach(), 1);
    assert!(document.edit().history().is_at_saved_mark());
    assert!(!document.is_dirty());
    assert!(document
        .structure()
        .expect("structure")
        .trailer()
        .get(b"Info")
        .is_none());
    assert_eq!(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("original preview")
            .as_ref(),
        original.as_slice()
    );

    let before_overlay = document.edit().overlay().clone();
    let before_history = document.edit().history().clone();
    let before_epoch = document.edit().epoch();
    let before_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("primed preview")
        .to_vec();
    let before_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit)
    };
    assert!(before_section.is_none());
    let before_bytes = document.bytes().as_ref().clone();
    let before_trailer = document.edit().trailer_edits();
    let before_dirty = document.is_dirty();
    let outcome: Result<(), Error> = document.edit_document("Aborted", |tx| {
        let root = tx
            .trailer_value(b"Root")
            .and_then(|value| value.as_reference())
            .expect("Root");
        let state = tx.object(root.number)?.expect("catalog");
        let mut catalog = state.object.as_dict().cloned().expect("catalog dictionary");
        catalog.set(Name::new("PageMode"), Object::name("UseOutlines"));
        tx.put_object(root.number, state.generation, Object::Dict(catalog))?;
        let number = tx.reserve();
        let mut dict = Dict::new();
        dict.set(Name::new("Title"), string("discarded"));
        tx.put_object(number, 0, Object::Dict(dict))?;
        tx.set_trailer(
            Name::new("Info"),
            Some(Object::Ref(onionskin_core::ObjRef::new(number, 0))),
        )?;
        Err(Error::Io(std::io::Error::other("closure failed")))
    });
    match outcome {
        Err(Error::Io(error)) => assert_eq!(error.to_string(), "closure failed"),
        other => panic!("expected the closure error, got {other:?}"),
    }
    assert_eq!(document.edit().overlay(), &before_overlay);
    assert_eq!(document.edit().epoch(), before_epoch);
    assert_eq!(document.edit().trailer_edits(), before_trailer);
    assert_eq!(document.bytes().as_ref(), &before_bytes);
    assert_eq!(document.is_dirty(), before_dirty);
    let history = document.edit().history();
    assert_eq!(history.reach(), before_history.reach());
    assert_eq!(history.redo_reach(), before_history.redo_reach());
    assert_eq!(history.can_undo(), before_history.can_undo());
    assert_eq!(history.can_redo(), before_history.can_redo());
    assert_eq!(history.resident_bytes(), before_history.resident_bytes());
    assert_eq!(history.forgotten(), before_history.forgotten());
    assert_eq!(
        history.forgot_saved_mark(),
        before_history.forgot_saved_mark()
    );
    assert_eq!(
        history.is_at_saved_mark(),
        before_history.is_at_saved_mark()
    );
    assert_eq!(history.undo_label(), before_history.undo_label());
    assert_eq!(history.redo_label(), before_history.redo_label());
    for index in 0..history.reach() + history.redo_reach() {
        assert_eq!(history.entry(index), before_history.entry(index));
    }
    assert_eq!(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("preview after abort")
            .as_ref(),
        before_preview.as_slice()
    );
    let fresh_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit)
    };
    assert!(fresh_section.is_none());
    let reopened = open(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("preview")
            .as_ref(),
    );
    assert!(reopened.trailer().get(b"Info").is_none());

    assert!(document.redo().expect("redo B"));
    assert_eq!(document.edit().history().reach(), 1);
    assert_eq!(document.edit().history().redo_reach(), 0);
    assert_eq!(document.edit().history().undo_label(), Some("Set B"));
    assert_eq!(document.edit().overlay(), &b_overlay);
    assert_eq!(document.edit().history().entry(0), Some(&b_entry));
    assert_eq!(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("B preview after redo")
            .as_ref(),
        b_preview.as_slice()
    );
    assert_eq!(
        document
            .info()
            .expect("B info")
            .description
            .title
            .as_deref(),
        Some("B")
    );
    let reopened = open(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("preview")
            .as_ref(),
    );
    assert_eq!(
        reopened
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(b_ref)
    );
    assert_eq!(document.bytes().as_ref(), &before_bytes);

    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("redo.pdf");
    let mut file = DocumentFile::from_document(document);
    file.save_as(&path).expect("save");
    let mut saved = Document::open_path(&path).expect("reopen saved");
    assert_eq!(
        saved
            .info()
            .expect("saved info")
            .description
            .title
            .as_deref(),
        Some("B")
    );
    assert_eq!(
        saved
            .structure()
            .expect("saved structure")
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(b_ref)
    );
    let saved_bytes = std::fs::read(&path).expect("saved bytes");
    let strict_saved = open(&saved_bytes);
    assert_eq!(
        strict_saved
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(b_ref)
    );
    assert!(saved_bytes.starts_with(&original));
    assert!(file.edit().history().is_at_saved_mark());
    assert!(!file.is_dirty());
}

#[test]
fn closure_errors_restore_first_write_membership_and_cleared_trailers() {
    let original = original_bytes();
    let (seed_base, mut seed_edit) = session();
    seed_edit
        .transact(&seed_base, "Probe", |tx| {
            tx.set_trailer(Name::new("AbortProbe"), Some(Object::Integer(1)))
        })
        .expect("probe trailer");
    let local_base = open(&append(
        &original,
        section(&seed_base, &seed_edit).expect("probe section"),
    ));
    let mut edit = EditSession::for_base(&local_base);
    edit.transact(&local_base, "Setup", |tx| {
        tx.set_trailer(Name::new("AbortProbe"), None)?;
        let state = tx.object(1)?.expect("catalog");
        let mut catalog = state.object.as_dict().cloned().expect("catalog dictionary");
        catalog.set(Name::new("PageMode"), Object::name("UseOutlines"));
        tx.put_object(1, state.generation, Object::Dict(catalog))
    })
    .expect("setup");
    assert_eq!(
        edit.trailer_edits().get(&Name::new("AbortProbe")),
        Some(&None)
    );
    let before_overlay = edit.overlay().clone();
    let before_epoch = edit.epoch();
    let before_section = section(&local_base, &edit);
    let before_history = edit.history().clone();
    let before_trailer = edit.trailer_edits();
    let outcome: Result<(), Error> = edit.transact(&local_base, "Aborted", |tx| {
        for value in [1, 2] {
            let state = tx.object(1)?.expect("catalog");
            let mut catalog = state.object.as_dict().cloned().expect("catalog dictionary");
            catalog.set(Name::new("Probe"), Object::Integer(value));
            tx.put_object(1, state.generation, Object::Dict(catalog))?;
        }
        for value in [1, 2] {
            let state = tx.object(2)?.expect("page tree");
            let mut pages = state
                .object
                .as_dict()
                .cloned()
                .expect("page tree dictionary");
            pages.set(Name::new("Probe"), Object::Integer(value));
            tx.put_object(2, state.generation, Object::Dict(pages))?;
        }
        let number = tx.reserve();
        tx.put_object(number, 0, marker(1))?;
        tx.put_object(number, 0, marker(2))?;
        let info = tx.reserve();
        tx.put_object(info, 0, marker(3))?;
        tx.set_trailer(
            Name::new("Info"),
            Some(Object::Ref(onionskin_core::ObjRef::new(info, 0))),
        )?;
        tx.set_trailer(Name::new("Info"), None)?;
        tx.set_trailer(Name::new("AbortProbe"), Some(Object::Integer(2)))?;
        tx.set_trailer(Name::new("AbortProbe"), None)?;
        Err(Error::Io(std::io::Error::other("closure failed")))
    });
    match outcome {
        Err(Error::Io(error)) => assert_eq!(error.to_string(), "closure failed"),
        other => panic!("expected the closure error, got {other:?}"),
    }
    assert_eq!(edit.overlay(), &before_overlay);
    assert_eq!(edit.epoch(), before_epoch);
    assert_eq!(section(&local_base, &edit), before_section);
    assert_eq!(edit.trailer_edits(), before_trailer);
    assert!(!edit.trailer_edits().contains_key(&Name::new("Info")));
    assert_eq!(
        edit.trailer_edits().get(&Name::new("AbortProbe")),
        Some(&None)
    );
    let history = edit.history();
    assert_eq!(history.reach(), before_history.reach());
    assert_eq!(history.redo_reach(), before_history.redo_reach());
    assert_eq!(history.can_undo(), before_history.can_undo());
    assert_eq!(history.can_redo(), before_history.can_redo());
    assert_eq!(history.resident_bytes(), before_history.resident_bytes());
    assert_eq!(history.forgotten(), before_history.forgotten());
    assert_eq!(
        history.forgot_saved_mark(),
        before_history.forgot_saved_mark()
    );
    assert_eq!(
        history.is_at_saved_mark(),
        before_history.is_at_saved_mark()
    );
    assert_eq!(history.undo_label(), before_history.undo_label());
    assert_eq!(history.redo_label(), before_history.redo_label());
    for index in 0..history.reach() + history.redo_reach() {
        assert_eq!(history.entry(index), before_history.entry(index));
    }
}

#[test]
fn orphan_scan_errors_restore_the_overlay_and_redo_tail() {
    let bytes = common::pdf(&[
        b"<< /Type /Catalog /Broken 2 0 R >>".to_vec(),
        b"(".to_vec(),
    ]);
    let base = open(&bytes);
    let (offset, detail) = match base.get(2) {
        Err(onionskin_cos::Error::Syntax { offset, detail }) => (offset, detail),
        other => panic!("expected the malformed object to fail with Syntax, got {other:?}"),
    };
    let mut edit = EditSession::for_base(&base);
    let catalog = |tx: &mut onionskin_core::Transaction<'_>, value| {
        let state = tx.object(1)?.expect("catalog");
        let mut dict = state.object.as_dict().cloned().expect("catalog dictionary");
        dict.set(Name::new("Probe"), Object::Integer(value));
        tx.put_object(1, state.generation, Object::Dict(dict))
    };
    edit.transact(&base, "A", |tx| catalog(tx, 1)).expect("A");
    edit.transact(&base, "B", |tx| catalog(tx, 2)).expect("B");
    let b_overlay = edit.overlay().clone();
    let b_entry = edit.history().entry(1).expect("B entry").clone();
    assert!(edit.undo(&base).expect("undo B"));
    let before_overlay = edit.overlay().clone();
    let before_epoch = edit.epoch();
    let before_dirty = edit.is_dirty();
    let before_history = edit.history().clone();
    let mut body_completed = false;
    let outcome = edit.transact(&base, "Aborted", |tx| {
        catalog(tx, 3)?;
        let number = tx.reserve();
        tx.put_object(number, 0, marker(10))?;
        tx.set_trailer(
            Name::new("Info"),
            Some(Object::Ref(onionskin_core::ObjRef::new(number, 0))),
        )?;
        body_completed = true;
        Ok(())
    });
    match outcome {
        Err(Error::Cos(onionskin_cos::Error::Syntax {
            offset: actual_offset,
            detail: actual_detail,
        })) => {
            assert_eq!(actual_offset, offset);
            assert_eq!(actual_detail, detail);
        }
        other => panic!("expected orphan scan Syntax, got {other:?}"),
    }
    assert!(
        body_completed,
        "the closure completed before orphan scanning"
    );
    assert_eq!(edit.overlay(), &before_overlay);
    assert_eq!(edit.epoch(), before_epoch);
    assert_eq!(edit.is_dirty(), before_dirty);
    let history = edit.history();
    assert_eq!(history.reach(), before_history.reach());
    assert_eq!(history.redo_reach(), before_history.redo_reach());
    assert_eq!(history.can_undo(), before_history.can_undo());
    assert_eq!(history.can_redo(), before_history.can_redo());
    assert_eq!(history.resident_bytes(), before_history.resident_bytes());
    assert_eq!(history.forgotten(), before_history.forgotten());
    assert_eq!(
        history.forgot_saved_mark(),
        before_history.forgot_saved_mark()
    );
    assert_eq!(
        history.is_at_saved_mark(),
        before_history.is_at_saved_mark()
    );
    assert_eq!(history.undo_label(), before_history.undo_label());
    assert_eq!(history.redo_label(), before_history.redo_label());
    for index in 0..history.reach() + history.redo_reach() {
        assert_eq!(history.entry(index), before_history.entry(index));
    }
    assert!(edit.redo(&base).expect("redo B"));
    assert_eq!(edit.overlay(), &b_overlay);
    assert_eq!(edit.history().entry(1), Some(&b_entry));
}

fn literal_info_null_document() -> Vec<u8> {
    let mut bytes = common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>"
            .to_vec(),
        common::stream(""),
    ]);
    let old = b"trailer\n<< /Size 5 /Root 1 0 R >>\n";
    let new = b"trailer\n<< /Size 5 /Root 1 0 R /Info null >>\n";
    let matches: Vec<usize> = bytes
        .windows(old.len())
        .enumerate()
        .filter_map(|(index, window)| (window == old).then_some(index))
        .collect();
    assert_eq!(matches.len(), 1);
    bytes.splice(matches[0]..matches[0] + old.len(), new.iter().copied());
    assert!(bytes
        .windows(b"/Info null".len())
        .any(|window| window == b"/Info null"));
    bytes
}

#[test]
fn literal_info_null_preserves_original_bytes_through_undo_redo() {
    let original = literal_info_null_document();
    let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
    let strict = open(&original);
    assert!(strict.provenance().is_clean());
    assert!(strict.trailer().get(b"Info").is_none());
    assert_eq!(document.page_count(), 1);
    assert_eq!(document.bytes().as_ref(), &original);
    assert!(document.edit().overlay().is_empty());
    assert_eq!(document.edit().history().reach(), 0);
    assert_eq!(document.edit().history().redo_reach(), 0);
    assert!(document.edit().history().is_at_saved_mark());
    let (edit, base) = document.edit_mut();
    assert!(section(base, edit).is_none());
    let baseline_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("baseline preview")
        .to_vec();
    assert_eq!(baseline_preview, original);
    assert_eq!(
        document.info().expect("baseline info").description,
        Default::default()
    );
    assert!(document.xmp().expect("baseline xmp").is_none());

    let properties = onionskin_core::metadata::PropertiesEdit {
        description: onionskin_core::metadata::Description {
            title: Some("created".into()),
            subject: Some("grouped".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    document
        .edit_document("Document Properties", |tx| {
            onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
        })
        .expect("properties commit");
    assert_eq!(document.edit().history().reach(), 1);
    assert_eq!(document.edit().history().redo_reach(), 0);
    let edited_ref = document
        .structure()
        .expect("edited structure")
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("edited Info reference");
    let edited_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit).expect("edited section")
    };
    let edited_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("edited preview")
        .to_vec();
    assert_eq!(edited_preview, append(&original, edited_section.clone()));
    assert_eq!(document.bytes().as_ref(), &original);
    let reopened = open(&edited_preview);
    assert_eq!(
        reopened
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(edited_ref)
    );
    let info = reopened.get(edited_ref.number).expect("edited Info object");
    assert_eq!(
        info.object.as_dict().and_then(|dict| dict.get(b"Title")),
        Some(&string("created"))
    );
    assert_eq!(
        info.object.as_dict().and_then(|dict| dict.get(b"Subject")),
        Some(&string("grouped"))
    );
    assert_eq!(
        document.info().expect("edited info").description,
        properties.description
    );
    let edited_xmp = document.xmp().expect("edited xmp").expect("edited XMP");
    assert_eq!(edited_xmp.title.as_deref(), Some("created"));
    assert_eq!(edited_xmp.subject.as_deref(), Some("grouped"));
    let edited_overlay = document.edit().overlay().clone();

    assert!(document.undo().expect("undo"));
    assert_eq!(document.edit().history().reach(), 0);
    assert_eq!(document.edit().history().redo_reach(), 1);
    assert!(document.edit().history().is_at_saved_mark());
    assert!(document.edit().overlay().is_empty());
    assert!(document.edit().trailer_edits().is_empty());
    let undone_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit)
    };
    assert!(undone_section.is_none());
    let undone_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("undone preview")
        .to_vec();
    assert_eq!(undone_preview, baseline_preview);
    assert_eq!(document.bytes().as_ref(), &original);
    let undone = open(&undone_preview);
    assert!(undone.trailer().get(b"Info").is_none());
    assert!(undone.xref().get(edited_ref.number).is_none());
    assert_eq!(
        document.info().expect("undone info").description,
        Default::default()
    );
    assert!(document.xmp().expect("undone xmp").is_none());

    assert!(document.redo().expect("redo"));
    assert_eq!(document.edit().history().reach(), 1);
    assert_eq!(document.edit().history().redo_reach(), 0);
    assert_eq!(document.edit().overlay(), &edited_overlay);
    let redone_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit).expect("redone section")
    };
    assert_eq!(redone_section, edited_section);
    let redone_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("redone preview")
        .to_vec();
    assert_eq!(redone_preview, edited_preview);
    assert_eq!(
        open(&redone_preview)
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(edited_ref)
    );
    let redone_info = open(&redone_preview)
        .get(edited_ref.number)
        .expect("redone Info object");
    assert_eq!(
        redone_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Title")),
        Some(&string("created"))
    );
    assert_eq!(
        redone_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Subject")),
        Some(&string("grouped"))
    );
    assert_eq!(
        document.info().expect("redone info").description,
        properties.description
    );
    let redone_xmp = document.xmp().expect("redone xmp").expect("redone XMP");
    assert_eq!(redone_xmp.title.as_deref(), Some("created"));
    assert_eq!(redone_xmp.subject.as_deref(), Some("grouped"));

    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("literal-null-info.pdf");
    let mut file = DocumentFile::from_document(document);
    let expected_start = {
        let (_, base) = file.edit_mut();
        base.next_section_start().expect("section start")
    };
    let outcome = file.save_as(&path).expect("save");
    assert_eq!(outcome.sections_appended, 1);
    assert!(outcome.saved_as);
    let saved_bytes = std::fs::read(&path).expect("saved bytes");
    assert!(saved_bytes.starts_with(&original));
    let redone = open(&redone_preview);
    let saved = open(&saved_bytes);
    assert!(saved.provenance().is_clean());
    let redone_numbers: Vec<u32> = redone.xref().iter().map(|(number, _)| number).collect();
    let saved_numbers: Vec<u32> = saved.xref().iter().map(|(number, _)| number).collect();
    assert_eq!(saved_numbers, redone_numbers);
    for number in redone_numbers {
        if number == 0 {
            continue;
        }
        let expected = redone.get(number).expect("redone object");
        let actual = saved.get(number).expect("saved object");
        assert_eq!(actual.objref, expected.objref);
        assert_eq!(actual.object, expected.object);
    }
    let expected_trailer = redone.trailer().clone();
    assert!(!expected_trailer.contains(b"OnionskinSection"));
    let mut saved_trailer = saved.trailer().clone();
    let stamp = saved_trailer
        .remove(b"OnionskinSection")
        .expect("save stamp");
    assert_eq!(saved_trailer, expected_trailer);
    let stamp = stamp.as_dict().expect("save stamp dictionary");
    assert_eq!(
        stamp.get(b"Start"),
        Some(&Object::Integer(expected_start as i64))
    );
    assert_eq!(
        stamp.get(b"Producer"),
        Some(&string(&format!("Onionskin {}", env!("CARGO_PKG_VERSION"))))
    );
    let Some(Object::String(date)) = stamp.get(b"Date") else {
        panic!("save date")
    };
    assert!(date.starts_with(b"D:"));
    assert_eq!(
        saved.trailer().get(b"Info").and_then(Object::as_reference),
        Some(edited_ref)
    );
    let saved_info = saved.get(edited_ref.number).expect("saved Info object");
    assert_eq!(
        saved_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Title")),
        Some(&string("created"))
    );
    assert_eq!(
        saved_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Subject")),
        Some(&string("grouped"))
    );
    let mut reopened_file = Document::open_path(&path).expect("path reopen");
    assert_eq!(
        reopened_file.info().expect("saved info").description,
        properties.description
    );
    let saved_xmp = reopened_file.xmp().expect("saved xmp").expect("saved XMP");
    assert_eq!(saved_xmp.title.as_deref(), Some("created"));
    assert_eq!(saved_xmp.subject.as_deref(), Some("grouped"));
    assert!(file.edit().history().is_at_saved_mark());
    assert!(!file.is_dirty());
}

#[test]
fn scan_repaired_info_null_survives_undo_and_recreates_on_redo() {
    let mut original = literal_info_null_document();
    let matches: Vec<usize> = original
        .windows(b"startxref".len())
        .enumerate()
        .filter_map(|(index, window)| (window == b"startxref").then_some(index))
        .collect();
    assert_eq!(matches.len(), 1);
    original[matches[0]..matches[0] + b"startxref".len()].copy_from_slice(b"badxref!!");
    assert!(original
        .windows(b"/Info null".len())
        .any(|window| window == b"/Info null"));
    let repaired =
        onionskin_cos::Document::open_repairing(Box::new(BytesSource::new(original.clone())))
            .expect("repairing fixture opens");
    let report = repaired.1.report().expect("repair report");
    assert!(report.rebuilt_by_scan);
    assert!(report
        .reasons
        .iter()
        .any(|reason| matches!(reason, onionskin_cos::RepairReason::MissingStartxref)));
    assert_eq!(repaired.0.trailer().get(b"Info"), Some(&Object::Null));
    assert!(repaired.0.xref().get(5).is_none());
    let mut document = Document::open_bytes(original.clone()).expect("core fixture opens");
    assert_eq!(document.page_count(), 1);
    let baseline_section = {
        let (edit, base) = document.edit_mut();
        let report = base.provenance().report().expect("core repair report");
        assert!(report.rebuilt_by_scan);
        assert!(report
            .reasons
            .iter()
            .any(|reason| matches!(reason, onionskin_cos::RepairReason::MissingStartxref)));
        assert_eq!(base.trailer().get(b"Info"), Some(&Object::Null));
        assert!(base.xref().get(5).is_none());
        section(base, edit).expect("repair section")
    };
    assert!(document.edit().overlay().is_empty());
    assert_eq!(document.edit().history().reach(), 0);
    assert_eq!(document.edit().history().redo_reach(), 0);
    assert!(document.edit().history().is_at_saved_mark());
    assert_eq!(
        document.info().expect("baseline info").description,
        Default::default()
    );
    assert!(document.xmp().expect("baseline xmp").is_none());
    let baseline_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("repair preview")
        .to_vec();
    assert_eq!(
        baseline_preview,
        append(&original, baseline_section.clone())
    );
    let baseline_reopened = open(&baseline_preview);
    assert!(baseline_reopened.provenance().is_clean());
    assert!(baseline_reopened.trailer().get(b"Info").is_none());
    assert!(baseline_reopened.xref().get(5).is_none());
    assert_eq!(document.bytes().as_ref(), &original);

    let properties = onionskin_core::metadata::PropertiesEdit {
        description: onionskin_core::metadata::Description {
            title: Some("created".into()),
            subject: Some("grouped".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    document
        .edit_document("Document Properties", |tx| {
            onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
        })
        .expect("properties commit");
    let edited_ref = document
        .structure()
        .expect("edited structure")
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("edited Info reference");
    let edited_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit).expect("edited section")
    };
    let edited_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("edited preview")
        .to_vec();
    assert_eq!(edited_preview, append(&original, edited_section.clone()));
    assert_eq!(document.bytes().as_ref(), &original);
    assert_eq!(document.edit().history().reach(), 1);
    assert_eq!(document.edit().history().redo_reach(), 0);
    let edited_reopened = open(&edited_preview);
    assert_eq!(
        edited_reopened
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(edited_ref)
    );
    let edited_info = edited_reopened
        .get(edited_ref.number)
        .expect("edited Info object");
    assert_eq!(
        edited_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Title")),
        Some(&string("created"))
    );
    assert_eq!(
        edited_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Subject")),
        Some(&string("grouped"))
    );
    let edited_overlay = document.edit().overlay().clone();
    assert_eq!(
        document.info().expect("edited info").description,
        properties.description
    );
    let edited_xmp = document.xmp().expect("edited xmp").expect("edited XMP");
    assert_eq!(edited_xmp.title.as_deref(), Some("created"));
    assert_eq!(edited_xmp.subject.as_deref(), Some("grouped"));

    assert!(document.undo().expect("undo"));
    assert_eq!(document.edit().history().reach(), 0);
    assert_eq!(document.edit().history().redo_reach(), 1);
    assert!(document.edit().history().is_at_saved_mark());
    let undone_section = {
        let (edit, base) = document.edit_mut();
        Some(section(base, edit).expect("repair section after undo"))
    };
    assert_eq!(undone_section, Some(baseline_section.clone()));
    let undone_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("undone preview")
        .to_vec();
    assert_eq!(undone_preview, baseline_preview);
    let undone = open(&undone_preview);
    assert!(undone.trailer().get(b"Info").is_none());
    assert!(undone.xref().get(edited_ref.number).is_none());
    assert_eq!(document.bytes().as_ref(), &original);
    let (edit, base) = document.edit_mut();
    assert_eq!(base.trailer().get(b"Info"), Some(&Object::Null));
    assert!(section(base, edit).is_some());
    assert!(document.edit().overlay().is_empty());
    assert!(document.edit().trailer_edits().is_empty());
    assert_eq!(
        document.info().expect("undone info").description,
        Default::default()
    );
    assert!(document.xmp().expect("undone xmp").is_none());

    assert!(document.redo().expect("redo"));
    assert_eq!(document.edit().history().reach(), 1);
    assert_eq!(document.edit().history().redo_reach(), 0);
    assert_eq!(document.edit().overlay(), &edited_overlay);
    let redone_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit).expect("redone section")
    };
    assert_eq!(redone_section, edited_section);
    let redone_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("redone preview")
        .to_vec();
    assert_eq!(redone_preview, edited_preview);
    assert_eq!(
        open(&redone_preview)
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(edited_ref)
    );
    let redone_info = open(&redone_preview)
        .get(edited_ref.number)
        .expect("redone Info object");
    assert_eq!(
        redone_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Title")),
        Some(&string("created"))
    );
    assert_eq!(
        redone_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Subject")),
        Some(&string("grouped"))
    );
    assert_eq!(
        document.info().expect("redone info").description,
        properties.description
    );
    let redone_xmp = document.xmp().expect("redone xmp").expect("redone XMP");
    assert_eq!(redone_xmp.title.as_deref(), Some("created"));
    assert_eq!(redone_xmp.subject.as_deref(), Some("grouped"));

    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("repaired-null-info.pdf");
    let mut file = DocumentFile::from_document(document);
    let expected_start = {
        let (_, base) = file.edit_mut();
        base.next_section_start().expect("section start")
    };
    let outcome = file.save_as(&path).expect("save");
    assert_eq!(outcome.sections_appended, 1);
    assert!(outcome.saved_as);
    let saved_bytes = std::fs::read(&path).expect("saved bytes");
    assert!(saved_bytes.starts_with(&original));
    let redone = open(&redone_preview);
    let saved = open(&saved_bytes);
    assert!(saved.provenance().is_clean());
    let redone_numbers: Vec<u32> = redone.xref().iter().map(|(number, _)| number).collect();
    let saved_numbers: Vec<u32> = saved.xref().iter().map(|(number, _)| number).collect();
    assert_eq!(saved_numbers, redone_numbers);
    for number in redone_numbers {
        if number == 0 {
            continue;
        }
        let expected = redone.get(number).expect("redone object");
        let actual = saved.get(number).expect("saved object");
        assert_eq!(actual.objref, expected.objref);
        assert_eq!(actual.object, expected.object);
    }
    let expected_trailer = redone.trailer().clone();
    assert!(!expected_trailer.contains(b"OnionskinSection"));
    let mut saved_trailer = saved.trailer().clone();
    let stamp = saved_trailer
        .remove(b"OnionskinSection")
        .expect("save stamp");
    assert_eq!(saved_trailer, expected_trailer);
    let stamp = stamp.as_dict().expect("save stamp dictionary");
    assert_eq!(
        stamp.get(b"Start"),
        Some(&Object::Integer(expected_start as i64))
    );
    assert_eq!(
        stamp.get(b"Producer"),
        Some(&string(&format!("Onionskin {}", env!("CARGO_PKG_VERSION"))))
    );
    let Some(Object::String(date)) = stamp.get(b"Date") else {
        panic!("save date")
    };
    assert!(date.starts_with(b"D:"));
    assert_eq!(
        saved.trailer().get(b"Info").and_then(Object::as_reference),
        Some(edited_ref)
    );
    let saved_info = saved.get(edited_ref.number).expect("saved Info object");
    assert_eq!(
        saved_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Title")),
        Some(&string("created"))
    );
    assert_eq!(
        saved_info
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Subject")),
        Some(&string("grouped"))
    );
    let mut reopened_file = Document::open_path(&path).expect("path reopen");
    assert_eq!(
        reopened_file.info().expect("saved info").description,
        properties.description
    );
    let saved_xmp = reopened_file.xmp().expect("saved xmp").expect("saved XMP");
    assert_eq!(saved_xmp.title.as_deref(), Some("created"));
    assert_eq!(saved_xmp.subject.as_deref(), Some("grouped"));
    assert!(file.edit().history().is_at_saved_mark());
    assert!(!file.is_dirty());
}

// ---------------------------------------------------------------------------
// The trailer
// ---------------------------------------------------------------------------

/// Asserted on the trailer and not on the overlay. Setting a Description on a
/// document with no `/Info` is one object write **and** one trailer write, so
/// without `Change::TrailerKey` an undo drops the object and leaves the trailer
/// naming it.
#[test]
fn setting_a_description_then_undoing_leaves_the_trailer_as_it_was() {
    let (base, mut edit) = session();
    assert!(
        base.trailer().get(b"Info").is_none(),
        "the fixture has no /Info, which is the point of it"
    );

    set_description(&mut edit, &base, "a description");
    assert!(
        edit.trailer_edits().contains_key(&Name::new("Info")),
        "creating /Info writes the trailer"
    );

    assert!(edit.undo(&base).expect("undo runs"));

    assert!(
        edit.trailer_edits().is_empty(),
        "undo leaves no trailer edit against a base that never had the key"
    );
    // T3's headline claim, on the one path that falsifies it.
    assert!(
        section(&base, &edit).is_none(),
        "the following save writes nothing at all"
    );
}

/// The case `TrailerState::Cleared` exists for. After the first save the base
/// trailer **has** `/Info`, so undo has to emit a cleared key rather than
/// forget an overlay entry. An assertion on the overlay alone passes while the
/// Description is still in the file, so both halves are asserted on the
/// reopened document.
#[test]
fn a_description_set_saved_undone_and_saved_again_leaves_the_file_without_one() {
    let original = original_bytes();
    let base = base();
    let mut edit = EditSession::for_base(&base);

    set_description(&mut edit, &base, "a description");

    let once_bytes = append(
        &original,
        section(&base, &edit).expect("the first save appends"),
    );
    let saved_once = open(&once_bytes);
    edit.rebase(&saved_once);

    let info = saved_once
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("the first save names an /Info");
    assert_eq!(
        saved_once
            .get(info.number)
            .expect("the info object parses")
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Description"))
            .cloned(),
        Some(string("a description")),
        "the first save really wrote the description"
    );

    assert!(edit.undo(&saved_once).expect("undo runs"));
    assert!(
        !edit.trailer_edits().is_empty(),
        "undoing across a save emits a cleared key rather than forgetting an overlay entry"
    );

    let twice_bytes = append(
        &once_bytes,
        section(&saved_once, &edit).expect("the second save appends"),
    );
    let saved_twice = open(&twice_bytes);

    let reopened = saved_twice.trailer().get(b"Info");
    assert!(
        matches!(reopened, None | Some(Object::Null)),
        "the reopened trailer no longer names an /Info, got {reopened:?}"
    );
}

// ---------------------------------------------------------------------------
// Verbs
// ---------------------------------------------------------------------------

#[test]
fn setting_a_catalog_entry_rewrites_the_catalog_and_nothing_else() {
    let (base, mut edit) = session();

    edit.apply(
        &base,
        DocumentEdit::SetCatalogEntry {
            key: Name::new("PageLayout"),
            value: Some(Object::name("TwoPageLeft")),
        },
    )
    .expect("the catalog entry is set");

    let pending = edit.pending_edits();
    assert_eq!(
        pending.keys().copied().collect::<Vec<_>>(),
        vec![1],
        "only the catalog is rewritten"
    );
    assert!(
        edit.trailer_edits().is_empty(),
        "a catalog entry is not a trailer key"
    );
    match pending.get(&1) {
        Some(PendingEdit::Set { object, .. }) => {
            assert_eq!(
                object.as_dict().and_then(|dict| dict.get(b"PageLayout")),
                Some(&Object::name("TwoPageLeft"))
            );
            assert!(
                object
                    .as_dict()
                    .and_then(|dict| dict.get(b"Pages"))
                    .is_some(),
                "the rest of the catalog survives the edit"
            );
        }
        other => panic!("expected a Set, got {other:?}"),
    }
}

/// Clearing a field removes the key rather than writing an empty string.
#[test]
fn clearing_an_info_field_removes_the_key() {
    let (base, mut edit) = session();
    set_description(&mut edit, &base, "a description");

    edit.apply(
        &base,
        DocumentEdit::SetInfoField {
            key: Name::new("Description"),
            value: None,
        },
    )
    .expect("the description is cleared");

    let info_number = edit
        .trailer_edits()
        .get(&Name::new("Info"))
        .and_then(|value| value.as_ref())
        .and_then(Object::as_reference)
        .map(|objref| objref.number)
        .expect("the overlay names an /Info");
    let state = overlay_object(&edit, info_number).expect("the info object is in the overlay");
    assert!(
        state
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Description"))
            .is_none(),
        "clearing removes the key"
    );
}

/// The projection onto the section writer is one-to-one and only ever `Set`,
/// because M3 frees no object number.
#[test]
fn the_projection_onto_pending_edits_only_ever_sets() {
    let (base, mut edit) = session();
    set_description(&mut edit, &base, "a description");

    let pending: BTreeMap<u32, PendingEdit> = edit.pending_edits();
    assert!(!pending.is_empty());
    for edit in pending.values() {
        assert!(
            matches!(edit, PendingEdit::Set { .. }),
            "nothing here can produce a Delete"
        );
    }
}

// ---------------------------------------------------------------------------
// The bound
// ---------------------------------------------------------------------------

/// A session past its bound has dropped its oldest entries, reports a shorter
/// reach, and says that it dropped the saved mark rather than appearing clean.
#[test]
fn a_history_past_its_bound_drops_oldest_and_says_so() {
    let base = base();
    // Small enough that a handful of marker dictionaries exceeds it.
    let mut edit = EditSession::with_history_bound(&base, 512);

    for n in 0..64 {
        edit.transact(&base, "Marker", |tx| tx.put_object(1, 0, marker(n)))
            .expect("the edit commits");
    }

    let history = edit.history();
    assert!(history.forgotten() > 0, "eviction happened");
    assert!(
        history.reach() < 64,
        "the reported reach is the retained count, not the count ever recorded"
    );
    assert!(
        history.resident_bytes() <= 512 || history.reach() == 1,
        "the stack fits its bound, or holds only the entry it may not evict"
    );
    assert!(
        history.forgot_saved_mark(),
        "a session that can no longer reach its last save says so"
    );
    assert!(
        !history.is_at_saved_mark(),
        "an evicted mark never reports clean"
    );
}

#[test]
fn non_dictionary_info_targets_are_refused_without_mutation() {
    let ordinary = b"<< /Title (old) /Subject (before) >>".to_vec();
    let cases = [
        (
            "direct-string",
            "(wrong shape)",
            ordinary.clone(),
            string("wrong shape"),
            None,
        ),
        (
            "direct-integer",
            "42",
            ordinary.clone(),
            Object::Integer(42),
            None,
        ),
        (
            "direct-name",
            "/Wrong",
            ordinary.clone(),
            Object::name("Wrong"),
            None,
        ),
        (
            "direct-array",
            "[]",
            ordinary.clone(),
            Object::Array(Vec::new()),
            None,
        ),
        (
            "indirect-integer",
            "4 0 R",
            b"42".to_vec(),
            Object::Ref(ObjRef::new(4, 0)),
            Some(ObjRef::new(4, 0)),
        ),
        (
            "indirect-stream",
            "4 0 R",
            common::stream("opaque metadata target"),
            Object::Ref(ObjRef::new(4, 0)),
            Some(ObjRef::new(4, 0)),
        ),
    ];

    for (fixture, info, target, expected_info, expected_ref) in cases {
        for properties_route in [false, true] {
            let route = if properties_route {
                "properties"
            } else {
                "generic"
            };
            let original = info_target_document(info, target.clone());
            let original_cos = open(&original);
            assert_eq!(
                original_cos.page_count().expect("one page"),
                1,
                "{fixture} {route} pages"
            );
            assert_info_fixture(
                &original_cos,
                info,
                &expected_info,
                expected_ref,
                expected_ref.map(|target| target.generation),
                expected_ref,
            );
            // The `free` case deliberately has no object 4, so what is compared
            // is each object's state including its absence.
            let original_objects: Vec<_> = (1..=4)
                .map(|number| original_cos.get(number).expect("fixture object").object)
                .collect();
            let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
            let before_preview = document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("primed preview")
                .to_vec();
            let before_overlay = document.edit().overlay().clone();
            let before_epoch = document.edit().epoch();
            let before_history = document.edit().history().clone();
            let before_trailer = document.edit().trailer_edits();
            let before_dirty = document.is_dirty();
            let before_bytes = document.bytes().as_ref().clone();
            let before_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            let properties = onionskin_core::metadata::PropertiesEdit {
                description: onionskin_core::metadata::Description {
                    title: Some("accepted".to_owned()),
                    subject: Some("grouped".to_owned()),
                    ..Default::default()
                },
                ..Default::default()
            };
            let outcome: Result<(), Error> = if properties_route {
                document.edit_document("Document Properties", |tx| {
                    onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
                })
            } else {
                let (edit, base) = document.edit_mut();
                edit.apply(
                    base,
                    DocumentEdit::SetInfoField {
                        key: Name::new("Title"),
                        value: Some(string("accepted")),
                    },
                )
            };
            if fixture == "indirect-stream" && !properties_route {
                assert!(
                    matches!(outcome, Err(Error::NotADictionary { number: 4 })),
                    "{fixture} {route} must reject the stream target directly: {outcome:?}"
                );
            } else {
                assert!(
                    outcome.is_err(),
                    "{fixture} {route} must refuse during the edit: {outcome:?}"
                );
            }

            assert_eq!(
                document.edit().overlay(),
                &before_overlay,
                "{fixture} {route}"
            );
            assert_eq!(document.edit().epoch(), before_epoch, "{fixture} {route}");
            assert_eq!(
                document.edit().trailer_edits(),
                before_trailer,
                "{fixture} {route}"
            );
            assert_eq!(
                document.bytes().as_ref(),
                &before_bytes,
                "{fixture} {route}"
            );
            assert_eq!(document.is_dirty(), before_dirty, "{fixture} {route}");
            assert_history_unchanged(&before_history, document.edit().history(), fixture, route);
            let fresh_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            assert_eq!(fresh_section, before_section, "{fixture} {route}");
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("preview after refusal")
                    .as_ref(),
                before_preview.as_slice(),
                "{fixture} {route} preview"
            );
            let reopened = open(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("fresh preview")
                    .as_ref(),
            );
            assert_eq!(
                reopened.trailer().get(b"Info"),
                original_cos.trailer().get(b"Info"),
                "{fixture} {route} Info trailer"
            );
            for (index, expected) in original_objects.iter().enumerate() {
                assert_eq!(
                    &reopened
                        .get((index + 1) as u32)
                        .expect("reopened object")
                        .object,
                    expected,
                    "{fixture} {route} object {}",
                    index + 1
                );
            }
        }
    }
}

#[test]
fn structural_info_aliases_are_refused_without_mutation() {
    let ordinary = b"<< /Title (old) /Subject (before) >>".to_vec();
    let cases = [
        ("root-alias", "1 0 R", ObjRef::new(1, 0), 0),
        ("pages-alias", "2 0 R", ObjRef::new(2, 0), 0),
        ("page-alias", "3 0 R", ObjRef::new(3, 0), 0),
        ("root-generation-mismatch", "1 7 R", ObjRef::new(1, 7), 0),
    ];

    for (fixture, info, expected_ref, target_generation) in cases {
        for properties_route in [false, true] {
            let route = if properties_route {
                "properties"
            } else {
                "generic"
            };
            let original = info_target_document(info, ordinary.clone());
            let original_cos = open(&original);
            assert_eq!(
                original_cos.page_count().expect("one page"),
                1,
                "{fixture} {route} pages"
            );
            assert_info_fixture(
                &original_cos,
                info,
                &Object::Ref(expected_ref),
                Some(expected_ref),
                Some(target_generation),
                Some(ObjRef::new(expected_ref.number, target_generation)),
            );
            let original_objects: Vec<_> = (1..=4)
                .map(|number| original_cos.get(number).expect("fixture object").object)
                .collect();
            let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
            let before_preview = document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("primed preview")
                .to_vec();
            let before_overlay = document.edit().overlay().clone();
            let before_epoch = document.edit().epoch();
            let before_history = document.edit().history().clone();
            let before_trailer = document.edit().trailer_edits();
            let before_dirty = document.is_dirty();
            let before_bytes = document.bytes().as_ref().clone();
            let before_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            let properties = onionskin_core::metadata::PropertiesEdit {
                description: onionskin_core::metadata::Description {
                    title: Some("accepted".to_owned()),
                    subject: Some("grouped".to_owned()),
                    ..Default::default()
                },
                ..Default::default()
            };
            let outcome: Result<(), Error> = if properties_route {
                document.edit_document("Document Properties", |tx| {
                    onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
                })
            } else {
                let (edit, base) = document.edit_mut();
                edit.apply(
                    base,
                    DocumentEdit::SetInfoField {
                        key: Name::new("Title"),
                        value: Some(string("accepted")),
                    },
                )
            };
            assert!(
                outcome.is_err(),
                "{fixture} {route} must refuse the structural alias during the edit: {outcome:?}"
            );
            // The contract refuses a structural alias "even if the selected
            // generation differs", so a structural cause is reported ahead of a
            // generation mismatch. The one fixture where both apply pins that
            // order: reordering the checks and never breaking `is_err` would
            // still violate the contract.
            if expected_ref.generation != target_generation {
                assert!(
                    matches!(outcome, Err(Error::StructuralInfoTarget { .. })),
                    "{fixture} {route} must report the structural alias, not the generation: \
                     {outcome:?}"
                );
            }

            assert_eq!(
                document.edit().overlay(),
                &before_overlay,
                "{fixture} {route}"
            );
            assert_eq!(document.edit().epoch(), before_epoch, "{fixture} {route}");
            assert_eq!(
                document.edit().trailer_edits(),
                before_trailer,
                "{fixture} {route}"
            );
            assert_eq!(
                document.bytes().as_ref(),
                &before_bytes,
                "{fixture} {route}"
            );
            assert_eq!(document.is_dirty(), before_dirty, "{fixture} {route}");
            assert_history_unchanged(&before_history, document.edit().history(), fixture, route);
            let fresh_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            assert_eq!(fresh_section, before_section, "{fixture} {route}");
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("preview after refusal")
                    .as_ref(),
                before_preview.as_slice(),
                "{fixture} {route} preview"
            );
            let reopened = open(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("fresh preview")
                    .as_ref(),
            );
            assert_eq!(
                reopened.trailer().get(b"Info"),
                original_cos.trailer().get(b"Info"),
                "{fixture} {route} Info trailer"
            );
            for (index, expected) in original_objects.iter().enumerate() {
                assert_eq!(
                    &reopened
                        .get((index + 1) as u32)
                        .expect("reopened object")
                        .object,
                    expected,
                    "{fixture} {route} object {}",
                    index + 1
                );
            }
        }
    }
}

/// Initial view first on purpose: the Properties dialog writes properties
/// first, so this reversed order probes transaction rollback, not the dialog's
/// sequence.
#[test]
fn properties_refusal_rolls_back_an_earlier_catalog_write() {
    let original = info_target_document("1 0 R", b"<< /Title (old) >>".to_vec());
    let original_cos = open(&original);
    assert_info_fixture(
        &original_cos,
        "1 0 R",
        &Object::Ref(ObjRef::new(1, 0)),
        Some(ObjRef::new(1, 0)),
        Some(0),
        Some(ObjRef::new(1, 0)),
    );
    let original_objects: Vec<_> = (1..=4)
        .map(|number| original_cos.get(number).expect("fixture object").object)
        .collect();
    let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
    let before_preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("primed preview")
        .to_vec();
    let before_overlay = document.edit().overlay().clone();
    let before_epoch = document.edit().epoch();
    let before_history = document.edit().history().clone();
    let before_trailer = document.edit().trailer_edits();
    let before_dirty = document.is_dirty();
    let before_bytes = document.bytes().as_ref().clone();
    let before_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit)
    };
    let properties = onionskin_core::metadata::PropertiesEdit {
        description: onionskin_core::metadata::Description {
            title: Some("accepted".to_owned()),
            subject: Some("grouped".to_owned()),
            ..Default::default()
        },
        ..Default::default()
    };
    let outcome: Result<(), Error> = document.edit_document("Document Properties", |tx| {
        onionskin_core::metadata::write_initial_view(
            tx,
            &onionskin_core::metadata::InitialView {
                mode: Some(onionskin_core::metadata::PageMode::UseOutlines),
                ..Default::default()
            },
        )
        .expect("the earlier initial-view write succeeds");
        let root = tx
            .trailer_value(b"Root")
            .and_then(|value| value.as_reference())
            .expect("Root after initial view");
        let catalog = tx
            .object(root.number)
            .expect("catalog lookup after initial view")
            .expect("catalog after initial view");
        assert_eq!(
            catalog
                .object
                .as_dict()
                .and_then(|dict| dict.get(b"PageMode")),
            Some(&Object::name("UseOutlines")),
            "the earlier initial-view write reached the catalog"
        );
        onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
    });
    assert!(
        outcome.is_err(),
        "the Root alias must refuse after the catalog write: {outcome:?}"
    );
    assert_eq!(document.edit().overlay(), &before_overlay);
    assert_eq!(document.edit().epoch(), before_epoch);
    assert_eq!(document.edit().trailer_edits(), before_trailer);
    assert_eq!(document.bytes().as_ref(), &before_bytes);
    assert_eq!(document.is_dirty(), before_dirty);
    assert_history_unchanged(
        &before_history,
        document.edit().history(),
        "root-alias",
        "properties",
    );
    let fresh_section = {
        let (edit, base) = document.edit_mut();
        section(base, edit)
    };
    assert_eq!(fresh_section, before_section);
    assert_eq!(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("preview after refusal")
            .as_ref(),
        before_preview.as_slice()
    );
    let reopened = open(
        document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("fresh preview")
            .as_ref(),
    );
    assert_eq!(
        reopened.trailer().get(b"Info"),
        original_cos.trailer().get(b"Info")
    );
    for (index, expected) in original_objects.iter().enumerate() {
        assert_eq!(
            &reopened
                .get((index + 1) as u32)
                .expect("reopened object")
                .object,
            expected,
            "object {}",
            index + 1
        );
    }
}

#[test]
fn intermediate_page_tree_info_alias_is_refused_without_mutation() {
    let original = with_info_selector(
        common::pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 3 0 R /MediaBox [0 0 612 792] /Resources << >> >>".to_vec(),
            b"<< /Title (old) /Subject (before) >>".to_vec(),
        ]),
        "3 0 R",
    );
    let original_cos = open(&original);
    assert_eq!(original_cos.page_count().expect("one page"), 1);
    assert_info_fixture(
        &original_cos,
        "3 0 R",
        &Object::Ref(ObjRef::new(3, 0)),
        Some(ObjRef::new(3, 0)),
        Some(0),
        Some(ObjRef::new(3, 0)),
    );
    let original_objects: Vec<_> = (1..=5)
        .map(|number| original_cos.get(number).expect("fixture object").object)
        .collect();

    for properties_route in [false, true] {
        let route = if properties_route {
            "properties"
        } else {
            "generic"
        };
        let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
        let before_preview = document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("primed preview")
            .to_vec();
        let before_overlay = document.edit().overlay().clone();
        let before_epoch = document.edit().epoch();
        let before_history = document.edit().history().clone();
        let before_trailer = document.edit().trailer_edits();
        let before_dirty = document.is_dirty();
        let before_bytes = document.bytes().as_ref().clone();
        let before_section = {
            let (edit, base) = document.edit_mut();
            section(base, edit)
        };
        let properties = onionskin_core::metadata::PropertiesEdit {
            description: onionskin_core::metadata::Description {
                title: Some("accepted".to_owned()),
                subject: Some("grouped".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        let outcome: Result<(), Error> = if properties_route {
            document.edit_document("Document Properties", |tx| {
                onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
            })
        } else {
            let (edit, base) = document.edit_mut();
            edit.apply(
                base,
                DocumentEdit::SetInfoField {
                    key: Name::new("Title"),
                    value: Some(string("accepted")),
                },
            )
        };
        assert!(
            matches!(outcome, Err(Error::StructuralInfoTarget { number: 3 })),
            "{route} must refuse the intermediate page-tree alias"
        );
        assert_eq!(document.edit().overlay(), &before_overlay, "{route}");
        assert_eq!(document.edit().epoch(), before_epoch, "{route}");
        assert_eq!(document.edit().trailer_edits(), before_trailer, "{route}");
        assert_eq!(document.bytes().as_ref(), &before_bytes, "{route}");
        assert_eq!(document.is_dirty(), before_dirty, "{route}");
        assert_history_unchanged(
            &before_history,
            document.edit().history(),
            "intermediate-alias",
            route,
        );
        let fresh_section = {
            let (edit, base) = document.edit_mut();
            section(base, edit)
        };
        assert_eq!(fresh_section, before_section, "{route}");
        assert_eq!(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("preview after refusal")
                .as_ref(),
            before_preview.as_slice(),
            "{route} preview"
        );
        let reopened = open(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("fresh preview")
                .as_ref(),
        );
        assert_eq!(
            reopened.trailer().get(b"Info"),
            original_cos.trailer().get(b"Info"),
            "{route} Info trailer"
        );
        for (number, expected) in original_objects.iter().enumerate() {
            assert_eq!(
                &reopened
                    .get((number + 1) as u32)
                    .expect("reopened object")
                    .object,
                expected,
                "{route} object {}",
                number + 1
            );
        }
    }
}

#[test]
fn cyclic_page_tree_info_write_is_refused_without_mutation() {
    let original = with_info_selector(
        common::pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 2 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 3 0 R /MediaBox [0 0 612 792] /Resources << >> >>".to_vec(),
            b"<< /Title (old) /Subject (before) >>".to_vec(),
        ]),
        "5 0 R",
    );
    let original_cos = open(&original);
    assert_eq!(original_cos.page_count().expect("one page"), 1);
    assert_info_fixture(
        &original_cos,
        "5 0 R",
        &Object::Ref(ObjRef::new(5, 0)),
        Some(ObjRef::new(5, 0)),
        Some(0),
        Some(ObjRef::new(5, 0)),
    );
    let original_objects: Vec<_> = (1..=5)
        .map(|number| original_cos.get(number).expect("fixture object").object)
        .collect();

    for properties_route in [false, true] {
        let route = if properties_route {
            "properties"
        } else {
            "generic"
        };
        let mut document = Document::open_bytes(original.clone()).expect("cycle fixture opens");
        let before_preview = document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("primed preview")
            .to_vec();
        let before_overlay = document.edit().overlay().clone();
        let before_epoch = document.edit().epoch();
        let before_history = document.edit().history().clone();
        let before_trailer = document.edit().trailer_edits();
        let before_dirty = document.is_dirty();
        let before_bytes = document.bytes().as_ref().clone();
        let before_section = {
            let (edit, base) = document.edit_mut();
            section(base, edit)
        };
        let properties = onionskin_core::metadata::PropertiesEdit {
            description: onionskin_core::metadata::Description {
                title: Some("accepted".to_owned()),
                subject: Some("grouped".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        let outcome: Result<(), Error> = if properties_route {
            document.edit_document("Document Properties", |tx| {
                onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
            })
        } else {
            let (edit, base) = document.edit_mut();
            edit.apply(
                base,
                DocumentEdit::SetInfoField {
                    key: Name::new("Title"),
                    value: Some(string("accepted")),
                },
            )
        };
        assert!(
            matches!(outcome, Err(Error::CyclicPageTree { number: 2 })),
            "{route} must report the page-tree cycle"
        );
        assert_eq!(document.edit().overlay(), &before_overlay, "{route}");
        assert_eq!(document.edit().epoch(), before_epoch, "{route}");
        assert_eq!(document.edit().trailer_edits(), before_trailer, "{route}");
        assert_eq!(document.bytes().as_ref(), &before_bytes, "{route}");
        assert_eq!(document.is_dirty(), before_dirty, "{route}");
        assert_history_unchanged(
            &before_history,
            document.edit().history(),
            "cyclic-tree",
            route,
        );
        let fresh_section = {
            let (edit, base) = document.edit_mut();
            section(base, edit)
        };
        assert_eq!(fresh_section, before_section, "{route}");
        assert_eq!(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("preview after refusal")
                .as_ref(),
            before_preview.as_slice(),
            "{route} preview"
        );
        let reopened = open(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("fresh preview")
                .as_ref(),
        );
        assert_eq!(
            reopened.trailer().get(b"Info"),
            original_cos.trailer().get(b"Info"),
            "{route} Info trailer"
        );
        for (number, expected) in original_objects.iter().enumerate() {
            assert_eq!(
                &reopened
                    .get((number + 1) as u32)
                    .expect("reopened object")
                    .object,
                expected,
                "{route} object {}",
                number + 1
            );
        }
    }
}

#[test]
fn nested_page_tree_info_accepts_edit_undo_redo_and_save() {
    let original = with_info_selector(
        common::pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 3 0 R /MediaBox [0 0 612 792] /Resources << >> >>".to_vec(),
            b"<< /Title (old) /Subject (before) >>".to_vec(),
        ]),
        "5 0 R",
    );
    let original_cos = open(&original);
    assert_eq!(original_cos.page_count().expect("one page"), 1);
    assert_info_fixture(
        &original_cos,
        "5 0 R",
        &Object::Ref(ObjRef::new(5, 0)),
        Some(ObjRef::new(5, 0)),
        Some(0),
        Some(ObjRef::new(5, 0)),
    );
    let original_pages: Vec<_> = (2..=4)
        .map(|number| original_cos.get(number).expect("page-tree object").object)
        .collect();

    for properties_route in [false, true] {
        let route = if properties_route {
            "properties"
        } else {
            "generic"
        };
        let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
        let properties = onionskin_core::metadata::PropertiesEdit {
            description: onionskin_core::metadata::Description {
                title: Some("accepted".to_owned()),
                subject: Some("grouped".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        if properties_route {
            document
                .edit_document("Document Properties", |tx| {
                    onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
                })
                .unwrap_or_else(|error| panic!("{route} edit: {error:?}"));
        } else {
            let (edit, base) = document.edit_mut();
            edit.apply(
                base,
                DocumentEdit::SetInfoField {
                    key: Name::new("Title"),
                    value: Some(string("accepted")),
                },
            )
            .unwrap_or_else(|error| panic!("{route} edit: {error:?}"));
        }
        let edited_preview = document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("edited preview")
            .to_vec();
        let edited = open(&edited_preview);
        assert_eq!(
            edited.trailer().get(b"Info").and_then(Object::as_reference),
            Some(ObjRef::new(5, 0)),
            "{route} Info reference"
        );
        assert_eq!(
            edited
                .get(5)
                .expect("edited Info")
                .object
                .as_dict()
                .and_then(|dict| dict.get(b"Title")),
            Some(&string("accepted")),
            "{route} Info title"
        );
        for (number, expected) in (2..=4).zip(&original_pages) {
            assert_eq!(
                &edited.get(number).expect("edited page-tree object").object,
                expected,
                "{route} page-tree object {number}"
            );
        }
        assert_eq!(
            edited
                .get(1)
                .expect("edited catalog")
                .object
                .as_dict()
                .and_then(|dict| dict.get(b"Pages")),
            original_cos
                .get(1)
                .expect("original catalog")
                .object
                .as_dict()
                .and_then(|dict| dict.get(b"Pages")),
            "{route} catalog Pages"
        );
        if properties_route {
            assert_eq!(
                document
                    .xmp()
                    .expect("edited XMP")
                    .expect("properties XMP")
                    .title
                    .as_deref(),
                Some("accepted"),
                "{route} edited XMP title"
            );
        }
        assert!(document.undo().expect("undo succeeds"), "{route} undo");
        assert_eq!(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("original preview")
                .as_ref(),
            original.as_slice(),
            "{route} undo restores original"
        );
        assert!(document.redo().expect("redo succeeds"), "{route} redo");
        assert_eq!(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("redo preview")
                .as_ref(),
            edited_preview.as_slice(),
            "{route} redo restores edit"
        );

        let dir = tempfile::tempdir().expect("save directory");
        let path = dir.path().join("nested-info.pdf");
        let mut file = DocumentFile::from_document(document);
        file.save_as(&path)
            .unwrap_or_else(|error| panic!("{route} save: {error:?}"));
        let saved_bytes = std::fs::read(&path).expect("saved bytes");
        let saved_cos = open(&saved_bytes);
        assert_eq!(
            saved_cos
                .trailer()
                .get(b"Info")
                .and_then(Object::as_reference),
            Some(ObjRef::new(5, 0)),
            "{route} saved Info reference"
        );
        assert_eq!(
            saved_cos
                .get(5)
                .expect("saved Info")
                .object
                .as_dict()
                .and_then(|dict| dict.get(b"Title")),
            Some(&string("accepted")),
            "{route} saved Info title"
        );
        for (number, expected) in (2..=4).zip(&original_pages) {
            assert_eq!(
                &saved_cos
                    .get(number)
                    .expect("saved page-tree object")
                    .object,
                expected,
                "{route} saved page-tree object {number}"
            );
        }
        let mut saved_document = Document::open_bytes(saved_bytes).expect("saved document opens");
        if properties_route {
            assert_eq!(
                saved_document
                    .xmp()
                    .expect("saved XMP")
                    .expect("saved properties XMP")
                    .title
                    .as_deref(),
                Some("accepted"),
                "{route} saved XMP title"
            );
        }
    }
}

#[test]
fn custom_info_keys_do_not_make_a_dictionary_structural() {
    let target =
        b"<< /Title (old) /Pages (custom) /Kids (custom) /Parent (custom) /Type (custom) >>"
            .to_vec();
    for properties_route in [false, true] {
        let route = if properties_route {
            "properties"
        } else {
            "generic"
        };
        let original = info_target_document("4 0 R", target.clone());
        let original_cos = open(&original);
        assert_info_fixture(
            &original_cos,
            "4 0 R",
            &Object::Ref(ObjRef::new(4, 0)),
            Some(ObjRef::new(4, 0)),
            Some(0),
            Some(ObjRef::new(4, 0)),
        );
        let original_preview = original.clone();
        let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
        let properties = onionskin_core::metadata::PropertiesEdit {
            description: onionskin_core::metadata::Description {
                title: Some("accepted".to_owned()),
                subject: Some("grouped".to_owned()),
                ..Default::default()
            },
            custom: ["Pages", "Kids", "Parent", "Type"]
                .into_iter()
                .map(|key| (key.to_owned(), "custom".to_owned()))
                .collect(),
        };
        if properties_route {
            document
                .edit_document("Document Properties", |tx| {
                    onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
                })
                .unwrap_or_else(|error| {
                    panic!("{route} must accept custom structural-looking keys: {error:?}")
                });
        } else {
            let (edit, base) = document.edit_mut();
            edit.apply(
                base,
                DocumentEdit::SetInfoField {
                    key: Name::new("Title"),
                    value: Some(string("accepted")),
                },
            )
            .unwrap_or_else(|error| {
                panic!("{route} must accept custom structural-looking keys: {error:?}")
            });
        }
        let edited_preview = document
            .preview_bytes(AnnotationFilter::DocumentAndMarkups)
            .expect("edited preview")
            .to_vec();
        let edited = open(&edited_preview);
        assert_eq!(
            edited.trailer().get(b"Info").and_then(Object::as_reference),
            Some(ObjRef::new(4, 0)),
            "{route} keeps the original Info reference"
        );
        let info = edited
            .get(4)
            .expect("Info object")
            .object
            .as_dict()
            .cloned()
            .expect("Info dictionary");
        assert_eq!(
            info.get(b"Title"),
            Some(&string("accepted")),
            "{route} title"
        );
        for key in [b"Pages".as_slice(), b"Kids", b"Parent", b"Type"] {
            assert_eq!(
                info.get(key),
                Some(&string("custom")),
                "{route} preserves custom key {}",
                String::from_utf8_lossy(key)
            );
        }
        if properties_route {
            assert_eq!(
                document
                    .xmp()
                    .expect("XMP reads")
                    .expect("properties writes XMP")
                    .title
                    .as_deref(),
                Some("accepted")
            );
        }
        assert!(document.undo().expect("undo succeeds"));
        assert_eq!(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("original preview")
                .as_ref(),
            original_preview.as_slice(),
            "{route} undo restores the original"
        );
        assert!(document.redo().expect("redo succeeds"));
        assert_eq!(
            document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("redo preview")
                .as_ref(),
            edited_preview.as_slice(),
            "{route} redo restores the accepted edit"
        );
    }
}

#[test]
fn properties_edit_reuses_new_unsaved_info() {
    let original = original_bytes();
    let mut document = Document::open_bytes(original.clone()).expect("opens");
    let first = onionskin_core::metadata::PropertiesEdit {
        description: onionskin_core::metadata::Description {
            title: Some("A".to_owned()),
            subject: Some("first".to_owned()),
            ..Default::default()
        },
        ..Default::default()
    };
    let second = onionskin_core::metadata::PropertiesEdit {
        description: onionskin_core::metadata::Description {
            title: Some("B".to_owned()),
            subject: Some("second".to_owned()),
            ..Default::default()
        },
        ..Default::default()
    };
    document
        .edit_document("Document Properties", |tx| {
            onionskin_core::metadata::write_properties(tx, &first, 1_789_999_500)
        })
        .expect("first properties edit");
    let first_ref = document
        .structure()
        .expect("first structure")
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("first Info reference");
    let (_, base) = document.edit_mut();
    assert!(
        base.xref().get(first_ref.number).is_none(),
        "the first Info object is only in the overlay"
    );
    document
        .edit_document("Document Properties", |tx| {
            onionskin_core::metadata::write_properties(tx, &second, 1_789_999_500)
        })
        .expect("second properties edit");
    assert_eq!(document.edit().history().reach(), 2);
    let second_ref = document
        .structure()
        .expect("second structure")
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("second Info reference");
    assert_eq!(
        second_ref, first_ref,
        "the unsaved Info reference is reused"
    );
    assert_eq!(
        document
            .info()
            .expect("second Info")
            .description
            .title
            .as_deref(),
        Some("B")
    );
    assert_eq!(
        document
            .info()
            .expect("second Info")
            .description
            .subject
            .as_deref(),
        Some("second")
    );
    assert_eq!(
        document
            .xmp()
            .expect("second XMP")
            .expect("second packet")
            .title
            .as_deref(),
        Some("B")
    );
    assert_eq!(
        document
            .xmp()
            .expect("second XMP")
            .expect("second packet")
            .subject
            .as_deref(),
        Some("second")
    );
    assert_eq!(document.bytes().as_ref(), &original);
    assert!(document.undo().expect("undo second"));
    assert_eq!(
        document
            .structure()
            .expect("first structure after undo")
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference),
        Some(first_ref)
    );
    assert_eq!(
        document
            .info()
            .expect("first Info")
            .description
            .title
            .as_deref(),
        Some("A")
    );
    assert_eq!(
        document
            .info()
            .expect("first Info")
            .description
            .subject
            .as_deref(),
        Some("first")
    );
    assert_eq!(
        document
            .xmp()
            .expect("first XMP")
            .expect("first packet")
            .subject
            .as_deref(),
        Some("first")
    );
    assert!(document.redo().expect("redo second"));
    assert_eq!(
        document
            .info()
            .expect("redo Info")
            .description
            .title
            .as_deref(),
        Some("B")
    );
    assert_eq!(
        document
            .info()
            .expect("redo Info")
            .description
            .subject
            .as_deref(),
        Some("second")
    );
    assert_eq!(document.bytes().as_ref(), &original);
}

#[test]
fn catalog_edits_preserve_unrelated_malformed_info() {
    let cases = [
        ("missing", "9 0 R", b"<< /Title (old) >>".to_vec(), None),
        (
            "generation-mismatch",
            "4 7 R",
            b"<< /Title (old) >>".to_vec(),
            Some(ObjRef::new(4, 7)),
        ),
    ];
    for (fixture, info, target, expected_ref) in cases {
        for properties_route in [false, true] {
            let route = if properties_route {
                "properties"
            } else {
                "generic"
            };
            let original = info_target_document(info, target.clone());
            let original_cos = open(&original);
            let expected_info = original_cos
                .trailer()
                .get(b"Info")
                .expect("Info selector")
                .clone();
            if fixture == "missing" {
                assert!(original_cos.xref().get(9).is_none(), "missing target xref");
                assert_info_fixture(&original_cos, info, &expected_info, None, None, None);
            } else {
                assert_info_fixture(
                    &original_cos,
                    info,
                    &expected_info,
                    expected_ref,
                    Some(0),
                    Some(ObjRef::new(4, 0)),
                );
            }
            let original_audit = original_cos.audit_references().expect("original audit");
            let mut document = Document::open_bytes(original.clone()).expect("opens");
            if properties_route {
                document
                    .edit_document("Initial View", |tx| {
                        onionskin_core::metadata::write_initial_view(
                            tx,
                            &onionskin_core::metadata::InitialView {
                                mode: Some(onionskin_core::metadata::PageMode::UseOutlines),
                                ..Default::default()
                            },
                        )
                    })
                    .unwrap_or_else(|error| panic!("{fixture} {route} edit: {error:?}"));
            } else {
                let (edit, base) = document.edit_mut();
                edit.apply(
                    base,
                    DocumentEdit::SetCatalogEntry {
                        key: Name::new("PageMode"),
                        value: Some(Object::name("UseOutlines")),
                    },
                )
                .unwrap_or_else(|error| panic!("{fixture} {route} edit: {error:?}"));
            }
            let edited_preview = document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("edited preview")
                .to_vec();
            let edited = open(&edited_preview);
            assert_eq!(
                onionskin_core::metadata::read_initial_view(&edited)
                    .expect("edited initial view")
                    .mode,
                Some(onionskin_core::metadata::PageMode::UseOutlines),
                "{fixture} {route} PageMode"
            );
            assert_eq!(edited.trailer().get(b"Info"), Some(&expected_info));
            assert_eq!(
                edited.audit_references().expect("edited audit"),
                original_audit
            );
            assert!(document.undo().expect("undo catalog edit"));
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("original preview")
                    .as_ref(),
                original.as_slice(),
                "{fixture} {route} undo"
            );
            assert!(document.redo().expect("redo catalog edit"));
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("redo preview")
                    .as_ref(),
                edited_preview.as_slice(),
                "{fixture} {route} redo"
            );
            let dir = tempfile::tempdir().expect("save directory");
            let path = dir.path().join("catalog.pdf");
            let mut file = DocumentFile::from_document(document);
            file.save_as(&path)
                .unwrap_or_else(|error| panic!("{fixture} {route} save: {error:?}"));
            let saved_bytes = std::fs::read(&path).expect("saved bytes");
            assert!(
                saved_bytes.starts_with(&original),
                "{fixture} {route} prefix"
            );
            let saved_cos = open(&saved_bytes);
            assert_eq!(
                onionskin_core::metadata::read_initial_view(&saved_cos)
                    .expect("saved initial view")
                    .mode,
                Some(onionskin_core::metadata::PageMode::UseOutlines)
            );
            assert_eq!(saved_cos.trailer().get(b"Info"), Some(&expected_info));
            assert_eq!(
                saved_cos.audit_references().expect("saved audit"),
                original_audit
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Chunk 3B: selector identity and accepted generations
// ---------------------------------------------------------------------------

/// An `/Info` fixture whose selector and the target's object-header generation
/// are set independently, so the accepted and refused generation cases are
/// expressible without a second PDF writer. Every substitution is equal-length,
/// so offsets and `startxref` stay valid.
fn generation_fixture(info: &str, header_generation: u16) -> Vec<u8> {
    let ordinary = b"<< /Title (old) /Subject (before) >>".to_vec();
    let mut bytes = info_target_document(info, ordinary);
    if header_generation != 0 {
        let from = b"4 0 obj";
        let to = format!("4 {header_generation} obj");
        let at = bytes
            .windows(from.len())
            .position(|window| window == from)
            .expect("the fourth object header");
        bytes.splice(at..at + from.len(), to.into_bytes());
    }
    bytes
}

/// The byte offset of object `object`'s xref row, which is always 20 bytes and
/// always preceded by the free head row for object 0.
fn xref_row_at(bytes: &[u8], object: u32) -> usize {
    let marker = b"\nxref\n0 ";
    let at = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("an xref table")
        + marker.len();
    let mut cursor = at;
    while bytes[cursor].is_ascii_digit() {
        cursor += 1;
    }
    let row = cursor + 1 + 20 * object as usize;
    assert_eq!(
        &bytes[row + 17..row + 20],
        b"n \n",
        "object {object} is an in-use row"
    );
    row
}

/// Rewrites only the five generation digits of object `object`'s xref row,
/// leaving its offset alone so the file still opens without repair.
fn with_xref_generation(bytes: &[u8], object: u32, generation: u16) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let row = xref_row_at(&out, object);
    out.splice(row + 11..row + 16, format!("{generation:05}").into_bytes());
    out
}

/// Frees object `object`: the row's offset and generation are written as the
/// writer writes a free row, which needs no meaningful offset.
fn with_xref_free(bytes: &[u8], object: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let row = xref_row_at(&out, object);
    out.splice(row..row + 20, b"0000000000 00001 f \n".to_vec());
    out
}

#[test]
fn invalid_info_reference_selectors_are_refused_without_mutation() {
    let cases = [
        // name, Info selector, the selector as a reference, xref generation
        // the live row carries
        ("missing", "9 0 R", Some(ObjRef::new(9, 0)), None),
        ("free", "4 0 R", Some(ObjRef::new(4, 0)), None),
        ("zero", "0 0 R", Some(ObjRef::new(0, 0)), None),
        (
            "selector-generation-mismatch",
            "4 7 R",
            Some(ObjRef::new(4, 7)),
            Some(0),
        ),
    ];

    for (fixture, info, expected_ref, xref_generation) in cases {
        for properties_route in [false, true] {
            let route = if properties_route {
                "properties"
            } else {
                "generic"
            };
            let ordinary = b"<< /Title (old) /Subject (before) >>".to_vec();
            let original = if fixture == "free" {
                with_xref_free(&info_target_document(info, ordinary), 4)
            } else {
                info_target_document(info, ordinary)
            };
            let original_cos = open(&original);
            assert_eq!(
                original_cos.page_count().expect("one page"),
                1,
                "{fixture} {route} pages"
            );
            // The malformed selector is preserved exactly as the file wrote it.
            assert_eq!(
                original_cos
                    .trailer()
                    .get(b"Info")
                    .and_then(Object::as_reference),
                expected_ref,
                "{fixture} {route} malformed selector"
            );
            match fixture {
                "missing" => assert!(
                    original_cos.xref().get(9).is_none(),
                    "{fixture} {route} has no row 9"
                ),
                "free" => assert!(
                    matches!(original_cos.xref().get(4), Some(XrefEntry::Free { .. })),
                    "{fixture} {route} row 4 is free"
                ),
                "zero" => assert!(
                    matches!(original_cos.xref().get(0), Some(XrefEntry::Free { .. })),
                    "{fixture} {route} row 0 is free"
                ),
                "selector-generation-mismatch" => {
                    assert_info_fixture(
                        &original_cos,
                        info,
                        &Object::Ref(expected_ref.expect("a reference selector")),
                        expected_ref,
                        xref_generation,
                        Some(ObjRef::new(4, 0)),
                    );
                }
                _ => {}
            }
            // The `free` case deliberately has no object 4, so what is compared
            // is each object's state including its absence.
            let original_objects: Vec<_> = (1..=4)
                .map(|number| (number, original_cos.get(number).ok()))
                .collect();
            let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
            let before_preview = document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("primed preview")
                .to_vec();
            let before_overlay = document.edit().overlay().clone();
            let before_epoch = document.edit().epoch();
            let before_history = document.edit().history().clone();
            let before_trailer = document.edit().trailer_edits();
            let before_dirty = document.is_dirty();
            let before_bytes = document.bytes().as_ref().clone();
            let before_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            let properties = onionskin_core::metadata::PropertiesEdit {
                description: onionskin_core::metadata::Description {
                    title: Some("accepted".to_owned()),
                    subject: Some("grouped".to_owned()),
                    ..Default::default()
                },
                ..Default::default()
            };
            let outcome: Result<(), Error> = if properties_route {
                document.edit_document("Document Properties", |tx| {
                    onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
                })
            } else {
                let (edit, base) = document.edit_mut();
                edit.apply(
                    base,
                    DocumentEdit::SetInfoField {
                        key: Name::new("Title"),
                        value: Some(string("accepted")),
                    },
                )
            };
            assert!(
                outcome.is_err(),
                "{fixture} {route} must refuse the selector during the edit: {outcome:?}"
            );

            assert_eq!(
                document.edit().overlay(),
                &before_overlay,
                "{fixture} {route}"
            );
            assert_eq!(document.edit().epoch(), before_epoch, "{fixture} {route}");
            assert_eq!(
                document.edit().trailer_edits(),
                before_trailer,
                "{fixture} {route}"
            );
            assert_eq!(
                document.bytes().as_ref(),
                &before_bytes,
                "{fixture} {route}"
            );
            assert_eq!(document.is_dirty(), before_dirty, "{fixture} {route}");
            assert_history_unchanged(&before_history, document.edit().history(), fixture, route);
            let fresh_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            assert_eq!(fresh_section, before_section, "{fixture} {route}");
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("preview after refusal")
                    .as_ref(),
                before_preview.as_slice(),
                "{fixture} {route} preview"
            );
            let reopened = open(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("fresh preview")
                    .as_ref(),
            );
            assert_eq!(
                reopened.trailer().get(b"Info"),
                original_cos.trailer().get(b"Info"),
                "{fixture} {route} Info trailer"
            );
            for (number, expected) in &original_objects {
                assert_eq!(
                    &reopened.get(*number).ok(),
                    expected,
                    "{fixture} {route} object {number}"
                );
            }
        }
    }
}

#[test]
fn info_selector_generation_survives_edit_undo_redo_and_save() {
    let cases = [
        // name, Info selector, the object header's generation, the xref row's
        // generation
        ("valid-nonzero", "4 7 R", 7, 7),
        ("advisory-header-mismatch", "4 0 R", 7, 0),
    ];

    for (fixture, info, header_generation, row_generation) in cases {
        for properties_route in [false, true] {
            let route = if properties_route {
                "properties"
            } else {
                "generic"
            };
            let original = if row_generation == 0 {
                generation_fixture(info, header_generation)
            } else {
                with_xref_generation(
                    &generation_fixture(info, header_generation),
                    4,
                    row_generation,
                )
            };
            let original_cos = open(&original);
            assert_eq!(
                original_cos.page_count().expect("one page"),
                1,
                "{fixture} {route} pages"
            );
            // The selector and the live row agree; only the header may disagree,
            // and the write must use the row's generation rather than the
            // header's.
            let selector = ObjRef::new(4, row_generation);
            assert_info_fixture(
                &original_cos,
                info,
                &Object::Ref(selector),
                Some(selector),
                Some(row_generation),
                Some(ObjRef::new(4, header_generation)),
            );
            let mut document = Document::open_bytes(original.clone()).expect("fixture opens");
            let properties = onionskin_core::metadata::PropertiesEdit {
                description: onionskin_core::metadata::Description {
                    title: Some("accepted".to_owned()),
                    subject: Some("grouped".to_owned()),
                    ..Default::default()
                },
                ..Default::default()
            };
            if properties_route {
                document
                    .edit_document("Document Properties", |tx| {
                        onionskin_core::metadata::write_properties(tx, &properties, 1_789_999_500)
                    })
                    .unwrap_or_else(|error| panic!("{fixture} {route} edit: {error:?}"));
            } else {
                let (edit, base) = document.edit_mut();
                edit.apply(
                    base,
                    DocumentEdit::SetInfoField {
                        key: Name::new("Title"),
                        value: Some(string("accepted")),
                    },
                )
                .unwrap_or_else(|error| panic!("{fixture} {route} edit: {error:?}"));
            }
            assert_eq!(
                document.edit().history().reach(),
                1,
                "{fixture} {route} one history entry"
            );
            assert_eq!(
                document.bytes().as_ref(),
                &original,
                "{fixture} {route} bytes"
            );
            let edited_preview = document
                .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                .expect("edited preview")
                .to_vec();
            let edited_overlay = document.edit().overlay().clone();
            let edited_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            let edited = open(&edited_preview);
            assert_eq!(
                edited.trailer().get(b"Info").and_then(Object::as_reference),
                Some(selector),
                "{fixture} {route} edited Info reference"
            );
            match edited.xref().get(4).expect("edited row 4") {
                XrefEntry::InFile { generation, .. } => {
                    assert_eq!(
                        generation, row_generation,
                        "{fixture} {route} edited row generation"
                    )
                }
                other => panic!("{fixture} {route} edited row is {other:?}"),
            }
            assert_eq!(
                edited.get(4).expect("edited object").objref,
                selector,
                "{fixture} {route} emitted header generation"
            );
            let info = edited
                .get(4)
                .expect("Info object")
                .object
                .as_dict()
                .cloned()
                .expect("Info dictionary");
            assert_eq!(
                info.get(b"Title"),
                Some(&string("accepted")),
                "{fixture} {route} title"
            );
            assert_eq!(
                info.get(b"Subject"),
                Some(&string(if properties_route {
                    "grouped"
                } else {
                    "before"
                })),
                "{fixture} {route} subject"
            );
            if properties_route {
                let xmp = document.xmp().expect("XMP reads").expect("XMP packet");
                assert_eq!(
                    xmp.title.as_deref(),
                    Some("accepted"),
                    "{fixture} {route} XMP"
                );
                assert_eq!(
                    xmp.subject.as_deref(),
                    Some("grouped"),
                    "{fixture} {route} XMP subject"
                );
            }

            assert!(document.undo().expect("undo"), "{fixture} {route} undo");
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("original preview")
                    .as_ref(),
                original.as_slice(),
                "{fixture} {route} undo restores the original"
            );
            let undone_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            assert!(
                undone_section.is_none(),
                "{fixture} {route} undo leaves no section"
            );
            assert!(document.redo().expect("redo"), "{fixture} {route} redo");
            assert_eq!(
                document.edit().overlay(),
                &edited_overlay,
                "{fixture} {route} redone overlay"
            );
            let redone_section = {
                let (edit, base) = document.edit_mut();
                section(base, edit)
            };
            assert_eq!(
                redone_section, edited_section,
                "{fixture} {route} redone section"
            );
            assert_eq!(
                document
                    .preview_bytes(AnnotationFilter::DocumentAndMarkups)
                    .expect("redo preview")
                    .as_ref(),
                edited_preview.as_slice(),
                "{fixture} {route} redone preview"
            );

            let dir = tempfile::tempdir().expect("save directory");
            let path = dir.path().join("generation.pdf");
            let mut file = DocumentFile::from_document(document);
            file.save_as(&path)
                .unwrap_or_else(|error| panic!("{fixture} {route} save: {error:?}"));
            let saved_bytes = std::fs::read(&path).expect("saved bytes");
            assert!(
                saved_bytes.starts_with(&original),
                "{fixture} {route} saved bytes keep the original"
            );
            let saved = open(&saved_bytes);
            assert_eq!(
                saved.trailer().get(b"Info").and_then(Object::as_reference),
                Some(selector),
                "{fixture} {route} saved Info reference"
            );
            assert_eq!(
                saved.get(4).expect("saved object").objref,
                selector,
                "{fixture} {route} saved header generation"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn info_target_document(info: &str, target: Vec<u8>) -> Vec<u8> {
    with_info_selector(
        common::pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>".to_vec(),
            target,
        ]),
        info,
    )
}

fn with_info_selector(mut bytes: Vec<u8>, info: &str) -> Vec<u8> {
    let marker = b">>\nstartxref";
    assert_eq!(
        bytes
            .windows(marker.len())
            .filter(|window| *window == marker)
            .count(),
        1,
        "the fixture has one final trailer marker"
    );
    let at = bytes
        .windows(marker.len())
        .rposition(|window| window == marker)
        .expect("the final trailer marker is present");
    bytes.splice(at..at, format!("/Info {info} ").bytes());
    bytes
}

/// Asserts a fixture's `/Info` selector and, when it names a live target, the
/// row and header generations that target actually carries. A selector's
/// generation may disagree with both, which is what the mismatch fixtures
/// exist to pin, so the caller states the row and header rather than letting
/// them be inferred from the selector.
fn assert_info_fixture(
    document: &CosDocument,
    info: &str,
    expected: &Object,
    expected_ref: Option<ObjRef>,
    xref_generation: Option<u16>,
    parsed_header: Option<ObjRef>,
) {
    let selected = document
        .trailer()
        .get(b"Info")
        .expect("fixture Info selector");
    assert_eq!(selected, expected, "fixture Info value {info}");
    let (Some(expected), Some(xref_generation), Some(parsed_header)) =
        (expected_ref, xref_generation, parsed_header)
    else {
        // The selector names no live row; the caller asserts that itself.
        return;
    };
    assert_eq!(
        selected.as_reference(),
        Some(expected),
        "fixture selector {info}"
    );
    match document
        .xref()
        .get(expected.number)
        .expect("target xref row")
    {
        XrefEntry::InFile { generation, .. } => {
            assert_eq!(generation, xref_generation, "xref generation {info}");
        }
        XrefEntry::InObjectStream { .. } => {
            assert_eq!(xref_generation, 0, "object-stream generation {info}")
        }
        XrefEntry::Free { .. } => panic!("fixture target {info} is free"),
    }
    assert_eq!(
        document.get(expected.number).expect("parsed target").objref,
        parsed_header,
        "parsed header generation {info}"
    );
}

fn assert_history_unchanged(before: &History, after: &History, fixture: &str, route: &str) {
    assert_eq!(after.reach(), before.reach(), "{fixture} {route} reach");
    assert_eq!(
        after.redo_reach(),
        before.redo_reach(),
        "{fixture} {route} redo reach"
    );
    assert_eq!(
        after.can_undo(),
        before.can_undo(),
        "{fixture} {route} can undo"
    );
    assert_eq!(
        after.can_redo(),
        before.can_redo(),
        "{fixture} {route} can redo"
    );
    assert_eq!(
        after.resident_bytes(),
        before.resident_bytes(),
        "{fixture} {route} resident bytes"
    );
    assert_eq!(
        after.forgotten(),
        before.forgotten(),
        "{fixture} {route} forgotten"
    );
    assert_eq!(
        after.forgot_saved_mark(),
        before.forgot_saved_mark(),
        "{fixture} {route} forgotten saved mark"
    );
    assert_eq!(
        after.is_at_saved_mark(),
        before.is_at_saved_mark(),
        "{fixture} {route} saved mark"
    );
    assert_eq!(
        after.undo_label(),
        before.undo_label(),
        "{fixture} {route} undo label"
    );
    assert_eq!(
        after.redo_label(),
        before.redo_label(),
        "{fixture} {route} redo label"
    );
    for index in 0..after.reach() + after.redo_reach() {
        assert_eq!(
            after.entry(index),
            before.entry(index),
            "{fixture} {route} history entry {index}"
        );
    }
}

fn append(bytes: &[u8], section: Vec<u8>) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.extend_from_slice(&section);
    out
}

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document reopens")
}

fn last_changes(edit: &EditSession) -> Vec<Change> {
    let reach = edit.history().reach();
    assert!(reach > 0, "there is an entry to read");
    edit.history()
        .entry(reach - 1)
        .expect("the entry exists")
        .changes()
        .to_vec()
}

fn every_change(edit: &EditSession) -> Vec<Change> {
    (0..edit.history().reach())
        .filter_map(|index| edit.history().entry(index))
        .flat_map(|entry| entry.changes().to_vec())
        .collect()
}

fn base_has(base: &CosDocument, number: u32) -> bool {
    base.get(number).is_ok()
}

/// A deterministic edit sequence with deliberate repetition: object numbers are
/// drawn from a small set so the same object is overwritten many times, which
/// is the shape a "drop the last overlay node" undo gets wrong.
fn generated_sequence(count: usize) -> Vec<(u32, i64)> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut out = Vec::with_capacity(count);
    for step in 0..count {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        // 1..=3 are the catalog, the page tree and the page: all present in the
        // base, which is what makes `before` non-`None` for every one of them.
        let number = 1 + (state % 3) as u32;
        out.push((number, step as i64));
    }
    out
}

// ---------------------------------------------------------------------------
// Orphans across transactions
// ---------------------------------------------------------------------------

/// An object an earlier edit created, orphaned by a later one, comes back
/// when the later one is undone and goes again on redo. Without the drop
/// being a recorded change, collapse forgets the object at commit and the
/// undo restores a reference to nothing.
#[test]
fn an_object_orphaned_by_a_later_edit_comes_back_on_undo_and_goes_on_redo() {
    let (base, mut edit) = session();
    let catalog = base
        .trailer()
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog");
    let original = base.get(catalog.number).expect("reads").object;
    let with = |extra: Option<u32>| {
        let mut dict = original.as_dict().expect("a dictionary").clone();
        if let Some(number) = extra {
            dict.set(
                Name::new("PieceInfo"),
                Object::Ref(onionskin_cos::ObjRef::new(number, 0)),
            );
        }
        Object::Dict(dict)
    };
    let created = edit
        .transact(&base, "Add", |tx| {
            let number = tx.reserve();
            tx.put_object(number, 0, marker(1))?;
            tx.put_object(catalog.number, 0, with(Some(number)))?;
            Ok(number)
        })
        .expect("adds");
    edit.transact(&base, "Remove", |tx| {
        tx.put_object(catalog.number, 0, with(None))
    })
    .expect("removes");
    assert_eq!(
        overlay_object(&edit, created),
        None,
        "orphaned, then dropped"
    );

    assert!(edit.undo(&base).expect("undoes"));
    assert_eq!(
        overlay_object(&edit, created).map(|state| state.object),
        Some(marker(1)),
        "the undo restores what the catalog names again"
    );
    assert!(edit.redo(&base).expect("redoes"));
    assert_eq!(overlay_object(&edit, created), None);
}

// ---------------------------------------------------------------------------
// Collapse and redo accounting, probed by mutation
// ---------------------------------------------------------------------------

/// Editing an object and then writing its base value back is not a change.
/// Both transactions keep their history entry, but the overlay collapses to
/// nothing, so a save appends no section. Skipping the collapse on commit
/// leaves the base value sitting in the overlay as a dirty entry.
#[test]
fn writing_the_base_value_back_collapses_the_overlay_but_keeps_both_entries() {
    let (base, mut edit) = session();
    let original = edit
        .transact(&base, "Edit", |tx| {
            let original = tx.object(1)?.expect("the catalog is in the base");
            tx.put_object(1, 0, marker(1))?;
            Ok(original)
        })
        .expect("the edit commits");
    assert_eq!(
        overlay_object(&edit, 1),
        Some(ObjectState::new(0, marker(1)))
    );

    edit.transact(&base, "Revert", |tx| {
        tx.put_object(1, original.generation, original.object.clone())
    })
    .expect("the revert commits");

    assert!(
        edit.overlay().is_empty(),
        "an overlay entry equal to the base is dropped on commit"
    );
    assert_eq!(section(&base, &edit), None, "a save would append nothing");
    assert_eq!(edit.history().reach(), 2, "both steps stay undoable");
}

/// Redo entries are still resident, so undoing and redoing never changes what
/// the history reports holding. Reporting only the applied prefix would let a
/// long redo tail sit outside the figure the bound is meant to cover.
#[test]
fn undo_and_redo_leave_the_resident_total_unchanged() {
    let (base, mut edit) = session();
    for n in 0..3 {
        edit.transact(&base, "Marker", |tx| tx.put_object(1, 0, marker(n)))
            .expect("the edit commits");
    }
    let held = edit.history().resident_bytes();
    assert!(held > 0);

    assert!(edit.undo(&base).expect("undoes"));
    assert!(edit.undo(&base).expect("undoes"));
    assert_eq!(edit.history().redo_reach(), 2);
    assert_eq!(
        edit.history().resident_bytes(),
        held,
        "redo entries still count"
    );

    assert!(edit.redo(&base).expect("redoes"));
    assert_eq!(edit.history().resident_bytes(), held);
}
