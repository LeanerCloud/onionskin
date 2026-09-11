//! The two reference checks, and the difference between them.
//!
//! `Document::audit_references` is the complete one: O(file), a query, and
//! what a package's verification runs over its fixtures. The gate inside
//! `section_for` is the cheap one, bounded by the edit. They share one walk,
//! and the one thing the gate cannot see is asserted here as a test rather
//! than left as a caveat.

mod common;

use std::collections::BTreeMap;

use common::{classic_pdf, corpus_dir, pdfs_in, skeleton, Tally};
use onionskin_cos::{
    BytesSource, Dangling, Document, Error, FileSource, Holder, Name, ObjRef, Object, PendingEdit,
};

fn open(bytes: &[u8]) -> Document {
    Document::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the fixture opens clean")
}

/// The skeleton, plus object 4 (which nothing in the page tree names) and
/// object 5, which points at 4 and is not rewritten by anything below.
fn referrer_and_target() -> Vec<u8> {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    bodies.push(b"<</Type/Referrer/Points 4 0 R>>");
    classic_pdf(&bodies, &[])
}

fn referrer(points: ObjRef) -> Object {
    let mut dict = onionskin_cos::Dict::new();
    dict.set("Type", Object::name("Referrer"));
    dict.set("Points", Object::Ref(points));
    Object::Dict(dict)
}

fn with_xref_generation(mut bytes: Vec<u8>, number: usize, generation: u16) -> Vec<u8> {
    let marker = b" 00000 n \n";
    let at = bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(|(index, window)| (window == marker).then_some(index))
        .nth(number - 1)
        .expect("the requested in-use xref row exists");
    let generation = format!("{generation:05}");
    bytes[at + 1..at + 6].copy_from_slice(generation.as_bytes());
    bytes
}

fn malformed_info_fixture() -> Vec<u8> {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Producer (old)>>");
    let mut bytes = classic_pdf(&bodies, &[]);
    let marker = b">>\nstartxref";
    let at = bytes
        .windows(marker.len())
        .rposition(|window| window == marker)
        .expect("the trailer is present");
    bytes.splice(at..at, b"/Info 4 1 R".iter().copied());
    bytes
}

#[test]
fn a_document_whose_references_all_resolve_reports_nothing() {
    assert_eq!(
        open(&referrer_and_target())
            .audit_references()
            .expect("the walk completes"),
        Vec::new(),
        "every reference in the fixture resolves, so there is nothing to report"
    );
}

#[test]
fn a_deleted_target_is_reported_with_the_pair_that_dangles() {
    let mut document = open(&referrer_and_target());
    document.delete_object(4).expect("object 4 is deletable");
    let saved = document.save_to_vec().expect("save");

    assert_eq!(
        open(&saved).audit_references().expect("the walk completes"),
        vec![Dangling {
            holder: Holder::Object(5),
            target: ObjRef::new(4, 0),
        }],
        "the one pair that dangles, and no other"
    );
}

/// The shallow-walk failure mode: a reference that is not a dictionary value
/// but sits inside an array inside an array inside a stream's dictionary.
#[test]
fn a_reference_buried_in_a_nested_array_in_a_stream_dictionary_is_found() {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Buried/Deep[1 0 R[[9 0 R]]]/Length 3>>\nstream\nabc\nendstream");
    let bytes = classic_pdf(&bodies, &[]);

    assert_eq!(
        open(&bytes).audit_references().expect("the walk completes"),
        vec![Dangling {
            holder: Holder::Object(4),
            target: ObjRef::new(9, 0),
        }],
        "the buried reference must be found, and the resolvable one beside it left alone"
    );
}

/// `0 0 R` is how a file writes a reference that resolves to null
/// (ISO 32000-1 7.3.10). Reporting it would make the walk noisy on ordinary
/// files rather than more correct.
#[test]
fn a_reference_to_object_zero_is_the_null_reference_and_not_a_dangling_one() {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Referrer/Points 0 0 R>>");
    assert_eq!(
        open(&classic_pdf(&bodies, &[]))
            .audit_references()
            .expect("the walk completes"),
        Vec::new()
    );
}

/// The honest statement of the gate's limit, as a test: an object already in
/// the file, which the section does not rewrite, pointing at a number the
/// section frees. Nothing bounded by the edit can see it, and the complete
/// walk over the result can.
#[test]
fn the_gate_accepts_what_the_audit_reports_and_that_is_the_difference_between_them() {
    let original = referrer_and_target();
    let document = open(&original);
    // Object 5 points at object 4 and this section does not write object 5.
    let overlay: BTreeMap<u32, PendingEdit> = [(4, PendingEdit::Delete { generation: 1 })]
        .into_iter()
        .collect();

    let section = document
        .section_for(&overlay, &BTreeMap::new())
        .expect("the gate accepts a section that frees a number none of its own bytes name")
        .expect("freeing an object is something to write");

    let mut saved = original.clone();
    saved.extend_from_slice(&section);
    assert_eq!(
        open(&saved).audit_references().expect("the walk completes"),
        vec![Dangling {
            holder: Holder::Object(5),
            target: ObjRef::new(4, 0),
        }],
        "the complete walk over the result reports what the gate could not see"
    );
}

#[test]
fn a_section_may_not_write_an_object_that_points_at_a_number_it_frees() {
    let document = open(&referrer_and_target());
    let mut rewritten = onionskin_cos::Dict::new();
    rewritten.set("Type", Object::name("Referrer"));
    rewritten.set("Points", Object::Ref(ObjRef::new(4, 0)));
    let overlay: BTreeMap<u32, PendingEdit> = [
        (4, PendingEdit::Delete { generation: 1 }),
        (
            5,
            PendingEdit::Set {
                generation: 0,
                object: Object::Dict(rewritten),
            },
        ),
    ]
    .into_iter()
    .collect();

    match document.section_for(&overlay, &BTreeMap::new()) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Object(5));
            assert_eq!(target.number, 4);
        }
        other => panic!("the section names a number it frees, so it must be refused: {other:?}"),
    }
}

/// The trailer is the one thing a section emits that is not one of its
/// objects, so a gate scoped to "the objects this section writes" would let
/// this through. It is what an undone "set a Description on a file with no
/// /Info" produces.
#[test]
fn a_trailer_key_the_section_sets_may_not_name_an_object_nobody_writes() {
    let document = open(&referrer_and_target());
    let trailer_edits: BTreeMap<Name, Option<Object>> =
        [(Name::new("Info"), Some(Object::Ref(ObjRef::new(100, 0))))]
            .into_iter()
            .collect();

    match document.section_for(&BTreeMap::new(), &trailer_edits) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Trailer);
            assert_eq!(target.number, 100);
        }
        other => panic!("a trailer naming an object nobody writes must be refused: {other:?}"),
    }

    // The same key, with the object written in the same section, is the
    // ordinary case and must still work.
    let overlay: BTreeMap<u32, PendingEdit> = [(
        100,
        PendingEdit::Set {
            generation: 0,
            object: Object::Dict(onionskin_cos::Dict::new()),
        },
    )]
    .into_iter()
    .collect();
    assert!(document
        .section_for(&overlay, &trailer_edits)
        .expect("the gate accepts a trailer key whose object the section writes")
        .is_some());
}

#[test]
fn an_audit_reports_a_reference_with_the_wrong_generation() {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    bodies.push(b"<</Type/Referrer/Points 4 1 R>>");
    let bytes = classic_pdf(&bodies, &[]);

    assert_eq!(
        open(&bytes).audit_references().expect("the walk completes"),
        vec![Dangling {
            holder: Holder::Object(5),
            target: ObjRef::new(4, 1),
        }]
    );
}

#[test]
fn a_section_requires_the_base_target_generation_for_an_introduced_reference() {
    let document = open(&referrer_and_target());
    let wrong: BTreeMap<u32, PendingEdit> = [(
        5,
        PendingEdit::Set {
            generation: 0,
            object: referrer(ObjRef::new(4, 1)),
        },
    )]
    .into_iter()
    .collect();
    match document.section_for(&wrong, &BTreeMap::new()) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Object(5));
            assert_eq!(target, ObjRef::new(4, 1));
        }
        other => panic!("a generation-mismatched reference must be refused: {other:?}"),
    }

    let matching: BTreeMap<u32, PendingEdit> = [(
        5,
        PendingEdit::Set {
            generation: 0,
            object: referrer(ObjRef::new(4, 0)),
        },
    )]
    .into_iter()
    .collect();
    assert!(document
        .section_for(&matching, &BTreeMap::new())
        .expect("a matching generation resolves")
        .is_some());
}

#[test]
fn a_trailer_reference_requires_the_base_target_generation() {
    let document = open(&referrer_and_target());
    let wrong: BTreeMap<Name, Option<Object>> =
        [(Name::new("Info"), Some(Object::Ref(ObjRef::new(4, 1))))]
            .into_iter()
            .collect();
    match document.section_for(&BTreeMap::new(), &wrong) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Trailer);
            assert_eq!(target, ObjRef::new(4, 1));
        }
        other => panic!("a trailer generation mismatch must be refused: {other:?}"),
    }

    let matching: BTreeMap<Name, Option<Object>> =
        [(Name::new("Info"), Some(Object::Ref(ObjRef::new(4, 0))))]
            .into_iter()
            .collect();
    assert!(document
        .section_for(&BTreeMap::new(), &matching)
        .expect("a matching trailer generation resolves")
        .is_some());
}

#[test]
fn an_overlay_target_requires_its_supplied_generation() {
    let document = open(&referrer_and_target());
    let target = |generation| {
        (
            4,
            PendingEdit::Set {
                generation,
                object: Object::name("overlay target"),
            },
        )
    };
    let matching: BTreeMap<u32, PendingEdit> = [
        target(7),
        (
            5,
            PendingEdit::Set {
                generation: 0,
                object: referrer(ObjRef::new(4, 7)),
            },
        ),
    ]
    .into_iter()
    .collect();
    assert!(document
        .section_for(&matching, &BTreeMap::new())
        .expect("the supplied generation matches")
        .is_some());

    let wrong: BTreeMap<u32, PendingEdit> = [
        target(7),
        (
            5,
            PendingEdit::Set {
                generation: 0,
                object: referrer(ObjRef::new(4, 0)),
            },
        ),
    ]
    .into_iter()
    .collect();
    match document.section_for(&wrong, &BTreeMap::new()) {
        Err(Error::DanglingReference { target, .. }) => {
            assert_eq!(target, ObjRef::new(4, 0));
        }
        other => panic!("the wrong overlay generation must be refused: {other:?}"),
    }
}

#[test]
fn an_inherited_malformed_info_reference_stays_unchanged_on_unrelated_save() {
    let original = malformed_info_fixture();
    let mut document = open(&original);
    assert_eq!(
        document.trailer().get(b"Info"),
        Some(&Object::Ref(ObjRef::new(4, 1)))
    );
    document.set_trailer_entry("Producer", Object::String(b"new metadata".to_vec()));
    let saved = document
        .save_to_vec()
        .expect("unrelated metadata remains writable");
    assert_eq!(&saved[..original.len()], &original[..]);

    let reopened = open(&saved);
    assert_eq!(
        reopened.trailer().get(b"Info"),
        Some(&Object::Ref(ObjRef::new(4, 1))),
        "the inherited malformed reference is not rewritten"
    );
    assert_eq!(
        reopened.trailer().get(b"Producer"),
        Some(&Object::String(b"new metadata".to_vec()))
    );
}

#[test]
fn an_xref_generation_mismatch_remains_advisory_when_reading() {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    let bytes = with_xref_generation(classic_pdf(&bodies, &[]), 4, 7);
    let document = open(&bytes);
    let parsed = document
        .get(4)
        .expect("the mismatched row remains readable");
    assert_eq!(parsed.objref, ObjRef::new(4, 0));
    assert_eq!(
        parsed.object,
        Object::Dict({
            let mut dict = onionskin_cos::Dict::new();
            dict.set("Type", Object::name("Spare"));
            dict.set("Which", Object::Integer(4));
            dict
        })
    );
}

/// One named set rather than the whole corpus: this walk is O(file) and the
/// external sets run to thousands of files, so an unbounded sweep would put a
/// new full-file walk over every one of them on every CI run. The 70 files of
/// `external/pdf-association` audit in about 30 milliseconds, which is what
/// makes this set the size it is.
///
/// A floor rather than an exact count, so a corpus refresh that adds files
/// does not fail the build for adding them. It is not 100%: one file in the
/// set, a deliberately odd `safedocs` dialect fixture, really does name an
/// object it does not contain, and a walk that called that clean would be
/// wrong about it.
///
/// A file the reader cannot produce an object for at all is counted apart. An
/// unsupported filter is not a dangling reference, and putting it in the same
/// bucket would let a real finding hide behind a parse failure.
#[test]
fn the_named_pdf_association_sample_audits_clean_to_its_floor() {
    let Some(dir) = corpus_dir("external/pdf-association") else {
        return;
    };
    let files = pdfs_in(&dir);
    assert!(!files.is_empty(), "the named sample holds no PDFs");

    let mut tally = Tally::new("external/pdf-association, reference audit");
    for path in &files {
        let source = match FileSource::open(path) {
            Ok(source) => source,
            Err(e) => {
                tally.skip(path, e.category());
                continue;
            }
        };
        let document = match Document::open_repairing(Box::new(source)) {
            Ok((document, provenance)) if provenance.is_clean() => document,
            Ok(_) => {
                tally.skip(path, "needed-repair");
                continue;
            }
            Err(e) => {
                tally.skip(path, e.category());
                continue;
            }
        };
        match document.audit_references() {
            Ok(dangling) if dangling.is_empty() => tally.pass(path),
            Ok(mut dangling) => {
                let count = dangling.len();
                dangling.truncate(3);
                tally.fail(
                    path,
                    "dangling",
                    &format!("{count} references resolve to nothing: {dangling:?}"),
                )
            }
            Err(e) => tally.skip(path, e.category()),
        }
    }
    tally.report();

    let considered = tally.passed.len() + tally.failure_count();
    assert!(
        considered >= 50,
        "only {considered} files were audited, so this walk proved nothing"
    );
    assert!(
        tally.passed.len() * 100 >= considered * 95,
        "{} of {considered} audited clean, below the 95% floor",
        tally.passed.len()
    );
}
