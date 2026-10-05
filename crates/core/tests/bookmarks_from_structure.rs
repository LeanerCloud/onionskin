//! New Bookmarks From Structure, end to end: a tagged document's headings
//! become an outline, in one undoable step, after any bookmarks already there.

mod common;

use onionskin_core::{add_bookmark, add_bookmark_tree, plan_bookmarks_from_structure, Document};

/// Two pages. Page 1 has "Intro" (H1), "Details" (H2) and a paragraph; page 2
/// has "Later" (H1) and "Fine print" (H3, which skips a level).
fn document() -> Vec<u8> {
    let page_one = "/H1 << /MCID 0 >> BDC BT /F1 18 Tf 20 170 Td (Intro) Tj ET EMC \
                    /H2 << /MCID 1 >> BDC BT /F1 14 Tf 20 140 Td (Details) Tj ET EMC \
                    /P << /MCID 2 >> BDC BT /F1 12 Tf 20 100 Td (Body) Tj ET EMC";
    let page_two = "/H1 << /MCID 0 >> BDC BT /F1 18 Tf 20 170 Td (Later) Tj ET EMC \
                    /H3 << /MCID 1 >> BDC BT /F1 12 Tf 20 140 Td (Fine print) Tj ET EMC";
    let stream = |data: &str| common::stream(data);
    common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 7 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 200] /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".to_vec(),
        stream(page_one),
        stream(page_two),
        b"<< /Type /StructTreeRoot /K [8 0 R 9 0 R 10 0 R 11 0 R 12 0 R] >>".to_vec(),
        b"<< /S /H1 /Pg 3 0 R /K 0 >>".to_vec(),
        b"<< /S /H2 /Pg 3 0 R /K 1 >>".to_vec(),
        b"<< /S /P /Pg 3 0 R /K 2 >>".to_vec(),
        b"<< /S /H1 /Pg 4 0 R /K 0 >>".to_vec(),
        b"<< /S /H3 /Pg 4 0 R /K 1 >>".to_vec(),
    ])
}

/// `(title, page, children as (title, page))` for each top-level bookmark.
type Shape = (String, Option<usize>, Vec<(String, Option<usize>)>);

fn outline_of(doc: &mut Document) -> Vec<Shape> {
    doc.outline()
        .expect("the outline reads")
        .iter()
        .map(|item| {
            (
                item.title.clone(),
                item.page,
                item.children
                    .iter()
                    .map(|c| (c.title.clone(), c.page))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn the_headings_become_an_outline_nested_by_level_with_each_pages_destination() {
    let mut doc = Document::open_bytes(document()).expect("opens");
    let blocks = doc.reading_blocks().expect("reads").expect("tagged");
    let plan = plan_bookmarks_from_structure(&blocks);

    let added = doc
        .edit_document("New Bookmarks From Structure", |tx| {
            add_bookmark_tree(tx, &plan)
        })
        .expect("the edit commits");
    assert_eq!(added, 4);
    assert_eq!(
        outline_of(&mut doc),
        [
            (
                "Intro".to_owned(),
                Some(0),
                vec![("Details".to_owned(), Some(0))]
            ),
            (
                "Later".to_owned(),
                Some(1),
                vec![("Fine print".to_owned(), Some(1))]
            ),
        ]
    );
}

#[test]
fn it_is_one_undo_step_and_goes_after_the_bookmarks_already_there() {
    let mut doc = Document::open_bytes(document()).expect("opens");
    doc.edit_document("New Bookmark", |tx| {
        add_bookmark(tx, &[], None, "Mine", Some(1))
    })
    .expect("the first bookmark is made");
    let blocks = doc.reading_blocks().expect("reads").expect("tagged");
    let plan = plan_bookmarks_from_structure(&blocks);
    doc.edit_document("New Bookmarks From Structure", |tx| {
        add_bookmark_tree(tx, &plan)
    })
    .expect("the edit commits");

    let titles: Vec<String> = outline_of(&mut doc).into_iter().map(|b| b.0).collect();
    assert_eq!(titles, ["Mine", "Intro", "Later"]);

    doc.undo().expect("undo");
    let titles: Vec<String> = outline_of(&mut doc).into_iter().map(|b| b.0).collect();
    assert_eq!(titles, ["Mine"], "one step takes all four away");
}

#[test]
fn a_document_with_no_headings_plans_nothing_and_writes_nothing() {
    let mut doc = Document::open_bytes(common::tagged()).expect("opens");
    let blocks = doc.reading_blocks().expect("reads").expect("tagged");
    let plan = plan_bookmarks_from_structure(&blocks);
    assert!(plan.is_empty());
    let added = doc
        .edit_document("New Bookmarks From Structure", |tx| {
            add_bookmark_tree(tx, &plan)
        })
        .expect("an empty plan commits");
    assert_eq!(added, 0);
    assert!(outline_of(&mut doc).is_empty());
}

#[test]
fn a_bookmark_for_every_page_of_a_long_document_is_written_in_linear_time() {
    use onionskin_core::PlannedBookmark;

    let pages = 2500;
    let mut doc = Document::open_bytes(common::flat(pages)).expect("opens");
    let plan: Vec<PlannedBookmark> = (0..pages)
        .map(|page| PlannedBookmark {
            title: format!("Page {}", page + 1),
            page,
            children: Vec::new(),
        })
        .collect();
    let started = std::time::Instant::now();
    let added = doc
        .edit_document("New Bookmarks From Structure", |tx| {
            add_bookmark_tree(tx, &plan)
        })
        .expect("the edit commits");
    let elapsed = started.elapsed();
    assert_eq!(added, pages);
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "{pages} bookmarks took {elapsed:?}: the page tree is being walked once per bookmark"
    );
    let outline = doc.outline().expect("reads");
    assert_eq!(outline.len(), pages);
    assert_eq!(outline[pages - 1].page, Some(pages - 1));
}

#[test]
fn an_empty_plan_leaves_a_document_with_no_outline_unchanged() {
    let mut doc = Document::open_bytes(common::flat(2)).expect("opens");
    let added = doc
        .edit_document("New Bookmarks From Structure", |tx| {
            add_bookmark_tree(tx, &[])
        })
        .expect("an empty plan commits");
    assert_eq!(added, 0);
    assert!(doc.outline().expect("reads").is_empty());
    let catalog = doc
        .structure()
        .expect("the document reads")
        .catalog()
        .expect("the catalog reads");
    assert!(
        catalog.get(b"Outlines").is_none(),
        "no outline root was made for nothing"
    );
}
