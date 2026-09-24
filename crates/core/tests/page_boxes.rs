//! `core::pages::set_page_box`: Crop Pages and Set Page Boxes.
//!
//! Every assertion reads a fresh parse of the original bytes with the edit's
//! section appended, and the crop box is also checked through `Document`,
//! which is what the canvas and the printer draw from.

use onionskin_core::pages::{set_media_size, set_page_box, Margins, PageBox};
use onionskin_core::{Document, EditSession, Error, Transaction};
use onionskin_cos::{BytesSource, Document as CosDocument, Object};

mod common;
use common::{deep, flat};

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

/// The original with `body`'s edit appended, or the error it refused with.
fn apply(
    original: &[u8],
    body: impl FnOnce(&mut Transaction<'_>) -> onionskin_core::Result<()>,
) -> onionskin_core::Result<Vec<u8>> {
    let base = open(original);
    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Crop Pages", body)?;
    let mut bytes = original.to_vec();
    if let Some(section) = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
    {
        bytes.extend_from_slice(&section);
    }
    Ok(bytes)
}

/// The page's own entry `key`, as numbers.
fn own_box(bytes: &[u8], index: usize, key: &[u8]) -> Option<Vec<f64>> {
    let page = open(bytes).page(index).expect("page");
    match page.dict.get(key)? {
        Object::Array(values) => Some(
            values
                .iter()
                .map(|value| match value {
                    Object::Real(v) => *v,
                    Object::Integer(v) => *v as f64,
                    other => panic!("not a number: {other:?}"),
                })
                .collect(),
        ),
        other => panic!("not an array: {other:?}"),
    }
}

fn even(margin: f64) -> Margins {
    Margins {
        top: margin,
        bottom: margin,
        left: margin,
        right: margin,
    }
}

#[test]
fn a_crop_writes_the_box_on_the_pages_asked_for_and_no_others() {
    let saved = apply(&flat(3), |tx| {
        set_page_box(tx, &[0, 2], PageBox::Crop, even(36.0))
    })
    .expect("crops");
    let cropped = Some(vec![36.0, 36.0, 576.0, 756.0]);
    assert_eq!(own_box(&saved, 0, b"CropBox"), cropped);
    assert_eq!(own_box(&saved, 1, b"CropBox"), None);
    assert_eq!(own_box(&saved, 2, b"CropBox"), cropped);
    assert_eq!(open(&saved).audit_references().expect("audits"), Vec::new());

    // What the viewer draws: the page is now the cropped size.
    let mut document = Document::open_bytes(saved).expect("opens");
    let geometry = document.page_geometry(0).expect("geometry").clone();
    assert_eq!(geometry.crop_box, Some([36.0, 36.0, 576.0, 756.0]));
    assert_eq!(geometry.render_size, (540.0, 720.0));
    assert_eq!(
        document.page_geometry(1).expect("geometry").render_size,
        (612.0, 792.0)
    );
}

/// The deep tree's first two pages inherit `/Rotate 270` and the last two a
/// 400 by 600 media box: both are read through inheritance, not assumed.
#[test]
fn margins_are_taken_as_shown_from_the_media_box_each_page_inherits() {
    let shown = Margins {
        top: 10.0,
        bottom: 20.0,
        left: 30.0,
        right: 40.0,
    };
    let saved = apply(&deep(), |tx| {
        set_page_box(tx, &[0, 3], PageBox::Trim, shown)
    })
    .expect("sets the trim box");
    // Turned 270 clockwise, the page's right edge is shown at the top.
    assert_eq!(
        own_box(&saved, 0, b"TrimBox"),
        Some(vec![20.0, 40.0, 602.0, 762.0])
    );
    assert_eq!(
        own_box(&saved, 3, b"TrimBox"),
        Some(vec![30.0, 20.0, 360.0, 590.0])
    );
    assert_eq!(
        own_box(&saved, 0, b"CropBox"),
        None,
        "only the box asked for"
    );
}

#[test]
fn every_box_has_its_own_key() {
    for (which, key) in [
        (PageBox::Crop, b"CropBox".as_slice()),
        (PageBox::Bleed, b"BleedBox"),
        (PageBox::Trim, b"TrimBox"),
        (PageBox::Art, b"ArtBox"),
    ] {
        let saved =
            apply(&flat(1), |tx| set_page_box(tx, &[0], which, even(1.0))).expect("sets the box");
        assert_eq!(
            own_box(&saved, 0, key),
            Some(vec![1.0, 1.0, 611.0, 791.0]),
            "{key:?}"
        );
    }
}

#[test]
fn a_crop_one_page_cannot_take_is_refused_for_every_page() {
    // 200 points each side leaves nothing of the deep tree's 400-wide pages
    // and most of its Letter ones: the whole edit is refused.
    let refused = apply(&deep(), |tx| {
        set_page_box(
            tx,
            &[1, 2],
            PageBox::Crop,
            Margins {
                left: 200.0,
                right: 200.0,
                ..Margins::default()
            },
        )
    });
    match refused {
        Err(Error::PageBoxTooSmall {
            page,
            width,
            height,
        }) => {
            assert_eq!(page, 2);
            assert_eq!((width, height), (0.0, 600.0));
        }
        other => panic!("not refused: {other:?}"),
    }
    let message = Error::PageBoxTooSmall {
        page: 2,
        width: 0.0,
        height: 600.0,
    }
    .to_string();
    assert!(message.contains("page 3"), "{message}");
}

#[test]
fn a_bad_margin_or_page_is_refused() {
    let negative = apply(&flat(2), |tx| {
        set_page_box(tx, &[0], PageBox::Crop, even(-5.0))
    });
    assert!(matches!(negative, Err(Error::InvalidMargin(value)) if value == -5.0));
    assert!(Error::InvalidMargin(-5.0).to_string().contains("-5"));

    let missing = apply(&flat(2), |tx| {
        set_page_box(tx, &[2], PageBox::Crop, even(5.0))
    });
    assert!(matches!(
        missing,
        Err(Error::NoSuchPage { page: 2, count: 2 })
    ));
}

#[test]
fn a_page_box_edit_undoes_like_any_other() {
    let original = flat(2);
    let base = open(&original);
    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Crop Pages", |tx| {
        set_page_box(tx, &[0], PageBox::Crop, even(10.0))
    })
    .expect("crops");
    assert!(edit.undo(&base).expect("undoes"));
    assert!(edit.pending_edits().is_empty());
}

#[test]
fn change_page_size_centres_the_media_box_and_shows_all_of_it() {
    // The deep tree's first page inherits /Rotate 270 and Letter.
    let saved = apply(&deep(), |tx| set_media_size(tx, &[0, 3], 892.0, 712.0)).expect("resizes");
    let letter_turned = Some(vec![-50.0, -50.0, 662.0, 842.0]);
    assert_eq!(own_box(&saved, 0, b"MediaBox"), letter_turned);
    assert_eq!(own_box(&saved, 0, b"CropBox"), letter_turned);
    // The fourth is 400 by 600 and unturned.
    assert_eq!(
        own_box(&saved, 3, b"MediaBox"),
        Some(vec![-246.0, -56.0, 646.0, 656.0])
    );
    assert_eq!(
        own_box(&saved, 1, b"MediaBox"),
        None,
        "only the pages asked for"
    );

    let mut document = Document::open_bytes(saved).expect("opens");
    assert_eq!(
        document.page_geometry(0).expect("geometry").render_size,
        (892.0, 712.0),
        "shown the size asked for"
    );
}

#[test]
fn a_page_size_too_small_to_show_is_refused() {
    for (width, height) in [(0.5, 100.0), (100.0, 0.0), (f64::NAN, 100.0)] {
        let refused = apply(&flat(2), |tx| set_media_size(tx, &[1], width, height));
        assert!(
            matches!(refused, Err(Error::PageBoxTooSmall { page: 1, .. })),
            "{width} x {height}: {refused:?}"
        );
    }
    assert!(matches!(
        apply(&flat(2), |tx| set_media_size(tx, &[5], 100.0, 100.0)),
        Err(Error::NoSuchPage { page: 5, count: 2 })
    ));
}
