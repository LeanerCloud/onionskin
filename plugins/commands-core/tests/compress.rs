//! Compress, on a document built to have something to compress: a page
//! drawing a photo-like image at 400 ppi.

use onionskin_commands_core::compress::{compress, CompressError, CompressOptions, Compressed};
use onionskin_core::images::{document_images, image_document, ImageColor, ImageData, ImagePage};
use onionskin_core::protection::Refusal;
use onionskin_core::Document;
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::Document as CosDocument;

/// A smooth gradient with a little texture: compresses like a photograph,
/// not like a flat colour a JPEG would shrink to nothing either way.
fn photo(width: u32, height: u32) -> Vec<u8> {
    let mut samples = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let texture = ((x * 7 + y * 13) % 17) as u8;
            samples.push((x * 255 / width) as u8 ^ texture);
            samples.push((y * 255 / height) as u8);
            samples.push(((x + y) * 255 / (width + height)) as u8 ^ (texture / 2));
        }
    }
    samples
}

/// A 5 x 4 inch page drawing a 2000 x 1600 image: 400 ppi.
fn heavy() -> Vec<u8> {
    image_document(&ImagePage {
        width: 2000,
        height: 1600,
        dpi: (400.0, 400.0),
        color: ImageColor::Rgb,
        data: ImageData::Samples(photo(2000, 1600)),
        alpha: None,
        inverted_cmyk: false,
        icc: None,
    })
    .expect("the document builds")
}

fn render(doc: &mut Document) -> Vec<u8> {
    doc.render_page_now(0, 0.25)
        .expect("renders")
        .raster
        .rgba()
        .to_vec()
}

#[test]
fn a_heavy_image_is_downsampled_into_a_smaller_rewrite_that_looks_the_same() {
    let bytes = heavy();
    let mut doc = Document::open_bytes(bytes.clone()).expect("opens");
    let before_render = render(&mut doc);
    let before_text = doc.page_text(0).expect("text").flatten().text;

    let Compressed::Smaller {
        bytes: out,
        before,
        images,
        ..
    } = compress(&mut doc, &CompressOptions::default()).expect("compresses")
    else {
        panic!("a 400 ppi image is something to compress");
    };
    assert_eq!(before, bytes.len());
    assert_eq!(images, 1);
    assert!(
        out.len() * 2 < bytes.len(),
        "{} bytes from {}",
        out.len(),
        bytes.len()
    );

    // The output is a new file: one section, a classic xref table.
    let reread = CosDocument::open(Box::new(onionskin_cos::BytesSource::new(out.clone())))
        .expect("the output parses");
    assert_eq!(
        reread.sections().expect("sections").len(),
        1,
        "no incremental section"
    );
    let tail = String::from_utf8_lossy(&out[out.len().saturating_sub(4096)..]).into_owned();
    assert!(tail.contains("\nxref\n") || out.windows(5).any(|w| w == b"xref\n"));
    assert!(
        !out.windows(10).any(|w| w == b"/Type/XRef")
            && !out.windows(11).any(|w| w == b"/Type /XRef"),
        "no cross-reference stream"
    );
    let image = &document_images(&reread).expect("images")[0];
    assert!(
        image.width.max(image.height) <= 750,
        "150 ppi on a 5 inch page"
    );

    let mut after = Document::open_bytes(out).expect("opens");
    assert_eq!(after.page_count(), 1);
    assert_eq!(
        after.page_text(0).expect("text").flatten().text,
        before_text
    );
    let after_render = render(&mut after);
    assert_eq!(before_render.len(), after_render.len());
    let mean_error = before_render
        .iter()
        .zip(&after_render)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum::<u64>() as f64
        / before_render.len() as f64;
    assert!(
        mean_error < 8.0,
        "renders within tolerance, off by {mean_error}"
    );
}

#[test]
fn a_document_with_nothing_to_compress_says_so_and_writes_nothing() {
    let small = image_document(&ImagePage {
        width: 100,
        height: 80,
        dpi: (72.0, 72.0),
        color: ImageColor::Rgb,
        data: ImageData::Samples(photo(100, 80)),
        alpha: None,
        inverted_cmyk: false,
        icc: None,
    })
    .expect("builds");
    let mut doc = Document::open_bytes(small).expect("opens");
    assert_eq!(
        compress(&mut doc, &CompressOptions::default()).expect("runs"),
        Compressed::Nothing
    );
}

#[test]
fn an_encrypted_document_is_refused() {
    let mut doc = Document::open_path(&encrypted_fixture("r4-aes-128.pdf")).expect("opens");
    assert!(matches!(
        compress(&mut doc, &CompressOptions::default()),
        Err(CompressError::Refused(Refusal::EncryptedSource))
    ));
}

#[test]
fn the_seed_documents_compress_or_say_they_have_nothing_to() {
    for name in ["hello.pdf", "two-page.pdf", "minimal.pdf"] {
        let mut doc = Document::open_path(&seed(name)).expect("opens");
        match compress(&mut doc, &CompressOptions::default()).expect("runs") {
            Compressed::Nothing => {}
            Compressed::Smaller { bytes, .. } => {
                let reread = Document::open_bytes(bytes).expect("the output opens");
                assert_eq!(reread.page_count(), doc.page_count(), "{name}");
            }
        }
    }
}
