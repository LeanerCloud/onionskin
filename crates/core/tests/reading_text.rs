//! `Document::reading_text`: a tagged page in structure order, kept until the
//! bytes change, and nothing for a document with no structure tree.

mod common;

use onionskin_core::pages::delete_pages;
use onionskin_core::Document;

#[test]
fn a_tagged_document_reads_in_structure_order_and_follows_an_edit() {
    let mut doc = Document::open_bytes(common::tagged()).expect("opens");
    assert_eq!(
        doc.reading_text(0).expect("reads").as_deref(),
        Some("Page 1")
    );
    assert_eq!(
        doc.reading_text(1).expect("reads").as_deref(),
        Some("Page 2")
    );

    doc.edit_pages("Delete", |tx, structure| delete_pages(tx, structure, &[0]))
        .expect("the delete commits");
    assert_eq!(
        doc.reading_text(0).expect("reads").as_deref(),
        Some("Page 2"),
        "the blocks read before the edit are not served after it"
    );
}

#[test]
fn a_document_with_no_structure_tree_has_no_reading_text() {
    let mut doc = Document::open_bytes(common::flat(2)).expect("opens");
    assert_eq!(doc.reading_text(0).expect("reads"), None);
}
