//! `core::pages` page marks: watermarks, backgrounds, headers and footers
//! and Bates numbers drawn on the page, and removed again.
//!
//! Structure is read from a fresh parse of the saved bytes; where a mark
//! lands is read from the renderer, which is what the user sees.

use onionskin_core::pages::{
    add_page_marks, mark_settings, marked_pages, page_marks, remove_page_marks, MarkKind, PageMark,
};
use onionskin_core::{Document, EditSession, Error, Transaction};
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Object};

mod common;
use common::{deep, flat, pdf, stream};

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn apply<T>(
    original: &[u8],
    body: impl FnOnce(&mut Transaction<'_>) -> onionskin_core::Result<T>,
) -> onionskin_core::Result<(Vec<u8>, T)> {
    let base = open(original);
    let mut edit = EditSession::for_base(&base);
    let value = edit.transact(&base, "Marks", body)?;
    let mut bytes = original.to_vec();
    if let Some(section) = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
    {
        bytes.extend_from_slice(&section);
    }
    Ok((bytes, value))
}

/// A mark filling a `size`-point square at the shown bottom-left, in blue.
fn square(size: f64) -> PageMark {
    PageMark {
        content: format!("0 0 1 rg 0 0 {size} {size} re f").into_bytes(),
        resources: Dict::new(),
        behind: false,
        settings: Vec::new(),
    }
}

fn kinds(bytes: &[u8], page: usize) -> Vec<MarkKind> {
    apply(bytes, |tx| page_marks(tx, page)).expect("reads").1
}

fn contents_len(bytes: &[u8], page: usize) -> usize {
    let document = open(bytes);
    let page = document.page(page).expect("page");
    match page.dict.get(b"Contents") {
        Some(Object::Array(items)) => items.len(),
        Some(_) => 1,
        None => 0,
    }
}

/// The pixel at shown `(x, y)` points from the top-left, at zoom 1.
fn pixel(bytes: &[u8], page: usize, (x, y): (u32, u32)) -> [u8; 4] {
    let mut document = Document::open_bytes(bytes.to_vec()).expect("opens");
    let render = document.render_page_now(page, 1.0).expect("renders");
    let raster = render.raster;
    let at = ((y * raster.width() + x) * 4) as usize;
    raster.rgba()[at..at + 4].try_into().expect("four bytes")
}

fn is_blue(pixel: [u8; 4]) -> bool {
    pixel[2] > 200 && pixel[0] < 60 && pixel[1] < 60
}

#[test]
fn a_watermark_is_drawn_over_the_page_at_the_shown_bottom_left() {
    let (saved, ()) = apply(&flat(2), |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(0, square(50.0))], false)
    })
    .expect("adds");
    assert_eq!(kinds(&saved, 0), [MarkKind::Watermark]);
    assert_eq!(kinds(&saved, 1), []);
    let marked = apply(&saved, |tx| marked_pages(tx, MarkKind::Watermark))
        .expect("reads")
        .1;
    assert_eq!(marked, [0]);
    // The page's own content between its guards, then the mark.
    assert_eq!(contents_len(&saved, 0), 4);
    assert!(is_blue(pixel(&saved, 0, (10, 792 - 10))));
    assert!(!is_blue(pixel(&saved, 0, (100, 100))));
    assert_eq!(open(&saved).audit_references().expect("audits"), Vec::new());

    // The shared resources the pages inherit are left alone.
    let document = open(&saved);
    let second = document.page(1).expect("page");
    assert!(second.dict.get(b"Resources").is_none(), "still inherited");
    let first = document.page(0).expect("page");
    match first.dict.get(b"Resources") {
        Some(Object::Dict(resources)) => {
            assert!(resources.get(b"Font").is_some(), "the inherited fonts kept");
            assert!(resources.get(b"XObject").is_some());
        }
        other => panic!("no own resources: {other:?}"),
    }
}

/// The deep tree's first page inherits `/Rotate 270`: the mark is still at
/// the bottom-left of the page as shown.
#[test]
fn a_mark_follows_the_page_as_it_is_shown() {
    let (saved, ()) = apply(&deep(), |tx| {
        add_page_marks(tx, MarkKind::HeaderFooter, &[(0, square(40.0))], false)
    })
    .expect("adds");
    // Turned, the page shows 792 wide and 612 high.
    assert!(is_blue(pixel(&saved, 0, (10, 612 - 10))));
    assert!(!is_blue(pixel(&saved, 0, (700, 10))));
}

#[test]
fn a_background_is_behind_the_page_and_the_text_still_shows() {
    let fill = PageMark {
        content: b"0 0 1 rg 0 0 612 792 re f".to_vec(),
        resources: Dict::new(),
        behind: false,
        settings: Vec::new(),
    };
    let (saved, ()) = apply(&flat(1), |tx| {
        add_page_marks(tx, MarkKind::Background, &[(0, fill)], false)
    })
    .expect("adds");
    assert_eq!(contents_len(&saved, 0), 2, "no guards for a background");
    assert!(is_blue(pixel(&saved, 0, (300, 400))));
    // "Page 1" at (72, 700) in black, over the blue.
    let mut document = Document::open_bytes(saved.clone()).expect("opens");
    let raster = document.render_page_now(0, 1.0).expect("renders").raster;
    let inked = (72..160).any(|x| {
        (80..100).any(|y| {
            let at = ((y * raster.width() + x) * 4) as usize;
            raster.rgba()[at + 2] < 60
        })
    });
    assert!(inked, "the text is drawn over the background");
}

#[test]
fn a_watermark_can_go_behind_the_page() {
    let behind = PageMark {
        behind: true,
        ..square(700.0)
    };
    let (saved, ()) = apply(&flat(1), |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(0, behind)], false)
    })
    .expect("adds");
    assert_eq!(
        contents_len(&saved, 0),
        2,
        "prepended, and nothing to guard"
    );
    let document = open(&saved);
    let page = document.page(0).expect("page");
    let Some(Object::Array(parts)) = page.dict.get(b"Contents") else {
        panic!("an array");
    };
    let first = document.resolve(&parts[0]).expect("resolves");
    let Object::Stream(first) = first else {
        panic!("a stream")
    };
    assert_eq!(
        first
            .dict
            .get(b"OnionskinMark")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"Watermark".to_vec())
    );
    assert_eq!(kinds(&saved, 0), [MarkKind::Watermark]);
}

#[test]
fn replacing_keeps_one_mark_and_removing_leaves_the_page_as_it_was() {
    let original = flat(1);
    let (once, ()) = apply(&original, |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(0, square(10.0))], false)
    })
    .expect("adds");
    let (twice, ()) = apply(&once, |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(0, square(20.0))], true)
    })
    .expect("replaces");
    assert_eq!(contents_len(&twice, 0), 4, "one mark, guards kept");
    assert!(is_blue(pixel(&twice, 0, (15, 792 - 15))), "the new one");

    let (added, ()) = apply(&twice, |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(0, square(30.0))], false)
    })
    .expect("adds another");
    assert_eq!(contents_len(&added, 0), 5, "two marks share the guards");

    let (removed, count) = apply(&added, |tx| {
        remove_page_marks(tx, MarkKind::Watermark, &[0])
    })
    .expect("removes");
    assert_eq!(count, 1);
    assert_eq!(contents_len(&removed, 0), 1, "the guards go too");
    assert_eq!(kinds(&removed, 0), []);
    let document = open(&removed);
    let page = document.page(0).expect("page");
    match page.dict.get(b"Resources") {
        Some(Object::Dict(resources)) => assert_eq!(
            resources.get(b"XObject"),
            Some(&Object::Dict(Dict::new())),
            "no form names left behind"
        ),
        other => panic!("{other:?}"),
    }
    let (_, none) = apply(&removed, |tx| {
        remove_page_marks(tx, MarkKind::Watermark, &[0])
    })
    .expect("runs");
    assert_eq!(none, 0, "nothing left to remove");
}

#[test]
fn each_kind_is_removed_on_its_own() {
    let (saved, ()) = apply(&flat(1), |tx| {
        add_page_marks(tx, MarkKind::HeaderFooter, &[(0, square(10.0))], false)?;
        add_page_marks(tx, MarkKind::Bates, &[(0, square(10.0))], false)?;
        add_page_marks(tx, MarkKind::Background, &[(0, square(10.0))], false)
    })
    .expect("adds");
    assert_eq!(
        kinds(&saved, 0),
        [
            MarkKind::Background,
            MarkKind::HeaderFooter,
            MarkKind::Bates
        ]
    );
    let (saved, _) =
        apply(&saved, |tx| remove_page_marks(tx, MarkKind::Bates, &[0])).expect("removes");
    assert_eq!(
        kinds(&saved, 0),
        [MarkKind::Background, MarkKind::HeaderFooter]
    );
    let (saved, _) = apply(&saved, |tx| {
        remove_page_marks(tx, MarkKind::HeaderFooter, &[0])
    })
    .expect("removes");
    assert_eq!(kinds(&saved, 0), [MarkKind::Background]);
    assert_eq!(contents_len(&saved, 0), 2, "the background and the page");
}

/// A page shaped the way Acrobat writes a watermark: a content stream of
/// its own that draws a form whose `/PieceInfo` says so, and nothing else.
fn acrobat_watermarked(extra: &str) -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents [4 0 R 5 0 R] /Resources << /XObject << /Fm0 6 0 R >> >> >>".to_vec(),
        stream("0 0 1 rg 0 0 10 10 re f"),
        stream(&format!("q /Artifact << /Subtype /Watermark /Type /Pagination >> BDC /Fm0 Do EMC Q {extra}")),
        common::stream_with(
            "1 0 0 rg 0 0 50 50 re f",
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /PieceInfo << /ADBE_CompoundType << /Private /Watermark >> >>",
        ),
    ])
}

#[test]
fn acrobats_own_watermark_is_recognised_and_removed() {
    let original = acrobat_watermarked("");
    assert_eq!(kinds(&original, 0), [MarkKind::Watermark]);
    let (removed, count) = apply(&original, |tx| {
        remove_page_marks(tx, MarkKind::Watermark, &[0])
    })
    .expect("removes");
    assert_eq!(count, 1);
    assert_eq!(contents_len(&removed, 0), 1);
    assert_eq!(kinds(&removed, 0), []);

    // A stream that also paints is the page's content, not a mark.
    let painting = acrobat_watermarked("0 0 5 5 re f");
    assert_eq!(kinds(&painting, 0), []);
}

#[test]
fn a_page_that_is_not_there_is_refused() {
    let refused = apply(&flat(1), |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(3, square(1.0))], false)
    });
    assert!(matches!(
        refused,
        Err(Error::NoSuchPage { page: 3, count: 1 })
    ));
    assert!(matches!(
        apply(&flat(1), |tx| remove_page_marks(
            tx,
            MarkKind::Watermark,
            &[1]
        )),
        Err(Error::NoSuchPage { page: 1, .. })
    ));
    assert!(matches!(
        apply(&flat(1), |tx| page_marks(tx, 2)),
        Err(Error::NoSuchPage { page: 2, .. })
    ));
}

#[test]
fn a_mark_undoes_like_any_other_edit() {
    let original = flat(1);
    let base = open(&original);
    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Add Watermark", |tx| {
        add_page_marks(tx, MarkKind::Watermark, &[(0, square(5.0))], false)
    })
    .expect("adds");
    assert!(edit.undo(&base).expect("undoes"));
    assert!(edit.pending_edits().is_empty());
}

#[test]
fn a_marks_settings_are_kept_for_update_to_read_back() {
    let noted = PageMark {
        settings: b"size=12".to_vec(),
        ..square(5.0)
    };
    let (saved, ()) = apply(&flat(2), |tx| {
        add_page_marks(tx, MarkKind::HeaderFooter, &[(1, noted)], false)?;
        add_page_marks(tx, MarkKind::Watermark, &[(0, square(5.0))], false)
    })
    .expect("adds");
    let read = |kind| {
        apply(&saved, |tx| mark_settings(tx, kind))
            .expect("reads")
            .1
    };
    assert_eq!(read(MarkKind::HeaderFooter), Some(b"size=12".to_vec()));
    assert_eq!(read(MarkKind::Watermark), None, "made without any");
    assert_eq!(read(MarkKind::Bates), None, "none on any page");
}
