//! Editing the images a page draws: moved, resized, turned, flipped,
//! replaced, taken away and added, each read back from a fresh parse, and
//! another placement of the same image left alone.

mod common;

use common::{apply, open, pdf, stream_with, try_apply};
use onionskin_content::placements::ImagePlacement;
use onionskin_content::{page_images, Matrix};
use onionskin_core::image_edit::{
    add_image, edit_placement, image_at, import_image, transforms, PlacementEdit,
};

fn image() -> Vec<u8> {
    let mut out = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
/BitsPerComponent 8 /Length 1 >>\nstream\n"
        .to_vec();
    out.push(0x80);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A page drawing the image at 100 by 50 from (10, 20), and again, small,
/// inside a form at (300, 300).
fn document() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im0 5 0 R /Fm0 6 0 R >> >> >>"
            .to_vec(),
        stream_with("q 100 0 0 50 10 20 cm /Im0 Do Q q 1 0 0 1 300 300 cm /Fm0 Do Q", ""),
        image(),
        stream_with(
            "q 20 0 0 20 0 0 cm /Im0 Do Q",
            "/Type /XObject /Subtype /Form /BBox [0 0 50 50] /Resources << /XObject << /Im0 5 0 R >> >>",
        ),
    ])
}

fn placements(bytes: &[u8]) -> Vec<ImagePlacement> {
    page_images(&open(bytes), 0).expect("reads")
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

fn edited(bytes: &[u8], which: usize, edit: PlacementEdit) -> Vec<u8> {
    let placed = placements(bytes)[which].clone();
    apply(bytes, |tx, _| edit_placement(tx, 0, &placed, &edit))
}

#[test]
fn an_image_is_moved_resized_turned_and_flipped_where_it_is() {
    let bytes = document();
    let before = placements(&bytes);
    assert_eq!(before.len(), 2);
    assert_eq!(
        image_at(&before, 50.0, 40.0).map(|p| p.name.as_str()),
        Some("Im0")
    );
    assert_eq!(
        image_at(&before, 305.0, 305.0).map(|p| p.bounds()),
        Some(before[1].bounds())
    );
    assert!(image_at(&before, 1.0, 1.0).is_none());

    let moved = edited(
        &bytes,
        0,
        PlacementEdit::Transform(transforms::translate(10.0, 5.0)),
    );
    let after = placements(&moved);
    assert!(
        close(after[0].bounds(), [20.0, 25.0, 120.0, 75.0]),
        "{:?}",
        after[0].bounds()
    );
    assert_eq!(
        after[1].bounds(),
        before[1].bounds(),
        "the other placement stays"
    );

    let centre = (60.0, 45.0);
    let bigger = edited(
        &bytes,
        0,
        PlacementEdit::Transform(transforms::scale_about(2.0, 2.0, centre)),
    );
    assert!(close(
        placements(&bigger)[0].bounds(),
        [-40.0, -5.0, 160.0, 95.0]
    ));

    let turned = edited(
        &bytes,
        0,
        PlacementEdit::Transform(transforms::rotate_about(1, centre)),
    );
    let bounds = placements(&turned)[0].bounds();
    assert!(
        close(bounds, [35.0, -5.0, 85.0, 95.0]),
        "50 wide, 100 tall: {bounds:?}"
    );
    for quarters in [0, 2, 3] {
        let m = transforms::rotate_about(quarters, (0.0, 0.0));
        let (x, y) = m.apply(1.0, 0.0);
        assert!((x.hypot(y) - 1.0).abs() < 1e-9);
    }

    let flipped = edited(
        &bytes,
        0,
        PlacementEdit::Transform(transforms::flip_about(true, centre)),
    );
    let ctm = placements(&flipped)[0].ctm;
    assert!(ctm.a < 0.0, "mirrored left to right");
    assert!(close(placements(&flipped)[0].bounds(), before[0].bounds()));
    let upside = transforms::flip_about(false, centre);
    assert_eq!(upside.apply(60.0, 45.0), (60.0, 45.0));

    let inside = edited(
        &bytes,
        1,
        PlacementEdit::Transform(transforms::translate(-100.0, 0.0)),
    );
    let after = placements(&inside);
    assert!(
        close(after[1].bounds(), [200.0, 300.0, 220.0, 320.0]),
        "moved inside its form"
    );
    assert_eq!(after[0].bounds(), before[0].bounds());
}

#[test]
fn an_image_is_replaced_in_its_frame_and_taken_away() {
    let bytes = document();
    // A 40 by 10 picture: it fits the 100 by 50 frame 100 wide, centred.
    let picture = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 10] /Contents 4 0 R /Resources << /XObject << /P 5 0 R >> >> >>"
            .to_vec(),
        stream_with("q 40 0 0 10 0 0 cm /P Do Q", ""),
        image(),
    ]);
    let source = open(&picture);
    let placed = placements(&bytes)[0].clone();
    let replaced = apply(&bytes, |tx, _| {
        let (form, bbox) = import_image(tx, &source)?;
        edit_placement(tx, 0, &placed, &PlacementEdit::Replace { form, bbox })
    });
    let after = placements(&replaced);
    assert_eq!(after.len(), 2);
    assert!(
        close(after[0].bounds(), [10.0, 32.5, 110.0, 57.5]),
        "fitted and centred: {:?}",
        after[0].bounds()
    );
    assert_ne!(after[0].image, placed.image, "the new picture's image");

    let inside = placements(&bytes)[1].clone();
    let in_form = apply(&bytes, |tx, _| {
        let (form, bbox) = import_image(tx, &source)?;
        edit_placement(tx, 0, &inside, &PlacementEdit::Replace { form, bbox })
    });
    let after = placements(&in_form);
    assert!(
        close(after[1].bounds(), [300.0, 307.5, 320.0, 312.5]),
        "{:?}",
        after[1].bounds()
    );

    let removed = edited(&bytes, 0, PlacementEdit::Remove);
    let left = placements(&removed);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].bounds(), placements(&bytes)[1].bounds());
}

#[test]
fn an_added_image_is_drawn_after_the_page_fitted_in_its_rectangle() {
    let bytes = document();
    let picture = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Contents 4 0 R /Resources << /XObject << /P 5 0 R >> >> >>"
            .to_vec(),
        stream_with("q 20 0 0 20 0 0 cm /P Do Q", ""),
        image(),
    ]);
    let source = open(&picture);
    let added = apply(&bytes, |tx, _| {
        add_image(tx, 0, &source, [100.0, 400.0, 300.0, 500.0])
    });
    let after = placements(&added);
    assert_eq!(after.len(), 3, "drawn last");
    assert!(
        close(after[2].bounds(), [150.0, 400.0, 250.0, 500.0]),
        "{:?}",
        after[2].bounds()
    );
    let again = apply(&added, |tx, _| {
        add_image(tx, 0, &source, [0.0, 0.0, 10.0, 10.0])
    });
    assert_eq!(
        placements(&again).len(),
        4,
        "a second image, under a name of its own"
    );
}

#[test]
fn an_image_that_moved_since_it_was_found_is_refused() {
    let bytes = document();
    let placed = placements(&bytes)[0].clone();
    let lost = ImagePlacement {
        provenance: None,
        ..placed.clone()
    };
    let error = try_apply(&bytes, |tx, _| {
        edit_placement(tx, 0, &lost, &PlacementEdit::Remove)
    })
    .unwrap_err();
    assert!(
        error.to_string().contains("could not be located"),
        "{error}"
    );
    let mut shifted = placed.clone();
    if let Some(found) = shifted.provenance.as_mut() {
        found.decoded.start += 2;
    }
    let error = try_apply(&bytes, |tx, _| {
        edit_placement(tx, 0, &shifted, &PlacementEdit::Remove)
    })
    .unwrap_err();
    assert!(error.to_string().contains("not where it was"), "{error}");
    let flat = ImagePlacement {
        ctm: Matrix::new(1.0, 1.0, 1.0, 1.0, 0.0, 0.0),
        ..placed
    };
    let error = try_apply(&bytes, |tx, _| {
        edit_placement(tx, 0, &flat, &PlacementEdit::Transform(Matrix::IDENTITY))
    })
    .unwrap_err();
    assert!(error.to_string().contains("drawn flat"), "{error}");
}
