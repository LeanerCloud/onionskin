//! Where a page draws its images: placed directly and inside a form, with
//! the `Do` that drew each located in its stream.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::{page_images, Matrix};

fn image() -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
        &[0x80],
    )
}

#[test]
fn every_image_is_found_where_it_is_drawn() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /XObject << /Im0 5 0 R >> >>",
        b"q 0 20 -10 0 150 100 cm /Im0 Do Q",
    );
    let content = "q 100 0 0 50 10 20 cm /Im0 Do Q /Fm0 Do /Fm0 Do";
    let resources = "<< /XObject << /Im0 5 0 R /Fm0 6 0 R >> >>";
    let doc = open_bytes(one_page(content, resources, &[image(), form]));
    let placed = page_images(&doc, 0).expect("reads");
    assert_eq!(placed.len(), 3, "the form's image, twice");
    let direct = &placed[0];
    assert_eq!(direct.name, "Im0");
    assert_eq!(direct.image.number, 5);
    assert_eq!(direct.bounds(), [10.0, 20.0, 110.0, 70.0]);
    assert!(direct.contains(60.0, 40.0));
    assert!(!direct.contains(5.0, 40.0));
    let found = direct.provenance.expect("located");
    assert_eq!(found.stream.number, 4, "in the page's content");
    let span = found.decoded.start as usize..found.decoded.end as usize;
    assert_eq!(&content.as_bytes()[span], b"/Im0 Do");

    let turned = &placed[1];
    assert_eq!(
        turned.provenance.expect("located").stream.number,
        6,
        "in the form"
    );
    assert_eq!(turned.bounds(), [140.0, 100.0, 150.0, 120.0]);
    assert!(turned.contains(145.0, 110.0), "rotated, and still hit");
    assert_eq!(
        turned.corners()[0],
        (150.0, 100.0),
        "its lower left corner, as the image has it"
    );

    let flat = onionskin_content::placements::ImagePlacement {
        ctm: Matrix::new(1.0, 1.0, 1.0, 1.0, 0.0, 0.0),
        ..direct.clone()
    };
    assert!(!flat.contains(0.5, 0.5), "a flattened image covers nothing");
    assert!(
        page_images(&open_bytes(one_page("/Nope Do", "<< >>", &[])), 0)
            .expect("reads")
            .is_empty()
    );
}

#[test]
fn a_matrix_is_undone_by_its_inverse() {
    let matrix = Matrix::new(2.0, 1.0, -1.0, 3.0, 5.0, 7.0);
    let inverse = matrix.inverse().expect("invertible");
    let (x, y) = matrix.apply(4.0, -2.0);
    let (u, v) = inverse.apply(x, y);
    assert!((u - 4.0).abs() < 1e-9 && (v + 2.0).abs() < 1e-9);
    assert!(Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0)
        .inverse()
        .is_none());
}
