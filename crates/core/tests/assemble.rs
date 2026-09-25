//! `core::pages::Assembly`: pages from many documents, in order, into one new
//! document - the primitive behind Combine, Split and Extract.
//!
//! Every assertion reads the output through a fresh parse. Page order is
//! checked by extracted text and every page by its render against its
//! source's, so an assembly that keeps the count and loses a font is caught.

use std::collections::BTreeSet;

use onionskin_core::pages::{Assembly, Tagging, Untagged};
use onionskin_core::protection::Refusal;
use onionskin_core::{check, read_structure, Document, Error};
use onionskin_corpus_testing::{encrypted_fixture, organize_fixture, seed};
use onionskin_cos::{BytesSource, Document as CosDocument, ObjRef, Object};

mod common;
use common::{flat, tagged};

/// Three committed documents, one of them with an embedded font, an image, a
/// cyclic form and an annotation: every page of the output is its source page,
/// by text and by pixels.
#[test]
fn a_combined_document_is_every_input_page_in_order_drawn_as_it_was() {
    let inputs = [
        std::fs::read(organize_fixture("embedded-font.pdf")).expect("read"),
        std::fs::read(seed("two-page.pdf")).expect("read"),
        flat(3),
    ];
    let mut assembly = Assembly::new();
    let mut expected = Vec::new();
    for (input, bytes) in inputs.iter().enumerate() {
        let source = open(bytes);
        let count = source.page_count().expect("pages") as usize;
        let landed = assembly
            .append(&source, &(0..count).collect::<Vec<_>>())
            .expect("appends");
        assert_eq!(landed.len(), count, "input {input}");
        expected.extend((0..count).map(|page| (input, page)));
    }
    let assembled = assembly.finish().expect("finishes");

    assert_eq!(assembled.page_count, 2 + 2 + 3, "the sum of the inputs");
    let output = open(&assembled.bytes);
    assert_eq!(output.page_count().expect("pages") as usize, expected.len());
    assert_eq!(output.audit_references().expect("audits"), Vec::new());

    let mut combined = Document::open_bytes(assembled.bytes.clone()).expect("opens");
    let mut sources: Vec<Document> = inputs
        .iter()
        .map(|bytes| Document::open_bytes(bytes.clone()).expect("opens"))
        .collect();
    for (index, (input, page)) in expected.into_iter().enumerate() {
        assert_eq!(
            text(&mut combined, index),
            text(&mut sources[input], page),
            "page {index} is input {input}'s page {page}"
        );
        let want = sources[input].render_page_now(page, 1.0).expect("renders");
        let got = combined.render_page_now(index, 1.0).expect("renders");
        assert_eq!(
            (got.raster.width(), got.raster.height()),
            (want.raster.width(), want.raster.height()),
            "page {index}: size"
        );
        assert!(
            got.raster.rgba() == want.raster.rgba(),
            "page {index} does not draw as input {input}'s page {page}"
        );
    }
}

/// The per-input shape of the encrypted-source rule. In position 2 of 3, where
/// a check written at the wrong loop level - once, on the first input - lets it
/// through.
#[test]
fn an_encrypted_input_anywhere_in_the_list_is_refused_before_anything_is_copied() {
    let encrypted =
        open(&std::fs::read(encrypted_fixture("r6-aes-256-print-only.pdf")).expect("read"));
    let plain = open(&flat(2));
    let mut assembly = Assembly::new();
    assembly.append(&plain, &[0, 1]).expect("the first input");
    let refused = assembly.append(&encrypted, &[0]);
    assert!(
        matches!(refused, Err(Error::Protected(Refusal::EncryptedSource))),
        "{:?}",
        refused.err()
    );
    assert_eq!(
        assembly.page_count(),
        2,
        "nothing of the refused input came"
    );
}

/// Legal, and the second copy owns its objects: an alias would make an edit to
/// one page's content an edit to the other's.
#[test]
fn a_document_combined_with_itself_gets_independent_copies() {
    let source = open(&std::fs::read(organize_fixture("embedded-font.pdf")).expect("read"));
    let mut assembly = Assembly::new();
    assembly.append(&source, &[0, 1]).expect("appends");
    assembly.append(&source, &[0, 1]).expect("appends again");
    let output = open(&assembly.finish().expect("finishes").bytes);

    assert_eq!(output.page_count().expect("pages"), 4);
    let first = reachable(&output, output.page(0).expect("page").objref);
    let again = reachable(&output, output.page(2).expect("page").objref);
    assert!(
        first.len() >= 8,
        "the page reaches its font, image and form"
    );
    assert!(
        first.is_disjoint(&again),
        "the two copies share objects: {:?}",
        first.intersection(&again).collect::<Vec<_>>()
    );
}

#[test]
fn tagged_inputs_make_a_tagged_output_whose_structure_is_valid() {
    let mut assembly = Assembly::new();
    for _ in 0..2 {
        assembly
            .append(&open(&tagged()), &[0, 1, 2])
            .expect("appends");
    }
    let assembled = assembly.finish().expect("finishes");
    assert_eq!(assembled.tagging, Tagging::Tagged);

    let output = open(&assembled.bytes);
    let structure = read_structure(&output).expect("the structure reads");
    let tree = structure.tree().expect("a structure tree");
    assert_eq!(tree.roots.len(), 6, "both inputs' paragraphs, in order");
    let report = check(&output, &structure, 6).expect("check runs");
    assert!(report.violations.is_empty(), "{:?}", report.violations);

    let keys: BTreeSet<i64> = (0..6)
        .map(|index| {
            output
                .page(index)
                .expect("page")
                .dict
                .get(b"StructParents")
                .and_then(Object::as_integer)
                .expect("each page keeps a /StructParents")
        })
        .collect();
    assert_eq!(keys.len(), 6, "no two pages share a /ParentTree key");
    assert_eq!(tree.parent_tree_next_key, Some(6));
}

/// Half-tagged is the failure: a reading order that silently skips the pages
/// of the untagged input. The output says which input made it untagged.
#[test]
fn one_untagged_input_makes_the_output_untagged_and_says_which() {
    let mut assembly = Assembly::new();
    assembly
        .append(&open(&tagged()), &[0, 1, 2])
        .expect("appends");
    assembly.append(&open(&flat(1)), &[0]).expect("appends");
    let assembled = assembly.finish().expect("finishes");
    assert_eq!(
        assembled.tagging,
        Tagging::Untagged(Untagged::InputUntagged { input: 1 })
    );
    assert_untagged(&assembled.bytes);
}

#[test]
fn part_of_a_tagged_input_makes_the_output_untagged() {
    let mut assembly = Assembly::new();
    assembly.append(&open(&tagged()), &[0, 2]).expect("appends");
    let assembled = assembly.finish().expect("finishes");
    assert_eq!(
        assembled.tagging,
        Tagging::Untagged(Untagged::PartialInput { input: 0 })
    );
    assert_untagged(&assembled.bytes);
}

#[test]
fn untagged_inputs_make_an_untagged_output() {
    let mut assembly = Assembly::new();
    assembly.append(&open(&flat(2)), &[0, 1]).expect("appends");
    let tagging = assembly.finish().expect("finishes").tagging;
    assert_eq!(
        tagging,
        Tagging::Untagged(Untagged::InputUntagged { input: 0 })
    );
}

/// Fresh, and stated: nothing from any input's `/Info` is carried.
#[test]
fn the_output_has_fresh_metadata_naming_only_the_producer() {
    let source = open(&std::fs::read(organize_fixture("embedded-font.pdf")).expect("read"));
    assert!(
        source.trailer().get(b"Info").is_some(),
        "the source has metadata of its own to not carry"
    );
    let mut assembly = Assembly::new();
    assembly.append(&source, &[0]).expect("appends");
    let output = open(&assembly.finish().expect("finishes").bytes);
    let info = output
        .trailer()
        .get(b"Info")
        .and_then(Object::as_reference)
        .expect("an /Info");
    let Object::Dict(info) = output.get(info.number).expect("present").object else {
        panic!("/Info is a dictionary");
    };
    let keys: Vec<&[u8]> = info.iter().map(|(key, _)| key.as_bytes()).collect();
    assert_eq!(keys, [b"Producer".as_slice()]);
}

#[test]
fn an_assembly_with_no_pages_is_refused() {
    assert!(matches!(
        Assembly::new().finish(),
        Err(Error::WouldLeaveNoPages)
    ));
}

#[test]
fn a_page_the_source_does_not_have_is_refused() {
    let refused = Assembly::new().append(&open(&flat(2)), &[2]);
    assert!(matches!(
        refused,
        Err(Error::NoSuchPage { page: 2, count: 2 })
    ));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn text(document: &mut Document, page: usize) -> String {
    document
        .page_text(page)
        .expect("text extracts")
        .runs
        .iter()
        .map(|run| run.decoded_text.as_str())
        .collect::<String>()
}

fn assert_untagged(bytes: &[u8]) {
    let output = open(bytes);
    let catalog = output.catalog().expect("catalog");
    assert!(catalog.get(b"StructTreeRoot").is_none());
    assert!(catalog.get(b"MarkInfo").is_none());
    for index in 0..output.page_count().expect("pages") as usize {
        assert!(
            output
                .page(index)
                .expect("page")
                .dict
                .get(b"StructParents")
                .is_none(),
            "page {index} still indexes a /ParentTree nobody wrote"
        );
    }
    assert_eq!(output.audit_references().expect("audits"), Vec::new());
    let elements: Vec<u32> = (1..output.next_object_number())
        .filter(|number| {
            output.get(*number).is_ok_and(|parsed| {
                parsed
                    .object
                    .as_dict()
                    .and_then(|dict| dict.get(b"Type"))
                    .and_then(Object::as_name)
                    .is_some_and(|name| name.as_bytes() == b"StructElem")
            })
        })
        .collect();
    assert!(
        elements.is_empty(),
        "structure elements left behind with no tree to hold them: {elements:?}"
    );
}

/// Every object reachable from `from`, not following `/Parent`.
fn reachable(document: &CosDocument, from: ObjRef) -> BTreeSet<u32> {
    let mut seen = BTreeSet::new();
    let mut queue = vec![from.number];
    while let Some(number) = queue.pop() {
        if !seen.insert(number) {
            continue;
        }
        if let Ok(parsed) = document.get(number) {
            refs(&parsed.object, &mut queue);
        }
    }
    seen
}

fn refs(object: &Object, into: &mut Vec<u32>) {
    match object {
        Object::Ref(objref) => into.push(objref.number),
        Object::Array(items) => items.iter().for_each(|item| refs(item, into)),
        Object::Dict(dict) => dict
            .iter()
            .filter(|(key, _)| key.as_bytes() != b"Parent")
            .for_each(|(_, value)| refs(value, into)),
        Object::Stream(stream) => stream.dict.iter().for_each(|(_, value)| refs(value, into)),
        _ => {}
    }
}
