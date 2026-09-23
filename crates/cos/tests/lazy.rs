//! Decision 11, as a measurement rather than a claim: opening a document and
//! reaching its first page must not read the whole file.

mod common;

use std::time::Instant;

use common::{classic_pdf, corpus_dir, corpus_root, pdfs_in, skeleton};
use onionskin_cos::{
    BytesSource, CountingSource, Document, Error, FileSource, ObjRef, Object, Origin, Provenance,
    Span, XrefEntry,
};

fn is_pdf_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn is_pdf_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn skip_pdf_trivia(bytes: &[u8], mut at: usize) -> Option<usize> {
    let start = at;
    loop {
        match bytes.get(at).copied() {
            Some(byte) if is_pdf_whitespace(byte) => at += 1,
            Some(b'%') => {
                at += 1;
                while let Some(byte) = bytes.get(at).copied() {
                    at += 1;
                    if matches!(byte, b'\r' | b'\n') {
                        if byte == b'\r' && bytes.get(at) == Some(&b'\n') {
                            at += 1;
                        }
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    (at > start).then_some(at)
}

fn read_unsigned_token(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut end = at;
    while bytes.get(end).is_some_and(|byte| byte.is_ascii_digit()) {
        end += 1;
    }
    (end > at)
        .then(|| {
            std::str::from_utf8(&bytes[at..end])
                .ok()?
                .parse()
                .ok()
                .map(|value| (value, end))
        })
        .flatten()
}

/// Checks only the lexical contract needed by the span oracle: an exact object
/// header at byte zero, followed by the object's body.
fn header_matches(bytes: &[u8], expected: ObjRef) -> bool {
    let Some((number, after_number)) = read_unsigned_token(bytes, 0) else {
        return false;
    };
    let Some(after_number) = skip_pdf_trivia(bytes, after_number) else {
        return false;
    };
    let Some((generation, after_generation)) = read_unsigned_token(bytes, after_number) else {
        return false;
    };
    let Some(after_generation) = skip_pdf_trivia(bytes, after_generation) else {
        return false;
    };
    if bytes.get(after_generation..after_generation + 3) != Some(b"obj") {
        return false;
    }
    let after_keyword = after_generation + 3;
    if bytes
        .get(after_keyword)
        .is_some_and(|byte| !is_pdf_whitespace(*byte) && !is_pdf_delimiter(*byte))
    {
        return false;
    }
    number == u64::from(expected.number) && generation == u64::from(expected.generation)
}

/// Only files this big make the claim interesting; below it, one read window
/// covers the document anyway.
const INTERESTING: u64 = 1024 * 1024;
/// Reading a quarter of a multi-megabyte file to show its first page would
/// already mean the laziness bet had failed.
const BUDGET_PERCENT: u64 = 25;

#[test]
fn opening_a_large_document_reads_far_less_than_the_whole_file() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    let mut candidates: Vec<_> = pdfs_in(&root)
        .into_iter()
        .filter_map(|path| {
            let len = std::fs::metadata(&path).ok()?.len();
            (len >= INTERESTING).then_some((len, path))
        })
        .collect();
    if candidates.is_empty() {
        common::missing(&format!(
            "the corpus holds no PDF of at least {INTERESTING} bytes, so laziness cannot be measured"
        ));
        return;
    }
    candidates.sort_by_key(|(len, _)| std::cmp::Reverse(*len));

    let candidate_count = candidates.len();
    // Every candidate rather than the largest few: which files the largest few
    // are depends on which corpus sets happen to be fetched, and the one file
    // in the corpus that comes anywhere near this budget is 4 MB sitting among
    // 10 MB neighbours.
    let mut measured = 0usize;
    let mut violations = Vec::new();
    for (len, path) in candidates.iter() {
        let file = FileSource::open(path).expect("corpus file opens");
        let (counting, stats) = CountingSource::new(Box::new(file));

        let started = Instant::now();
        let Ok((document, provenance)) = Document::open_repairing(Box::new(counting)) else {
            continue;
        };
        // A repaired open scans the whole file by design; that is the price of
        // damage, and it is not what this budget is about.
        if !matches!(provenance, Provenance::Clean) {
            continue;
        }
        if document.first_page().is_err() {
            continue;
        }
        let elapsed = started.elapsed();

        let read = stats.total();
        println!(
            "{}: {} bytes read of {} ({:.2}%), {} pages, first page in {:?}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            read,
            len,
            read as f64 * 100.0 / *len as f64,
            document.page_count().unwrap_or(-1),
            elapsed
        );
        if read >= *len {
            violations.push(format!(
                "{}: opening read {read} bytes of a {len} byte file",
                path.display()
            ));
        }
        if read * 100 > BUDGET_PERCENT * *len {
            violations.push(format!(
                "{}: opening read {read} bytes of a {len} byte file, over the {BUDGET_PERCENT}% budget",
                path.display()
            ));
        }
        measured += 1;
    }

    println!("measured {measured} of {candidate_count} size-qualified candidates");
    assert!(
        measured > 0,
        "no large corpus file opened cleanly, so laziness went unmeasured"
    );
    assert!(
        violations.is_empty(),
        "laziness budget violations: {violations:?}"
    );
}

/// Decision 7: every parsed object knows the bytes it came from. The check is
/// that the recorded span, cut out of the file on its own, holds exactly that
/// object.
#[test]
fn every_parsed_object_records_the_bytes_it_came_from() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    // The seeds cover plain objects; the external files bring the compressed
    // ones, whose span lives inside a container rather than in the file. Only
    // the seeds are committed, so the compressed half is checked when the
    // external corpus is present and skipped loudly when it is not.
    let external = corpus_dir("external");
    let mut files = pdfs_in(&root.join("seeds"));
    if let Some(dir) = &external {
        files.extend(pdfs_in(dir).into_iter().take(400));
    }

    let mut in_file = 0usize;
    let mut compressed = 0usize;
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(document) = Document::open(Box::new(BytesSource::new(bytes.clone()))) else {
            continue;
        };
        let name = path.display();
        let mut spans = Vec::new();

        for (number, _) in document.xref().iter() {
            let Ok(parsed) = document.get(number) else {
                continue;
            };
            let span = match parsed.origin {
                Origin::File(span) => span,
                // A compressed object's span is inside its container's decoded
                // data, so the bytes to preserve for it are the container's.
                Origin::ObjectStream {
                    container,
                    container_span,
                    within,
                } => {
                    assert!(
                        within.end > within.start,
                        "{name}: object {number} has an empty span in its container"
                    );
                    let container_number = container;
                    let container = document
                        .get(container_number)
                        .expect("object-stream container must be readable");
                    assert_eq!(
                        container.objref.number, container_number,
                        "{name}: object {number} resolved the wrong container identity"
                    );
                    assert!(
                        container_span.end <= bytes.len() as u64,
                        "{name}: object {number} names a container span outside the file"
                    );
                    assert!(
                        header_matches(
                            &bytes[container_span.start as usize..container_span.end as usize],
                            container.objref,
                        ),
                        "{name}: object {number} names container {container_number}, whose span starts elsewhere"
                    );
                    compressed += 1;
                    continue;
                }
                Origin::Pending => continue,
            };
            let slice = &bytes[span.start as usize..span.end as usize];
            assert_eq!(
                parsed.objref.number, number,
                "{name}: object {number}'s span resolved a different object identity"
            );
            assert!(
                header_matches(slice, parsed.objref),
                "{name}: object {number}'s span does not start at its header (objref={:?}, span={:?}, bytes={:?})",
                parsed.objref,
                span,
                &slice[..slice.len().min(32)]
            );
            let end = slice
                .iter()
                .rposition(|b| !b.is_ascii_whitespace())
                .map_or(slice, |i| &slice[..=i]);
            assert!(
                end.ends_with(b"endobj") || end.ends_with(b"endstream"),
                "{name}: object {number}'s span does not end at the object's end"
            );
            spans.push((number, span));
            in_file += 1;
        }

        // Two objects claiming the same bytes would make redaction and
        // selection-to-source mapping lie later on.
        spans.sort_by_key(|(_, span)| span.start);
        for pair in spans.windows(2) {
            assert!(
                pair[0].1.end <= pair[1].1.start,
                "{name}: objects {} and {} overlap in the file",
                pair[0].0,
                pair[1].0
            );
        }
    }
    assert!(in_file > 0, "no in-file spans were checked");
    if external.is_some() {
        assert!(
            compressed > 0,
            "the external corpus is present but produced no object-stream spans"
        );
    }
    println!("byte spans verified: {in_file} in file, {compressed} in object streams");
}

#[test]
fn header_oracle_matches_pdf_trivia_and_rejects_wrong_boundaries() {
    let accepted = [
        (b"12 0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"12  0 obj\n".as_slice(), ObjRef::new(12, 0)),
        (b"12\t0\nobj<".as_slice(), ObjRef::new(12, 0)),
        (b"12\r0\x0cobj/".as_slice(), ObjRef::new(12, 0)),
        (b"12 0 obj>".as_slice(), ObjRef::new(12, 0)),
        (
            b"12\0% first comment\r\n0007%second\nobj(".as_slice(),
            ObjRef::new(12, 7),
        ),
        (b"00012 0007 obj".as_slice(), ObjRef::new(12, 7)),
    ];
    for (bytes, expected) in accepted {
        assert!(header_matches(bytes, expected), "accepted case: {bytes:?}");
    }

    let rejected = [
        (b"13 0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"12 1 obj".as_slice(), ObjRef::new(12, 0)),
        (b"18446744073709551616 0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"+12 0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"12 0 object".as_slice(), ObjRef::new(12, 0)),
        (b"12 0 objX".as_slice(), ObjRef::new(12, 0)),
        (b"\n12 0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"% comment\n12 0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"12x0 obj".as_slice(), ObjRef::new(12, 0)),
        (b"12 0obj".as_slice(), ObjRef::new(12, 0)),
        (b"2 0 obj".as_slice(), ObjRef::new(12, 0)),
    ];
    for (bytes, expected) in rejected {
        assert!(!header_matches(bytes, expected), "rejected case: {bytes:?}");
    }
}

#[test]
fn header_oracle_checks_the_actual_file_span_and_identity() {
    let variants = [
        b"4 0 obj\n<</Type/Spare/Marker 1>>\nendobj".as_slice(),
        b"4  0 obj\n<</Type/Spare/Marker 1>>\nendobj".as_slice(),
        b"4\t0\nobj\n<</Type/Spare/Marker 1>>\nendobj".as_slice(),
    ];
    for inner in variants {
        let body = [b"prefix\n".as_slice(), inner].concat();
        let mut bodies = skeleton();
        bodies.push(&body);
        let honest = classic_pdf(&bodies, &[]);
        let inner_at = honest
            .windows(inner.len())
            .position(|window| window == inner)
            .expect("the varied header is in the generated object body")
            as u64;
        let bytes = classic_pdf(&bodies, &[(4, inner_at)]);
        let document = Document::open(Box::new(BytesSource::new(bytes.clone())))
            .expect("the synthetic file opens cleanly");
        assert_eq!(
            document.xref().get(4),
            Some(XrefEntry::InFile {
                offset: inner_at,
                generation: 0,
            })
        );
        let parsed = document
            .get(4)
            .expect("the xref points to the inner object");
        assert_eq!(parsed.objref, ObjRef::new(4, 0));
        let expected_span = Span::new(inner_at, inner_at + inner.len() as u64);
        assert_eq!(parsed.origin, Origin::File(expected_span));
        let span = expected_span;
        let slice = &bytes[span.start as usize..span.end as usize];
        assert_eq!(slice, inner);
        assert!(header_matches(slice, parsed.objref));
        assert!(!header_matches(slice, ObjRef::new(5, 0)));
        assert!(!header_matches(
            &bytes[(span.start - 1) as usize..span.end as usize],
            parsed.objref
        ));
        assert!(!header_matches(&slice[1..], parsed.objref));

        let old_literal = format!("{} {} obj", parsed.objref.number, parsed.objref.generation);
        assert_eq!(
            slice.starts_with(old_literal.as_bytes()),
            inner == variants[0]
        );
    }
}

#[test]
fn an_object_is_parsed_only_when_it_is_asked_for() {
    let Some(root) = corpus_root() else {
        common::missing("no corpus found; set ONIONSKIN_CORPUS");
        return;
    };
    let path = root.join("seeds").join("two-page.pdf");
    if !path.is_file() {
        common::missing(&format!("{} is absent", path.display()));
        return;
    }

    let file = FileSource::open(&path).expect("seed opens");
    let (counting, stats) = CountingSource::new(Box::new(file));
    let document = Document::open(Box::new(counting)).expect("seed opens clean");
    let after_open = stats.total();

    let catalog = document.catalog().expect("catalog resolves");
    let after_catalog = stats.total();
    assert_eq!(
        after_catalog, after_open,
        "validation already parsed the catalog before the public access"
    );
    assert!(catalog.contains(b"Pages"));

    let before_page = stats.total();
    document.first_page().expect("first page resolves");
    assert!(
        stats.total() > before_page,
        "an unvalidated leaf must still be read on first access"
    );
    let after_page = stats.total();

    // The same objects again come from the cache, not from the source.
    document.first_page().expect("first page resolves again");
    document.catalog().expect("catalog resolves again");
    assert_eq!(
        stats.total(),
        after_page,
        "a cached object must not be re-read"
    );
}

#[test]
fn validated_large_page_tree_is_reused_before_lazy_leaf_access() {
    const PAGE_COUNT: usize = 10_000;

    let mut bodies = skeleton();
    let pages_body = format!(
        "<</Type/Pages/Kids[{}]/Count {PAGE_COUNT}>>",
        (3..=PAGE_COUNT + 2)
            .map(|number| format!("{number} 0 R "))
            .collect::<String>()
    );
    bodies[1] = pages_body.as_bytes();
    let leaf = bodies[2];
    bodies.extend((0..PAGE_COUNT - 1).map(|_| leaf));
    let mut bytes = classic_pdf(&bodies, &[]);

    let pages_at = bytes
        .windows(b"2 0 obj\n".len())
        .position(|window| window == b"2 0 obj\n")
        .expect("the generated Pages object has its exact header");
    bytes[pages_at + 2] = b'7';
    let pages_end = pages_at
        + bytes[pages_at..]
            .windows(b"\nendobj".len())
            .position(|window| window == b"\nendobj")
            .expect("the generated Pages object has an endobj")
        + b"\nendobj".len();
    let expected_pages = format!("2 7 obj\n{pages_body}\nendobj").into_bytes();
    let source_bytes = bytes.clone();

    let (counting, stats) = CountingSource::new(Box::new(BytesSource::new(bytes)));
    let (document, provenance) =
        Document::open_repairing(Box::new(counting)).expect("large flat page tree opens");
    assert_eq!(provenance, Provenance::Clean);
    let after_open = stats.total();

    document.catalog().expect("catalog resolves");
    document.get(2).expect("Pages resolves");
    assert_eq!(
        document.page_count().expect("page count resolves"),
        PAGE_COUNT as i64
    );
    assert_eq!(
        stats.total(),
        after_open,
        "validated catalog and Pages dictionaries must be reused"
    );
    let pages = document.get(2).expect("Pages remains available");
    assert_eq!(pages.objref, ObjRef::new(2, 7));
    assert_eq!(
        pages.origin,
        Origin::File(Span::new(pages_at as u64, pages_end as u64))
    );
    assert_eq!(pages.recovered_boundary, None);
    let span = pages.origin.file_span().expect("Pages has a file span");
    assert_eq!(
        &source_bytes[span.start as usize..span.end as usize],
        &expected_pages
    );

    let pages_dict = pages.object.as_dict().expect("Pages is a dictionary");
    assert_eq!(
        pages_dict.get(b"Count").and_then(Object::as_integer),
        Some(PAGE_COUNT as i64)
    );
    let kids = pages_dict
        .get(b"Kids")
        .and_then(Object::as_array)
        .expect("Pages has Kids");
    let expected_kids: Vec<_> = (3..=PAGE_COUNT as u32 + 2)
        .map(|number| Object::Ref(ObjRef::new(number, 0)))
        .collect();
    assert_eq!(kids, expected_kids.as_slice());

    let before_leaf = stats.total();
    let first = document.first_page().expect("first page resolves");
    assert_eq!(first.objref.number, 3);
    let first_dict = first.object.as_dict().expect("first page is a dictionary");
    assert_eq!(first_dict.get(b"Type"), Some(&Object::name("Page")));
    let expected_media_box = [
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(200),
        Object::Integer(100),
    ];
    assert_eq!(
        first_dict.get(b"MediaBox").and_then(Object::as_array),
        Some(expected_media_box.as_slice())
    );
    assert!(
        stats.total() > before_leaf,
        "the unvalidated leaf must be read"
    );
    let after_leaf = stats.total();
    let cached = document.get(3).expect("first leaf is cached");
    assert_eq!(cached, first);
    assert_eq!(stats.total(), after_leaf);
    let repeated_first = document.first_page().expect("first page resolves again");
    assert_eq!(repeated_first, first);
    let repeated = document.get(3).expect("first leaf remains cached");
    assert_eq!(repeated, first);
    assert_eq!(stats.total(), after_leaf);
}

#[test]
fn edits_and_deletions_supersede_validated_dictionaries() {
    let bytes = classic_pdf(&skeleton(), &[]);
    let mut edited =
        Document::open(Box::new(BytesSource::new(bytes.clone()))).expect("fixture opens clean");
    let mut catalog = edited.catalog().expect("catalog resolves");
    catalog.set("Marker", Object::name("Edited"));
    edited
        .set_object(1, 0, Object::Dict(catalog))
        .expect("catalog can be replaced");
    let parsed = edited.get(1).expect("edited catalog resolves");
    assert_eq!(parsed.origin, Origin::Pending);
    assert_eq!(parsed.recovered_boundary, None);
    assert_eq!(
        parsed
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Marker"))
            .and_then(Object::as_name)
            .map(|name| name.as_bytes()),
        Some(&b"Edited"[..])
    );

    let mut deleted =
        Document::open(Box::new(BytesSource::new(bytes))).expect("fixture opens clean");
    deleted.delete_object(2).expect("Pages can be deleted");
    match deleted.get(2) {
        Err(Error::MissingObject(objref)) => assert_eq!(objref, ObjRef::new(2, 1)),
        other => panic!("deleted Pages must stay missing, got {other:?}"),
    }
}

#[test]
fn compressed_structural_objects_remain_lazy() {
    for object_number in [1u32, 2] {
        let mut bytes = common::xref_stream_pdf(None);
        let marker = bytes
            .windows(b"\nstream\n".len())
            .position(|window| window == b"\nstream\n")
            .expect("xref stream marker");
        let rows_at = marker + b"\nstream\n".len();
        let row_at = rows_at + object_number as usize * 4;
        bytes[row_at..row_at + 4].copy_from_slice(&[2, 0, 3, 0]);

        let document = Document::open(Box::new(BytesSource::new(bytes)))
            .expect("structural validation only reaches compressed container");
        match document.get(object_number) {
            Err(Error::Unrecoverable { detail }) => assert!(
                detail.contains("object 3 is referenced as an object stream but is not a stream"),
                "unexpected compressed access error: {detail}"
            ),
            other => panic!("compressed structural object must stay undecoded, got {other:?}"),
        }
    }
}
