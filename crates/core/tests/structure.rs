//! The structure tree, from the outside.
//!
//! Two kinds of test here, and they answer different questions.
//!
//! The **fixture tests** ask whether the reader agrees with an independent
//! implementation about what is in a real tagged document. The **hand-built
//! tests** ask whether the invariant and the maintenance hook do their job,
//! which needs documents whose structure is the only variable and which run on
//! a fresh clone where `corpus/external/` is absent.
//!
//! The one that matters most is
//! `the_invariant_fails_on_a_page_removed_without_the_hook`. An invariant that
//! passes on a tree the edit destroyed is worth nothing, and the M2 audit's
//! guarantee-6 lesson is that this has to be demonstrated against a
//! known-broken input in the same suite rather than argued.

use std::collections::BTreeMap;
use std::path::PathBuf;

use onionskin_core::{
    attach_annotation, check, read_structure, reorder_pages, EditSession, Kid, Maintenance,
    Structure, Violation,
};
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Document as CosDocument, Name, ObjRef, Object};

// ---------------------------------------------------------------------------
// Fixture expectations
// ---------------------------------------------------------------------------

/// Expected element counts and page-to-element mappings for four tagged
/// fixtures from the veraPDF corpus.
///
/// **Provenance.** veraPDF itself is a Java tool and is not available in this
/// workspace, so the oracle here is **pikepdf 10.5.1**, which is a binding over
/// qpdf and an implementation entirely independent of anything in this
/// repository. `corpus/tagged/derive.py` is the script that produced these
/// numbers and re-derives them on demand; it walks `/StructTreeRoot` `/K`
/// depth-first with a seen-set, counts every kid that is a dictionary and is
/// not an `/MCR` or `/OBJR`, and maps each element's `/Pg` to a page index.
/// The reader under test has to agree with that walk, having been written from
/// the specification rather than from the script.
///
/// `(fixture, elements, pages, per-page counts as (page index, elements))`.
type Expectation = (&'static str, usize, usize, &'static [(usize, usize)]);

const FIXTURES: &[Expectation] = &[
    (
        "Isartor test files/doc/Isartor test suite manual.pdf",
        374,
        20,
        &[
            (1, 13),
            (2, 28),
            (3, 30),
            (4, 29),
            (5, 6),
            (6, 31),
            (7, 31),
            (8, 13),
            (9, 23),
            (10, 13),
            (11, 30),
            (12, 23),
            (13, 2),
        ],
    ),
    ("PDF_UA-1/7.2 Text/7.2-t27-pass-a.pdf", 32, 1, &[(0, 22)]),
    ("PDF_UA-1/7.2 Text/7.2-t15-pass-a.pdf", 23, 1, &[(0, 22)]),
    (
        "PDF_UA-2/8.2 Logical structure/8.2.5 Additional requirements for specific structure types/8.2.5.26 Table (Table, TR, TH, TD, THead, TBody, TFoot)/8.2.5.26-t01-pass-a.pdf",
        23,
        1,
        &[(0, 22)],
    ),
];

fn verapdf_root() -> Option<PathBuf> {
    let root = onionskin_corpus_testing::corpus_dir("external/verapdf")?;
    root.is_dir().then_some(root)
}

/// The reader agrees with pikepdf about every fixture, named by filename so a
/// failure says which document disagreed.
#[test]
fn the_reader_recovers_each_fixture_element_count_and_page_mapping() {
    let Some(root) = verapdf_root() else {
        eprintln!("SKIPPED: corpus/external/verapdf is absent; fetch it with corpus/fetch.sh");
        return;
    };

    for (fixture, elements, pages, per_page) in FIXTURES {
        let path = root.join(fixture);
        assert!(path.is_file(), "{fixture} is missing from the fetched set");
        let doc = CosDocument::open_path(&path).unwrap_or_else(|e| panic!("{fixture} opens: {e}"));
        let structure = read_structure(&doc).unwrap_or_else(|e| panic!("{fixture} reads: {e}"));
        let tree = structure
            .tree()
            .unwrap_or_else(|| panic!("{fixture} is tagged"));

        assert_eq!(
            tree.elements.len(),
            *elements,
            "{fixture}: element count disagrees with pikepdf"
        );

        let page_count = doc.page_count().expect("page count") as usize;
        assert_eq!(page_count, *pages, "{fixture}: page count");

        let mut index_of = BTreeMap::new();
        for index in 0..page_count {
            index_of.insert(doc.page(index).expect("page resolves").objref.number, index);
        }
        let mut counted: BTreeMap<usize, usize> = BTreeMap::new();
        for element in tree.elements.values() {
            let Some(page) = element.page else { continue };
            let Some(index) = index_of.get(&page.number) else {
                continue;
            };
            *counted.entry(*index).or_default() += 1;
        }
        let expected: BTreeMap<usize, usize> = per_page.iter().copied().collect();
        assert_eq!(
            counted, expected,
            "{fixture}: page-to-element mapping disagrees with pikepdf"
        );
    }
}

// ---------------------------------------------------------------------------
// The invariant is not vacuous
// ---------------------------------------------------------------------------

/// **The test that proves the invariant means something.** A page removed from
/// the page tree without the maintenance hook leaves a surviving element whose
/// `/Pg` names a page that is gone, and nothing in any `/K` references that
/// page. A `/K`-only invariant passes here.
#[test]
fn the_invariant_fails_on_a_page_removed_without_the_hook() {
    let original = tagged_two_pages();
    let base = open(&original);
    let mut edit = EditSession::for_base(&base);

    edit.transact(&base, "Remove Page", remove_second_page)
        .expect("the removal commits");

    let after = open(&append(&original, section(&base, &edit)));
    let structure = read_structure(&after).expect("the tree still reads");
    let report = check(&after, &structure, 1).expect("the invariant runs");

    assert!(
        report
            .violations
            .iter()
            .any(|v| matches!(v, Violation::ElementPageMissing { .. })),
        "an element still names the removed page, and the invariant has to say so: {:?}",
        report.violations
    );
}

/// The same removal with the hook, which is what makes the failure above a
/// statement about the hook rather than about page removal in general.
#[test]
fn the_hook_keeps_the_tree_consistent_through_a_page_removal() {
    let original = tagged_two_pages();
    let base = open(&original);
    let structure = read_structure(&base).expect("the tree reads");
    let mut edit = EditSession::for_base(&base);

    let outcome = edit
        .transact(&base, "Remove Page", |tx| {
            let done = onionskin_core::remove_page(tx, &structure, ObjRef::new(4, 0))?;
            remove_second_page(tx)?;
            Ok(done)
        })
        .expect("the removal commits");
    assert_eq!(outcome, Maintenance::Changed, "the hook rewrote the tree");

    let after = open(&append(&original, section(&base, &edit)));
    let structure = read_structure(&after).expect("the tree still reads");
    let report = check(&after, &structure, 1).expect("the invariant runs");

    assert!(
        report.is_clean(),
        "the hook has to leave the tree consistent: {:?}",
        report.violations
    );
    let surviving = structure
        .tree()
        .and_then(|tree| tree.elements.get(&6))
        .expect("page 1's element stays");
    assert_eq!(
        surviving.kids,
        [Kid::Mcid(0)],
        "an element on a page that stays keeps its marked content"
    );
}

// ---------------------------------------------------------------------------
// The untagged path
// ---------------------------------------------------------------------------

/// Every operation is a no-op **and says so**. The assertion is on the returned
/// [`Maintenance`], not inferred from the document being unchanged, because
/// "nothing to do" and "done" must not look alike to a caller.
#[test]
fn an_untagged_document_takes_the_untagged_path_and_reports_it() {
    let base = CosDocument::open_path(&seed("minimal.pdf")).expect("the seed opens");
    let structure = read_structure(&base).expect("reading an untagged document is not an error");
    assert!(
        !structure.is_tagged(),
        "minimal.pdf carries no structure tree"
    );
    assert!(matches!(structure, Structure::Untagged));

    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Untagged", |tx| {
        assert_eq!(
            onionskin_core::remove_page(tx, &structure, ObjRef::new(3, 0))?,
            Maintenance::Untagged
        );
        assert_eq!(
            reorder_pages(tx, &structure, &[ObjRef::new(3, 0)])?,
            Maintenance::Untagged
        );
        let (done, key) = attach_annotation(tx, &structure, ObjRef::new(3, 0), ObjRef::new(9, 0))?;
        assert_eq!(done, Maintenance::Untagged);
        assert_eq!(key, None, "an untagged document hands out no parent key");
        Ok(())
    })
    .expect("the transaction commits");

    assert!(
        edit.pending_edits().is_empty(),
        "an untagged document is not written to"
    );
    let report = check(&base, &structure, 1).expect("the invariant runs");
    assert!(report.is_clean(), "an untagged document passes trivially");
}

/// Untagged and tagged-but-unreadable are different answers. A
/// `/StructTreeRoot` the file cannot back is an error, because treating it as
/// untagged would skip structure maintenance on a document that claims to have
/// a tree.
#[test]
fn a_structure_root_that_is_not_a_dictionary_is_an_error_not_untagged() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
        b"42".to_vec(),
    ]);
    let doc = open(&bytes);
    assert!(
        read_structure(&doc).is_err(),
        "a /StructTreeRoot that is not a dictionary is a file contradicting itself"
    );
}

// ---------------------------------------------------------------------------
// Hostile shapes terminate
// ---------------------------------------------------------------------------

#[test]
fn a_cyclic_k_terminates() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
        b"<< /Type /StructElem /S /P /Pg 3 0 R /K [6 0 R] >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [4 0 R] >>".to_vec(),
        b"<< /Type /StructElem /S /P /Pg 3 0 R /K [4 0 R] >>".to_vec(),
    ]);
    let doc = open(&bytes);
    let structure = read_structure(&doc).expect("a cycle is walked once, not forever");
    let tree = structure.tree().expect("tagged");
    assert_eq!(
        tree.elements.len(),
        2,
        "each element is read once and the cycle closes"
    );
}

/// `/Nums` is required to be sorted and producers get it wrong. The reader does
/// not rely on the order, so an unsorted tree reads completely rather than
/// stopping at the first key that goes backwards.
#[test]
fn an_unsorted_parent_tree_reads_completely_and_terminates() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 0 >>"
            .to_vec(),
        b"<< /Type /StructElem /S /P /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [4 0 R] /ParentTree << /Nums [7 [4 0 R] 3 [4 0 R] 0 [4 0 R]] >> >>".to_vec(),
    ]);
    let doc = open(&bytes);
    let structure = read_structure(&doc).expect("an unsorted /Nums still reads");
    let tree = structure.tree().expect("tagged");
    assert_eq!(
        tree.parent_tree.keys().copied().collect::<Vec<_>>(),
        vec![0, 3, 7],
        "every pair is read, in key order, whatever order the file wrote them"
    );
}

// ---------------------------------------------------------------------------
// The other two operations
// ---------------------------------------------------------------------------

#[test]
fn reordering_rewrites_the_root_sequence_to_match_the_new_page_order() {
    let original = tagged_two_pages();
    let base = open(&original);
    let structure = read_structure(&base).expect("the tree reads");
    let mut edit = EditSession::for_base(&base);

    let outcome = edit
        .transact(&base, "Reorder Pages", |tx| {
            reorder_pages(tx, &structure, &[ObjRef::new(4, 0), ObjRef::new(3, 0)])
        })
        .expect("the reorder commits");
    assert_eq!(outcome, Maintenance::Changed);

    let after = open(&append(&original, section(&base, &edit)));
    let structure = read_structure(&after).expect("the tree still reads");
    let tree = structure.tree().expect("tagged");
    assert_eq!(
        tree.roots,
        vec![
            onionskin_core::Kid::Element(7),
            onionskin_core::Kid::Element(6)
        ],
        "the root sequence follows the pages"
    );
    assert!(
        check(&after, &structure, 2)
            .expect("the invariant runs")
            .is_clean(),
        "a reorder leaves the tree consistent"
    );
}

#[test]
fn attaching_an_annotation_takes_the_next_free_parent_key() {
    let original = tagged_two_pages();
    let base = open(&original);
    let structure = read_structure(&base).expect("the tree reads");
    let mut edit = EditSession::for_base(&base);

    let key = edit
        .transact(&base, "Attach Annotation", |tx| {
            let annotation = tx.reserve();
            let mut annot = onionskin_cos::Dict::new();
            annot.set(Name::new("Type"), Object::name("Annot"));
            annot.set(Name::new("Subtype"), Object::name("Text"));
            tx.put_object(annotation, 0, Object::Dict(annot))?;
            let (done, key) = attach_annotation(
                tx,
                &structure,
                ObjRef::new(3, 0),
                ObjRef::new(annotation, 0),
            )?;
            assert_eq!(done, Maintenance::Changed);
            Ok(key)
        })
        .expect("the attach commits");

    assert_eq!(
        key,
        Some(2),
        "the fixture states /ParentTreeNextKey 2, and the keys in use are 0 and 1"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A two-page tagged document with one structure element per page, a
/// `/ParentTree` naming both, and an `/IDTree` naming both. Object numbers are
/// written by hand so a test can address page 2 as `4 0 R` and its element as
/// `7 0 R`.
fn tagged_two_pages() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 0 >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 1 >>"
            .to_vec(),
        b"<< /Type /StructTreeRoot /K [6 0 R 7 0 R] /ParentTree 8 0 R /ParentTreeNextKey 2 /IDTree 9 0 R >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 /ID (elem-one) >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 5 0 R /Pg 4 0 R /K 0 /ID (elem-two) >>".to_vec(),
        b"<< /Nums [0 [6 0 R] 1 [7 0 R]] >>".to_vec(),
        b"<< /Names [(elem-one) 6 0 R (elem-two) 7 0 R] >>".to_vec(),
    ])
}

/// Drop page 2 from the page tree. P5 owns the real page transformation; this
/// is the smallest thing that removes a page, which is all the structure tests
/// need it to be.
fn remove_second_page(tx: &mut onionskin_core::Transaction<'_>) -> onionskin_core::Result<()> {
    let mut pages = onionskin_cos::Dict::new();
    pages.set(Name::new("Type"), Object::name("Pages"));
    pages.set(
        Name::new("Kids"),
        Object::Array(vec![Object::Ref(ObjRef::new(3, 0))]),
    );
    pages.set(Name::new("Count"), Object::Integer(1));
    tx.put_object(2, 0, Object::Dict(pages))
}

fn section(base: &CosDocument, edit: &EditSession) -> Option<Vec<u8>> {
    base.section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
}

fn append(bytes: &[u8], section: Option<Vec<u8>>) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if let Some(section) = section {
        out.extend_from_slice(&section);
    }
    out
}

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

/// The same assembler `crates/core/src/testpdf.rs` uses, restated because a
/// crate's test helpers are not visible to its integration tests.
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

/// A document element with no page of its own, over a paragraph on each of
/// two pages. Deleting page 1 keeps the document element, ranked by the
/// page that stays, with page 2's paragraph and its marked content.
#[test]
fn deleting_a_page_keeps_an_element_that_spans_it_and_a_page_that_stays() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 100] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << >> /StructParents 0 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << >> /StructParents 1 >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [6 0 R] /ParentTree << /Nums [0 [7 0 R] 1 [8 0 R]] >> /ParentTreeNextKey 2 >>".to_vec(),
        b"<< /Type /StructElem /S /Document /P 5 0 R /K [7 0 R 8 0 R] >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 4 0 R /K 0 >>".to_vec(),
    ]);
    let mut doc = onionskin_core::Document::open_bytes(bytes).expect("opens");
    doc.edit_annotations("Delete Pages", |tx, structure| {
        onionskin_core::pages::delete_pages(tx, structure, &[0]).map(|_| ())
    })
    .expect("deletes");
    let after = doc.structure().expect("reads");
    let structure = read_structure(after).expect("the tree reads");
    let tree = structure.tree().expect("still tagged");
    assert_eq!(tree.roots, [Kid::Element(6)], "the document element stays");
    assert_eq!(tree.elements[&6].kids, [Kid::Element(8)]);
    assert_eq!(tree.elements[&8].kids, [Kid::Mcid(0)]);
    let report = check(after, &structure, 1).expect("the invariant runs");
    assert!(report.is_clean(), "{:?}", report.violations);
}

/// A section on page 1 holding a paragraph on each page. Deleting page 1
/// keeps the section for the paragraph on page 2, and drops the page it no
/// longer has.
#[test]
fn a_parent_on_a_deleted_page_stays_for_a_child_that_does() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 100] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << >> /StructParents 0 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << >> /StructParents 1 >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [6 0 R] /ParentTree << /Nums [0 [7 0 R] 1 [8 0 R]] >> /ParentTreeNextKey 2 >>".to_vec(),
        b"<< /Type /StructElem /S /Sect /P 5 0 R /Pg 3 0 R /K [7 0 R 8 0 R] >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 6 0 R /Pg 4 0 R /K 0 >>".to_vec(),
    ]);
    let mut doc = onionskin_core::Document::open_bytes(bytes).expect("opens");
    doc.edit_annotations("Delete Pages", |tx, structure| {
        onionskin_core::pages::delete_pages(tx, structure, &[0]).map(|_| ())
    })
    .expect("deletes");
    let after = doc.structure().expect("reads");
    let structure = read_structure(after).expect("the tree reads");
    let tree = structure.tree().expect("still tagged");
    assert_eq!(tree.roots, [Kid::Element(6)]);
    assert_eq!(
        tree.elements[&6].kids,
        [Kid::Element(8)],
        "page 1's paragraph goes"
    );
    assert_eq!(tree.elements[&6].page, None, "the section's page went");
    let report = check(after, &structure, 1).expect("the invariant runs");
    assert!(report.is_clean(), "{:?}", report.violations);
}
