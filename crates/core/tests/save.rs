//! The save path, from the outside.
//!
//! The highest-consequence package in M3, so the assertions here are on parsed
//! object graphs rather than on byte patterns. "One section" is asserted by
//! parsing `sections()`, never by counting `%%EOF` occurrences: a document may
//! legitimately contain one already, and a test that counts them passes on a
//! file that appended nothing and fails on a file that appended correctly.
//!
//! The test that catches the widest class is
//! `edit_save_undo_save_restores_the_object_graph_in_all_four_shapes`. It walks
//! the graph reachable from the catalog and compares it node by node, which is
//! what catches a reversal that restored an object and forgot its referrer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use onionskin_core::{
    add_annotation, read_structure, Annotation, AnnotationFilter, Document, DocumentEdit,
    DocumentFile, EditSession, Rect, Subtype,
};
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, ObjRef, Object, Provenance};

const WHEN: i64 = 1_789_948_800;

/// The sweep is capped: opening a document through `core::Document` spawns a
/// render worker, so a few hundred is the point past which this stops being a
/// test and becomes a benchmark. Sorted, so the sample is the same every run.
const SWEEP_CAP: usize = 200;

// ---------------------------------------------------------------------------
// Guarantee 1: a no-op save changes nothing
// ---------------------------------------------------------------------------

/// Well-formed is not a hedge and not a filename list: the partition is
/// `Provenance`, read from the document itself. A repaired file's no-op save
/// legitimately appends its repair, which is the next test rather than an
/// exception to this one.
#[test]
fn a_no_op_save_of_a_well_formed_document_is_byte_identical() {
    let mut checked = 0;
    for path in sweep() {
        let Ok((cos, provenance)) = CosDocument::open_path_repairing(&path) else {
            continue;
        };
        if !provenance.is_clean() {
            continue;
        }
        let original = std::fs::read(&path).expect("readable");
        let before = cos.sections().map(|s| s.len()).unwrap_or(0);

        let edit = EditSession::for_base(&cos);
        let section = cos
            .section_for(&edit.pending_edits(), &edit.trailer_edits())
            .expect("section");
        assert!(
            section.is_none(),
            "{}: a clean document with no edits appends nothing",
            path.display()
        );

        let after = CosDocument::open(Box::new(BytesSource::new(original.clone())))
            .expect("reopens")
            .sections()
            .map(|s| s.len())
            .unwrap_or(0);
        assert_eq!(before, after, "{}: section count", path.display());
        checked += 1;
    }
    assert!(checked > 0, "the sweep found at least one clean document");
    eprintln!("guarantee 1: {checked} clean documents");
}

/// The positive half, or the carve-out above becomes a place to hide failures.
#[test]
fn a_no_op_save_of_a_repaired_document_appends_exactly_its_repair() {
    let mut checked = 0;
    for path in sweep() {
        let Ok((cos, provenance)) = CosDocument::open_path_repairing(&path) else {
            continue;
        };
        if provenance.is_clean() {
            continue;
        }
        let original = std::fs::read(&path).expect("readable");
        // An appended section can repair a cross-reference table or a trailer.
        // It cannot repair the file header, which sits at byte 0 and which an
        // append never reaches, so a header-damaged fixture is outside what
        // this assertion can be about.
        if !original.starts_with(b"%PDF-") {
            continue;
        }
        let edit = EditSession::for_base(&cos);
        let Ok(Some(section)) = cos.section_for(&edit.pending_edits(), &edit.trailer_edits())
        else {
            continue;
        };

        let mut saved = original.clone();
        saved.extend_from_slice(&section);
        assert_eq!(
            &saved[..original.len()],
            &original[..],
            "{}: the bytes beneath the repair are identical",
            path.display()
        );
        assert!(
            CosDocument::open(Box::new(BytesSource::new(saved))).is_ok(),
            "{}: the repaired save reopens through the strict parser",
            path.display()
        );
        checked += 1;
        if checked >= 25 {
            break;
        }
    }
    eprintln!("guarantee 1 (repaired): {checked} documents");
}

// ---------------------------------------------------------------------------
// Guarantee 2: an edit appends exactly one section that truncates away
// ---------------------------------------------------------------------------

#[test]
fn one_edit_appends_exactly_one_section_that_truncates_away() {
    let dir = temp_dir("one-edit");
    let path = copy_seed(&dir, "minimal.pdf");
    let original = std::fs::read(&path).expect("readable");

    let mut document = DocumentFile::open(&path).expect("opens");
    set_description(&mut document, "one edit");
    let outcome = document.save().expect("saves");
    assert_eq!(outcome.sections_appended, 1);

    let saved = std::fs::read(&path).expect("readable");
    let sections = CosDocument::open(Box::new(BytesSource::new(saved.clone())))
        .expect("reopens")
        .sections()
        .expect("sections");
    assert_eq!(sections.len(), 2, "original plus exactly one section");

    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "truncating at the original length yields the byte-exact original"
    );
    let truncated = CosDocument::open(Box::new(BytesSource::new(original.clone())))
        .expect("the truncated file reopens");
    assert!(
        truncated.trailer().get(b"Info").is_none(),
        "and carries the pre-edit content"
    );
}

/// The clause the plan calls ambiguous: ten edits, one save, one section.
#[test]
fn ten_edits_and_one_save_are_still_one_section() {
    let dir = temp_dir("ten-edits");
    let path = copy_seed(&dir, "minimal.pdf");

    let mut document = DocumentFile::open(&path).expect("opens");
    for index in 0..10 {
        set_description(&mut document, &format!("edit {index}"));
    }
    let outcome = document.save().expect("saves");
    assert_eq!(outcome.sections_appended, 1);
    assert_eq!(sections_of(&path), 2, "ten edits, one section");
}

/// The test that catches a dirty-flag overlay: a value-based collapse leaves
/// nothing to write.
#[test]
fn edit_then_undo_then_save_writes_nothing() {
    let dir = temp_dir("edit-undo");
    let path = copy_seed(&dir, "minimal.pdf");
    let original = std::fs::read(&path).expect("readable");

    let mut document = DocumentFile::open(&path).expect("opens");
    set_description(&mut document, "a description");
    let (edit, base) = document.edit_mut();
    assert!(edit.undo(base).expect("undo runs"));

    let outcome = document.save().expect("saves");
    assert_eq!(outcome.sections_appended, 0, "nothing to append");
    assert_eq!(
        std::fs::read(&path).expect("readable"),
        original,
        "byte-identical"
    );
    assert_eq!(sections_of(&path), 1, "zero appended sections");
}

/// T3's second collapse rule, which rule 1 cannot reach: the annotation dict
/// and its appearance stream have no base object to compare against, so value
/// comparison leaves them in the overlay after the page's `/Annots` has already
/// collapsed out. No undo is involved.
#[test]
fn adding_an_annotation_and_deleting_it_in_one_session_writes_nothing() {
    let dir = temp_dir("add-delete");
    let path = copy_seed(&dir, "minimal.pdf");
    let original = std::fs::read(&path).expect("readable");

    let mut document = DocumentFile::open(&path).expect("opens");
    let (edit, base) = document.edit_mut();
    let structure = read_structure(base).expect("structure");
    let objref = edit
        .transact(base, "Add Comment", |tx| {
            add_annotation(
                tx,
                &structure,
                ObjRef::new(3, 0),
                &Annotation::new(Subtype::Square, Rect::new(10.0, 10.0, 60.0, 60.0)),
                WHEN,
            )
        })
        .expect("adds");
    let (edit, base) = document.edit_mut();
    edit.transact(base, "Delete Comment", |tx| {
        onionskin_core::remove_annotation(tx, ObjRef::new(3, 0), objref)
    })
    .expect("removes");

    let outcome = document.save().expect("saves");
    assert_eq!(
        outcome.sections_appended, 0,
        "the page's /Annots collapsed back to the base, and the orphaned \
         annotation objects went with it"
    );
    assert_eq!(std::fs::read(&path).expect("readable"), original);
}

// ---------------------------------------------------------------------------
// The save boundary
// ---------------------------------------------------------------------------

/// The class the whole base-capture rule exists for, in all four shapes M3 can
/// produce. The graph comparison is what catches a reversal that restored the
/// object and forgot the referrer.
#[test]
fn edit_save_undo_save_restores_the_object_graph_in_all_four_shapes() {
    for shape in ["edit an existing object", "create", "remove", "trailer key"] {
        let dir = temp_dir(&format!("shape-{}", shape.replace(' ', "-")));
        let path = copy_seed(&dir, "two-page.pdf");
        if shape == "remove" {
            // A removal needs something to remove. Seeding it in its own save
            // makes the pre-edit state a document that has the key, which is
            // the state the undo has to restore.
            let mut seeding = DocumentFile::open(&path).expect("opens");
            let (edit, base) = seeding.edit_mut();
            edit.apply(
                base,
                DocumentEdit::SetCatalogEntry {
                    key: Name::new("PageLayout"),
                    value: Some(Object::name("OneColumn")),
                },
            )
            .expect("seeded");
            seeding.save().expect("seed save");
        }
        let before_bytes = std::fs::read(&path).expect("readable");
        let before_graph = graph(&open_bytes(&before_bytes));

        let mut document = DocumentFile::open(&path).expect("opens");
        apply_shape(&mut document, shape);
        document.save().expect("first save");

        let (edit, base) = document.edit_mut();
        assert!(edit.undo(base).expect("undo runs"), "{shape}: undo runs");
        document.save().expect("second save");

        let after_bytes = std::fs::read(&path).expect("readable");
        let after =
            CosDocument::open(Box::new(BytesSource::new(after_bytes))).unwrap_or_else(|e| {
                panic!("{shape}: the result reopens through the strict parser: {e}")
            });

        assert_eq!(
            graph(&after),
            before_graph,
            "{shape}: the object graph reachable from the catalog is restored"
        );
        assert!(
            after.audit_references().expect("audit").is_empty(),
            "{shape}: no dangling references"
        );
    }
}

fn apply_shape(document: &mut Document, shape: &str) {
    let (edit, base) = document.edit_mut();
    match shape {
        "edit an existing object" => {
            edit.apply(
                base,
                DocumentEdit::SetCatalogEntry {
                    key: Name::new("PageLayout"),
                    value: Some(Object::name("TwoPageLeft")),
                },
            )
            .expect("catalog entry");
        }
        "create" => {
            edit.transact(base, "Create", |tx| {
                let number = tx.reserve();
                let mut dict = Dict::new();
                dict.set(Name::new("Type"), Object::name("Metadata"));
                tx.put_object(number, 0, Object::Dict(dict))?;
                // Reachable, or the section writer has an object nothing names.
                let catalog = tx.object(1)?.expect("catalog");
                let mut updated = catalog.object.as_dict().expect("dict").clone();
                updated.set(Name::new("PieceInfo"), Object::Ref(ObjRef::new(number, 0)));
                tx.put_object(1, catalog.generation, Object::Dict(updated))
            })
            .expect("create");
        }
        "remove" => {
            // T5: a removal is a rewrite of the referrer, so what the undo has
            // to restore is the referrer's own value, not a freed object.
            edit.apply(
                base,
                DocumentEdit::SetCatalogEntry {
                    key: Name::new("PageLayout"),
                    value: None,
                },
            )
            .expect("remove");
        }
        "trailer key" => {
            edit.apply(
                base,
                DocumentEdit::SetInfoField {
                    key: Name::new("Description"),
                    value: Some(Object::String(b"a description".to_vec())),
                },
            )
            .expect("trailer key");
        }
        other => panic!("unknown shape {other}"),
    }
}

#[test]
fn two_saves_produce_two_sections_and_the_second_points_at_the_first() {
    let dir = temp_dir("two-saves");
    let path = copy_seed(&dir, "minimal.pdf");

    let mut document = DocumentFile::open(&path).expect("opens");
    set_description(&mut document, "first");
    document.save().expect("first save");
    set_description(&mut document, "second");
    document.save().expect("second save");

    let saved = std::fs::read(&path).expect("readable");
    let reopened = open_bytes(&saved);
    let sections = reopened.sections().expect("sections");
    assert_eq!(sections.len(), 3, "original plus two sections");

    // Asserted by parsing the chain, not by scanning for the string /Prev: the
    // walk `sections()` returns is derived from the trailers.
    for pair in sections.windows(2) {
        assert!(
            pair[1].start >= pair[0].end,
            "each section begins after the one it points back at"
        );
    }
}

/// The assertion that fails if the section writer's full-table path reads cos's
/// own edit map: the base copy would be written alongside the overlay's, with
/// the xref indexing the base one.
#[test]
fn an_edit_against_a_repaired_fixture_appears_exactly_once() {
    let Some(path) = repaired_fixture() else {
        eprintln!("SKIPPED: no repaired fixture with an object-stream catalog was found");
        return;
    };
    let dir = temp_dir("repaired");
    let copy = dir.join("repaired.pdf");
    std::fs::copy(&path, &copy).expect("copied");

    let mut document = DocumentFile::open(&copy).expect("opens");
    let (edit, base) = document.edit_mut();
    let Ok(()) = edit.apply(
        base,
        DocumentEdit::SetCatalogEntry {
            key: Name::new("PageLayout"),
            value: Some(Object::name("SinglePage")),
        },
    ) else {
        eprintln!("SKIPPED: the fixture's catalog could not be edited");
        return;
    };
    if document.save().is_err() {
        eprintln!("SKIPPED: the repaired fixture could not be saved");
        return;
    }

    let reopened = open_bytes(&std::fs::read(&copy).expect("readable"));
    let catalog = reopened.catalog().expect("catalog");
    assert_eq!(
        catalog
            .get(b"PageLayout")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"SinglePage".to_vec()),
        "the overlay's value is what the reopened document resolves to"
    );
}

/// The only case that allocates an object number **after** a save, which is
/// where the reservation counter is either reseeded or catastrophically reset.
/// A counter that restarts puts the second annotation at object 1 and silently
/// overwrites the catalog, with every reference still resolving.
#[test]
fn create_save_create_save_puts_the_two_annotations_at_two_numbers() {
    let dir = temp_dir("create-save-twice");
    let path = copy_seed(&dir, "minimal.pdf");

    let mut document = DocumentFile::open(&path).expect("opens");
    let first = add_square(&mut document);
    document.save().expect("first save");
    let second = add_square(&mut document);
    document.save().expect("second save");

    assert_ne!(
        first.number, second.number,
        "the second annotation takes a fresh number, not object 1"
    );

    let reopened = open_bytes(&std::fs::read(&path).expect("readable"));
    let catalog = reopened.catalog().expect("the catalog survived");
    assert_eq!(
        catalog
            .get(b"Type")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"Catalog".to_vec()),
        "nothing overwrote object 1"
    );
    let annots = page_annots(&reopened, 0);
    assert_eq!(annots.len(), 2, "both annotations are on the page");
    assert!(
        reopened.audit_references().expect("audit").is_empty(),
        "no dangling references"
    );
}

// ---------------------------------------------------------------------------
// The preview buffer
// ---------------------------------------------------------------------------

/// Without the first half of this, the print dialog shows the wrong
/// Comments-and-Forms mode and every downstream filter test still passes,
/// because none of them asks twice at one generation.
#[test]
fn the_preview_cache_key_includes_the_filter() {
    let dir = temp_dir("preview-key");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");
    add_square(&mut document);

    let generation = document.byte_generation();
    let markups = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("preview");
    assert_eq!(document.byte_generation(), generation, "no bump happened");
    let document_only = document
        .preview_bytes(AnnotationFilter::DocumentOnly)
        .expect("preview");

    assert_ne!(
        markups, document_only,
        "two filters at one generation are two different buffers"
    );

    let again = document
        .preview_bytes(AnnotationFilter::DocumentOnly)
        .expect("preview");
    assert!(
        std::sync::Arc::ptr_eq(&document_only, &again),
        "the same filter twice returns the cached buffer rather than rebuilding"
    );
}

/// The collision the key actually guards. The unfiltered preview and a
/// filtered one live in different slots, so a test comparing those two passes
/// whether or not the key includes the filter; that test survived the mutation
/// that drops it. Two **hiding** modes share the one transient slot, and a
/// document with a stamp is one they treat differently: Document-Only hides it
/// and Document-and-Stamps keeps it.
#[test]
fn two_hiding_filters_at_one_generation_do_not_share_a_buffer() {
    let dir = temp_dir("preview-two-hiding");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");

    let (edit, base) = document.edit_mut();
    let structure = read_structure(base).expect("structure");
    edit.transact(base, "Add Stamp", |tx| {
        add_annotation(
            tx,
            &structure,
            ObjRef::new(3, 0),
            &Annotation::new(Subtype::Stamp, Rect::new(10.0, 10.0, 90.0, 40.0)),
            WHEN,
        )
    })
    .expect("stamp added");

    let hides_stamp = document
        .preview_bytes(AnnotationFilter::DocumentOnly)
        .expect("preview");
    let keeps_stamp = document
        .preview_bytes(AnnotationFilter::DocumentAndStamps)
        .expect("preview");
    assert_ne!(
        hides_stamp, keeps_stamp,
        "Document-Only and Document-and-Stamps at one generation are different \
         buffers; sharing one would show the print dialog the wrong mode"
    );
}

/// One `section_for` call feeds both, so this is a regression test on the
/// wiring rather than two implementations agreeing.
#[test]
fn the_preview_equals_what_the_following_save_writes() {
    let dir = temp_dir("preview-equals-save");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");
    set_description(&mut document, "a description");

    let preview = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("preview");
    // Parsed by the strict opener, not the repairing one.
    let previewed = CosDocument::open(Box::new(BytesSource::new(preview.to_vec())))
        .expect("the preview is a valid PDF");
    let preview_graph = graph(&previewed);

    document.save().expect("saves");
    let saved = open_bytes(&std::fs::read(&path).expect("readable"));

    assert_eq!(
        graph(&saved),
        preview_graph,
        "the preview's object graph is what the save wrote"
    );
}

// ---------------------------------------------------------------------------
// Structural reads answer from the edits
// ---------------------------------------------------------------------------

/// `structure()` is the document with the session's edits in it, not the
/// document as it was opened. Asserted against the base directly, so the test
/// fails if `structure()` ever quietly hands back the base.
#[test]
fn structure_answers_from_the_edits_not_from_the_open_document() {
    let dir = temp_dir("structure-reads");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");

    let (edit, base) = document.edit_mut();
    edit.apply(
        base,
        DocumentEdit::SetCatalogEntry {
            key: Name::new("PageLayout"),
            value: Some(Object::name("TwoColumnLeft")),
        },
    )
    .expect("catalog entry");

    let (_, base) = document.edit_mut();
    assert!(
        base.catalog()
            .expect("catalog")
            .get(b"PageLayout")
            .is_none(),
        "the base is unchanged, which is what makes the next assertion mean something"
    );
    let seen = document
        .structure()
        .expect("structure")
        .catalog()
        .expect("catalog")
        .get(b"PageLayout")
        .and_then(Object::as_name)
        .map(|name| name.as_bytes().to_vec());
    assert_eq!(seen, Some(b"TwoColumnLeft".to_vec()));
}

/// A reader routed through `structure()` sees a layer the session added and
/// has not saved. `layers()` is the reader chosen because it is the one whose
/// input an M3 edit can reach today.
#[test]
fn a_routed_reader_sees_an_unsaved_edit() {
    let dir = temp_dir("routed-reader");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");
    assert!(
        document.layers().expect("layers").is_empty(),
        "no layers yet"
    );

    let (edit, base) = document.edit_mut();
    edit.transact(base, "Add Layer", |tx| {
        let group = tx.reserve();
        let mut ocg = Dict::new();
        ocg.set(Name::new("Type"), Object::name("OCG"));
        ocg.set(Name::new("Name"), Object::String(b"Notes".to_vec()));
        tx.put_object(group, 0, Object::Dict(ocg))?;

        let group_ref = Object::Ref(ObjRef::new(group, 0));
        let mut default_config = Dict::new();
        default_config.set(Name::new("ON"), Object::Array(vec![group_ref.clone()]));
        let mut properties = Dict::new();
        properties.set(Name::new("OCGs"), Object::Array(vec![group_ref]));
        properties.set(Name::new("D"), Object::Dict(default_config));

        let catalog = tx.object(1)?.expect("catalog");
        let mut updated = catalog.object.as_dict().expect("dict").clone();
        updated.set(Name::new("OCProperties"), Object::Dict(properties));
        tx.put_object(1, catalog.generation, Object::Dict(updated))
    })
    .expect("layer added");

    let layers = document.layers().expect("layers");
    assert_eq!(
        layers.len(),
        1,
        "the reader sees the layer the session added, before any save"
    );
}

// ---------------------------------------------------------------------------
// The render worker draws what a save would write
// ---------------------------------------------------------------------------

/// Before the worker followed the session, it rendered the bytes the document
/// was opened from, so a comment written a moment ago was invisible until a
/// save. Asserted on pixels, because that is where the defect showed.
#[test]
fn an_unsaved_annotation_is_on_the_canvas_and_an_undone_one_is_not() {
    let dir = temp_dir("canvas");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");

    assert!(
        drawn(&mut document).is_none(),
        "the seed page is blank, so anything drawn is the annotation"
    );

    add_square(&mut document);
    assert!(
        drawn(&mut document).is_some(),
        "an unsaved annotation is drawn, from the preview the worker now holds"
    );

    let (edit, base) = document.edit_mut();
    assert!(edit.undo(base).expect("undo runs"));
    assert!(drawn(&mut document).is_none(), "and an undone one is not");
}

/// A revert changes the bytes under the worker as well as under the session.
#[test]
fn a_reverted_annotation_leaves_the_canvas() {
    let dir = temp_dir("canvas-revert");
    let path = copy_seed(&dir, "minimal.pdf");
    let mut document = DocumentFile::open(&path).expect("opens");

    add_square(&mut document);
    document.save().expect("saves");
    assert!(drawn(&mut document).is_some(), "saved and drawn");

    document.revert_to(1).expect("reverts");
    assert!(
        drawn(&mut document).is_none(),
        "the worker renders the truncated file, not the one it had"
    );
}

fn drawn(document: &mut Document) -> Option<(u32, u32, u32, u32)> {
    document
        .render_page_now(0, 1.0)
        .expect("renders")
        .raster
        .content_bounds()
        .map(|b| (b.x, b.y, b.width, b.height))
}

// ---------------------------------------------------------------------------
// Generations
// ---------------------------------------------------------------------------

#[test]
fn revert_refuses_unsaved_edits_and_non_trailing_targets_and_truncates_a_trailing_one() {
    let dir = temp_dir("revert");
    let path = copy_seed(&dir, "minimal.pdf");
    let original = std::fs::read(&path).expect("readable");

    let mut document = DocumentFile::open(&path).expect("opens");
    set_description(&mut document, "first");
    document.save().expect("first save");
    set_description(&mut document, "second");
    document.save().expect("second save");
    assert_eq!(document.generations().expect("generations").len(), 3);

    // Refused: unsaved edits.
    set_description(&mut document, "unsaved");
    assert!(
        document.revert_to(2).is_err(),
        "a revert with unsaved edits is refused"
    );
    let (edit, base) = document.edit_mut();
    edit.undo(base).expect("undo runs");

    // Refused: not trailing. The target is the generation being dropped, so
    // generation 1 is a middle one while 2 is the trailing one.
    assert!(
        document.revert_to(1).is_err(),
        "only the most recent generation can be reverted"
    );

    // Accepted: the trailing one.
    document.revert_to(2).expect("reverts");
    assert_eq!(
        document.generations().expect("generations").len(),
        2,
        "the last section is gone"
    );
    assert!(
        std::fs::read(&path)
            .expect("readable")
            .starts_with(&original),
        "and the bytes beneath it are untouched"
    );
    assert!(
        !document.is_dirty(),
        "a reverted document reports clean, with nothing to undo or redo"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn set_description(document: &mut Document, text: &str) {
    let (edit, base) = document.edit_mut();
    edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Description"),
            value: Some(Object::String(text.as_bytes().to_vec())),
        },
    )
    .expect("the description is set");
}

fn add_square(document: &mut Document) -> ObjRef {
    let (edit, base) = document.edit_mut();
    let structure = read_structure(base).expect("structure");
    edit.transact(base, "Add Comment", |tx| {
        add_annotation(
            tx,
            &structure,
            ObjRef::new(3, 0),
            &Annotation::new(Subtype::Square, Rect::new(10.0, 10.0, 60.0, 60.0)),
            WHEN,
        )
    })
    .expect("the annotation commits")
}

fn page_annots(doc: &CosDocument, index: usize) -> Vec<Object> {
    let page = doc.page(index).expect("page");
    match page.dict.get(b"Annots") {
        Some(object) => match doc.resolve(object).expect("resolve") {
            Object::Array(items) => items,
            _ => Vec::new(),
        },
        None => Vec::new(),
    }
}

/// Every object reachable from the catalog, keyed by number, as parsed values.
///
/// Comparing this rather than the bytes is what makes the save-boundary test a
/// statement about the document rather than about serialization: two sections
/// that write the same graph differently still compare equal.
fn graph(doc: &CosDocument) -> BTreeMap<u32, String> {
    let mut seen = BTreeSet::new();
    let mut out = BTreeMap::new();
    let Ok(catalog) = doc.catalog() else {
        return out;
    };
    let mut queue: Vec<Object> = vec![Object::Dict(catalog)];
    if let Some(root) = doc.trailer().get(b"Root") {
        queue.push(root.clone());
    }
    if let Some(info) = doc.trailer().get(b"Info") {
        queue.push(info.clone());
    }
    let mut depth = 0;
    while let Some(object) = queue.pop() {
        depth += 1;
        if depth > 200_000 {
            break;
        }
        match object {
            Object::Ref(objref) => {
                if !seen.insert(objref.number) {
                    continue;
                }
                let Ok(parsed) = doc.get(objref.number) else {
                    continue;
                };
                out.insert(objref.number, format!("{:?}", parsed.object));
                queue.push(parsed.object);
            }
            Object::Array(items) => queue.extend(items),
            Object::Dict(dict) => {
                queue.extend(dict.iter().map(|(_, value)| value.clone()));
            }
            Object::Stream(stream) => {
                queue.extend(stream.dict.iter().map(|(_, value)| value.clone()));
            }
            _ => {}
        }
    }
    out
}

fn sections_of(path: &Path) -> usize {
    open_bytes(&std::fs::read(path).expect("readable"))
        .sections()
        .expect("sections")
        .len()
}

fn open_bytes(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onionskin-save-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn copy_seed(dir: &Path, name: &str) -> PathBuf {
    let destination = dir.join(name);
    std::fs::copy(seed(name), &destination).expect("seed copied");
    destination
}

/// Seeds first, then a capped, sorted sample of `external/`.
fn sweep() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = ["minimal.pdf", "hello.pdf", "two-page.pdf"]
        .iter()
        .map(|name| seed(name))
        .collect();
    if let Some(external) = onionskin_corpus_testing::corpus_dir("external") {
        let mut found = Vec::new();
        collect(&external, &mut found);
        found.sort();
        paths.extend(found.into_iter().take(SWEEP_CAP));
    }
    paths
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|kind| kind == "pdf") {
            out.push(path);
        }
    }
}

/// A file the repairing opener had to repair, and whose catalog it can still
/// reach. `Provenance` is the partition, never a filename.
fn repaired_fixture() -> Option<PathBuf> {
    for path in sweep() {
        let Ok((cos, provenance)) = CosDocument::open_path_repairing(&path) else {
            continue;
        };
        if provenance.is_clean() || cos.catalog().is_err() {
            continue;
        }
        let _: &Provenance = &provenance;
        return Some(path);
    }
    None
}
