//! The overlay-taking section builder: the caller holds the edits, the
//! document holds none, and one call serves both a save and the preview that
//! has to agree with it byte for byte.

mod common;

use std::collections::BTreeMap;

use common::{classic_pdf, classic_pdf_covering, corpus_dir, skeleton};
use onionskin_cos::{
    BytesSource, Document, Error, Holder, Name, Object, PendingEdit, Provenance, RepairReason,
};

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

fn last_startxref(bytes: &[u8]) -> i64 {
    let at = bytes
        .windows(b"startxref\n".len())
        .rposition(|window| window == b"startxref\n")
        .expect("the fixture has startxref")
        + b"startxref\n".len();
    let end = bytes[at..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |offset| at + offset);
    std::str::from_utf8(&bytes[at..end])
        .expect("startxref is ASCII")
        .parse()
        .expect("startxref is an integer")
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

#[test]
fn a_scan_repaired_overlay_may_not_write_object_zero() {
    let original = classic_pdf_covering(&skeleton(), &[], 0);
    let (document, provenance) = Document::open_repairing(Box::new(BytesSource::new(original)))
        .expect("the scan-repaired fixture opens");
    match provenance {
        Provenance::Repaired(report) => assert!(report.rebuilt_by_scan),
        Provenance::Clean => panic!("the incomplete xref must be rebuilt by scan"),
    }
    assert_eq!(document.xref().get(0), None);

    let overlay: BTreeMap<u32, PendingEdit> = [set(0, Object::Integer(7))].into_iter().collect();
    match document.section_for(&overlay, &BTreeMap::new()) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("free list"), "{detail}"),
        other => panic!("object zero must remain the free-list head: {other:?}"),
    }
}

#[test]
fn effective_encryption_in_an_overlay_is_refused_but_null_is_writable() {
    let original = fixture();
    let document = open(&original);
    let mut encryption = onionskin_cos::Dict::new();
    encryption.set("Filter", Object::name("Standard"));

    let encrypted: BTreeMap<Name, Option<Object>> =
        [(Name::new("Encrypt"), Some(Object::Dict(encryption)))]
            .into_iter()
            .collect();
    match document.section_for(&BTreeMap::new(), &encrypted) {
        Err(Error::EncryptedWrite) => {}
        other => panic!("an effective Encrypt entry must be refused, got {other:?}"),
    }

    for value in [None, Some(Object::Null)] {
        let removed: BTreeMap<Name, Option<Object>> =
            [(Name::new("Encrypt"), value)].into_iter().collect();
        let saved = saved_with(&document, &original, &BTreeMap::new(), &removed);
        let reopened = open(&saved);
        assert_eq!(reopened.page_count().ok(), Some(1));
        assert!(matches!(
            reopened.trailer().get(b"Encrypt"),
            None | Some(Object::Null)
        ));
    }
}

#[test]
fn an_indirect_effective_encryption_entry_is_refused() {
    let original = fixture();
    let document = open(&original);
    let number = document.next_object_number();
    let mut encryption = onionskin_cos::Dict::new();
    encryption.set("Filter", Object::name("Standard"));
    let overlay: BTreeMap<u32, PendingEdit> = [set(number, Object::Dict(encryption))]
        .into_iter()
        .collect();
    let trailer_edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Encrypt"),
        Some(Object::Ref(onionskin_cos::ObjRef::new(number, 0))),
    )]
    .into_iter()
    .collect();

    match document.section_for(&overlay, &trailer_edits) {
        Err(Error::EncryptedWrite) => {}
        other => panic!("an indirect effective Encrypt entry must be refused, got {other:?}"),
    }
}

#[test]
fn an_internal_encryption_edit_is_refused_before_save() {
    let original = fixture();
    let mut encryption = onionskin_cos::Dict::new();
    encryption.set("Filter", Object::name("Standard"));
    let mut document = open(&original);
    document.set_trailer_entry("Encrypt", Object::Dict(encryption));
    match document.save_to_vec() {
        Err(Error::EncryptedWrite) => {}
        other => panic!("an internal Encrypt edit must be refused, got {other:?}"),
    }

    let mut cleared = open(&original);
    cleared.set_trailer_entry("Encrypt", Object::Null);
    let saved = cleared
        .save_to_vec()
        .expect("a null internal Encrypt edit is writable");
    let reopened = open(&saved);
    assert_eq!(reopened.page_count().ok(), Some(1));
    assert!(matches!(
        reopened.trailer().get(b"Encrypt"),
        None | Some(Object::Null)
    ));
}

#[test]
fn encrypted_path_saves_leave_existing_destinations_untouched() {
    let scratch = std::env::temp_dir().join(format!(
        "onionskin-cos-encrypted-save-{}",
        std::process::id()
    ));
    std::fs::create_dir(&scratch).expect("unique scratch directory");

    let direct_path = scratch.join("direct.pdf");
    std::fs::write(&direct_path, b"direct sentinel").expect("direct sentinel");
    let mut direct = open(&fixture());
    let mut encryption = onionskin_cos::Dict::new();
    encryption.set("Filter", Object::name("Standard"));
    direct.set_trailer_entry("Encrypt", Object::Dict(encryption));
    assert!(matches!(
        direct.save_to_path(&direct_path),
        Err(Error::EncryptedWrite)
    ));
    assert_eq!(
        std::fs::read(&direct_path).expect("direct destination"),
        b"direct sentinel"
    );

    let overlay_path = scratch.join("overlay.pdf");
    std::fs::write(&overlay_path, b"overlay sentinel").expect("overlay sentinel");
    let document = open(&fixture());
    let mut encryption = onionskin_cos::Dict::new();
    encryption.set("Filter", Object::name("Standard"));
    let trailer_edits: BTreeMap<Name, Option<Object>> =
        [(Name::new("Encrypt"), Some(Object::Dict(encryption)))]
            .into_iter()
            .collect();
    assert!(matches!(
        document.save_overlay_to_path(&BTreeMap::new(), &trailer_edits, &overlay_path),
        Err(Error::EncryptedWrite)
    ));
    assert_eq!(
        std::fs::read(&overlay_path).expect("overlay destination"),
        b"overlay sentinel"
    );

    std::fs::remove_dir_all(&scratch).expect("scratch cleans up");
}

#[test]
fn base_only_compressed_edits_do_not_change_an_external_section() {
    for warm in [false, true] {
        for (number, delete) in [(5, false), (5, true), (4, false), (4, true)] {
            let original = repaired_with_compressed_objects(false, b"<</Type/Spare/Which 6>>");
            let (base, base_provenance) =
                Document::open_repairing(Box::new(BytesSource::new(original.clone())))
                    .expect("the base fixture opens");
            assert!(matches!(base_provenance, Provenance::Repaired(_)));
            let expected = base
                .section_for(&BTreeMap::new(), &BTreeMap::new())
                .expect("the untouched base section builds");

            let (mut document, provenance) =
                Document::open_repairing(Box::new(BytesSource::new(original)))
                    .expect("the edited fixture opens");
            assert!(matches!(provenance, Provenance::Repaired(_)));
            if warm {
                document.get(4).expect("the container resolves");
                document.get(5).expect("member 5 resolves");
                document.get(6).expect("member 6 resolves");
            }
            if delete {
                document
                    .delete_object(number)
                    .expect("the selected object can be deleted");
            } else {
                document
                    .set_object(number, 0, Object::name("internal edit"))
                    .expect("the selected object can be edited");
            }
            if delete {
                match document.get(number) {
                    Err(Error::MissingObject(objref)) => {
                        assert_eq!(objref, onionskin_cos::ObjRef::new(number, 1))
                    }
                    other => panic!("the deleted object must be pending-missing: {other:?}"),
                }
            } else {
                assert_eq!(
                    document
                        .get(number)
                        .expect("the edited object is pending")
                        .object,
                    Object::name("internal edit")
                );
            }

            assert_eq!(
                document
                    .section_for(&BTreeMap::new(), &BTreeMap::new())
                    .expect("the external empty overlay section builds"),
                expected,
                "an external section reads only the base, warm={warm}, number={number}, delete={delete}"
            );
            if delete {
                match document.get(number) {
                    Err(Error::MissingObject(objref)) => {
                        assert_eq!(objref, onionskin_cos::ObjRef::new(number, 1))
                    }
                    other => panic!("the deleted object must remain pending-missing: {other:?}"),
                }
            } else {
                assert_eq!(
                    document
                        .get(number)
                        .expect("the edited object remains pending")
                        .object,
                    Object::name("internal edit")
                );
            }
        }
    }
}

#[test]
fn internal_compressed_member_edits_are_applied_by_the_internal_save() {
    for (number, delete) in [(5, false), (5, true), (4, false), (4, true)] {
        let original = repaired_with_compressed_objects(false, b"<</Type/Spare/Which 6>>");
        let (mut document, provenance) =
            Document::open_repairing(Box::new(BytesSource::new(original)))
                .expect("the fixture opens");
        assert!(matches!(provenance, Provenance::Repaired(_)));
        if delete {
            document
                .delete_object(number)
                .expect("the selected object can be deleted");
        } else {
            document
                .set_object(number, 0, Object::name("internal save"))
                .expect("the selected object can be edited");
        }
        let saved = document.save_to_vec().expect("the internal save succeeds");
        let (reopened, _) = Document::open_repairing(Box::new(BytesSource::new(saved)))
            .expect("the internal save reopens");
        if delete {
            assert!(matches!(reopened.get(number), Err(Error::MissingObject(_))));
        } else {
            assert_eq!(
                reopened
                    .get(number)
                    .expect("the edited object resolves")
                    .object,
                Object::name("internal save")
            );
        }
    }
}

fn assert_indirect_dependency_isolation(number: u32, mutation: Object) {
    let original = repaired_with_compressed_objects(true, b"");
    let (base, provenance) = Document::open_repairing(Box::new(BytesSource::new(original.clone())))
        .expect("the indirect-dependency fixture opens");
    assert!(matches!(provenance, Provenance::Repaired(_)));
    assert!(provenance.report().is_some_and(|report| {
        report
            .reasons
            .iter()
            .any(|reason| matches!(reason, RepairReason::JunkBeforeHeader { .. }))
    }));
    let expected = base
        .section_for(&BTreeMap::new(), &BTreeMap::new())
        .expect("the untouched base section builds");
    assert!(
        expected.is_some(),
        "the repaired base owes a full-table section"
    );
    let expected_member5 = base.get(5).expect("the base member 5 resolves").object;
    let expected_member6 = base.get(6).expect("the base member 6 resolves").object;
    assert!(
        matches!(&expected_member6, Object::Dict(_)),
        "the identity Crypt and DecodeParms are accepted"
    );

    let (mut document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(original.clone())))
            .expect("the edited fixture opens");
    assert!(matches!(provenance, Provenance::Repaired(_)));
    document
        .set_object(number, 0, mutation.clone())
        .expect("the dependency can be edited");
    assert_eq!(
        document
            .get(number)
            .expect("the mutation is pending")
            .object,
        mutation,
        "the dependency mutation remains pending"
    );
    let actual = document
        .section_for(&BTreeMap::new(), &BTreeMap::new())
        .expect("the empty external overlay builds");
    assert!(
        actual.is_some(),
        "the repaired overlay owes a full-table section"
    );
    assert_eq!(
        actual, expected,
        "dependency {number} is resolved from the untouched base view"
    );
    let mut saved = original;
    saved.extend_from_slice(actual.as_ref().expect("the section is present"));
    let (reopened, reopened_provenance) =
        Document::open_repairing(Box::new(BytesSource::new(saved)))
            .expect("the repaired overlay reopens");
    assert_eq!(
        reopened_provenance.is_clean(),
        provenance.is_clean(),
        "the reopened fixture preserves its source repair provenance"
    );
    assert!(reopened_provenance.report().is_some_and(|report| {
        report
            .reasons
            .iter()
            .any(|reason| matches!(reason, RepairReason::JunkBeforeHeader { .. }))
    }));
    assert_eq!(
        reopened.get(5).expect("member 5 reopens").object,
        expected_member5
    );
    assert_eq!(
        reopened.get(6).expect("member 6 reopens").object,
        expected_member6
    );
    assert_eq!(reopened.page_count().ok(), Some(1));
    assert_eq!(
        document
            .get(number)
            .expect("the mutation remains pending")
            .object,
        mutation,
        "building an external section does not consume the dependency edit"
    );
}

#[test]
fn base_view_ignores_internal_length_dependency_edits() {
    assert_indirect_dependency_isolation(8, Object::Integer(0));
}

#[test]
fn base_view_ignores_internal_count_dependency_edits() {
    assert_indirect_dependency_isolation(9, Object::Integer(0));
}

#[test]
fn base_view_ignores_internal_first_dependency_edits() {
    assert_indirect_dependency_isolation(10, Object::Integer(0));
}

#[test]
fn base_view_ignores_internal_filter_dependency_edits() {
    assert_indirect_dependency_isolation(11, Object::name("Unsupported"));
}

#[test]
fn base_view_ignores_internal_decode_parameters_edits() {
    let mut dict = onionskin_cos::Dict::new();
    dict.set("Name", Object::name("Unsupported"));
    assert_indirect_dependency_isolation(12, Object::Dict(dict));
}

#[test]
fn a_compressed_target_accepts_generation_zero_and_rejects_one() {
    let original = repaired_with_compressed_objects(false, b"<</Type/Spare/Which 6>>");
    let (document, provenance) = Document::open_repairing(Box::new(BytesSource::new(original)))
        .expect("the compressed fixture opens");
    assert!(matches!(provenance, Provenance::Repaired(_)));

    let overlay_for = |generation| {
        let mut edits = BTreeMap::new();
        edits.insert(
            5,
            PendingEdit::Set {
                generation: 0,
                object: {
                    let mut dict = onionskin_cos::Dict::new();
                    dict.set(
                        "Points",
                        Object::Ref(onionskin_cos::ObjRef::new(6, generation)),
                    );
                    Object::Dict(dict)
                },
            },
        );
        edits
    };

    assert!(document
        .section_for(&overlay_for(0), &BTreeMap::new())
        .expect("the compressed generation zero target resolves")
        .is_some());
    match document.section_for(&overlay_for(1), &BTreeMap::new()) {
        Err(Error::DanglingReference { target, .. }) => {
            assert_eq!(target, onionskin_cos::ObjRef::new(6, 1));
        }
        other => panic!("a compressed target has generation zero only: {other:?}"),
    }
}

fn catalog_object(with_type: bool) -> Object {
    let mut catalog = onionskin_cos::Dict::new();
    if with_type {
        catalog.set("Type", Object::name("Catalog"));
    }
    catalog.set("Pages", Object::Ref(onionskin_cos::ObjRef::new(2, 0)));
    Object::Dict(catalog)
}

#[test]
fn an_introduced_root_must_be_an_indirect_nonzero_reference() {
    let document = open(&fixture());
    for value in [
        None,
        Some(Object::Null),
        Some(Object::Integer(1)),
        Some(Object::Ref(onionskin_cos::ObjRef::new(0, 0))),
    ] {
        let edits: BTreeMap<Name, Option<Object>> =
            [(Name::new("Root"), value)].into_iter().collect();
        match document.section_for(&BTreeMap::new(), &edits) {
            Err(Error::Unrecoverable { detail }) => assert!(detail.contains("Root"), "{detail}"),
            other => panic!("an invalid effective Root must be refused: {other:?}"),
        }
    }
}

#[test]
fn an_introduced_root_must_name_a_catalog_dictionary() {
    for replacement in [Object::Integer(7), catalog_object(false), {
        let mut page = onionskin_cos::Dict::new();
        page.set("Type", Object::name("Page"));
        Object::Dict(page)
    }] {
        let document = open(&fixture());
        let mut overlay = BTreeMap::new();
        overlay.insert(
            6,
            PendingEdit::Set {
                generation: 0,
                object: replacement,
            },
        );
        let trailer_edits: BTreeMap<Name, Option<Object>> = [(
            Name::new("Root"),
            Some(Object::Ref(onionskin_cos::ObjRef::new(6, 0))),
        )]
        .into_iter()
        .collect();
        match document.section_for(&overlay, &trailer_edits) {
            Err(Error::Unrecoverable { detail }) => assert!(detail.contains("catalog"), "{detail}"),
            other => panic!("an introduced Root must resolve to a Catalog: {other:?}"),
        }
    }
}

#[test]
fn a_valid_alternate_catalog_root_is_written_and_reopened() {
    let document = open(&fixture());
    let overlay: BTreeMap<u32, PendingEdit> = [(
        6,
        PendingEdit::Set {
            generation: 0,
            object: catalog_object(true),
        },
    )]
    .into_iter()
    .collect();
    let trailer_edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Root"),
        Some(Object::Ref(onionskin_cos::ObjRef::new(6, 0))),
    )]
    .into_iter()
    .collect();
    let section = document
        .section_for(&overlay, &trailer_edits)
        .expect("the alternate Catalog is valid")
        .expect("the alternate root writes a section");
    let mut saved = fixture();
    saved.extend_from_slice(&section);
    assert_eq!(open(&saved).page_count().ok(), Some(1));
}

#[test]
fn an_inherited_catalog_without_type_remains_saveable() {
    let mut bodies = skeleton();
    bodies[0] = b"<</Pages 2 0 R>>";
    let original = classic_pdf(&bodies, &[]);
    let document = open(&original);
    let edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Producer"),
        Some(Object::String(b"unrelated edit".to_vec())),
    )]
    .into_iter()
    .collect();
    let section = document
        .section_for(&BTreeMap::new(), &edits)
        .expect("an inherited catalog is not newly validated")
        .expect("the metadata edit writes a section");
    let mut saved = original.clone();
    saved.extend_from_slice(&section);
    assert_eq!(&saved[..original.len()], &original[..]);
    assert_eq!(open(&saved).page_count().ok(), Some(1));
}

#[test]
fn replacing_the_inherited_catalog_with_a_scalar_is_refused() {
    let document = open(&fixture());
    let overlay: BTreeMap<u32, PendingEdit> = [(
        1,
        PendingEdit::Set {
            generation: 0,
            object: Object::Integer(7),
        },
    )]
    .into_iter()
    .collect();
    match document.section_for(&overlay, &BTreeMap::new()) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("catalog"), "{detail}"),
        other => panic!("replacing the Root target with a scalar is unsafe: {other:?}"),
    }
}

#[test]
fn external_root_validation_uses_the_pristine_base_catalog() {
    let original = fixture();
    let mut document = open(&original);
    document
        .set_object(1, 0, Object::Integer(7))
        .expect("the internal catalog edit is pending");
    let trailer_edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Root"),
        Some(Object::Ref(onionskin_cos::ObjRef::new(1, 0))),
    )]
    .into_iter()
    .collect();
    let section = document
        .section_for(&BTreeMap::new(), &trailer_edits)
        .expect("the external Root uses the base Catalog")
        .expect("the Root edit writes a section");
    let mut saved = original;
    saved.extend_from_slice(&section);
    assert_eq!(open(&saved).page_count().ok(), Some(1));
}

#[test]
fn external_root_validation_does_not_use_an_internal_catalog_rescue() {
    let mut document = open(&fixture());
    document
        .set_object(4, 0, catalog_object(true))
        .expect("the internal spare edit is pending");
    let trailer_edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Root"),
        Some(Object::Ref(onionskin_cos::ObjRef::new(4, 0))),
    )]
    .into_iter()
    .collect();
    match document.section_for(&BTreeMap::new(), &trailer_edits) {
        Err(Error::Unrecoverable { detail }) => assert!(detail.contains("catalog"), "{detail}"),
        other => panic!("an internal Catalog must not rescue the base spare: {other:?}"),
    }
}

#[test]
fn an_overlay_root_generation_mismatch_is_dangling() {
    let document = open(&fixture());
    let overlay: BTreeMap<u32, PendingEdit> = [(
        6,
        PendingEdit::Set {
            generation: 0,
            object: catalog_object(true),
        },
    )]
    .into_iter()
    .collect();
    let trailer_edits: BTreeMap<Name, Option<Object>> = [(
        Name::new("Root"),
        Some(Object::Ref(onionskin_cos::ObjRef::new(6, 1))),
    )]
    .into_iter()
    .collect();
    match document.section_for(&overlay, &trailer_edits) {
        Err(Error::DanglingReference { holder, target }) => {
            assert_eq!(holder, Holder::Trailer);
            assert_eq!(target, onionskin_cos::ObjRef::new(6, 1));
        }
        other => panic!("Root generation must match the overlay: {other:?}"),
    }
}

#[test]
fn reserved_trailer_edits_cannot_override_generated_delta_fields() {
    let original = fixture();
    let expected_prev = last_startxref(&original);
    let document = open(&original);
    for prev in [None, Some(Object::Integer(0)), Some(Object::Integer(999))] {
        let number = document.next_object_number();
        let mut info = onionskin_cos::Dict::new();
        info.set("Producer", Object::String(b"metadata".to_vec()));
        let overlay: BTreeMap<u32, PendingEdit> =
            [set(number, Object::Dict(info))].into_iter().collect();
        let trailer_edits: BTreeMap<Name, Option<Object>> = [
            (
                Name::new("Info"),
                Some(Object::Ref(onionskin_cos::ObjRef::new(number, 0))),
            ),
            (Name::new("Prev"), prev),
            (Name::new("Size"), Some(Object::Integer(1))),
            (Name::new("XRefStm"), Some(Object::Integer(123))),
            (Name::new("Type"), Some(Object::name("XRef"))),
            (
                Name::new("W"),
                Some(Object::Array(vec![Object::Integer(1)])),
            ),
            (
                Name::new("Index"),
                Some(Object::Array(vec![Object::Integer(0), Object::Integer(1)])),
            ),
            (Name::new("Filter"), Some(Object::name("FlateDecode"))),
            (
                Name::new("DecodeParms"),
                Some(Object::Dict(onionskin_cos::Dict::new())),
            ),
            (Name::new("Length"), Some(Object::Integer(1))),
        ]
        .into_iter()
        .collect();
        let section = document
            .section_for(&overlay, &trailer_edits)
            .expect("reserved keys are normalized")
            .expect("the metadata overlay writes a section");
        let mut saved = original.clone();
        saved.extend_from_slice(&section);
        let reopened = open(&saved);
        assert_eq!(reopened.page_count().ok(), Some(1));
        assert_eq!(
            reopened.trailer().get(b"Prev").and_then(Object::as_integer),
            Some(expected_prev)
        );
        assert_eq!(
            reopened.trailer().get(b"Size").and_then(Object::as_integer),
            Some(i64::from(number) + 1)
        );
        for key in [
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
        match reopened
            .get(number)
            .expect("metadata object resolves")
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Producer"))
        {
            Some(Object::String(value)) => assert_eq!(value, b"metadata"),
            other => panic!("metadata was not preserved: {other:?}"),
        }
    }
}

#[test]
fn reserved_trailer_edits_cannot_add_prev_to_a_repaired_full_table() {
    let original = repaired_with_compressed_objects(false, b"<</Type/Spare/Which 6>>");
    let (document, provenance) =
        Document::open_repairing(Box::new(BytesSource::new(original.clone())))
            .expect("the fixture opens by repair");
    assert!(matches!(provenance, Provenance::Repaired(_)));
    let trailer_edits: BTreeMap<Name, Option<Object>> = [
        (Name::new("Prev"), Some(Object::Integer(999))),
        (Name::new("XRefStm"), Some(Object::Integer(123))),
        (Name::new("Type"), Some(Object::name("XRef"))),
    ]
    .into_iter()
    .collect();
    let section = document
        .section_for(&BTreeMap::new(), &trailer_edits)
        .expect("the repaired full table normalizes reserved fields")
        .expect("the trailer edit writes a section");
    let mut saved = original;
    saved.extend_from_slice(&section);
    let (reopened, _) = Document::open_repairing(Box::new(BytesSource::new(saved)))
        .expect("the repaired save reopens");
    assert!(reopened.trailer().get(b"Prev").is_none());
    assert!(reopened.trailer().get(b"XRefStm").is_none());
    assert!(reopened.trailer().get(b"Type").is_none());
    assert_eq!(reopened.page_count().ok(), Some(1));
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
fn repaired_with_compressed_objects(indirect_dependencies: bool, six: &[u8]) -> Vec<u8> {
    let six = if indirect_dependencies {
        b"<</Type/Referrer/Length 8 0 R/N 9 0 R/First 10 0 R/Filter 11 0 R/DecodeParms 12 0 R/Marker(endstream)>>"
    } else {
        six
    };
    let five: &[u8] = b"<</Type/Spare/Which 5>>";
    let header = format!("5 0 6 {} ", five.len() + 1);
    let first = header.len();
    let mut data = header.into_bytes();
    data.extend_from_slice(five);
    data.push(b' ');
    data.extend_from_slice(six);

    let mut bytes = Vec::from(&b"%PDF-1.5\n"[..]);
    let mut offsets = [0u64; 13];
    for (index, body) in skeleton().iter().enumerate() {
        offsets[index + 1] = bytes.len() as u64;
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    offsets[4] = bytes.len() as u64;
    let container_dictionary = if indirect_dependencies {
        "4 0 obj\n<</Type/ObjStm/Length 8 0 R/N 9 0 R/First 10 0 R/Filter 11 0 R/DecodeParms 12 0 R>>\nstream\n".to_string()
    } else {
        format!(
            "4 0 obj\n<</Type/ObjStm/N 2/First {first}/Length {}>>\nstream\n",
            data.len()
        )
    };
    bytes.extend_from_slice(container_dictionary.as_bytes());
    bytes.extend_from_slice(&data);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    if indirect_dependencies {
        let dependencies = [
            data.len().to_string().into_bytes(),
            b"2".to_vec(),
            first.to_string().into_bytes(),
            b"/Crypt".to_vec(),
            b"<</Name/Identity>>".to_vec(),
        ];
        for (index, body) in dependencies.iter().enumerate() {
            offsets[index + 8] = bytes.len() as u64;
            bytes.extend_from_slice(format!("{} 0 obj\n", index + 8).as_bytes());
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
    }

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
    if indirect_dependencies {
        for offset in &offsets[8..=12] {
            rows.extend_from_slice(&row(1, *offset, 0));
        }
    }

    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<</Type/XRef/Size {}/W[1 2 1]/Index[0 {}]/Root 1 0 R/Length {}>>\nstream\n",
            if indirect_dependencies { 13 } else { 8 },
            if indirect_dependencies { 13 } else { 8 },
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
    let original = repaired_with_compressed_objects(false, b"<</Type/Spare/Which 6>>");
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
    let original = repaired_with_compressed_objects(false, b"<</Type/Referrer/Points 5 0 R>>");
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
