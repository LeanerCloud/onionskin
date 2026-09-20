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

use onionskin_core::{Change, DocumentEdit, EditSession, ObjectState};
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, Object, PendingEdit};

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

    edit.transact(&base, "Marker", |tx| tx.set_object(1, 0, marker(1)))
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

    edit.transact(&base, "First", |tx| tx.set_object(1, 0, marker(1)))
        .expect("the first edit commits");
    edit.transact(&base, "Second", |tx| tx.set_object(1, 0, marker(2)))
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
            tx.set_object(*number, 0, marker(*value))
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
            tx.set_object(*number, 0, marker(*value))
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
    edit.transact(&base, "First", |tx| tx.set_object(1, 0, marker(1)))
        .expect("the first edit commits");
    edit.transact(&base, "Second", |tx| tx.set_object(1, 0, marker(2)))
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
    edit.transact(&base, "First", |tx| tx.set_object(1, 0, marker(1)))
        .expect("the first edit commits");
    edit.transact(&base, "Second", |tx| tx.set_object(1, 0, marker(2)))
        .expect("the second edit commits");

    assert!(edit.undo(&base).expect("undo runs"));
    assert_eq!(edit.history().redo_reach(), 1);

    edit.transact(&base, "Third", |tx| tx.set_object(1, 0, marker(3)))
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
            tx.set_object(1, 0, marker(first))?;
            tx.set_object(1, 0, marker(second))
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
    edit.transact(&base, "First", |tx| tx.set_object(1, 0, marker(1)))
        .expect("the first edit commits");
    let before = edit.overlay().clone();

    let outcome: Result<(), _> = edit.transact(&base, "Aborted", |tx| {
        tx.set_object(1, 0, marker(9))?;
        let reserved = tx.reserve();
        tx.set_object(reserved, 0, marker(10))?;
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
        edit.transact(&base, "Marker", |tx| tx.set_object(1, 0, marker(n)))
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
