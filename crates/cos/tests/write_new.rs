//! `Document::write_new`: the documents Onionskin authors, which have nothing
//! to append to. Combine, split, extract, create-from-image, compress and the
//! printed-sheet backend all produce one, and the core invariant applies to
//! them from that first write onwards.

use onionskin_cos::{BytesSource, Dict, Document, Error, Holder, ObjRef, Object};

/// A two-page document: catalog, page tree, two pages, one shared content
/// stream.
fn two_pages() -> (Vec<(ObjRef, Object)>, Dict) {
    let mut catalog = Dict::new();
    catalog.set("Type", Object::name("Catalog"));
    catalog.set("Pages", Object::Ref(ObjRef::new(2, 0)));

    let mut pages = Dict::new();
    pages.set("Type", Object::name("Pages"));
    pages.set(
        "Kids",
        Object::Array(vec![
            Object::Ref(ObjRef::new(3, 0)),
            Object::Ref(ObjRef::new(4, 0)),
        ]),
    );
    pages.set("Count", Object::Integer(2));

    let page = |number: u32| {
        let mut page = Dict::new();
        page.set("Type", Object::name("Page"));
        page.set("Parent", Object::Ref(ObjRef::new(2, 0)));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(200),
                Object::Integer(100),
            ]),
        );
        page.set("Resources", Object::Dict(Dict::new()));
        page.set("Contents", Object::Ref(ObjRef::new(5, 0)));
        (ObjRef::new(number, 0), Object::Dict(page))
    };

    let mut content = Dict::new();
    content.set("Length", Object::Integer(0));
    let objects = vec![
        (ObjRef::new(1, 0), Object::Dict(catalog)),
        (ObjRef::new(2, 0), Object::Dict(pages)),
        page(3),
        page(4),
        (
            ObjRef::new(5, 0),
            Object::Stream(onionskin_cos::Stream {
                dict: content,
                raw: Vec::new(),
            }),
        ),
    ];

    let mut trailer = Dict::new();
    trailer.set("Root", Object::Ref(ObjRef::new(1, 0)));
    (objects, trailer)
}

/// One cross-reference subsection, as read back out of the bytes this crate
/// wrote: the header's two numbers and the entries under it.
#[derive(Debug)]
struct Subsection {
    start: u32,
    entries: Vec<(u64, u16, char)>,
}

/// Parses the classic table at the end of `bytes` into its subsections.
///
/// Reopening through our own parser would prove our parser accepts what our
/// writer emits, which it always will. Five M3 commands hand these files to
/// other readers, so the table is read here the way a stricter reader would:
/// the subsection headers and the fixed-width entries, as written.
fn subsections(bytes: &[u8]) -> Vec<Subsection> {
    let at = bytes
        .windows(6)
        .rposition(|w| w == b"\nxref\n")
        .expect("the file has a classic cross-reference table")
        + 1;
    let text = String::from_utf8_lossy(&bytes[at..]).into_owned();
    let mut words = text.split_ascii_whitespace();
    assert_eq!(words.next(), Some("xref"));

    let mut out: Vec<Subsection> = Vec::new();
    while let Some(first) = words.next() {
        if first == "trailer" {
            break;
        }
        let start: u32 = first
            .parse()
            .expect("a subsection header starts with a number");
        let count: u32 = words
            .next()
            .expect("a subsection header carries a count")
            .parse()
            .expect("the count is a number");
        let mut entries = Vec::new();
        for _ in 0..count {
            let field: u64 = words.next().expect("entry field").parse().expect("field");
            let generation: u16 = words
                .next()
                .expect("entry generation")
                .parse()
                .expect("generation");
            let kind = words.next().expect("entry kind");
            assert!(kind == "n" || kind == "f", "unknown entry kind {kind}");
            entries.push((field, generation, kind.chars().next().expect("one letter")));
        }
        out.push(Subsection { start, entries });
    }
    out
}

#[test]
fn a_written_document_opens_clean_and_has_the_pages_it_was_given() {
    let (objects, trailer) = two_pages();
    let bytes = Document::write_new(&objects, trailer).expect("the document is writable");

    assert!(
        bytes.starts_with(b"%PDF-"),
        "a PDF starts with its header, not with an object"
    );
    // `open` rather than `open_repairing`: a file this crate wrote must not
    // need this crate's repair path to be readable.
    let document =
        Document::open(Box::new(BytesSource::new(bytes.clone()))).expect("it opens clean");
    assert_eq!(document.page_count().ok(), Some(2));
    assert_eq!(
        document.page(1).expect("the second page").objref.number,
        4,
        "the pages come back in the order they were listed"
    );

    // The invariant applies from the first write on: reopening and saving
    // nothing must append nothing at all.
    assert_eq!(
        document.save_to_vec().expect("a no-op save"),
        bytes,
        "a document we wrote must round-trip byte for byte"
    );
}

#[test]
fn the_table_carries_the_free_list_head_and_covers_every_object_written() {
    let (objects, trailer) = two_pages();
    let bytes = Document::write_new(&objects, trailer).expect("writable");
    let table = subsections(&bytes);

    let first = table.first().expect("the table has a subsection");
    assert_eq!(
        first.start, 0,
        "the first subsection starts at object 0, the head of the free list"
    );
    assert_eq!(
        first.entries.first().copied(),
        Some((0, 65535, 'f')),
        "the head is a free entry at generation 65535 linking back to itself"
    );

    let covered: Vec<u32> = table
        .iter()
        .flat_map(|s| (0..s.entries.len() as u32).map(move |i| s.start + i))
        .collect();
    assert_eq!(
        covered,
        vec![0, 1, 2, 3, 4, 5],
        "the subsections must cover object 0 and every object written, with no gap"
    );

    for (index, (field, generation, kind)) in first.entries.iter().enumerate().skip(1) {
        assert_eq!(*kind, 'n', "object {index} was written, so it is in use");
        assert_eq!(*generation, 0);
        assert_eq!(
            &bytes[*field as usize..*field as usize + 1],
            format!("{index}").as_bytes(),
            "the offset recorded for object {index} must be where its header is"
        );
    }
}

/// Object numbers need not be dense: an undone allocation leaves a gap, and a
/// document built from one that had gaps keeps them. The table then needs one
/// subsection per run, which is what ISO 32000-1 7.5.4 provides for.
#[test]
fn a_gap_in_the_object_numbers_becomes_a_second_subsection() {
    let (mut objects, trailer) = two_pages();
    for (objref, _) in objects.iter_mut() {
        if objref.number == 5 {
            objref.number = 9;
        }
    }
    let mut contents_repointed = Vec::new();
    for (objref, object) in objects {
        let object = match object {
            Object::Dict(mut dict) if dict.get(b"Contents").is_some() => {
                dict.set("Contents", Object::Ref(ObjRef::new(9, 0)));
                Object::Dict(dict)
            }
            other => other,
        };
        contents_repointed.push((objref, object));
    }

    let bytes = Document::write_new(&contents_repointed, trailer).expect("writable");
    let table = subsections(&bytes);
    let covered: Vec<(u32, usize)> = table.iter().map(|s| (s.start, s.entries.len())).collect();
    assert_eq!(
        covered,
        vec![(0, 5), (9, 1)],
        "one subsection per run of numbers, and no invented entries in between"
    );
    assert_eq!(
        Document::open(Box::new(BytesSource::new(bytes)))
            .expect("it opens clean")
            .page_count()
            .ok(),
        Some(2)
    );
}

#[test]
fn a_document_that_could_not_be_opened_is_refused_rather_than_written() {
    let (objects, trailer) = two_pages();

    let mut rootless = trailer.clone();
    rootless.remove(b"Root");
    match Document::write_new(&objects, rootless) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("/Root"), "{detail}"),
        other => panic!("a trailer with no /Root must be refused, got {other:?}"),
    }

    let mut with_zero = objects.clone();
    with_zero.push((ObjRef::new(0, 0), Object::Integer(1)));
    match Document::write_new(&with_zero, trailer.clone()) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("free list"), "{detail}"),
        other => panic!("object 0 must be refused, got {other:?}"),
    }

    let mut twice = objects.clone();
    twice.push((ObjRef::new(3, 0), Object::Integer(1)));
    match Document::write_new(&twice, trailer) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("twice"), "{detail}"),
        other => panic!("one number given two objects must be refused, got {other:?}"),
    }
}

#[test]
fn write_new_refuses_effective_encryption_but_accepts_null() {
    let (objects, trailer) = two_pages();
    let mut encryption = Dict::new();
    encryption.set("Filter", Object::name("Standard"));

    let mut direct = trailer.clone();
    direct.set("Encrypt", Object::Dict(encryption.clone()));
    match Document::write_new(&objects, direct) {
        Err(Error::EncryptedWrite) => {}
        other => panic!("a direct Encrypt entry must be refused, got {other:?}"),
    }

    let encryption_ref = ObjRef::new(6, 0);
    let mut with_encryption_object = objects.clone();
    with_encryption_object.push((encryption_ref, Object::Dict(encryption)));
    let mut indirect = trailer.clone();
    indirect.set("Encrypt", Object::Ref(encryption_ref));
    match Document::write_new(&with_encryption_object, indirect) {
        Err(Error::EncryptedWrite) => {}
        other => panic!("an indirect Encrypt entry must be refused, got {other:?}"),
    }

    let mut null = trailer;
    null.set("Encrypt", Object::Null);
    let saved = Document::write_new(&objects, null).expect("a null Encrypt entry is writable");
    let reopened = Document::open(Box::new(BytesSource::new(saved))).expect("reopens clean");
    assert_eq!(reopened.page_count().ok(), Some(2));
    assert!(matches!(
        reopened.trailer().get(b"Encrypt"),
        None | Some(Object::Null)
    ));
}

#[test]
fn write_new_requires_a_nonzero_catalog_root() {
    for root in [
        Object::Null,
        Object::Integer(1),
        Object::Ref(ObjRef::new(0, 0)),
        Object::Ref(ObjRef::new(3, 0)),
    ] {
        let (objects, mut trailer) = two_pages();
        trailer.set("Root", root);
        match Document::write_new(&objects, trailer) {
            Err(Error::Unrecoverable { detail }) => assert!(
                detail.contains("Root") || detail.contains("catalog"),
                "{detail}"
            ),
            other => panic!("an invalid catalog root must be refused: {other:?}"),
        }
    }
}

#[test]
fn write_new_rejects_non_catalog_root_targets() {
    for (number, object) in [(6, Object::Integer(7)), (7, Object::Dict(Dict::new()))] {
        let (mut objects, mut trailer) = two_pages();
        objects.push((ObjRef::new(number, 0), object));
        trailer.set("Root", Object::Ref(ObjRef::new(number, 0)));
        match Document::write_new(&objects, trailer) {
            Err(Error::Unrecoverable { detail }) => assert!(detail.contains("catalog"), "{detail}"),
            other => panic!("a Root target must be a Catalog dictionary: {other:?}"),
        }
    }

    let (objects, mut trailer) = two_pages();
    trailer.set("Root", Object::Ref(ObjRef::new(3, 0)));
    match Document::write_new(&objects, trailer) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("catalog"), "{detail}"),
        other => panic!("a Page Root target must be refused: {other:?}"),
    }
}

#[test]
fn write_new_rejects_wrong_generation_in_a_member_reference() {
    let (mut objects, trailer) = two_pages();
    let pages = objects
        .iter_mut()
        .find_map(|(objref, object)| (objref.number == 2).then_some(object))
        .expect("the page tree exists");
    if let Object::Dict(dict) = pages {
        dict.set(
            "Kids",
            Object::Array(vec![
                Object::Ref(ObjRef::new(3, 1)),
                Object::Ref(ObjRef::new(4, 0)),
            ]),
        );
    } else {
        panic!("the page tree is a dictionary");
    }
    match Document::write_new(&objects, trailer) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Object(2));
            assert_eq!(target, ObjRef::new(3, 1));
        }
        other => panic!("a member reference must match the supplied generation: {other:?}"),
    }
}

#[test]
fn write_new_rejects_wrong_generation_in_the_trailer_root() {
    let (objects, mut trailer) = two_pages();
    trailer.set("Root", Object::Ref(ObjRef::new(1, 1)));
    match Document::write_new(&objects, trailer) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Trailer);
            assert_eq!(target, ObjRef::new(1, 1));
        }
        other => panic!("the trailer Root must match the catalog generation: {other:?}"),
    }
}

#[test]
fn write_new_normalizes_reserved_trailer_fields_and_generated_size() {
    let (objects, mut trailer) = two_pages();
    trailer.set("Prev", Object::Integer(999));
    trailer.set("Subject", Object::String(b"metadata".to_vec()));
    trailer.set("Size", Object::Integer(1));
    trailer.set("XRefStm", Object::Integer(123));
    trailer.set("Type", Object::name("XRef"));
    trailer.set("W", Object::Array(vec![Object::Integer(1)]));
    trailer.set(
        "Index",
        Object::Array(vec![Object::Integer(0), Object::Integer(1)]),
    );
    trailer.set("Filter", Object::name("FlateDecode"));
    trailer.set("DecodeParms", Object::Dict(Dict::new()));
    trailer.set("Length", Object::Integer(1));
    let bytes = Document::write_new(&objects, trailer).expect("reserved fields are normalized");
    let reopened = Document::open(Box::new(BytesSource::new(bytes))).expect("reopens clean");
    assert_eq!(reopened.page_count().ok(), Some(2));
    assert_eq!(
        reopened.trailer().get(b"Root"),
        Some(&Object::Ref(ObjRef::new(1, 0)))
    );
    assert_eq!(
        reopened.trailer().get(b"Size").and_then(Object::as_integer),
        Some(6)
    );
    assert_eq!(
        reopened.trailer().get(b"Subject"),
        Some(&Object::String(b"metadata".to_vec()))
    );
    for key in [
        b"Prev".as_slice(),
        b"XRefStm".as_slice(),
        b"Type".as_slice(),
        b"W".as_slice(),
        b"Index".as_slice(),
        b"Filter".as_slice(),
        b"DecodeParms".as_slice(),
        b"Length".as_slice(),
    ] {
        assert!(
            reopened.trailer().get(key).is_none(),
            "reserved key {key:?}"
        );
    }
}

/// A document written from scratch is the whole of its own object graph, so a
/// reference to a number nobody wrote resolves to nothing at all. The same
/// check the section builder's gate runs, and here it is complete rather than
/// bounded, because there is nothing underneath for a reference to reach.
#[test]
fn a_reference_to_an_object_nobody_wrote_is_refused() {
    let (objects, trailer) = two_pages();

    let mut orphaned = Vec::new();
    for (objref, object) in objects.clone() {
        let object = match object {
            // The page tree loses one of its two pages, so its /Kids names an
            // object the file does not contain.
            Object::Dict(mut dict) if dict.get(b"Kids").is_some() => {
                dict.set("Kids", Object::Array(vec![Object::Ref(ObjRef::new(3, 0))]));
                dict.set("Count", Object::Integer(1));
                Object::Dict(dict)
            }
            other => other,
        };
        if objref.number != 4 {
            orphaned.push((objref, object));
        }
    }
    assert!(
        Document::write_new(&orphaned, trailer.clone()).is_ok(),
        "dropping a page and its /Kids entry together is a legal document"
    );

    let mut dangling = orphaned.clone();
    for (objref, object) in dangling.iter_mut() {
        if let (2, Object::Dict(dict)) = (objref.number, object) {
            dict.set(
                "Kids",
                Object::Array(vec![
                    Object::Ref(ObjRef::new(3, 0)),
                    Object::Ref(ObjRef::new(4, 0)),
                ]),
            );
        }
    }
    match Document::write_new(&dangling, trailer.clone()) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Object(2));
            assert_eq!(target.number, 4);
        }
        other => panic!("a /Kids naming an object nobody wrote must be refused, got {other:?}"),
    }

    // The trailer dangles on its own, and it is not one of the objects.
    let mut rootless = trailer;
    rootless.set("Info", Object::Ref(ObjRef::new(77, 0)));
    match Document::write_new(&orphaned, rootless) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Trailer);
            assert_eq!(target.number, 77);
        }
        other => panic!("a trailer naming an object nobody wrote must be refused, got {other:?}"),
    }
}
