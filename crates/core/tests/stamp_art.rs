//! A stamp's artwork: a drawing, or a page of another PDF, scaled into the
//! stamp's rect and rendered.

use std::sync::Arc;

use onionskin_core::images::{image_document, ImageColor, ImageData, ImagePage};
use onionskin_core::{
    add_annotation, Annotation, BaseFont, Document, Error, Rect, StampArt, Subtype,
};
use onionskin_corpus_testing::{encrypted_fixture, seed};

const NOW: i64 = 1_758_000_000;

fn place(document: &mut Document, rect: Rect, art: StampArt) -> Result<(), Error> {
    let page = document
        .structure()
        .expect("the document")
        .page(0)
        .expect("page one")
        .objref;
    let mut annotation = Annotation::new(Subtype::Stamp, rect);
    annotation.stamp_art = Some(art);
    document.edit_annotations("Stamp", |tx, structure| {
        add_annotation(tx, structure, page, &annotation, NOW).map(|_| ())
    })
}

/// Pixels in `rect` (page space, zoom 1) that are mostly `channel`.
fn coloured(document: &mut Document, rect: Rect, channel: usize) -> usize {
    let render = document.render_page_now(0, 1.0).expect("renders");
    let (width, height) = (render.raster.width(), render.raster.height());
    let mut count = 0;
    for y in 0..height {
        for x in 0..width {
            let (px, py) = (f64::from(x) + 0.5, f64::from(height - y) - 0.5);
            if px < rect.x0 || px > rect.x1 || py < rect.y0 || py > rect.y1 {
                continue;
            }
            let at = ((y * width + x) * 4) as usize;
            let pixel = &render.raster.rgba()[at..at + 3];
            let others = (0..3)
                .filter(|c| *c != channel)
                .map(|c| i32::from(pixel[c]));
            if others
                .clone()
                .all(|other| i32::from(pixel[channel]) > other + 80)
            {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_drawing_is_scaled_from_its_own_box_into_the_rect() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    // A 10x5 box filled red, drawn into a 60x30 rect: the whole rect is red.
    let rect = Rect::new(20.0, 20.0, 80.0, 50.0);
    place(
        &mut document,
        rect,
        StampArt::Drawing {
            size: (10.0, 5.0),
            content: "1 0 0 rg 0 0 10 5 re f".into(),
            fonts: vec![BaseFont::HelveticaBold],
        },
    )
    .expect("stamps");
    let red = coloured(&mut document, rect, 0);
    assert!(red > 60 * 30 * 8 / 10, "the rect is filled: {red}");
    let outside = coloured(&mut document, Rect::new(90.0, 60.0, 200.0, 100.0), 0);
    assert_eq!(outside, 0, "and nothing outside it");
}

/// A custom stamp is a page: here, a page drawn from a green image, placed
/// at a size unlike its own.
#[test]
fn a_page_of_another_pdf_becomes_the_stamp_and_survives_a_save() {
    let art = image_document(&ImagePage {
        width: 4,
        height: 2,
        dpi: (72.0, 72.0),
        color: ImageColor::Rgb,
        data: ImageData::Samples([0u8, 200, 0].repeat(8)),
        alpha: None,
        inverted_cmyk: false,
        icc: None,
    })
    .expect("writes");
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("stamped.pdf");
    std::fs::copy(seed("hello.pdf"), &path).expect("copies");
    let mut file = onionskin_core::DocumentFile::open(&path).expect("opens");
    let rect = Rect::new(100.0, 10.0, 180.0, 50.0);
    place(file.document_mut(), rect, StampArt::Page(Arc::new(art))).expect("stamps");
    file.save().expect("saves");
    drop(file);

    let mut reopened = Document::open_path(&path).expect("reopens");
    let green = coloured(&mut reopened, rect, 1);
    assert!(
        green > 80 * 40 * 8 / 10,
        "the page fills the stamp: {green}"
    );
}

#[test]
fn a_page_from_an_encrypted_pdf_is_refused() {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    let encrypted = std::fs::read(encrypted_fixture("r4-aes-128.pdf")).expect("reads");
    let refused = place(
        &mut document,
        Rect::new(0.0, 0.0, 10.0, 10.0),
        StampArt::Page(Arc::new(encrypted)),
    );
    assert!(matches!(refused, Err(Error::Protected(_))), "{refused:?}");
    assert_eq!(document.edit().history().reach(), 0);
}
