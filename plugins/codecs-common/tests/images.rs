//! Image import, JPEG and TIFF export, and Export All Images, asserted on what
//! they write, read back by a decoder that is not the one that wrote it.
//!
//! The page an image becomes is asserted by its physical size and by its
//! pixels. The mutation this must catch is the one that fixes the page to A4:
//! the 300 DPI scan below is not A4-shaped, so a fixed page size fails it.

use std::io::Cursor;

use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{ExtendedColorType, ImageEncoder, RgbImage};
use onionskin_codecs_common::{
    extract_image, extract_images, CommonCodecsPlugin, JpegCodec, PngCodec, TiffCodec,
};
use onionskin_core::images::{document_images, ImageColor, ImageData};
use onionskin_core::pages::Assembly;
use onionskin_core::Document;
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Document as CosDocument, Object};
use onionskin_plugin_api::{CodecPlugin, ExportRequest, ImportError, PageRange, PluginRegistry};
use tiff::encoder::{colortype, Rational, TiffEncoder};
use tiff::tags::ResolutionUnit;

// ---------------------------------------------------------------------------
// Fixtures, written here so each carries exactly what its test states.
// ---------------------------------------------------------------------------

/// A test card: four coloured quadrants and a diagonal, so a page drawn
/// mirrored, flipped or cropped differs from its source.
fn card(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        let on_diagonal = x * height / width == y;
        match (x < width / 2, y < height / 2, on_diagonal) {
            (_, _, true) => image::Rgb([0, 0, 0]),
            (true, true, _) => image::Rgb([220, 40, 40]),
            (false, true, _) => image::Rgb([40, 200, 40]),
            (true, false, _) => image::Rgb([40, 40, 220]),
            (false, false, _) => image::Rgb([240, 240, 240]),
        }
    })
}

/// A PNG of `pixels`, with a `pHYs` chunk stating `dpi` when given.
fn png(pixels: &RgbImage, dpi: Option<f64>) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            ExtendedColorType::Rgb8,
        )
        .expect("encodes");
    if let Some(dpi) = dpi {
        let per_metre = (dpi / 0.0254).round() as u32;
        let mut data = per_metre.to_be_bytes().to_vec();
        data.extend_from_slice(&per_metre.to_be_bytes());
        data.push(1);
        // After the eight-byte signature and the 25-byte IHDR chunk.
        bytes.splice(33..33, chunk(b"pHYs", &data));
    }
    bytes
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn jpeg(pixels: &RgbImage, dpi: u16, quality: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(Cursor::new(&mut bytes), quality);
    encoder.set_pixel_density(PixelDensity::dpi(dpi));
    encoder
        .encode(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            ExtendedColorType::Rgb8,
        )
        .expect("encodes");
    bytes
}

/// A TIFF with one directory per entry of `pages`, CMYK, at `dpi`.
fn cmyk_tiff(pages: &[(u32, u32, Vec<u8>)], dpi: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = TiffEncoder::new(Cursor::new(&mut bytes)).expect("encoder");
    for (width, height, samples) in pages {
        let mut image = encoder
            .new_image::<colortype::CMYK8>(*width, *height)
            .expect("image");
        image.resolution(ResolutionUnit::Inch, Rational { n: dpi, d: 1 });
        image.write_data(samples).expect("writes");
    }
    bytes
}

fn open(pdf: Vec<u8>) -> Document {
    Document::open_bytes(pdf).expect("the import is a PDF that opens")
}

fn media_box(doc: &mut Document, page: usize) -> [f64; 4] {
    let current = doc.structure().expect("the document");
    let page = current.page(page).expect("the page");
    let Some(Object::Array(items)) = page.dict.get(b"MediaBox") else {
        panic!("the page has a /MediaBox");
    };
    let number = |item: &Object| match item {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => *value,
        other => panic!("a number: {other:?}"),
    };
    [
        number(&items[0]),
        number(&items[1]),
        number(&items[2]),
        number(&items[3]),
    ]
}

/// The largest per-channel difference between a render and `source`, sampled
/// at the centre of every source pixel. The page is rendered at one device
/// pixel per image pixel.
fn worst_difference(doc: &mut Document, source: &RgbImage, zoom: f32) -> u8 {
    let render = doc.render_page_now(0, zoom).expect("renders");
    let raster = &render.raster;
    assert_eq!(
        (raster.width(), raster.height()),
        source.dimensions(),
        "one device pixel per image pixel"
    );
    let mut worst = 0;
    for (x, y, pixel) in source.enumerate_pixels() {
        // Skip the diagonal and quadrant edges, where resampling blends.
        let near_edge = |value: u32, size: u32| value.abs_diff(size / 2) < 2;
        if near_edge(x, source.width()) || near_edge(y, source.height()) {
            continue;
        }
        if (x * source.height() / source.width()).abs_diff(y) < 3 {
            continue;
        }
        let at = ((y * raster.width() + x) * 4) as usize;
        for channel in 0..3 {
            worst = worst.max(raster.rgba()[at + channel].abs_diff(pixel[channel]));
        }
    }
    worst
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// A US Letter scan at 300 DPI, 2550 by 3300 pixels, is a Letter page - not
/// A4, and not 2550 points wide.
#[test]
fn a_300_dpi_png_is_a_page_of_its_physical_size() {
    let scan = card(2550, 3300);
    let mut doc = open(PngCodec.import(&png(&scan, Some(300.0))).expect("imports"));
    let [x0, y0, x1, y1] = media_box(&mut doc, 0);
    assert_eq!((x0, y0), (0.0, 0.0));
    assert!((x1 - 612.0).abs() < 0.05, "8.5 inches wide: {x1}");
    assert!((y1 - 792.0).abs() < 0.05, "11 inches tall: {y1}");
}

#[test]
fn a_png_page_draws_the_png() {
    let source = card(96, 64);
    let mut doc = open(PngCodec.import(&png(&source, None)).expect("imports"));
    assert_eq!(
        media_box(&mut doc, 0),
        [0.0, 0.0, 96.0, 64.0],
        "no pHYs is 72 DPI"
    );
    let worst = worst_difference(&mut doc, &source, 1.0);
    assert!(
        worst <= 8,
        "the page is the image: worst channel difference {worst}"
    );
}

#[test]
fn a_png_with_transparency_keeps_it_as_a_soft_mask() {
    let mut bytes = Vec::new();
    let pixels: Vec<u8> = (0..16)
        .flat_map(|index| [0, 0, 0, if index % 4 < 2 { 255 } else { 0 }])
        .collect();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, 4, 4, ExtendedColorType::Rgba8)
        .expect("encodes");
    let mut doc = open(PngCodec.import(&bytes).expect("imports"));
    let render = doc.render_page_now(0, 4.0).expect("renders");
    let luma = |x: u32| render.raster.rgba()[((8 * render.raster.width() + x) * 4) as usize];
    assert!(luma(2) < 40, "the opaque half is black");
    assert!(luma(13) > 215, "the clear half shows the page");
}

/// The JPEG is in the PDF byte for byte, and the page is its JFIF size.
#[test]
fn a_jpeg_is_carried_unchanged_and_sized_by_its_density() {
    let source = card(300, 150);
    let file = jpeg(&source, 150, 92);
    let mut doc = open(JpegCodec::default().import(&file).expect("imports"));
    assert_eq!(media_box(&mut doc, 0), [0.0, 0.0, 144.0, 72.0]);

    let images = document_images(doc.structure().expect("the document")).expect("reads");
    assert_eq!(images.len(), 1);
    assert_eq!(
        images[0].content,
        Ok((ImageColor::Rgb, ImageData::Jpeg(file))),
        "never decoded and re-encoded"
    );

    let worst = worst_difference(&mut doc, &source, 150.0 / 72.0);
    assert!(worst <= 24, "the page draws the JPEG: worst {worst}");
}

/// CMYK stays CMYK: the samples that come back out are the samples that
/// went in, and each directory of the file is a page at its own size.
#[test]
fn a_multi_page_cmyk_tiff_is_a_page_per_directory_in_cmyk() {
    let first: Vec<u8> = (0..4 * 2 * 4).map(|value| (value * 7) as u8).collect();
    let second = vec![0, 0, 0, 255, 255, 0, 0, 0];
    let file = cmyk_tiff(&[(4, 2, first.clone()), (2, 1, second.clone())], 200);
    let mut doc = open(TiffCodec.import(&file).expect("imports"));
    assert_eq!(doc.page_count(), 2);
    let [_, _, width, height] = media_box(&mut doc, 0);
    assert!(
        (width - 4.0 * 72.0 / 200.0).abs() < 1e-6 && (height - 2.0 * 72.0 / 200.0).abs() < 1e-6
    );

    let images = document_images(doc.structure().expect("the document")).expect("reads");
    let contents: Vec<_> = images.into_iter().map(|image| image.content).collect();
    assert_eq!(
        contents,
        vec![
            Ok((ImageColor::Cmyk, ImageData::Samples(first))),
            Ok((ImageColor::Cmyk, ImageData::Samples(second))),
        ]
    );
}

#[test]
fn a_sixteen_bit_gray_tiff_is_read_at_eight() {
    let mut bytes = Vec::new();
    let mut encoder = TiffEncoder::new(Cursor::new(&mut bytes)).expect("encoder");
    encoder
        .write_image::<colortype::Gray16>(2, 1, &[0x1234, 0xFF00])
        .expect("writes");
    let mut doc = open(TiffCodec.import(&bytes).expect("imports"));
    let images = document_images(doc.structure().expect("the document")).expect("reads");
    assert_eq!(
        images[0].content,
        Ok((ImageColor::Gray, ImageData::Samples(vec![0x12, 0xFF])))
    );
}

#[test]
fn each_format_is_recognised_by_its_signature_and_only_by_it() {
    let mut registry = PluginRegistry::new();
    registry.install(&CommonCodecsPlugin);
    let importer = |bytes: &[u8]| registry.importer(bytes).map(|codec| codec.id());

    assert_eq!(importer(&png(&card(2, 2), None)), Some("png"));
    assert_eq!(importer(&jpeg(&card(8, 8), 72, 80)), Some("jpeg"));
    assert_eq!(
        importer(&cmyk_tiff(&[(1, 1, vec![0; 4])], 72)),
        Some("tiff")
    );
    assert_eq!(importer(b"%PDF-1.7"), None);
    assert_eq!(importer(b"GIF89a"), None);
    let importing: Vec<_> = registry
        .codecs()
        .filter(|codec| codec.imports())
        .map(|codec| codec.id())
        .collect();
    assert_eq!(importing, ["png", "jpeg", "tiff"]);
}

#[test]
fn a_file_that_claims_a_format_and_is_not_one_says_so() {
    let mut truncated = png(&card(16, 16), None);
    truncated.truncate(40);
    assert!(matches!(
        PngCodec.import(&truncated),
        Err(ImportError::Decode(_))
    ));
    assert!(matches!(
        TiffCodec.import(b"II*\0garbage"),
        Err(ImportError::Decode(_))
    ));
    assert!(matches!(
        JpegCodec::default().import(&[0xFF, 0xD8, 0xFF, 0xD9]),
        Err(ImportError::Decode(_))
    ));
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

fn export(codec: &dyn CodecPlugin, dpi: f32) -> Vec<u8> {
    export_at(codec, dpi, None)
}

fn export_at(codec: &dyn CodecPlugin, dpi: f32, quality: Option<u8>) -> Vec<u8> {
    let mut doc = Document::open_path(&seed("hello.pdf")).expect("seed opens");
    let request = ExportRequest {
        pages: PageRange::whole(doc.page_count()).expect("pages"),
        dpi,
        quality,
    };
    codec
        .export_page(&mut doc, &request, 0, true)
        .expect("exports")
}

fn seed_page_pixels(dpi: f32) -> (u32, u32) {
    let mut doc = Document::open_path(&seed("hello.pdf")).expect("seed opens");
    let render = doc.render_page_now(0, dpi / 72.0).expect("renders");
    (render.raster.width(), render.raster.height())
}

#[test]
fn a_jpeg_export_is_the_page_at_its_resolution_and_quality_changes_its_size() {
    let small = export(&JpegCodec::new(20), 144.0);
    let large = export(&JpegCodec::new(95), 144.0);
    let decoded =
        image::load_from_memory_with_format(&large, image::ImageFormat::Jpeg).expect("a JPEG");
    assert_eq!(decoded.width(), seed_page_pixels(144.0).0);
    assert_eq!(decoded.height(), seed_page_pixels(144.0).1);
    assert!(
        small.len() * 3 / 2 < large.len(),
        "quality 20 is well under quality 95: {} vs {}",
        small.len(),
        large.len()
    );
    assert_eq!(
        export_at(&JpegCodec::new(95), 144.0, Some(20)).len(),
        small.len(),
        "the request's quality wins over the codec's"
    );
    let header_dpi = u16::from_be_bytes([large[14], large[15]]);
    assert_eq!(
        header_dpi, 144,
        "the JFIF density is the export's resolution"
    );
}

#[test]
fn a_tiff_export_decodes_and_states_its_resolution() {
    let bytes = export(&TiffCodec, 100.0);
    let decoded =
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff).expect("a TIFF");
    assert_eq!((decoded.width(), decoded.height()), seed_page_pixels(100.0));

    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(&bytes)).expect("reads");
    let resolution = decoder
        .get_tag(tiff::tags::Tag::XResolution)
        .expect("tagged");
    assert_eq!(resolution, tiff::decoder::ifd::Value::Rational(100, 1));
}

// ---------------------------------------------------------------------------
// Export All Images
// ---------------------------------------------------------------------------

/// Three pages, three images: a JPEG, RGB samples from a PNG, and CMYK
/// samples from a TIFF, plus the JPEG's page again so one image is drawn
/// twice.
fn three_image_document() -> (Vec<u8>, Vec<u8>, RgbImage, Vec<u8>) {
    let photo = jpeg(&card(40, 20), 72, 85);
    let drawing = card(12, 8);
    let print = vec![10, 20, 30, 40, 50, 60, 70, 80];
    let inputs = [
        JpegCodec::default().import(&photo).expect("imports"),
        PngCodec.import(&png(&drawing, None)).expect("imports"),
        TiffCodec
            .import(&cmyk_tiff(&[(2, 1, print.clone())], 72))
            .expect("imports"),
    ];
    let mut assembly = Assembly::new();
    for input in &inputs {
        let (source, _) =
            CosDocument::open_repairing(Box::new(BytesSource::new(input.clone()))).expect("opens");
        assembly.append(&source, &[0]).expect("appends");
    }
    (
        assembly.finish().expect("assembles").bytes,
        photo,
        drawing,
        print,
    )
}

#[test]
fn every_image_comes_out_once_in_the_format_that_keeps_it() {
    let (pdf, photo, drawing, print) = three_image_document();
    let (doc, _) = CosDocument::open_repairing(Box::new(BytesSource::new(pdf))).expect("opens");
    let extraction = extract_images(&doc).expect("extracts");
    assert!(extraction.skipped.is_empty(), "{:?}", extraction.skipped);
    assert_eq!(extraction.images.len(), 3, "the known image count");

    let names: Vec<&str> = extraction
        .images
        .iter()
        .map(|image| image.name.as_str())
        .collect();
    assert!(
        names[0].starts_with("page-1-") && names[0].ends_with(".jpg"),
        "{names:?}"
    );
    assert!(
        names[1].starts_with("page-2-") && names[1].ends_with(".png"),
        "{names:?}"
    );
    assert!(
        names[2].starts_with("page-3-") && names[2].ends_with(".tif"),
        "{names:?}"
    );

    assert_eq!(
        extraction.images[0].bytes, photo,
        "the JPEG as it is in the file"
    );
    let png =
        image::load_from_memory_with_format(&extraction.images[1].bytes, image::ImageFormat::Png)
            .expect("decodes")
            .to_rgb8();
    assert_eq!(png, drawing, "PNG is lossless");

    let mut decoder =
        tiff::decoder::Decoder::new(Cursor::new(&extraction.images[2].bytes)).expect("reads");
    assert_eq!(
        decoder.colortype().expect("typed"),
        tiff::ColorType::CMYK(8)
    );
    let tiff::decoder::DecodingResult::U8(samples) = decoder.read_image().expect("decodes") else {
        panic!("8-bit samples");
    };
    assert_eq!(samples, print, "CMYK stays CMYK");
}

#[test]
fn an_image_that_cannot_be_extracted_is_listed_with_its_reason() {
    let pdf = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /I 4 0 R >> >> >> endobj
4 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /JBIG2Decode /Length 1 >>
stream
x
endstream endobj
trailer << /Root 1 0 R >>
%%EOF
"
    .to_vec();
    let (doc, _) = CosDocument::open_repairing(Box::new(BytesSource::new(pdf))).expect("repairs");
    let extraction = extract_images(&doc).expect("extracts");
    assert!(extraction.images.is_empty());
    assert_eq!(extraction.skipped.len(), 1);
    assert_eq!(
        (extraction.skipped[0].page, extraction.skipped[0].object),
        (0, 4)
    );
    assert!(
        extraction.skipped[0].reason.contains("JBIG2"),
        "{:?}",
        extraction.skipped
    );
}

#[test]
fn one_image_comes_out_as_export_all_images_writes_it() {
    let (pdf, photo, _, _) = three_image_document();
    let (doc, _) = CosDocument::open_repairing(Box::new(BytesSource::new(pdf))).expect("opens");
    let all = extract_images(&doc).expect("extracts");
    let first = document_images(&doc).expect("finds")[0].object;
    let one = extract_image(&doc, first, 0).expect("extracts one");
    assert_eq!(one, all.images[0], "same name, same bytes");
    assert_eq!(one.bytes, photo);

    let catalog = onionskin_cos::ObjRef::new(1, 0);
    let refused = extract_image(&doc, catalog, 0).expect_err("not an image");
    assert!(refused.contains("not an image"), "{refused}");
    let missing = onionskin_cos::ObjRef::new(9999, 0);
    assert!(extract_image(&doc, missing, 0).is_err());
}
