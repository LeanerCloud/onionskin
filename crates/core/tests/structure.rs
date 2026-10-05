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

/// One page with five elements that between them exercise every enrichment:
/// a custom type two hops from `Sect`, a `/Figure` with `/Alt`, UTF-16 and
/// PDFDocEncoding text strings, a `/Lang`, `/A` as a dictionary and as an array
/// with a revision number and a non-dictionary entry, and a type no role map
/// reaches.
fn enriched() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
        b"<< /Type /Font >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 15 0 R 16 0 R 18 0 R 21 0 R 22 0 R 23 0 R] \
          /RoleMap 19 0 R >>"
            .to_vec(),
        b"<< /S /Section /P 5 0 R /Pg 3 0 R /Lang (en-GB) /T <FEFF00540069006500740065> >>"
            .to_vec(),
        b"<< /S /Figure /P 5 0 R /Pg 3 0 R /Alt 11 0 R /ActualText <FEFF00E90021> \
          /E (Expanded) /A << /O /Layout /Placement /Block >> >>"
            .to_vec(),
        b"<< /S /Table /P 5 0 R /A [ 13 0 R 3 (not a dict) \
          << /O /Layout /BBox [0 0 1 1] >> ] >>"
            .to_vec(),
        b"<< /S /Loop /P 5 0 R >>".to_vec(),
        b"<< /S /Nothing /P 5 0 R /A 14 0 R >>".to_vec(),
        b"(A \\(small\\) cat)".to_vec(),
        b"/Sect".to_vec(),
        b"<< /O /Table /RowSpan 2 >>".to_vec(),
        b"<< /O /Table /ColSpan 3 /Length 0 >>\nstream\n\nendstream".to_vec(),
        b"<< /S /Title /P 5 0 R /A 13 0 R >>".to_vec(),
        b"<< /S /P /P 5 0 R /Alt 99 0 R /A [99 0 R] /T 98 0 R /Lang (fr) >>".to_vec(),
        b"<< /Type /Namespace /NS (http://iso.org/pdf2/ssn) >>".to_vec(),
        b"<< /S /Title /P 5 0 R /NS 17 0 R >>".to_vec(),
        b"<< /Section /Chapter /Chapter 12 0 R /Loop /Loop2 /Loop2 /Loop /Bad 5 \
          /Title /P /Ghost 99 0 R >>"
            .to_vec(),
        b"[ << /O /Layout /Placement /Inline >> ]".to_vec(),
        b"<< /S /P /P 5 0 R /A 20 0 R >>".to_vec(),
        b"<< /S /P /P 5 0 R /A 20 0 R >>".to_vec(),
        b"<< /S /Title /P 5 0 R /NS << /Type /Namespace /NS (urn:example) >> >>".to_vec(),
    ])
}

fn enriched_tree() -> onionskin_core::StructureTree {
    let doc = open(&enriched());
    read_structure(&doc)
        .expect("the tree reads")
        .tree()
        .cloned()
        .expect("tagged")
}

#[test]
fn a_custom_type_resolves_through_the_role_map_and_is_still_reported_as_written() {
    let tree = enriched_tree();
    let section = &tree.elements[&6];
    assert_eq!(section.struct_type, Some(Name::new("Section")));
    assert_eq!(section.standard_type, Some(Name::new("Sect")));
    assert_eq!(
        tree.elements[&7].standard_type,
        Some(Name::new("Figure")),
        "a standard type resolves to itself"
    );
}

/// ISO 32000-2 14.7.4.2: an element with no `/NS` is in the default namespace,
/// which is the PDF 1.7 one, so `/Title` is a custom type there that a role map
/// may define (a PDF/UA-1 pass file maps it to `/P`) even in a file that
/// declares 2.0. The 2.0 types are standard for an element that names the 2.0
/// namespace.
#[test]
fn what_is_a_standard_type_follows_the_elements_namespace_not_the_files_version() {
    let standard_of = |bytes: &[u8], element: u32| {
        let doc = open(bytes);
        let structure = read_structure(&doc).expect("reads");
        structure.tree().expect("tagged").elements[&element]
            .standard_type
            .clone()
    };
    let mut two = enriched();
    two[..8].copy_from_slice(b"%PDF-2.0");
    for header in [enriched(), two] {
        assert_eq!(
            standard_of(&header, 15),
            Some(Name::new("P")),
            "no /NS: the 1.7 namespace, whatever the header says"
        );
        assert_eq!(
            standard_of(&header, 18),
            Some(Name::new("Title")),
            "/NS naming the 2.0 namespace"
        );
        assert_eq!(
            standard_of(&header, 23),
            Some(Name::new("P")),
            "/NS naming some other namespace"
        );
    }
}

/// ISO 32000-1 7.3.10: a reference to an object that is not there is null. An
/// optional, descriptive entry that points at one must cost that entry, not
/// the tree, because the tree is read on every structure-aware edit.
#[test]
fn a_dangling_reference_in_an_optional_entry_costs_the_entry_not_the_tree() {
    let tree = enriched_tree();
    let damaged = &tree.elements[&16];
    assert_eq!(damaged.alt, None);
    assert_eq!(damaged.title, None);
    assert!(damaged.attributes.is_empty());
    assert_eq!(
        damaged.lang.as_deref(),
        Some("fr"),
        "its other entries are kept"
    );
    assert!(!tree.role_map.contains_key(&Name::new("Ghost")));
}

#[test]
fn an_indirect_attribute_object_is_read_once_and_shared_and_a_stream_one_is_read() {
    let tree = enriched_tree();
    let from_table = &tree.elements[&8].attributes[0].entries;
    let from_title = &tree.elements[&15].attributes[0].entries;
    assert!(
        std::sync::Arc::ptr_eq(from_table, from_title),
        "one object, however many elements name it"
    );
    assert!(
        std::sync::Arc::ptr_eq(
            &tree.elements[&21].attributes[0].entries,
            &tree.elements[&22].attributes[0].entries
        ),
        "an indirect /A array is read once for every element that names it"
    );
    let stream = &tree.elements[&10].attributes;
    assert_eq!(stream.len(), 1);
    assert_eq!(
        stream[0]
            .entries
            .get(b"ColSpan")
            .and_then(Object::as_integer),
        Some(3)
    );
}

#[test]
fn a_cyclic_or_unmapped_role_has_no_standard_type() {
    let tree = enriched_tree();
    assert_eq!(tree.elements[&9].standard_type, None, "Loop <-> Loop2");
    assert_eq!(tree.elements[&10].standard_type, None, "no mapping");
    assert!(
        !tree.role_map.contains_key(&Name::new("Bad")),
        "a role map value that is not a name is not an entry"
    );
}

#[test]
fn text_entries_decode_utf16_and_pdfdoc_and_language_is_stated_not_inherited() {
    let tree = enriched_tree();
    assert_eq!(tree.elements[&6].title.as_deref(), Some("Tiete"));
    assert_eq!(tree.elements[&6].lang.as_deref(), Some("en-GB"));
    let figure = &tree.elements[&7];
    assert_eq!(figure.alt.as_deref(), Some("A (small) cat"));
    assert_eq!(figure.actual_text.as_deref(), Some("\u{e9}!"));
    assert_eq!(figure.expansion.as_deref(), Some("Expanded"));
    assert_eq!(figure.lang, None, "the figure states no language");
}

#[test]
fn attributes_are_a_list_whether_the_file_wrote_a_dictionary_or_an_array() {
    let tree = enriched_tree();
    let owners = |number: u32| -> Vec<Option<Name>> {
        tree.elements[&number]
            .attributes
            .iter()
            .map(|a| a.owner.clone())
            .collect()
    };
    assert_eq!(owners(7), [Some(Name::new("Layout"))]);
    assert_eq!(
        owners(8),
        [Some(Name::new("Table")), Some(Name::new("Layout"))],
        "the revision number and the string are skipped, both owners are kept"
    );
    assert!(tree.elements[&6].attributes.is_empty());
    assert_eq!(
        tree.elements[&8].attributes[0]
            .entries
            .get(b"RowSpan")
            .and_then(Object::as_integer),
        Some(2)
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

/// A page holding a key its tree does not list, and a `/ParentTreeNextKey`
/// that states it: the key is taken all the same, so a new annotation's
/// entry does not take the page's place.
#[test]
fn a_key_a_page_holds_is_taken_even_when_the_tree_lost_it() {
    let original = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 0 >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /StructParents 1 >>"
            .to_vec(),
        b"<< /Type /StructTreeRoot /K [6 0 R] /ParentTree << /Nums [0 [6 0 R]] >> /ParentTreeNextKey 1 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 >>".to_vec(),
    ]);
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
            let (_, key) = attach_annotation(
                tx,
                &structure,
                ObjRef::new(3, 0),
                ObjRef::new(annotation, 0),
            )?;
            Ok(key)
        })
        .expect("attaches");
    assert_eq!(key, Some(2), "page 2 holds key 1");
}

/// One indirect string shared by many elements is decoded into each of them,
/// so the reader holds a total rather than trusting a small file.
#[test]
fn text_shared_across_elements_is_bounded_in_total() {
    let megabyte = format!("({})", "a".repeat(1 << 20)).into_bytes();
    let build = |elements: usize| {
        let kids: String = (0..elements).map(|n| format!("{} 0 R ", 7 + n)).collect();
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
                .to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
            b"<< /Type /Font >>".to_vec(),
            format!("<< /Type /StructTreeRoot /K [{kids}] >>").into_bytes(),
            megabyte.clone(),
        ];
        objects.extend((0..elements).map(|_| b"<< /S /P /P 5 0 R /Alt 6 0 R >>".to_vec()));
        open(&pdf(&objects))
    };
    assert!(read_structure(&build(8)).is_ok(), "8 MiB of text is fine");
    let error = read_structure(&build(20)).expect_err("20 MiB of text is not");
    assert!(error.to_string().contains("more text"), "{error}");
}

/// Direct text is in the file once, so a file with a lot of it is large rather
/// than amplified; the budget must not reject it.
#[test]
fn a_large_tree_of_direct_text_is_not_mistaken_for_an_amplified_one() {
    let elements = 400;
    let kids: String = (0..elements).map(|n| format!("{} 0 R ", 7 + n)).collect();
    let text = "x".repeat(60_000);
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
        b"<< /Type /Font >>".to_vec(),
        format!("<< /Type /StructTreeRoot /K [{kids}] >>").into_bytes(),
        b"null".to_vec(),
    ];
    objects.extend(
        (0..elements).map(|_| format!("<< /S /P /P 5 0 R /ActualText ({text}) >>").into_bytes()),
    );
    let doc = open(&pdf(&objects));
    let structure = read_structure(&doc).expect("24 MB of direct text in a 24 MB file reads");
    assert_eq!(structure.tree().expect("tagged").elements.len(), elements);
}

// ---------------------------------------------------------------------------
// From an element to the content it marks
// ---------------------------------------------------------------------------

/// One page whose content opens ids 0 to 3 (text, a rectangle, an empty
/// sequence, text inside an `/Artifact`), and eight elements that between them
/// name every outcome `content_of` has: found, found empty, dangling, no page,
/// into a form's stream, on a page that is not one, and found but artifact.
fn content_map_document() -> Vec<u8> {
    let content = "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (hello) Tj ET EMC \
        /Figure << /MCID 1 >> BDC 0 0 20 20 re f EMC \
        /Span << /MCID 2 >> BDC EMC \
        /Artifact BMC /P << /MCID 3 >> BDC BT /F1 12 Tf 10 100 Td (art) Tj ET EMC EMC";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_vec(),
        stream_object(content),
        b"<< /Type /StructTreeRoot /K [7 0 R 8 0 R 9 0 R 10 0 R 11 0 R 12 0 R 13 0 R 14 0 R] >>"
            .to_vec(),
        stream_object(""),
        b"<< /S /P /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /S /Figure /Pg 3 0 R /K 1 >>".to_vec(),
        b"<< /S /Span /K << /Type /MCR /Pg 3 0 R /MCID 2 >> >>".to_vec(),
        b"<< /S /P /Pg 3 0 R /K 99 >>".to_vec(),
        b"<< /S /P /K 0 >>".to_vec(),
        b"<< /S /P /K << /Type /MCR /Pg 3 0 R /MCID 0 /Stm 6 0 R >> >>".to_vec(),
        b"<< /S /P /K << /Type /MCR /Pg 60 0 R /MCID 0 >> >>".to_vec(),
        b"<< /S /P /Pg 3 0 R /K 3 >>".to_vec(),
    ])
}

fn stream_object(data: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data.as_bytes());
    out.extend_from_slice(b"\nendstream");
    out
}

fn content_of(number: u32) -> onionskin_core::ElementContent {
    let doc = open(&content_map_document());
    let structure = read_structure(&doc).expect("the tree reads");
    let tree = structure.tree().expect("tagged");
    onionskin_core::ContentMap::new(&doc)
        .expect("the pages index")
        .content_of(&tree.elements[&number])
        .expect("the content reads")
}

#[test]
fn an_element_finds_the_text_its_marked_content_id_names() {
    let found = content_of(7);
    assert!(found.unplaced.is_empty(), "{:?}", found.unplaced);
    let [item] = &found.items[..] else {
        panic!("one run: {:?}", found.items);
    };
    assert_eq!(item.kind, onionskin_core::ItemKind::Text);
    assert_eq!(item.text.as_deref(), Some("hello"));
    assert!(!item.artifact);
    assert!(
        item.bounds[0] == 10.0
            && item.bounds[1] < 10.0
            && item.bounds[3] > 10.0
            && item.bounds[2] > item.bounds[0],
        "the glyph box starts at the text origin and spans the baseline: {:?}",
        item.bounds
    );
}

#[test]
fn a_path_is_found_by_its_id_and_an_mcr_reaches_it_like_a_bare_id() {
    let found = content_of(8);
    let [item] = &found.items[..] else {
        panic!("one path: {:?}", found.items);
    };
    assert_eq!(item.kind, onionskin_core::ItemKind::Path);
    assert_eq!(item.bounds, [0.0, 0.0, 20.0, 20.0]);
    assert_eq!(item.text, None);
}

#[test]
fn a_sequence_that_draws_nothing_is_an_empty_element_not_a_missing_one() {
    let found = content_of(9);
    assert!(found.items.is_empty());
    assert!(found.unplaced.is_empty(), "{:?}", found.unplaced);
}

#[test]
fn every_reference_that_cannot_be_followed_is_reported() {
    use onionskin_core::Unplaced;
    assert_eq!(
        content_of(10).unplaced,
        [Unplaced::Dangling { page: 0, mcid: 99 }]
    );
    assert_eq!(content_of(11).unplaced, [Unplaced::NoPage { mcid: 0 }]);
    assert_eq!(
        content_of(12).unplaced,
        [Unplaced::InStream {
            mcid: 0,
            stream: ObjRef::new(6, 0)
        }],
        "an id into a form's stream numbers that stream's content, not the page's"
    );
    assert_eq!(
        content_of(13).unplaced,
        [Unplaced::NotAPage {
            page: ObjRef::new(60, 0),
            mcid: 0
        }]
    );
    assert!(content_of(12).items.is_empty() && content_of(10).items.is_empty());
}

#[test]
fn text_inside_an_artifact_is_found_and_says_so() {
    let found = content_of(14);
    let [item] = &found.items[..] else {
        panic!("{:?}", found.items);
    };
    assert_eq!(item.text.as_deref(), Some("art"));
    assert!(item.artifact);
}

/// The plan's corpus claim: over the tagged files, every id the tree names is
/// found in content. It holds except in the files below, each checked by hand
/// against `qpdf --qdf`: the tree names an id the page never opens (the
/// `Dangling` ones: the content has no marked content, or none with that id),
/// or names one through an `/MCR` `/Stm` into a form (the `InStream` ones).
/// Pinning the files makes a change in either direction a failure: a new
/// dangling file is a reader or interpreter regression, and one that
/// disappears means a fixture changed under the claim.
const DANGLING: &[&str] = &[
    "PDF_A-1a/6.3 Fonts/6.3.8 Unicode character maps/veraPDF test suite 6-3-8-t01-fail-c.pdf",
    "PDF_A-2u/6.2 Graphics/6.2.11 Fonts/6.2.11.7 Unicode character maps/6.2.11.7.2 Level A and Level U conformance/veraPDF test suite 6-2-11-7-2-t01-fail-d.pdf",
    "PDF_A-4/6.3 Annotations/6.3.2 Annotation dictionaries/veraPDF test suite 6-3-2-t01-fail-u.pdf",
    "PDF_A-4/6.3 Annotations/6.3.3 Annotation appearances/veraPDF test suite 6-3-3-t01-pass-d.pdf",
    "PDF_UA-1/7.1 General/7.1-t01-pass-b.pdf",
    "PDF_UA-1/7.15 XFA/7.15-t01-fail-a.pdf",
    "PDF_UA-2/8.8 Intra-document destinations/8.8-t02-fail-a.pdf",
];
const IN_STREAM: &[&str] = &[
    "PDF_UA-1/7.20 XObjects/7.20-t02-fail-a.pdf",
    "PDF_UA-1/7.20 XObjects/7.20-t02-pass-a.pdf",
];

#[test]
fn the_corpus_tree_names_only_ids_its_pages_open() {
    use onionskin_core::Unplaced;
    use std::collections::BTreeSet;

    let Some(root) = verapdf_root() else {
        eprintln!("SKIPPED: corpus/external/verapdf is absent; fetch it with corpus/fetch.sh");
        return;
    };
    let mut tagged = 0usize;
    let mut dangling = BTreeSet::new();
    let mut in_stream = BTreeSet::new();
    for path in onionskin_corpus_testing::pdfs_in(&root) {
        let name = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let Ok(doc) = CosDocument::open_path(&path) else {
            continue;
        };
        let Ok(structure) = read_structure(&doc) else {
            continue;
        };
        let Some(tree) = structure.tree() else {
            continue;
        };
        tagged += 1;
        let mut map = onionskin_core::ContentMap::new(&doc).expect("the pages index");
        for element in tree.elements.values() {
            let content = map
                .content_of(element)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            for unplaced in content.unplaced {
                match unplaced {
                    Unplaced::Dangling { .. } => dangling.insert(name.clone()),
                    Unplaced::InStream { .. } => in_stream.insert(name.clone()),
                    other => panic!("{name}: {other:?}"),
                };
            }
        }
    }
    assert!(tagged > 500, "only {tagged} tagged files read");
    let expected = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
    assert_eq!(dangling, expected(DANGLING));
    assert_eq!(in_stream, expected(IN_STREAM));
}

// ---------------------------------------------------------------------------
// Reading order
// ---------------------------------------------------------------------------

/// A document whose drawing order is not its reading order, with mixed
/// content, an inherited language, a custom role and a figure.
///
/// Reading order: Document, H1 "Heading", P "See ", Link "here", the rest of
/// that P "now" (drawn first), a `Para` of two lines, and a Figure.
fn reading_document() -> Vec<u8> {
    let content = "/P << /MCID 4 >> BDC BT /F1 12 Tf 70 150 Td (now) Tj ET EMC \
        /H1 << /MCID 0 >> BDC BT /F1 12 Tf 10 180 Td (Heading) Tj ET EMC \
        /Span << /MCID 1 >> BDC BT /F1 12 Tf 10 150 Td (See ) Tj ET EMC \
        /Link << /MCID 2 >> BDC BT /F1 12 Tf 40 150 Td (here) Tj ET EMC \
        /P << /MCID 3 >> BDC BT /F1 12 Tf 10 100 Td (line one) Tj 0 -20 Td (line two) Tj ET EMC \
        /Figure << /MCID 5 >> BDC 0 0 20 20 re f EMC";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_vec(),
        stream_object(content),
        b"<< /Type /StructTreeRoot /K [7 0 R] /RoleMap << /Para /P >> >>".to_vec(),
        b"null".to_vec(),
        b"<< /S /Document /Pg 3 0 R /Lang (en) /K [8 0 R 9 0 R 11 0 R 12 0 R] >>".to_vec(),
        b"<< /S /H1 /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /S /P /Pg 3 0 R /Lang (fr) /Alt (para alt) /K [1 10 0 R 4] >>".to_vec(),
        b"<< /S /Link /Pg 3 0 R /Alt (a link) /K 2 >>".to_vec(),
        b"<< /S /Para /Pg 3 0 R /K 3 >>".to_vec(),
        b"<< /S /Figure /Pg 3 0 R /Alt (A figure) /K 5 >>".to_vec(),
    ])
}

type BlockSummary = (u32, bool, usize, &'static str, &'static str, String);

fn summarize(blocks: &[onionskin_core::Block]) -> Vec<BlockSummary> {
    blocks
        .iter()
        .map(|b| {
            let name = |n: &Option<Name>| match n.as_ref().map(|n| n.as_bytes()) {
                Some(b"Document") => "Document",
                Some(b"H1") => "H1",
                Some(b"P") => "P",
                Some(b"Link") => "Link",
                Some(b"Figure") => "Figure",
                _ => "?",
            };
            let lang = match b.lang.as_deref() {
                Some("en") => "en",
                Some("fr") => "fr",
                _ => "-",
            };
            (
                b.element,
                b.continuation,
                b.depth,
                name(&b.standard_type),
                lang,
                b.text.clone(),
            )
        })
        .collect()
}

#[test]
fn blocks_come_in_structure_order_with_mixed_content_split_around_its_child() {
    let doc = open(&reading_document());
    let structure = read_structure(&doc).expect("reads");
    let blocks = onionskin_core::reading_order(&doc, structure.tree().expect("tagged"))
        .expect("the order reads");
    assert_eq!(
        summarize(&blocks),
        [
            (7, false, 0, "Document", "en", String::new()),
            (8, false, 1, "H1", "en", "Heading".into()),
            (9, false, 1, "P", "fr", "See ".into()),
            (10, false, 2, "Link", "fr", "here".into()),
            (9, true, 1, "P", "fr", "now".into()),
            (11, false, 1, "P", "en", "line one line two".into()),
            (12, false, 1, "Figure", "en", String::new()),
        ],
        "drawing order puts \"now\" first; the tree puts it after the link. A custom role \
         reads as its standard type, a language is inherited, a line break reads as a space"
    );
}

#[test]
fn only_an_elements_first_block_carries_what_describes_the_element() {
    let doc = open(&reading_document());
    let structure = read_structure(&doc).expect("reads");
    let blocks = onionskin_core::reading_order(&doc, structure.tree().expect("tagged"))
        .expect("the order reads");
    assert_eq!(blocks[3].alt.as_deref(), Some("a link"));
    assert_eq!(blocks[2].alt.as_deref(), Some("para alt"));
    assert_eq!(blocks[6].alt.as_deref(), Some("A figure"));
    assert_eq!(blocks[6].items.len(), 1, "the figure's content is its path");
    assert!(blocks[4].continuation && blocks[4].alt.is_none());
}

#[test]
fn a_cyclic_tree_is_walked_once() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
        b"<< /S /P /Pg 3 0 R /K [6 0 R] >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [4 0 R] >>".to_vec(),
        b"<< /S /P /Pg 3 0 R /K [4 0 R] >>".to_vec(),
    ]);
    let doc = open(&bytes);
    let structure = read_structure(&doc).expect("reads");
    let blocks =
        onionskin_core::reading_order(&doc, structure.tree().expect("tagged")).expect("walks");
    assert_eq!(blocks.iter().map(|b| b.element).collect::<Vec<_>>(), [4, 6]);
}

/// The reading order and effective language of the four fixtures, from
/// pikepdf. `corpus/tagged/reading_order.py` prints this table; it is an
/// implementation independent of the reader, as `FIXTURES` above is.
///
/// `(fixture, element object numbers in depth-first /K order, (object number,
/// effective /Lang) for each element that has one)`.
type ReadingExpectation = (&'static str, &'static [u32], &'static [(u32, &'static str)]);

const READING_ORDER: &[ReadingExpectation] = &[
    ("Isartor test files/doc/Isartor test suite manual.pdf",
     &[286, 321, 320, 322, 563, 564, 646, 647, 654, 655, 656, 657, 648, 651, 652, 649, 650, 565, 625, 626, 645, 627, 642, 643, 644, 628, 641, 629, 638, 639, 640, 630, 635, 636, 637, 631, 634, 632, 633, 566, 605, 606, 622, 623, 624, 607, 618, 619, 620, 290, 676, 677, 621, 608, 613, 614, 615, 616, 617, 609, 610, 611, 612, 567, 568, 569, 602, 603, 604, 570, 595, 596, 597, 598, 599, 600, 601, 571, 592, 593, 594, 572, 589, 590, 591, 573, 582, 583, 584, 585, 586, 587, 588, 574, 579, 580, 581, 575, 576, 577, 578, 323, 512, 513, 560, 561, 562, 514, 555, 556, 557, 558, 559, 515, 554, 516, 553, 517, 552, 518, 551, 519, 548, 549, 550, 520, 547, 521, 546, 522, 545, 523, 542, 543, 544, 524, 539, 540, 541, 525, 538, 526, 535, 536, 537, 527, 532, 533, 534, 528, 529, 530, 531, 324, 474, 475, 511, 476, 504, 505, 508, 509, 510, 506, 507, 477, 478, 479, 503, 480, 502, 481, 489, 490, 491, 492, 493, 494, 495, 496, 497, 498, 499, 500, 501, 482, 483, 484, 485, 486, 487, 295, 675, 488, 325, 432, 433, 444, 445, 469, 297, 674, 470, 298, 673, 471, 299, 672, 472, 473, 446, 468, 447, 466, 300, 671, 467, 448, 463, 464, 465, 449, 462, 450, 457, 458, 459, 460, 461, 451, 454, 455, 456, 452, 453, 434, 435, 436, 437, 302, 669, 670, 438, 439, 303, 668, 440, 441, 442, 443, 326, 327, 328, 404, 405, 425, 426, 427, 428, 429, 430, 431, 406, 419, 420, 421, 422, 423, 407, 416, 417, 418, 408, 411, 412, 413, 414, 415, 409, 410, 329, 387, 388, 403, 389, 398, 399, 400, 401, 402, 390, 395, 396, 397, 391, 392, 393, 394, 330, 386, 331, 383, 384, 307, 667, 385, 332, 378, 379, 380, 381, 308, 666, 382, 333, 373, 374, 375, 376, 309, 665, 377, 334, 370, 371, 310, 664, 372, 335, 367, 368, 311, 663, 369, 336, 347, 348, 366, 349, 365, 350, 362, 363, 364, 351, 361, 352, 358, 359, 360, 353, 357, 354, 355, 313, 662, 356, 337, 342, 343, 344, 314, 661, 345, 346, 338, 339, 340, 341, 319, 316, 318, 317, 659],
     &[(286, "en-US"), (290, "en-US"), (295, "en-US"), (297, "en-US"), (298, "en-US"), (299, "en-US"), (300, "en-US"), (302, "en-US"), (303, "en-US"), (307, "en-US"), (308, "en-US"), (309, "en-US"), (310, "en-US"), (311, "en-US"), (313, "en-US"), (314, "en-US"), (316, "en-US"), (317, "en-US"), (318, "en-US"), (319, "en-US"), (320, "en-US"), (321, "en-US"), (322, "en-US"), (323, "en-US"), (324, "en-US"), (325, "en-US"), (326, "en-US"), (327, "en-US"), (328, "en-US"), (329, "en-US"), (330, "en-US"), (331, "en-US"), (332, "en-US"), (333, "en-US"), (334, "en-US"), (335, "en-US"), (336, "en-US"), (337, "en-US"), (338, "en-US"), (339, "en-US"), (340, "en-US"), (341, "en-US"), (342, "en-US"), (343, "en-US"), (344, "en-US"), (345, "en-US"), (346, "en-US"), (347, "en-US"), (348, "en-US"), (349, "en-US"), (350, "en-US"), (351, "en-US"), (352, "en-US"), (353, "en-US"), (354, "en-US"), (355, "en-US"), (356, "en-US"), (357, "en-US"), (358, "en-US"), (359, "en-US"), (360, "en-US"), (361, "en-US"), (362, "en-US"), (363, "en-US"), (364, "en-US"), (365, "en-US"), (366, "en-US"), (367, "en-US"), (368, "en-US"), (369, "en-US"), (370, "en-US"), (371, "en-US"), (372, "en-US"), (373, "en-US"), (374, "en-US"), (375, "en-US"), (376, "en-US"), (377, "en-US"), (378, "en-US"), (379, "en-US"), (380, "en-US"), (381, "en-US"), (382, "en-US"), (383, "en-US"), (384, "en-US"), (385, "en-US"), (386, "en-US"), (387, "en-US"), (388, "en-US"), (389, "en-US"), (390, "en-US"), (391, "en-US"), (392, "en-US"), (393, "en-US"), (394, "en-US"), (395, "en-US"), (396, "en-US"), (397, "en-US"), (398, "en-US"), (399, "en-US"), (400, "en-US"), (401, "en-US"), (402, "en-US"), (403, "en-US"), (404, "en-US"), (405, "en-US"), (406, "en-US"), (407, "en-US"), (408, "en-US"), (409, "en-US"), (410, "en-US"), (411, "en-US"), (412, "en-US"), (413, "en-US"), (414, "en-US"), (415, "en-US"), (416, "en-US"), (417, "en-US"), (418, "en-US"), (419, "en-US"), (420, "en-US"), (421, "en-US"), (422, "en-US"), (423, "en-US"), (425, "en-US"), (426, "en-US"), (427, "en-US"), (428, "en-US"), (429, "en-US"), (430, "en-US"), (431, "en-US"), (432, "en-US"), (433, "en-US"), (434, "en-US"), (435, "en-US"), (436, "en-US"), (437, "en-US"), (438, "en-US"), (439, "en-US"), (440, "en-US"), (441, "en-US"), (442, "en-US"), (443, "en-US"), (444, "en-US"), (445, "en-US"), (446, "en-US"), (447, "en-US"), (448, "en-US"), (449, "en-US"), (450, "en-US"), (451, "en-US"), (452, "en-US"), (453, "en-US"), (454, "en-US"), (455, "en-US"), (456, "en-US"), (457, "en-US"), (458, "en-US"), (459, "en-US"), (460, "en-US"), (461, "en-US"), (462, "en-US"), (463, "en-US"), (464, "en-US"), (465, "en-US"), (466, "en-US"), (467, "en-US"), (468, "en-US"), (469, "en-US"), (470, "en-US"), (471, "en-US"), (472, "en-US"), (473, "en-US"), (474, "en-US"), (475, "en-US"), (476, "en-US"), (477, "en-US"), (478, "en-US"), (479, "en-US"), (480, "en-US"), (481, "en-US"), (482, "en-US"), (483, "en-US"), (484, "en-US"), (485, "en-US"), (486, "en-US"), (487, "en-US"), (488, "en-US"), (502, "en-US"), (503, "en-US"), (504, "en-US"), (505, "en-US"), (506, "en-US"), (507, "en-US"), (508, "en-US"), (509, "en-US"), (510, "en-US"), (511, "en-US"), (512, "en-US"), (513, "en-US"), (514, "en-US"), (515, "en-US"), (516, "en-US"), (517, "en-US"), (518, "en-US"), (519, "en-US"), (520, "en-US"), (521, "en-US"), (522, "en-US"), (523, "en-US"), (524, "en-US"), (525, "en-US"), (526, "en-US"), (527, "en-US"), (528, "en-US"), (529, "en-US"), (530, "en-US"), (531, "en-US"), (532, "en-US"), (533, "en-US"), (534, "en-US"), (535, "en-US"), (536, "en-US"), (537, "en-US"), (538, "en-US"), (539, "en-US"), (540, "en-US"), (541, "en-US"), (542, "en-US"), (543, "en-US"), (544, "en-US"), (545, "en-US"), (547, "en-US"), (548, "en-US"), (549, "en-US"), (550, "en-US"), (552, "en-US"), (553, "en-US"), (555, "en-US"), (556, "en-US"), (557, "en-US"), (558, "en-US"), (559, "en-US"), (560, "en-US"), (561, "en-US"), (562, "en-US"), (563, "en-US"), (564, "en-US"), (565, "en-US"), (566, "en-US"), (567, "en-US"), (568, "en-US"), (569, "en-US"), (570, "en-US"), (571, "en-US"), (572, "en-US"), (573, "en-US"), (574, "en-US"), (575, "en-US"), (576, "en-US"), (577, "en-US"), (578, "en-US"), (579, "en-US"), (580, "en-US"), (581, "en-US"), (582, "en-US"), (583, "en-US"), (584, "en-US"), (585, "en-US"), (586, "en-US"), (587, "en-US"), (588, "en-US"), (589, "en-US"), (590, "en-US"), (591, "en-US"), (592, "en-US"), (593, "en-US"), (594, "en-US"), (595, "en-US"), (596, "en-US"), (597, "en-US"), (598, "en-US"), (599, "en-US"), (600, "en-US"), (601, "en-US"), (602, "en-US"), (603, "en-US"), (604, "en-US"), (605, "en-US"), (606, "en-US"), (607, "en-US"), (608, "en-US"), (609, "en-US"), (610, "en-US"), (611, "en-US"), (612, "en-US"), (613, "en-US"), (614, "en-US"), (615, "en-US"), (616, "en-US"), (617, "en-US"), (618, "en-US"), (619, "en-US"), (620, "en-US"), (621, "en-US"), (622, "en-US"), (623, "en-US"), (624, "en-US"), (625, "en-US"), (626, "en-US"), (627, "en-US"), (628, "en-US"), (629, "en-US"), (630, "en-US"), (631, "en-US"), (632, "en-US"), (633, "en-US"), (634, "en-US"), (635, "en-US"), (636, "en-US"), (637, "en-US"), (638, "en-US"), (639, "en-US"), (640, "en-US"), (641, "en-US"), (642, "en-US"), (643, "en-US"), (644, "en-US"), (645, "en-US"), (646, "en-US"), (647, "en-US"), (648, "en-US"), (649, "en-US"), (650, "en-US"), (651, "en-US"), (652, "en-US"), (654, "en-US"), (655, "en-US"), (656, "en-US"), (657, "en-US"), (659, "en-US"), (661, "en-US"), (662, "en-US"), (663, "en-US"), (664, "en-US"), (665, "en-US"), (666, "en-US"), (667, "en-US"), (668, "en-US"), (669, "en-US"), (670, "en-US"), (671, "en-US"), (672, "en-US"), (673, "en-US"), (674, "en-US"), (675, "en-US"), (676, "en-US"), (677, "en-US")]),
    ("PDF_UA-1/7.2 Text/7.2-t27-pass-a.pdf",
     &[18, 32, 37, 57, 46, 65, 52, 43, 38, 58, 47, 66, 51, 44, 39, 59, 74, 75, 71, 50, 40, 41, 68, 48, 67, 49, 45, 42, 33, 34, 35, 36],
     &[(18, "en-US"), (32, "en-US"), (33, "en-US"), (34, "en-US"), (35, "en-US"), (36, "en-US"), (37, "en-US"), (38, "en-US"), (39, "en-US"), (40, "en-US"), (41, "en-US"), (42, "en-US"), (43, "en-US"), (44, "en-US"), (45, "en-US"), (46, "en-US"), (47, "en-US"), (48, "en-US"), (49, "en-US"), (50, "en-US"), (51, "en-US"), (52, "en-US"), (57, "en-US"), (58, "en-US"), (59, "en-US"), (65, "en-US"), (66, "en-US"), (67, "en-US"), (68, "en-US"), (71, "en-US"), (74, "en-US"), (75, "en-US")]),
    ("PDF_UA-1/7.2 Text/7.2-t15-pass-a.pdf",
     &[14, 24, 45, 72, 25, 46, 26, 27, 28, 47, 29, 30, 31, 32, 48, 33, 34, 35, 49, 36, 37, 38, 39],
     &[(14, "en-US"), (24, "en-US"), (25, "en-US"), (26, "en-US"), (27, "en-US"), (28, "en-US"), (29, "en-US"), (30, "en-US"), (31, "en-US"), (32, "en-US"), (33, "en-US"), (34, "en-US"), (35, "en-US"), (36, "en-US"), (37, "en-US"), (38, "en-US"), (39, "en-US"), (45, "en-US"), (46, "en-US"), (47, "en-US"), (48, "en-US"), (49, "en-US"), (72, "en-US")]),
    ("PDF_UA-2/8.2 Logical structure/8.2.5 Additional requirements for specific structure types/8.2.5.26 Table (Table, TR, TH, TD, THead, TBody, TFoot)/8.2.5.26-t01-pass-a.pdf",
     &[11, 19, 40, 69, 20, 41, 21, 22, 23, 42, 24, 25, 26, 27, 43, 28, 29, 30, 44, 31, 32, 33, 34],
     &[(11, "en-US"), (19, "en-US"), (20, "en-US"), (21, "en-US"), (22, "en-US"), (23, "en-US"), (24, "en-US"), (25, "en-US"), (26, "en-US"), (27, "en-US"), (28, "en-US"), (29, "en-US"), (30, "en-US"), (31, "en-US"), (32, "en-US"), (33, "en-US"), (34, "en-US"), (40, "en-US"), (41, "en-US"), (42, "en-US"), (43, "en-US"), (44, "en-US"), (69, "en-US")]),
];

#[test]
fn the_corpus_reading_order_and_language_agree_with_pikepdf() {
    let Some(root) = verapdf_root() else {
        eprintln!("SKIPPED: corpus/external/verapdf is absent; fetch it with corpus/fetch.sh");
        return;
    };
    for (fixture, order, languages) in READING_ORDER {
        let doc = CosDocument::open_path(&root.join(fixture)).expect("opens");
        let structure = read_structure(&doc).expect("reads");
        let blocks = onionskin_core::reading_order(&doc, structure.tree().expect("tagged"))
            .unwrap_or_else(|e| panic!("{fixture}: {e}"));
        let mut seen = std::collections::BTreeSet::new();
        let ours: Vec<u32> = blocks
            .iter()
            .filter(|b| seen.insert(b.element))
            .map(|b| b.element)
            .collect();
        assert_eq!(
            &ours, order,
            "{fixture}: element order disagrees with pikepdf"
        );
        let expected: BTreeMap<u32, &str> = languages.iter().copied().collect();
        for block in blocks.iter().filter(|b| !b.continuation) {
            assert_eq!(
                block.lang.as_deref(),
                expected.get(&block.element).copied(),
                "{fixture}: language of element {}",
                block.element
            );
        }
    }
}

/// An empty `/Lang` says the language is unknown. It is stated, so it does not
/// fall back to the ancestor's, and what is below it inherits "unknown".
#[test]
fn an_empty_language_ends_inheritance_rather_than_inheriting() {
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec(),
        b"<< /S /Document /Lang (en) /K [6 0 R 8 0 R] >>".to_vec(),
        b"<< /Type /StructTreeRoot /K [4 0 R] >>".to_vec(),
        b"<< /S /P /Lang () /K [7 0 R] >>".to_vec(),
        b"<< /S /Span >>".to_vec(),
        b"<< /S /P >>".to_vec(),
    ]);
    let doc = open(&bytes);
    let structure = read_structure(&doc).expect("reads");
    let blocks =
        onionskin_core::reading_order(&doc, structure.tree().expect("tagged")).expect("walks");
    let langs: Vec<(u32, Option<&str>)> = blocks
        .iter()
        .map(|b| (b.element, b.lang.as_deref()))
        .collect();
    assert_eq!(
        langs,
        [(4, Some("en")), (6, None), (7, None), (8, Some("en"))]
    );
}

/// One page (optionally turned) with `content`, and elements as objects 6
/// onward under a root, for the reading-order cases that need only a few
/// runs. `catalog` is spliced into the catalog dictionary.
fn small_reading_document(catalog: &str, rotate: i64, content: &str, elements: &[&str]) -> Vec<u8> {
    let kids: String = (0..elements.len())
        .map(|n| format!("{} 0 R ", 6 + n))
        .collect();
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> {catalog} >>")
            .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Rotate {rotate} /Contents 4 0 R \
             /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
        )
        .into_bytes(),
        stream_object(content),
        format!("<< /Type /StructTreeRoot /K [{kids}] >>").into_bytes(),
    ];
    objects.extend(elements.iter().map(|e| e.as_bytes().to_vec()));
    pdf(&objects)
}

fn blocks_of(bytes: &[u8]) -> Vec<onionskin_core::Block> {
    let doc = open(bytes);
    let structure = read_structure(&doc).expect("reads");
    onionskin_core::reading_order(&doc, structure.tree().expect("tagged")).expect("walks")
}

#[test]
fn the_catalog_language_applies_until_an_element_overrides_it() {
    let bytes = small_reading_document(
        "/Lang (de-AT)",
        0,
        "",
        &[
            "<< /S /P >>",
            "<< /S /P /Lang (fr) >>",
            "<< /S /P /Lang () >>",
        ],
    );
    let langs: Vec<_> = blocks_of(&bytes).iter().map(|b| b.lang.clone()).collect();
    assert_eq!(
        langs,
        [Some("de-AT".to_string()), Some("fr".to_string()), None]
    );
}

#[test]
fn an_element_with_only_an_objr_kid_lists_the_object_and_is_not_empty() {
    let bytes = small_reading_document(
        "",
        0,
        "",
        &["<< /S /Link /K << /Type /OBJR /Obj 40 0 R >> >>"],
    );
    let blocks = blocks_of(&bytes);
    assert_eq!(blocks[0].objects, [ObjRef::new(40, 0)]);
    assert!(blocks[0].items.is_empty() && blocks[0].unplaced.is_empty());
}

#[test]
fn a_sequence_named_by_two_elements_is_read_once_and_the_second_claim_is_reported() {
    let bytes = small_reading_document(
        "",
        0,
        "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (once) Tj ET EMC",
        &["<< /S /P /Pg 3 0 R /K 0 >>", "<< /S /P /Pg 3 0 R /K 0 >>"],
    );
    let blocks = blocks_of(&bytes);
    assert_eq!(blocks[0].text, "once");
    assert_eq!(blocks[1].text, "");
    assert_eq!(
        blocks[1].unplaced,
        [onionskin_core::Unplaced::Claimed { page: 0, mcid: 0 }]
    );
}

#[test]
fn content_between_two_children_makes_a_continuation_that_does_not_repeat_the_elements_own_fields()
{
    let bytes = small_reading_document(
        "",
        0,
        "/Span << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (a) Tj ET EMC \
         /Span << /MCID 1 >> BDC BT /F1 12 Tf 30 10 Td (b) Tj ET EMC \
         /Span << /MCID 2 >> BDC BT /F1 12 Tf 50 10 Td (c) Tj ET EMC \
         /Span << /MCID 3 >> BDC BT /F1 12 Tf 70 10 Td (d) Tj ET EMC \
         /Span << /MCID 4 >> BDC BT /F1 12 Tf 90 10 Td (e) Tj ET EMC",
        &[
            "<< /S /P /Pg 3 0 R /T (title) /ActualText (said) /K [0 7 0 R 1 8 0 R 2] >>",
            "<< /S /Span /Pg 3 0 R /K 3 >>",
            "<< /S /Span /Pg 3 0 R /K 4 >>",
        ],
    );
    let summary: Vec<_> = blocks_of(&bytes)
        .iter()
        .map(|b| {
            (
                b.element,
                b.continuation,
                b.title.is_some(),
                b.actual_text.is_some(),
                b.page,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (6, false, true, true, Some(0)),
            (7, false, false, false, Some(0)),
            (6, true, false, false, Some(0)),
            (8, false, false, false, Some(0)),
            (6, true, false, false, Some(0)),
        ]
    );
}

fn joined(rotate: i64, content: &str) -> String {
    let bytes = small_reading_document("", rotate, content, &["<< /S /P /Pg 3 0 R /K 0 >>"]);
    blocks_of(&bytes)[0].text.clone()
}

#[test]
fn a_space_goes_where_a_line_breaks_or_a_word_gap_opens_and_nowhere_else() {
    let sequence = |body: &str| format!("/P << /MCID 0 >> BDC BT /F1 12 Tf {body} ET EMC");
    assert_eq!(
        joined(
            0,
            &sequence("10 100 Td (line one) Tj 0 -20 Td (line two) Tj")
        ),
        "line one line two",
        "a new line"
    );
    assert_eq!(
        joined(0, &sequence("10 100 Td (ab) Tj 40 0 Td (cd) Tj")),
        "ab cd",
        "a word gap on the line"
    );
    assert_eq!(
        joined(0, &sequence("10 100 Td (ab) Tj 13.3 0 Td (cd) Tj")),
        "abcd",
        "one operator's text split in two touches the next"
    );
    assert_eq!(
        joined(0, &sequence("10 100 Td (x) Tj 0 4 Td (2) Tj")),
        "x2",
        "a superscript is not a new line"
    );
    assert_eq!(
        joined(
            0,
            &sequence("0 1 -1 0 100 50 Tm (line one) Tj 0 1 -1 0 80 50 Tm (line two) Tj")
        ),
        "line one line two",
        "lines of text turned a quarter run down the page"
    );
    assert_eq!(
        joined(
            0,
            &sequence("0 1 -1 0 100 50 Tm (ab) Tj 0 1 -1 0 100 90 Tm (cd) Tj")
        ),
        "ab cd",
        "and a word gap on such a line"
    );
    assert_eq!(
        joined(0, &sequence("10 100 Td (ab ) Tj 40 0 Td (cd) Tj")),
        "ab cd",
        "a space already there is not doubled"
    );
    assert_eq!(
        joined(0, &sequence("10 100 Td (ab) Tj 40 0 Td ( cd) Tj")),
        "ab cd"
    );
    assert_eq!(
        joined(
            0,
            "/P << /MCID 0 /ActualText (fi) >> BDC BT /F1 12 Tf 10 100 Td (a) Tj 40 0 Td (b) Tj ET EMC"
        ),
        "fi",
        "the later runs of one /ActualText say nothing, so they add no space"
    );
}

// ---------------------------------------------------------------------------
// Reading text
// ---------------------------------------------------------------------------

fn reading_text_of(bytes: &[u8], page: usize) -> String {
    onionskin_core::reading_text(&blocks_of(bytes), page)
}

#[test]
fn a_page_reads_a_line_per_block_element_and_runs_inline_ones_into_their_line() {
    assert_eq!(
        reading_text_of(&reading_document(), 0),
        "Heading\nSee here now\nline one line two\nA figure",
        "the link and the rest of its paragraph continue the paragraph's line, \
         the figure says its alternate text, and the Document says nothing"
    );
    assert_eq!(
        reading_text_of(&reading_document(), 1),
        "",
        "a page nothing is on reads as empty"
    );
}

#[test]
fn an_elements_actual_text_replaces_what_it_marks() {
    let bytes = small_reading_document(
        "",
        0,
        "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 100 Td (f) Tj (i) Tj ET EMC",
        &["<< /S /P /Pg 3 0 R /ActualText (fi) /K 0 >>"],
    );
    assert_eq!(reading_text_of(&bytes, 0), "fi");
}

#[test]
fn text_drawn_in_an_artifact_is_not_part_of_the_reading() {
    let bytes = small_reading_document(
        "",
        0,
        "/Artifact BMC /P << /MCID 0 >> BDC BT /F1 12 Tf 10 100 Td (page 3) Tj ET EMC EMC \
         /P << /MCID 1 >> BDC BT /F1 12 Tf 10 50 Td (body) Tj ET EMC",
        &["<< /S /P /Pg 3 0 R /K [0 1] >>"],
    );
    assert_eq!(reading_text_of(&bytes, 0), "body");
}

#[test]
fn list_items_read_one_line_each_with_the_label_and_body_together() {
    let bytes = small_reading_document(
        "",
        0,
        "/Lbl << /MCID 0 >> BDC BT /F1 12 Tf 10 100 Td (1.) Tj ET EMC \
         /LBody << /MCID 1 >> BDC BT /F1 12 Tf 30 100 Td (first) Tj ET EMC \
         /Lbl << /MCID 2 >> BDC BT /F1 12 Tf 10 80 Td (2.) Tj ET EMC \
         /LBody << /MCID 3 >> BDC BT /F1 12 Tf 30 80 Td (second) Tj ET EMC",
        &[
            "<< /S /L /K [7 0 R 10 0 R] >>",
            "<< /S /LI /K [8 0 R 9 0 R] >>",
            "<< /S /Lbl /Pg 3 0 R /K 0 >>",
            "<< /S /LBody /Pg 3 0 R /K 1 >>",
            "<< /S /LI /K [11 0 R 12 0 R] >>",
            "<< /S /Lbl /Pg 3 0 R /K 2 >>",
            "<< /S /LBody /Pg 3 0 R /K 3 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "1. first\n2. second");
}

#[test]
fn a_span_runs_on_but_a_block_child_breaks_the_line_and_so_does_what_follows_it() {
    let content = "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 150 Td (a) Tj ET EMC \
         /Span << /MCID 1 >> BDC BT /F1 12 Tf 20 150 Td (b) Tj ET EMC \
         /P << /MCID 2 >> BDC BT /F1 12 Tf 30 150 Td (c) Tj ET EMC \
         /Div << /MCID 3 >> BDC BT /F1 12 Tf 10 100 Td (d) Tj ET EMC \
         /P << /MCID 4 >> BDC BT /F1 12 Tf 10 50 Td (e) Tj ET EMC";
    let bytes = small_reading_document(
        "",
        0,
        content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 2 8 0 R 4] >>",
            "<< /S /Span /Pg 3 0 R /K 1 >>",
            "<< /S /Div /Pg 3 0 R /K 3 >>",
        ],
    );
    assert_eq!(
        reading_text_of(&bytes, 0),
        "a b c\nd\ne",
        "the span continues the line, the Div takes its own, and the content after the \
         Div starts another"
    );
}

/// Every tagged corpus file reads as text on every page without error, and most
/// of them say something: a reading that came out empty everywhere would pass
/// a test that only looked for panics.
#[test]
fn every_tagged_corpus_page_reads_as_text() {
    let Some(root) = verapdf_root() else {
        eprintln!("SKIPPED: corpus/external/verapdf is absent; fetch it with corpus/fetch.sh");
        return;
    };
    let (mut files, mut speaking) = (0usize, 0usize);
    for path in onionskin_corpus_testing::pdfs_in(&root) {
        let Ok(doc) = CosDocument::open_path(&path) else {
            continue;
        };
        let Ok(structure) = read_structure(&doc) else {
            continue;
        };
        let Some(tree) = structure.tree() else {
            continue;
        };
        files += 1;
        let blocks = onionskin_core::reading_order(&doc, tree)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let pages = doc.page_count().expect("page count") as usize;
        let said: usize = (0..pages)
            .map(|page| onionskin_core::reading_text(&blocks, page).len())
            .sum();
        speaking += usize::from(said > 0);
    }
    assert!(files > 500, "only {files} tagged files");
    assert!(
        speaking * 10 > files * 8,
        "only {speaking} of {files} tagged files read as any text"
    );
}

fn text_run(mcid: u32, x: u32, y: u32, words: &str) -> String {
    format!("/Span << /MCID {mcid} >> BDC BT /F1 12 Tf {x} {y} Td ({words}) Tj ET EMC ")
}

#[test]
fn an_actual_text_replaces_the_whole_subtree_once_and_not_just_the_first_block() {
    let content = format!(
        "{}{}{}",
        text_run(0, 10, 100, "own"),
        text_run(1, 40, 100, "kid"),
        text_run(2, 70, 100, "later")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /ActualText (WHOLE) /K [0 7 0 R 2] >>",
            "<< /S /Span /Pg 3 0 R /K 1 >>",
        ],
    );
    assert_eq!(
        reading_text_of(&bytes, 0),
        "WHOLE",
        "the paragraph's own text, its Span and what follows the Span are all replaced"
    );
}

#[test]
fn an_element_with_only_children_is_replaced_and_so_is_content_on_other_pages() {
    let bytes = small_reading_document(
        "",
        0,
        &text_run(0, 10, 100, "a"),
        &[
            "<< /S /P /ActualText (WHOLE) /K [7 0 R] >>",
            "<< /S /Span /Pg 3 0 R /K 0 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "WHOLE");
    assert_eq!(
        reading_text_of(&bytes, 1),
        "",
        "said once, on the first page it marks"
    );
}

#[test]
fn a_figures_alt_stands_for_content_in_a_child_and_a_paragraph_with_text_keeps_its_words() {
    let content = format!(
        "{}{}",
        text_run(0, 10, 100, "label"),
        text_run(1, 10, 50, "click")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /Figure /Alt (A chart) /K [7 0 R] >>",
            "<< /S /Span /Pg 3 0 R /K 0 >>",
            "<< /S /P /Alt (tooltip) /Pg 3 0 R /K 1 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "A chart\nclick");
}

#[test]
fn a_space_stands_before_and_after_a_replacement_in_a_line() {
    let content = format!(
        "{}{}{}",
        text_run(0, 10, 100, "a"),
        text_run(1, 30, 100, "x"),
        text_run(2, 50, 100, "c")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 2] >>",
            "<< /S /Span /Pg 3 0 R /ActualText (X) /K 1 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "a X c");
}

#[test]
fn what_follows_a_block_child_starts_a_new_line_even_when_the_last_block_was_inline() {
    let content = format!(
        "{}{}{}{}",
        text_run(0, 10, 150, "a"),
        text_run(1, 10, 100, "b"),
        text_run(2, 30, 100, "c"),
        text_run(3, 10, 50, "d")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 3] >>",
            "<< /S /Div /Pg 3 0 R /K [1 8 0 R] >>",
            "<< /S /Span /Pg 3 0 R /K 2 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "a\nb c\nd");
}

#[test]
fn a_formula_a_bibliography_entry_and_a_form_run_on_in_their_line_and_an_artifact_element_is_dropped(
) {
    let content = format!(
        "{}{}{}{}",
        text_run(0, 10, 100, "x is"),
        text_run(1, 40, 100, "a+b"),
        text_run(2, 70, 100, "[1]"),
        text_run(3, 10, 20, "page 3")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 8 0 R] >>",
            "<< /S /Formula /Pg 3 0 R /K 1 >>",
            "<< /S /BibEntry /Pg 3 0 R /K 2 >>",
            "<< /S /Artifact /Pg 3 0 R /K 3 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "x is a+b [1]");
}

#[test]
fn a_word_space_the_width_of_helveticas_is_a_gap_and_touching_runs_are_not() {
    let sequence = |body: &str| format!("/P << /MCID 0 >> BDC BT /F1 12 Tf {body} ET EMC");
    assert_eq!(
        joined(0, &sequence("10 100 Td (Click) Tj 29.2 0 Td (here) Tj")),
        "Click here",
        "Click is 26pt wide; a space is 3.3pt"
    );
}

#[test]
fn quotes_notes_references_and_ruby_run_on_while_an_unmapped_type_takes_a_line() {
    let content = format!(
        "{}{}{}{}{}",
        text_run(0, 10, 100, "a"),
        text_run(1, 20, 100, "q"),
        text_run(2, 30, 100, "n"),
        text_run(3, 40, 100, "r"),
        text_run(4, 10, 50, "custom")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 8 0 R 9 0 R] >>",
            "<< /S /Quote /Pg 3 0 R /K 1 >>",
            "<< /S /Note /Pg 3 0 R /K 2 >>",
            "<< /S /Ruby /Pg 3 0 R /K 3 >>",
            "<< /S /Weird /Pg 3 0 R /K 4 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "a q n r\ncustom");
}

#[test]
fn an_inline_element_touching_the_run_before_it_adds_no_space() {
    let content = "/Span << /MCID 0 >> BDC BT /F1 12 Tf 10 100 Td (ab) Tj ET EMC \
                   /Span << /MCID 1 >> BDC BT /F1 12 Tf 23.34 100 Td (cd) Tj ET EMC";
    let bytes = small_reading_document(
        "",
        0,
        content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R] >>",
            "<< /S /Span /Pg 3 0 R /K 1 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "abcd");
}

#[test]
fn an_empty_actual_text_or_alt_replaces_nothing() {
    let bytes = small_reading_document(
        "",
        0,
        &format!(
            "{}{}",
            text_run(0, 10, 100, "kept"),
            text_run(1, 10, 50, "also")
        ),
        &[
            "<< /S /Document /ActualText () /Alt () /K [7 0 R 8 0 R] >>",
            "<< /S /P /Pg 3 0 R /K 0 >>",
            "<< /S /P /Pg 3 0 R /K 1 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "kept\nalso");
}

#[test]
fn a_caption_inside_a_figure_is_read_after_the_figures_alt() {
    let content = format!(
        "/Figure << /MCID 0 >> BDC 0 0 20 20 re f EMC {}",
        text_run(1, 10, 50, "Figure 1: a cat")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /Figure /Pg 3 0 R /Alt (A cat) /K [0 7 0 R] >>",
            "<< /S /Caption /Pg 3 0 R /K 1 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "A cat\nFigure 1: a cat");
}

#[test]
fn an_excluded_child_does_not_leave_the_break_before_it_to_decide_the_next_line() {
    let content = format!(
        "{}{}{}{}{}",
        text_run(0, 10, 150, "a"),
        text_run(1, 10, 100, "b"),
        text_run(2, 10, 50, "c"),
        text_run(3, 10, 20, "page 3"),
        text_run(4, 30, 50, "d")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 2 8 0 R 4] >>",
            "<< /S /Div /Pg 3 0 R /K 1 >>",
            "<< /S /Artifact /Pg 3 0 R /K 3 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "a\nb\nc d");
}

#[test]
fn a_form_and_a_label_run_on_in_a_paragraph() {
    let content = format!(
        "{}{}{}",
        text_run(0, 10, 100, "a"),
        text_run(1, 30, 100, "b"),
        text_run(2, 50, 100, "c")
    );
    let bytes = small_reading_document(
        "",
        0,
        &content,
        &[
            "<< /S /P /Pg 3 0 R /K [0 7 0 R 8 0 R] >>",
            "<< /S /Form /Pg 3 0 R /K 1 >>",
            "<< /S /Lbl /Pg 3 0 R /K 2 >>",
        ],
    );
    assert_eq!(reading_text_of(&bytes, 0), "a b c");
}

#[test]
fn an_alt_is_said_for_an_element_whose_only_text_is_an_artifact() {
    let content =
        "/Artifact BMC /P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (footer) Tj ET EMC EMC \
                   /P << /MCID 1 >> BDC 0 0 5 5 re f EMC";
    let bytes = small_reading_document(
        "",
        0,
        content,
        &["<< /S /P /Pg 3 0 R /Alt (the alt) /K [0 1] >>"],
    );
    assert_eq!(reading_text_of(&bytes, 0), "the alt");
}

#[test]
fn a_gap_of_a_fifth_of_the_text_size_is_a_word_space() {
    let sequence = |body: &str| format!("/P << /MCID 0 >> BDC BT /F1 12 Tf {body} ET EMC");
    assert_eq!(
        joined(0, &sequence("10 100 Td (Click) Tj 28.71 0 Td (here) Tj")),
        "Click here",
        "2.7pt after a 26pt word at 12pt"
    );
    assert_eq!(
        joined(0, &sequence("10 100 Td (Click) Tj 27.9 0 Td (here) Tj")),
        "Clickhere",
        "1.9pt is tracking, not a space"
    );
}

#[test]
fn a_figure_with_an_empty_alt_keeps_its_own_text() {
    let bytes = small_reading_document(
        "",
        0,
        &text_run(0, 10, 100, "label"),
        &["<< /S /Figure /Pg 3 0 R /Alt () /K 0 >>"],
    );
    assert_eq!(reading_text_of(&bytes, 0), "label");
}

/// The replacement is said on the page of the first real content below the
/// element. An artifact run on an earlier page does not move it there.
#[test]
fn a_replacement_is_said_on_the_page_of_the_first_content_that_is_not_an_artifact() {
    let page_content = |data: &str| stream_object(data);
    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 7 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 200] /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".to_vec(),
        page_content(
            "/Artifact BMC /P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (header) Tj ET EMC EMC",
        ),
        page_content("/P << /MCID 1 >> BDC BT /F1 12 Tf 10 100 Td (real) Tj ET EMC"),
        b"<< /Type /StructTreeRoot /K [8 0 R] >>".to_vec(),
        b"<< /S /P /ActualText (REPL) /K [<< /Type /MCR /Pg 3 0 R /MCID 0 >> \
          << /Type /MCR /Pg 4 0 R /MCID 1 >>] >>"
            .to_vec(),
    ]);
    assert_eq!(reading_text_of(&bytes, 0), "");
    assert_eq!(reading_text_of(&bytes, 1), "REPL");
}
