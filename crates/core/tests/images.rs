//! `core::images`: a page from an image, and the images out of a document.
//!
//! The page size is asserted against the physical size the image states, not
//! against its pixel count. The mutation this must catch is the one that
//! ignores the resolution: a 300 DPI A4 scan then becomes a page over four
//! times A4's width.

mod common;

use common::{pdf, stream_with};
use onionskin_core::images::{document_images, image_document, ImageColor, ImageData, ImagePage};
use onionskin_core::{Document, Error};
use onionskin_cos::Object;

fn samples(width: u32, height: u32, color: ImageColor, dpi: f64, data: Vec<u8>) -> ImagePage {
    ImagePage {
        width,
        height,
        dpi: (dpi, dpi),
        color,
        data: ImageData::Samples(data),
        alpha: None,
        inverted_cmyk: false,
    }
}

fn media_box(doc: &mut Document) -> Vec<f64> {
    let current = doc.structure().expect("the document");
    let page = current.page(0).expect("one page");
    let Some(Object::Array(items)) = page.dict.get(b"MediaBox") else {
        panic!("the page has a /MediaBox");
    };
    items
        .iter()
        .map(|item| match item {
            Object::Integer(value) => *value as f64,
            Object::Real(value) => *value,
            other => panic!("a number: {other:?}"),
        })
        .collect()
}

/// An A4 page scanned at 300 DPI is 2480 by 3508 pixels.
#[test]
fn a_300_dpi_scan_of_a4_makes_an_a4_page() {
    let (width, height) = (2480, 3508);
    let image = samples(
        width,
        height,
        ImageColor::Gray,
        300.0,
        vec![200; (width * height) as usize],
    );
    let mut doc = Document::open_bytes(image_document(&image).expect("writes")).expect("opens");
    let bounds = media_box(&mut doc);
    assert_eq!(bounds[0], 0.0);
    assert_eq!(bounds[1], 0.0);
    assert!(
        (bounds[2] - 595.2).abs() < 0.01,
        "A4 is 595 points wide: {bounds:?}"
    );
    assert!((bounds[3] - 842.0).abs() < 0.1, "and 842 tall: {bounds:?}");
}

#[test]
fn horizontal_and_vertical_resolution_are_each_their_own() {
    let mut image = samples(144, 144, ImageColor::Gray, 72.0, vec![0; 144 * 144]);
    image.dpi = (144.0, 72.0);
    let mut doc = Document::open_bytes(image_document(&image).expect("writes")).expect("opens");
    let bounds = media_box(&mut doc);
    assert!((bounds[2] - 72.0).abs() < 1e-9, "{bounds:?}");
    assert!((bounds[3] - 144.0).abs() < 1e-9, "{bounds:?}");
}

/// Four quadrants, rendered: the image fills the page, the right way up.
#[test]
fn the_image_fills_the_page_the_right_way_up() {
    let (red, green, blue, white) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]);
    let size = 20u32;
    let mut data = Vec::new();
    for row in 0..size {
        for column in 0..size {
            let quadrant = match (row < size / 2, column < size / 2) {
                (true, true) => red,
                (true, false) => green,
                (false, true) => blue,
                (false, false) => white,
            };
            data.extend_from_slice(&quadrant);
        }
    }
    let image = samples(size, size, ImageColor::Rgb, 36.0, data);
    let mut doc = Document::open_bytes(image_document(&image).expect("writes")).expect("opens");
    let render = doc.render_page_now(0, 1.0).expect("renders");
    let raster = &render.raster;
    assert_eq!(
        (raster.width(), raster.height()),
        (40, 40),
        "20 pixels at 36 DPI is 40 points"
    );

    let pixel = |x: u32, y: u32| {
        let at = ((y * raster.width() + x) * 4) as usize;
        [
            raster.rgba()[at],
            raster.rgba()[at + 1],
            raster.rgba()[at + 2],
        ]
    };
    let near = |got: [u8; 3], want: [u8; 3]| {
        got.iter()
            .zip(want)
            .all(|(g, w)| (i32::from(*g) - i32::from(w)).abs() < 16)
    };
    assert!(
        near(pixel(10, 10), red),
        "top left is red: {:?}",
        pixel(10, 10)
    );
    assert!(near(pixel(30, 10), green), "top right: {:?}", pixel(30, 10));
    assert!(
        near(pixel(10, 30), blue),
        "bottom left: {:?}",
        pixel(10, 30)
    );
    assert!(
        near(pixel(30, 30), white),
        "bottom right: {:?}",
        pixel(30, 30)
    );
}

#[test]
fn transparency_becomes_a_soft_mask() {
    let mut image = samples(10, 10, ImageColor::Gray, 72.0, vec![0; 100]);
    image.alpha = Some(
        (0..100)
            .map(|index| if index % 10 < 5 { 255 } else { 0 })
            .collect(),
    );
    let mut doc = Document::open_bytes(image_document(&image).expect("writes")).expect("opens");
    let render = doc.render_page_now(0, 2.0).expect("renders");
    let raster = &render.raster;
    let luma = |x: u32, y: u32| raster.rgba()[((y * raster.width() + x) * 4) as usize];
    assert!(luma(4, 10) < 40, "the opaque half is black");
    assert!(
        luma(16, 10) > 215,
        "the transparent half shows the white page"
    );
}

#[test]
fn a_jpeg_is_embedded_as_it_is() {
    let jpeg = b"\xFF\xD8 not decoded, only carried \xFF\xD9".to_vec();
    let image = ImagePage {
        width: 8,
        height: 8,
        dpi: (72.0, 72.0),
        color: ImageColor::Cmyk,
        data: ImageData::Jpeg(jpeg.clone()),
        alpha: None,
        inverted_cmyk: true,
    };
    let mut doc = Document::open_bytes(image_document(&image).expect("writes")).expect("opens");
    let images = document_images(doc.structure().expect("the document")).expect("reads");
    assert_eq!(images.len(), 1);
    assert_eq!(
        images[0].content,
        Ok((ImageColor::Cmyk, ImageData::Jpeg(jpeg)))
    );

    let current = doc.structure().expect("the document");
    let parsed = current.get(images[0].object.number).expect("the image");
    let Object::Stream(stream) = parsed.object else {
        panic!("an image stream");
    };
    let decode = stream
        .dict
        .get(b"Decode")
        .expect("Adobe CMYK is inverted back");
    assert_eq!(
        decode,
        &Object::Array([1, 0, 1, 0, 1, 0, 1, 0].map(Object::Integer).to_vec())
    );
}

#[test]
fn samples_round_trip_through_a_document() {
    let data: Vec<u8> = (0..4 * 3 * 3).map(|value| value as u8 * 7).collect();
    let image = samples(4, 3, ImageColor::Rgb, 72.0, data.clone());
    let mut doc = Document::open_bytes(image_document(&image).expect("writes")).expect("opens");
    let images = document_images(doc.structure().expect("the document")).expect("reads");
    assert_eq!(images.len(), 1);
    assert_eq!(
        (images[0].width, images[0].height, images[0].page),
        (4, 3, 0)
    );
    assert_eq!(
        images[0].content,
        Ok((ImageColor::Rgb, ImageData::Samples(data)))
    );
}

#[test]
fn an_image_that_does_not_add_up_is_refused() {
    let short = samples(4, 4, ImageColor::Rgb, 72.0, vec![0; 4 * 4]);
    assert!(matches!(
        image_document(&short),
        Err(Error::InvalidImage(_))
    ));

    let empty = samples(0, 4, ImageColor::Gray, 72.0, Vec::new());
    assert!(matches!(
        image_document(&empty),
        Err(Error::InvalidImage(_))
    ));

    let no_resolution = samples(1, 1, ImageColor::Gray, 0.0, vec![0]);
    assert!(matches!(
        image_document(&no_resolution),
        Err(Error::InvalidImage(_))
    ));

    let mut bad_alpha = samples(2, 2, ImageColor::Gray, 72.0, vec![0; 4]);
    bad_alpha.alpha = Some(vec![0; 3]);
    let refused = image_document(&bad_alpha).expect_err("refused");
    assert!(refused.to_string().contains("alpha"), "{refused}");
}

/// Two pages share one image, a form on the second draws another, and a
/// third is JPEG 2000: three images, each once, the JPX with its reason.
#[test]
fn every_image_once_through_forms_with_what_cannot_be_extracted_said() {
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /XObject << /A 5 0 R >> >> /Contents 9 0 R >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /XObject << /A 5 0 R /F 6 0 R >> >> /Contents 9 0 R >>"
            .to_vec(),
        stream_with(
            "0080ff40>",
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /ASCIIHexDecode",
        ),
        stream_with(
            "/B Do /J Do",
            "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << /B 7 0 R /J 8 0 R /A 5 0 R >> >>",
        ),
        stream_with(
            "a040>",
            "/Type /XObject /Subtype /Image /Width 3 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /ASCIIHexDecode",
        ),
        stream_with(
            "not really a codestream",
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode",
        ),
        stream_with("/A Do", ""),
    ];
    let mut doc = Document::open_bytes(pdf(&objects)).expect("opens");
    let images = document_images(doc.structure().expect("the document")).expect("reads");

    let numbers: Vec<u32> = images.iter().map(|image| image.object.number).collect();
    assert_eq!(
        numbers,
        vec![5, 7, 8],
        "shared images once each, in page order"
    );
    assert_eq!(images[0].page, 0);
    assert_eq!(
        images[1].page, 1,
        "the form's images belong to the page drawing it"
    );

    assert_eq!(
        images[0].content,
        Ok((
            ImageColor::Gray,
            ImageData::Samples(vec![0, 0x80, 0xff, 0x40])
        ))
    );
    assert_eq!(
        images[1].content,
        Ok((
            ImageColor::Gray,
            ImageData::Samples(vec![255, 0, 255, 0, 255, 0])
        )),
        "one-bit rows are unpacked, each padded to a byte"
    );
    let reason = images[2].content.clone().expect_err("JPX is not extracted");
    assert!(reason.contains("JPXDecode"), "{reason}");
}
