//! The overlay-taking section builder: the caller holds the edits, the
//! document holds none, and one call serves both a save and the preview that
//! has to agree with it byte for byte.

mod common;

use std::collections::BTreeMap;

use common::{classic_pdf, corpus_dir, skeleton};
use onionskin_cos::{BytesSource, Document, Error, Holder, Name, Object, PendingEdit, Provenance};

/// The skeleton plus two objects nothing points at, so a test can rewrite or
/// free one without disturbing the page tree.
fn fixture() -> Vec<u8> {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    bodies.push(b"<</Type/Spare/Which 5>>");
    classic_pdf(&bodies, &[])
}

fn open(bytes: &[u8]) -> Document {
    Document::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the fixture opens clean")
}

/// The bytes a save of `overlay` would produce: the original with the section
/// appended, which is also what a preview renders.
fn saved_with(
    document: &Document,
    original: &[u8],
    overlay: &BTreeMap<u32, PendingEdit>,
    trailer_edits: &BTreeMap<Name, Option<Object>>,
) -> Vec<u8> {
    let section = document
        .section_for(overlay, trailer_edits)
        .expect("the section builds")
        .expect("the overlay is not empty, so there are bytes");
    let mut out = original.to_vec();
    out.extend_from_slice(&section);
    out
}

fn set(number: u32, object: Object) -> (u32, PendingEdit) {
    (
        number,
        PendingEdit::Set {
            generation: 0,
            object,
        },
    )
}

/// An overlay the document's own edit map does not contain, written into a
/// document that never hears about it. Comparing `incremental_section` with
/// `section_for` would prove nothing: one calls the other, so they agree by
/// construction. This is the half that does not.
#[test]
fn the_section_is_built_from_the_overlay_it_is_given() {
    let original = fixture();
    let document = open(&original);
    let overlay: BTreeMap<u32, PendingEdit> = [
        set(4, Object::name("overlaid")),
        set(9, Object::Integer(1234)),
    ]
    .into_iter()
    .collect();

    assert!(
        !document.has_pending_changes(),
        "the document's own edit map stays empty; the caller holds the edits"
    );
    let saved = saved_with(&document, &original, &overlay, &BTreeMap::new());
    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "the original bytes are never rewritten"
    );

    let reopened = open(&saved);
    assert_eq!(
        reopened.get(4).expect("object 4 resolves").object,
        Object::name("overlaid"),
        "the number the overlay rewrote must resolve to the overlay's object"
    );
    assert_eq!(
        reopened.get(9).expect("object 9 resolves").object,
        Object::Integer(1234),
        "and so must the number the overlay invented"
    );
    assert_eq!(reopened.page_count().ok(), Some(1));
}

/// An empty overlay on a clean document writes nothing; on a repaired one it
/// still writes the repair, which is the clause that keeps a damaged file from
/// silently staying damaged.
#[test]
fn an_empty_overlay_writes_nothing_unless_the_document_owes_a_repair() {
    let original = fixture();
    assert_eq!(
        open(&original)
            .section_for(&BTreeMap::new(), &BTreeMap::new())
            .expect("no section"),
        None,
        "a clean document with nothing overlaid appends nothing at all"
    );

    let (repaired, provenance) = Document::open_repairing(Box::new(BytesSource::new(
        with_junk_before_the_header(&original),
    )))
    .expect("the damaged fixture opens");
    assert!(matches!(provenance, Provenance::Repaired(_)));
    assert!(
        repaired
            .section_for(&BTreeMap::new(), &BTreeMap::new())
            .expect("the section builds")
            .is_some(),
        "a repaired document owes its repair even with an empty overlay"
    );
}

fn with_junk_before_the_header(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::from(&b"junk\n"[..]);
    out.extend_from_slice(bytes);
    out
}

/// An xref-stream file whose objects 5 and 6 live inside object stream 4, with
/// junk before the header so the document opens repaired. Both halves matter:
/// a repaired document is the one whose section carries a table over every
/// object, and a compressed object is the one that table cannot point at, so
/// the section has to carry a copy of it.
fn repaired_with_compressed_objects(six: &[u8]) -> Vec<u8> {
    let five: &[u8] = b"<</Type/Spare/Which 5>>";
    let header = format!("5 0 6 {} ", five.len() + 1);
    let first = header.len();
    let mut data = header.into_bytes();
    data.extend_from_slice(five);
    data.push(b' ');
    data.extend_from_slice(six);

    let mut bytes = Vec::from(&b"%PDF-1.5\n"[..]);
    let mut offsets = [0u64; 8];
    for (index, body) in skeleton().iter().enumerate() {
        offsets[index + 1] = bytes.len() as u64;
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    offsets[4] = bytes.len() as u64;
    bytes.extend_from_slice(
        format!(
            "4 0 obj\n<</Type/ObjStm/N 2/First {first}/Length {}>>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&data);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    offsets[7] = bytes.len() as u64;
    // /W [1 2 1]: type, a two-byte field, then one byte.
    let row = |kind: u8, field: u64, last: u8| [kind, (field >> 8) as u8, field as u8, last];
    let mut rows = Vec::new();
    rows.extend_from_slice(&row(0, 0, 255));
    for offset in &offsets[1..=4] {
        rows.extend_from_slice(&row(1, *offset, 0));
    }
    rows.extend_from_slice(&row(2, 4, 0));
    rows.extend_from_slice(&row(2, 4, 1));
    rows.extend_from_slice(&row(1, offsets[7], 0));

    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<</Type/XRef/Size 8/W[1 2 1]/Index[0 8]/Root 1 0 R/Length {}>>\nstream\n",
            rows.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&rows);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(format!("startxref\n{}\n%%EOF\n", offsets[7]).as_bytes());
    with_junk_before_the_header(&bytes)
}

/// A save of an overlay goes through the same section builder a preview does,
/// so what lands on disk is what the canvas was drawing.
#[test]
fn saving_an_overlay_writes_the_bytes_the_section_builder_returned() {
    let original = fixture();
    let document = open(&original);
    let overlay: BTreeMap<u32, PendingEdit> =
        [set(4, Object::name("overlaid"))].into_iter().collect();

    let scratch =
        std::env::temp_dir().join(format!("onionskin-cos-overlay-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let path = scratch.join("saved.pdf");
    document
        .save_overlay_to_path(&overlay, &BTreeMap::new(), &path)
        .expect("the overlay saves");

    assert_eq!(
        std::fs::read(&path).expect("the saved file is readable"),
        saved_with(&document, &original, &overlay, &BTreeMap::new()),
        "the file on disk must be the original plus the section, byte for byte"
    );
    assert_eq!(
        open(&std::fs::read(&path).expect("readable"))
            .get(4)
            .expect("object 4 resolves")
            .object,
        Object::name("overlaid")
    );
    std::fs::remove_dir_all(&scratch).expect("scratch cleans up");
}

/// Setting a trailer key and then taking it back across a save. Nothing above
/// the trailer can stop naming a key, so the section has to be able to say the
/// key is gone: it writes `null`, which ISO 32000-1 7.3.7 makes equivalent to
/// the entry being absent.
#[test]
fn a_cleared_trailer_key_is_absent_when_the_file_is_reopened() {
    let Some(dir) = corpus_dir("seeds") else {
        return;
    };
    // minimal.pdf has no /Info, so the first save creates the key rather than
    // overwriting one, which is the case an undo cannot express by dropping an
    // overlay node.
    let original = std::fs::read(dir.join("minimal.pdf")).expect("the seed is readable");
    let document = open(&original);
    assert!(document.trailer().get(b"Info").is_none());

    let mut info = onionskin_cos::Dict::new();
    info.set("Producer", Object::String(b"created by the edit".to_vec()));
    let number = document.next_object_number();
    let overlay: BTreeMap<u32, PendingEdit> =
        [set(number, Object::Dict(info))].into_iter().collect();
    let trailer_edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Info"),
        Some(Object::Ref(onionskin_cos::ObjRef::new(number, 0))),
    )]
    .into_iter()
    .collect();
    let with_info = saved_with(&document, &original, &overlay, &trailer_edits);

    let document = open(&with_info);
    assert!(
        document.trailer().get(b"Info").is_some(),
        "the first save has to create the key, or the clearing below proves nothing"
    );

    let cleared: BTreeMap<Name, Option<Object>> = [(Name::new("Info"), None)].into_iter().collect();
    let undone = saved_with(&document, &with_info, &BTreeMap::new(), &cleared);
    assert_eq!(
        &undone[..with_info.len()],
        &with_info[..],
        "clearing a key appends; it does not rewrite"
    );

    let reopened = open(&undone);
    assert_eq!(
        reopened.trailer().get(b"Info"),
        None,
        "a key the newest section cleared must read as absent, not as the value underneath it"
    );
    assert_eq!(
        open(&with_info)
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference)
            .map(|r| r.number),
        Some(number),
        "and the generation underneath still has it, because nothing was rewritten"
    );
}

fn spare(which: i64) -> Object {
    let mut dict = onionskin_cos::Dict::new();
    dict.set("Type", Object::name("Spare"));
    dict.set("Which", Object::Integer(which));
    Object::Dict(dict)
}

/// The fourth of the four places the overlay has to be read instead of the
/// document's own edit map, and the one that fails silently.
///
/// On a repaired document the section carries a table over every object, and a
/// compressed object is copied into the section because no row can point at
/// it. If the skip that keeps that copy out of an overlaid object's way reads
/// the document's edit map - permanently empty when the caller keeps its own
/// overlay - the object goes in twice: the overlay's copy first, the base copy
/// second, and the table's last-write-wins row points at the base. The edit is
/// written into the file and then indexed away, and nothing else notices.
#[test]
fn an_overlay_over_a_compressed_object_in_a_repaired_document_is_the_one_indexed() {
    let original = repaired_with_compressed_objects(b"<</Type/Spare/Which 6>>");
    let (document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(original.clone())))
            .expect("the fixture opens by repair");
    assert!(
        matches!(provenance, Provenance::Repaired(_)),
        "the fixture has to be repaired, or the section carries no full table"
    );
    assert_eq!(
        document.get(5).expect("object 5 resolves").object,
        spare(5),
        "the fixture has to hold object 5 inside the object stream, or it tests nothing"
    );

    let overlay: BTreeMap<u32, PendingEdit> =
        [set(5, Object::name("overlaid"))].into_iter().collect();
    let saved = saved_with(&document, &original, &overlay, &BTreeMap::new());

    let (reopened, _) = Document::open_repairing(Box::new(BytesSource::new(saved)))
        .expect("the saved file reopens");
    assert_eq!(
        reopened.get(5).expect("object 5 resolves").object,
        Object::name("overlaid"),
        "the cross-reference row must point at the overlay's copy, not the base one"
    );
    assert_eq!(
        reopened.get(6).expect("object 6 resolves").object,
        spare(6),
        "the compressed object the overlay did not touch is carried through"
    );
    assert_eq!(reopened.page_count().ok(), Some(1));
}

/// A caller that keeps its own overlay allocates its own numbers, so it needs
/// to know where the file's own numbering stops. Calling `add_object` to find
/// out would leave an edit the document cannot withdraw.
#[test]
fn the_next_object_number_is_one_above_everything_the_file_names() {
    let mut document = open(&fixture());
    // The fixture writes objects 1 through 5 and a /Size of 6.
    assert_eq!(document.next_object_number(), 6);

    let first = document.add_object(Object::Integer(1)).expect("a number");
    assert_eq!(
        first.number, 6,
        "the accessor names the number that is handed out"
    );
    assert_eq!(
        document.next_object_number(),
        7,
        "and it moves on once that number is taken"
    );

    document
        .set_object(2, 0, Object::Integer(2))
        .expect("object 2 is writable");
    assert_eq!(
        document.next_object_number(),
        7,
        "rewriting an existing object takes no new number"
    );
}

/// The other half of the gate's rule, on the one path where a section writes
/// an object it did not author: a repaired document's full table has to
/// re-serialize every compressed object, so an object the caller never touched
/// goes into the section, and freeing what it names would leave the section
/// carrying a reference to a number nothing can resolve.
#[test]
fn a_section_may_not_free_what_a_copy_it_carries_forward_still_names() {
    let original = repaired_with_compressed_objects(b"<</Type/Referrer/Points 5 0 R>>");
    let (document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(original.clone())))
            .expect("the fixture opens by repair");
    assert!(matches!(provenance, Provenance::Repaired(_)));

    // Object 6 is compressed, names object 5, and nothing in this overlay
    // rewrites it: the section carries a copy of it because a rebuilt table
    // cannot point into an object stream.
    let overlay: BTreeMap<u32, PendingEdit> = [(5, PendingEdit::Delete { generation: 1 })]
        .into_iter()
        .collect();
    match document.section_for(&overlay, &BTreeMap::new()) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Object(6));
            assert_eq!(target.number, 5);
        }
        other => panic!("freeing an object a carried copy names must be refused: {other:?}"),
    }

    // The same document, freeing the object nothing names, still saves.
    let legal: BTreeMap<u32, PendingEdit> = [(6, PendingEdit::Delete { generation: 1 })]
        .into_iter()
        .collect();
    assert!(document
        .section_for(&legal, &BTreeMap::new())
        .expect("freeing an object nothing in the section names is allowed")
        .is_some());
}

/// An overlay comes straight from a caller, so the refusal `set_object` makes
/// at its own door has to be made at this one too: a number the file has
/// already marked free cannot be written back, because the free entry is in a
/// section that is already on disk.
#[test]
fn an_overlay_may_not_write_a_number_the_file_has_marked_free() {
    let mut first = open(&fixture());
    first.delete_object(4).expect("object 4 is deletable");
    let once = first.save_to_vec().expect("save");

    let second = open(&once);
    let overlay: BTreeMap<u32, PendingEdit> = [set(4, Object::Integer(7))].into_iter().collect();
    match second.section_for(&overlay, &BTreeMap::new()) {
        Err(Error::FreedObject(objref)) => assert_eq!(objref.number, 4),
        other => panic!("writing a freed number must be refused, got {other:?}"),
    }
}

/// The other half of the same door. `delete_object` refuses object 0, the
/// catalog, and a number the file has already freed; an overlay reaches the
/// same writer, and each of those writes a file nothing downstream would
/// complain about: a free entry linked to itself is a cycle in the free list,
/// and a document whose catalog is free does not open at all.
#[test]
fn an_overlay_may_not_free_what_the_document_cannot_do_without() {
    let mut first = open(&fixture());
    first.delete_object(4).expect("object 4 is deletable");
    let once = first.save_to_vec().expect("save");
    let document = open(&once);

    let free = |number: u32| -> BTreeMap<u32, PendingEdit> {
        [(number, PendingEdit::Delete { generation: 1 })]
            .into_iter()
            .collect()
    };

    // Object 4 is already free in the base: freeing it again would write
    // `4 -> 4` into the list.
    match document.section_for(&free(4), &BTreeMap::new()) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.number, 4),
        other => panic!("freeing an already-free number must be refused, got {other:?}"),
    }
    // Object 9 was never in the file at all.
    match document.section_for(&free(9), &BTreeMap::new()) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref.number, 9),
        other => panic!("freeing a number the file never had must be refused, got {other:?}"),
    }
    match document.section_for(&free(0), &BTreeMap::new()) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("free list"), "{detail}"),
        other => panic!("object 0 is the free-list head, got {other:?}"),
    }
    // Object 1 is the catalog: the file it would produce does not open.
    match document.section_for(&free(1), &BTreeMap::new()) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("catalog"), "{detail}"),
        other => panic!("freeing the catalog must be refused, got {other:?}"),
    }

    // Object 5 is in use and nothing names it, which is the legal case and
    // has to stay legal.
    assert!(document
        .section_for(&free(5), &BTreeMap::new())
        .expect("freeing an object nothing names is allowed")
        .is_some());
}
