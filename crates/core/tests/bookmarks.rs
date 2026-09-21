//! Bookmark authoring, read back through the outline reader it is the
//! inverse of, after a save and a reopen.
//!
//! The mutation this must catch: a writer that put every bookmark at the top
//! level would pass a count of bookmarks and fail the nesting assertion.

mod common;

use std::path::{Path, PathBuf};

use onionskin_core::pages::delete_pages;
use onionskin_core::{
    add_bookmark, delete_bookmark, move_bookmark, rename_bookmark, set_bookmark_destination,
    Document, DocumentFile, Error, OutlineItem,
};
use onionskin_cos::Object;

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

fn item(title: &str, page: Option<usize>, children: Vec<OutlineItem>) -> OutlineItem {
    OutlineItem {
        title: title.into(),
        page,
        children,
    }
}

/// Chapter 1 (page 1) > Section 1.1 (page 2); Chapter 2 (page 2).
fn build(document: &mut Document) {
    document
        .edit_document("Add Bookmark", |tx| {
            add_bookmark(tx, &[], None, "Chapter 1", Some(0))
        })
        .expect("adds");
    document
        .edit_document("Add Bookmark", |tx| {
            add_bookmark(tx, &[], None, "Chapter 2", Some(1))
        })
        .expect("adds");
    let nested = document
        .edit_document("Add Bookmark", |tx| {
            add_bookmark(tx, &[0], None, "Section 1.1", Some(1))
        })
        .expect("adds");
    assert_eq!(nested, [0, 0]);
}

fn built() -> Vec<OutlineItem> {
    vec![
        item(
            "Chapter 1",
            Some(0),
            vec![item("Section 1.1", Some(1), Vec::new())],
        ),
        item("Chapter 2", Some(1), Vec::new()),
    ]
}

fn outline(document: &mut Document) -> Vec<OutlineItem> {
    document.outline().expect("reads").to_vec()
}

#[test]
fn a_nested_outline_reads_back_with_its_shape_and_pages_after_a_reopen() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("two-page.pdf");
    std::fs::copy(seed("two-page.pdf"), &path).expect("copies");
    let mut file = DocumentFile::open(&path).expect("opens");
    build(file.document_mut());
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    assert_eq!(outline(&mut reopened), built());
    let dangling = reopened
        .structure()
        .expect("doc")
        .audit_references()
        .expect("audits");
    assert_eq!(dangling, Vec::new());
}

#[test]
fn renaming_keeps_the_destination_and_retargeting_replaces_an_action() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    build(&mut document);
    document
        .edit_document("Rename Bookmark", |tx| {
            rename_bookmark(tx, &[0, 0], "Section One")
        })
        .expect("renames");
    assert_eq!(
        outline(&mut document)[0].children[0],
        item("Section One", Some(1), Vec::new())
    );

    document
        .edit_document("Set Destination", |tx| {
            set_bookmark_destination(tx, &[1], Some(0))
        })
        .expect("retargets");
    assert_eq!(outline(&mut document)[1].page, Some(0));
    document
        .edit_document("Set Destination", |tx| {
            set_bookmark_destination(tx, &[1], None)
        })
        .expect("clears");
    assert_eq!(outline(&mut document)[1].page, None);
}

/// Deleting a parent removes its children with it; that is the rule, and it
/// is asserted here. Undo brings the whole subtree back.
#[test]
fn deleting_a_parent_removes_its_children_and_undo_restores_them() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    build(&mut document);
    document
        .edit_document("Delete Bookmark", |tx| delete_bookmark(tx, &[0]))
        .expect("deletes");
    assert_eq!(
        outline(&mut document),
        [item("Chapter 2", Some(1), Vec::new())],
        "Section 1.1 went with its parent"
    );
    let (session, base) = document.edit_mut();
    assert!(session.undo(base).expect("undoes"));
    assert_eq!(outline(&mut document), built());
}

#[test]
fn nesting_moves_a_bookmark_under_its_neighbour_and_not_into_itself() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    build(&mut document);
    let moved = document
        .edit_document("Move Bookmark", |tx| move_bookmark(tx, &[1], &[0], 1))
        .expect("nests");
    assert_eq!(moved, [0, 1]);
    assert_eq!(
        outline(&mut document),
        [item(
            "Chapter 1",
            Some(0),
            vec![
                item("Section 1.1", Some(1), Vec::new()),
                item("Chapter 2", Some(1), Vec::new()),
            ],
        )]
    );

    let into_itself =
        document.edit_document("Move Bookmark", |tx| move_bookmark(tx, &[0], &[0, 0], 0));
    assert!(matches!(into_itself, Err(Error::NoSuchBookmark(_))));

    // Back out to the top, after the chapter it was in.
    let moved = document
        .edit_document("Move Bookmark", |tx| move_bookmark(tx, &[0, 1], &[], 1))
        .expect("un-nests");
    assert_eq!(moved, [1]);
    assert_eq!(outline(&mut document), built());
}

#[test]
fn a_path_with_no_bookmark_is_refused_and_writes_nothing() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    build(&mut document);
    let reach = document.edit().history().reach();
    for result in [
        document.edit_document("Rename", |tx| rename_bookmark(tx, &[5], "x")),
        document.edit_document("Delete", |tx| delete_bookmark(tx, &[0, 3])),
        document.edit_document("Add", |tx| {
            add_bookmark(tx, &[0], Some(4), "x", None).map(|_| ())
        }),
    ] {
        assert!(
            matches!(result, Err(Error::NoSuchBookmark(_))),
            "{result:?}"
        );
    }
    assert_eq!(document.edit().history().reach(), reach);
}

/// A closed bookmark stays closed when the outline around it changes: the
/// sign of `/Count` is kept, and the root counts only what is visible.
#[test]
fn a_closed_bookmark_stays_closed_and_the_root_counts_what_shows() {
    use common::pdf;

    let bytes = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
        b"<< /Type /Outlines /First 5 0 R /Last 5 0 R /Count 1 >>".to_vec(),
        b"<< /Title (Closed) /Parent 4 0 R /First 6 0 R /Last 6 0 R /Count -1 >>".to_vec(),
        b"<< /Title (Hidden) /Parent 5 0 R >>".to_vec(),
    ]);
    let mut document = Document::open_bytes(bytes).expect("opens");
    document
        .edit_document("Add Bookmark", |tx| {
            add_bookmark(tx, &[], None, "Open", Some(0))
        })
        .expect("adds");
    let count = |document: &mut Document, number: u32| {
        let current = document.structure().expect("doc");
        let Ok(parsed) = current.get(number) else {
            panic!("object {number}");
        };
        parsed
            .object
            .as_dict()
            .and_then(|dict| dict.get(b"Count"))
            .and_then(Object::as_integer)
    };
    assert_eq!(count(&mut document, 5), Some(-1), "still closed");
    assert_eq!(
        count(&mut document, 4),
        Some(2),
        "Closed and Open show; Hidden does not"
    );
}

/// P5's fix-up, exercised on a bookmark made here: deleting the page it goes
/// to drops it, and the report counts it.
#[test]
fn a_bookmark_to_a_deleted_page_is_dropped_and_counted() {
    let mut document = Document::open_path(&seed("two-page.pdf")).expect("opens");
    build(&mut document);
    let report = document
        .edit_pages("Delete Page", |tx, structure| {
            delete_pages(tx, structure, &[1])
        })
        .expect("deletes");
    assert_eq!(report.bookmarks_dropped, 2, "Section 1.1 and Chapter 2");
    assert_eq!(
        outline(&mut document),
        [item("Chapter 1", Some(0), Vec::new())]
    );
}

#[test]
fn a_document_without_an_outline_gets_one_that_undo_removes() {
    let mut document = Document::open_path(&seed("minimal.pdf")).expect("opens");
    document
        .edit_document("Add Bookmark", |tx| {
            add_bookmark(tx, &[], None, "Start", Some(0))
        })
        .expect("adds");
    assert_eq!(outline(&mut document), [item("Start", Some(0), Vec::new())]);
    let (session, base) = document.edit_mut();
    assert!(session.undo(base).expect("undoes"));
    assert!(outline(&mut document).is_empty());
    let catalog = document
        .structure()
        .expect("doc")
        .catalog()
        .expect("catalog");
    assert!(
        catalog.get(b"Outlines").is_none(),
        "the catalog is as it was"
    );
}
