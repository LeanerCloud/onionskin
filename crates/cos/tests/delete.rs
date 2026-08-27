//! Carry-forward 5 from the spike: removing an object had no verb. M2's page
//! deletion needs one, and under the core invariant a deletion is not an
//! erasure: the object's bytes stay where they are and the appended section
//! marks the number free, on a properly chained free list (ISO 32000-1 7.5.4).

mod common;

use std::collections::BTreeMap;

use common::{classic_pdf, effective_xref, last_xref_table, skeleton, XrefRow};
use onionskin_cos::{BytesSource, Document, Error, Object};

/// The skeleton plus two objects nothing points at, so a test can delete one
/// without leaving the page tree dangling.
fn fixture() -> Vec<u8> {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    bodies.push(b"<</Type/Spare/Which 5>>");
    classic_pdf(&bodies, &[])
}

fn open(bytes: &[u8]) -> Document {
    Document::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the fixture opens clean")
}

/// Walks the free list from object 0 and returns the objects on it, checking
/// as it goes that every link lands on a free entry, that nothing repeats and
/// that the list ends by linking back to 0.
fn free_list(table: &BTreeMap<u32, XrefRow>) -> Vec<u32> {
    let head = table
        .get(&0)
        .expect("object 0, the head of the free list, must have an entry");
    assert!(head.free, "object 0 must be a free entry");
    assert_eq!(
        head.generation, 65535,
        "object 0 carries generation 65535 by definition"
    );

    let mut chain: Vec<u32> = Vec::new();
    let mut at = head.field as u32;
    while at != 0 {
        let row = table
            .get(&at)
            .unwrap_or_else(|| panic!("the free list points at object {at}, which has no entry"));
        assert!(
            row.free,
            "object {at} is on the free list but its entry says it is in use"
        );
        assert!(
            !chain.contains(&at),
            "the free list loops back to object {at}"
        );
        chain.push(at);
        at = row.field as u32;
    }
    chain
}

#[test]
fn a_deleted_object_is_gone_from_the_saved_file() {
    let original = fixture();
    let mut document = open(&original);
    assert!(document.get(4).is_ok(), "object 4 is there to begin with");

    document.delete_object(4).expect("object 4 is deletable");
    match document.get(4) {
        Err(Error::MissingObject(objref)) => assert_eq!(
            objref.generation, 1,
            "a deletion bumps the generation, so the number cannot come back at the old one"
        ),
        other => panic!("expected MissingObject, got {other:?}"),
    }

    let saved = document.save_to_vec().expect("save");
    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "a deletion appends; it does not rewrite"
    );

    let reopened =
        Document::open(Box::new(BytesSource::new(saved.clone()))).expect("reopens clean");
    match reopened.get(4) {
        Err(Error::MissingObject(_)) => {}
        other => panic!("a deleted object must not resolve after a reopen, got {other:?}"),
    }
    assert_eq!(
        reopened.page_count().ok(),
        Some(1),
        "deleting an unreferenced object must not disturb the page tree"
    );
    assert!(
        reopened.get(5).is_ok(),
        "the object next to the deleted one must be untouched"
    );

    // Truncating at the section boundary is still the whole of undo.
    let cut = document.original_len() as usize;
    let rolled_back = Document::open(Box::new(BytesSource::new(saved[..cut].to_vec())))
        .expect("the truncated file opens clean");
    assert!(
        rolled_back.get(4).is_ok(),
        "truncating the section must bring the deleted object back"
    );
}

#[test]
fn the_appended_section_carries_a_well_formed_free_list() {
    let mut document = open(&fixture());
    document.delete_object(4).expect("deletable");
    let saved = document.save_to_vec().expect("save");

    let appended = last_xref_table(&saved);
    let by_number: BTreeMap<u32, XrefRow> = appended.iter().map(|r| (r.number, *r)).collect();
    assert_eq!(
        free_list(&by_number),
        vec![4],
        "the appended section's free list must hold exactly the deleted object"
    );
    let row = by_number.get(&4).expect("object 4 has a row");
    assert!(row.free);
    assert_eq!(
        row.generation, 1,
        "the free entry records the next generation"
    );

    // Nothing else was rewritten: the section describes the head and the one
    // deletion, and no other object.
    let mut numbers: Vec<u32> = appended.iter().map(|r| r.number).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, vec![0, 4]);
}

/// A second deletion, in a later generation of the same file, must go in at
/// the head of the list without dropping what was already on it.
#[test]
fn a_later_deletion_splices_into_the_existing_free_list() {
    let mut first = open(&fixture());
    first.delete_object(4).expect("deletable");
    let once = first.save_to_vec().expect("save");

    let mut second = Document::open(Box::new(BytesSource::new(once.clone()))).expect("reopens");
    second.delete_object(5).expect("deletable");
    let twice = second.save_to_vec().expect("save");

    assert_eq!(&twice[..once.len()], &once[..], "the second save appends");
    assert_eq!(
        free_list(&effective_xref(&twice)),
        vec![5, 4],
        "the newest free entry goes in at the head and the older one stays on the list"
    );

    let reopened = Document::open(Box::new(BytesSource::new(twice))).expect("reopens clean");
    assert!(reopened.get(4).is_err());
    assert!(reopened.get(5).is_err());
    assert_eq!(reopened.page_count().ok(), Some(1));
}

#[test]
fn deleting_what_is_not_there_is_an_error_rather_than_a_no_op() {
    let mut document = open(&fixture());

    match document.delete_object(99) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.number, 99),
        other => panic!("deleting an object that does not exist must fail, got {other:?}"),
    }
    match document.delete_object(0) {
        Err(Error::Unrecoverable { .. }) => {}
        other => panic!("object 0 is the free-list head, not a document object, got {other:?}"),
    }

    document.delete_object(4).expect("the first deletion works");
    match document.delete_object(4) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.generation, 1),
        other => panic!("deleting twice must fail the second time, got {other:?}"),
    }
    // The three refusals left nothing behind: the one deletion that worked is
    // all the appended section describes.
    let saved = document.save_to_vec().expect("save");
    let numbers: Vec<u32> = last_xref_table(&saved).iter().map(|r| r.number).collect();
    assert_eq!(numbers, vec![0, 4]);
}

/// The two edit verbs work on one map, so the last one called wins. Writing an
/// object back after deleting it has to bring it back, or a caller undoing a
/// deletion would silently save a free entry over the object it just wrote.
#[test]
fn writing_an_object_back_supersedes_its_deletion() {
    let mut document = open(&fixture());
    document.delete_object(4).expect("deletable");
    document.set_object(4, 0, Object::Integer(7));
    assert_eq!(
        document.get(4).expect("object 4 is back").object,
        Object::Integer(7)
    );

    let saved = document.save_to_vec().expect("save");
    let reopened = Document::open(Box::new(BytesSource::new(saved))).expect("reopens clean");
    assert_eq!(
        reopened.get(4).expect("object 4 survived").object,
        Object::Integer(7)
    );
    assert_eq!(
        free_list(&effective_xref(&reopened.save_to_vec().expect("save"))),
        Vec::<u32>::new(),
        "an undone deletion must leave nothing on the free list"
    );
}
