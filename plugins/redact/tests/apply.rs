//! Apply Redactions end to end: marks made through the plugin, applied,
//! and the new file read back by a fresh parse.

use onionskin_content::extract_page;
use onionskin_core::images::{decode_image, ImageData};
use onionskin_core::redactions::{Align, Overlay, RedactionLook};
use onionskin_core::{Document, SearchOptions};
use onionskin_cos::{BytesSource, Document as CosDocument, Object};
use onionskin_redact::find::{find, Pattern, Query};
use onionskin_redact::mark::{mark_found, mark_pages, mark_region, marks, unmark};
use onionskin_redact::{apply_redactions, RedactError};

/// Numbered object bodies as a classic-xref PDF, object `n` at `objects[n - 1]`.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// One 300 by 300 page drawing `content`, with Helvetica as /F1 and a grey
/// 10 by 10 image as /Im0; `annots` and `extra` follow from object 7.
fn page(content: &str, annots: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> /XObject << /Im0 6 0 R >> >> {annots} >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Image /Width 10 /Height 10 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[128; 100],
        ),
    ];
    objects.extend_from_slice(extra);
    pdf(&objects)
}

const TEXT: &str = "BT /F1 12 Tf 20 200 Td (Hello Secret World) Tj 0 -20 Td (Public line) Tj ET";

fn open(bytes: Vec<u8>) -> Document {
    Document::open_bytes(bytes).expect("opens")
}

fn reopen(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the redacted file opens")
}

fn text(bytes: &[u8]) -> String {
    let doc = reopen(bytes);
    extract_page(&doc, 0)
        .expect("extracts")
        .runs
        .iter()
        .map(|run| run.text.clone())
        .collect::<Vec<_>>()
        .join("|")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn secret(doc: &mut Document) {
    let found = find(
        doc,
        &Query::Text("Secret".to_owned(), SearchOptions::default()),
    )
    .expect("finds");
    assert_eq!(found.len(), 1);
    mark_found(doc, &found, &RedactionLook::default()).expect("marks");
}

#[test]
fn marked_text_is_gone_from_the_text_and_the_bytes() {
    let mut doc = open(page(TEXT, "", &[]));
    secret(&mut doc);
    let applied = apply_redactions(&mut doc).expect("applies");
    assert!(applied.verification.passed());
    assert_eq!(applied.verification.pages, 1);
    assert!(applied.verification.streams_scanned >= 1);
    assert_eq!(applied.report.counts.glyphs, 6);
    assert_eq!(applied.report.text, [(0, "Secret".to_owned())]);
    assert_eq!(applied.report.pages, [0]);

    let after = text(&applied.bytes);
    assert!(!after.contains("Secret"), "{after}");
    assert!(after.contains("Hello") && after.contains("World") && after.contains("Public line"));
    assert!(
        !contains(&applied.bytes, b"Secret"),
        "no byte of it is left"
    );
    assert!(
        !contains(&applied.bytes, b"/Redact"),
        "the mark went with it"
    );
    assert_eq!(
        applied.bytes.windows(5).filter(|w| w == b"%%EOF").count(),
        1,
        "one revision"
    );
    assert!(!contains(&applied.bytes, b"/Prev"));
    // The session itself is untouched: the mark is still there to undo.
    assert_eq!(marks(&mut doc).expect("reads").len(), 1);
}

#[test]
fn an_image_under_a_mark_is_painted_out_and_annotations_there_go() {
    let content = format!("q 100 0 0 100 0 0 cm /Im0 Do Q {TEXT}");
    let annots = "/Annots [7 0 R 8 0 R]";
    let extra = [
        b"<< /Type /Annot /Subtype /Square /Rect [10 10 40 40] /Popup 9 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Square /Rect [250 250 260 260] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Popup /Rect [0 0 1 1] >>".to_vec(),
    ];
    let mut doc = open(page(&content, annots, &extra));
    mark_region(
        &mut doc,
        0,
        [0.0, 0.0, 50.0, 100.0],
        &RedactionLook::default(),
    )
    .expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");
    assert_eq!(applied.report.counts.images, 1);
    assert_eq!(applied.report.annotations, 1);
    assert_eq!(applied.verification.images_checked, 1);

    let out = reopen(&applied.bytes);
    let page = out.page(0).expect("page");
    let annots = out
        .resolve(page.dict.get(b"Annots").expect("annots"))
        .expect("resolves");
    assert_eq!(
        annots.as_array().map(<[Object]>::len),
        Some(1),
        "the far one stays"
    );
    let resources = page
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("resources");
    let xobjects = out
        .resolve(resources.get(b"XObject").expect("xobjects"))
        .expect("resolves");
    let scrubbed = xobjects
        .as_dict()
        .expect("dict")
        .iter()
        .find(|(name, _)| name.as_bytes() != b"Im0")
        .and_then(|(_, value)| value.as_reference())
        .expect("a scrubbed copy");
    let stream = out.get(scrubbed.number).expect("image").object;
    let (_, _, _, ImageData::Samples(samples)) =
        decode_image(&out, stream.as_stream().expect("a stream")).expect("decodes")
    else {
        panic!("samples");
    };
    // The left half, columns 0 to 4, is black; the right half is as it was.
    assert_eq!(&samples[..5], &[0; 5]);
    assert_eq!(&samples[5..10], &[128; 5]);
    assert!(
        text(&applied.bytes).contains("Hello"),
        "text away from the mark stays"
    );
}

#[test]
fn overlay_text_is_written_over_the_area_and_passes_verification() {
    let mut doc = open(page(TEXT, "", &[]));
    let look = RedactionLook {
        overlay: Some(Overlay {
            text: "(b)(6)".to_owned(),
            align: Align::Left,
            ..Overlay::default()
        }),
        ..RedactionLook::default()
    };
    mark_region(&mut doc, 0, [15.0, 195.0, 290.0, 215.0], &look).expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");
    let after = text(&applied.bytes);
    assert!(after.contains("(b)(6)"), "{after}");
    assert!(
        !after.contains("Hello") && after.contains("Public line"),
        "{after}"
    );
}

#[test]
fn patterns_and_whole_pages_are_marked() {
    let content = "BT /F1 12 Tf 20 200 Td (Call 555-123-4567 now) Tj ET";
    let mut doc = open(page(content, "", &[]));
    let found = find(&mut doc, &Query::Pattern(Pattern::PhoneNumbers)).expect("finds");
    assert_eq!(found[0].text, "555-123-4567");
    mark_found(&mut doc, &found, &RedactionLook::default()).expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");
    let after = text(&applied.bytes);
    assert!(after.contains("Call") && !after.contains("4567"), "{after}");

    let mut whole = open(page(TEXT, "", &[]));
    assert_eq!(
        mark_pages(&mut whole, &[0], &RedactionLook::default()).expect("marks"),
        1
    );
    let applied = apply_redactions(&mut whole).expect("applies");
    assert_eq!(text(&applied.bytes), "");
}

#[test]
fn a_structure_element_no_longer_spells_what_was_removed() {
    let extra = [b"<< /S /Span /Alt (the Secret word) /ActualText (kept) >>".to_vec()];
    // The page names the element, so the new file keeps it.
    let mut doc = open(page(TEXT, "/OsTest 7 0 R", &extra));
    secret(&mut doc);
    let applied = apply_redactions(&mut doc).expect("applies");
    let out = reopen(&applied.bytes);
    let element = out.get(7).expect("element").object;
    let element = element.as_dict().expect("dict");
    assert!(!element.contains(b"Alt"));
    assert!(
        element.contains(b"ActualText"),
        "what does not spell it stays"
    );
}

#[test]
fn an_earlier_revision_is_not_carried_into_the_new_file() {
    let original = page("BT /F1 12 Tf 20 200 Td (Old Secret draft) Tj ET", "", &[]);
    // Draw new content over the old revision, saved incrementally.
    let revised = {
        let mut bytes = original;
        let tail = format!(
            "4 0 obj\n{}\nendobj\n",
            String::from_utf8(stream("", TEXT.as_bytes())).expect("ascii")
        );
        let offset = bytes.len();
        bytes.extend_from_slice(tail.as_bytes());
        let xref = bytes.len();
        let prev = bytes
            .windows(6)
            .position(|window| window == b"xref\n0")
            .expect("the first table");
        bytes.extend_from_slice(
            format!(
                "xref\n4 1\n{offset:010} 00000 n \ntrailer\n<< /Size 7 /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        bytes
    };
    let mut doc = open(revised);
    secret(&mut doc);
    let applied = apply_redactions(&mut doc).expect("applies");
    assert!(
        !contains(&applied.bytes, b"Old Secret"),
        "the old revision is gone"
    );
    assert!(!contains(&applied.bytes, b"Secret"));
}

#[test]
fn nothing_marked_is_refused_and_a_mark_can_be_taken_off() {
    let mut doc = open(page(TEXT, "", &[]));
    assert!(matches!(
        apply_redactions(&mut doc),
        Err(RedactError::NothingMarked)
    ));
    let mark = mark_region(
        &mut doc,
        0,
        [0.0, 0.0, 10.0, 10.0],
        &RedactionLook::default(),
    )
    .expect("marks");
    unmark(&mut doc, 0, mark).expect("unmarks");
    assert!(marks(&mut doc).expect("reads").is_empty());
    let error = apply_redactions(&mut doc).unwrap_err();
    assert_eq!(error.to_string(), "nothing is marked for redaction");
}

#[test]
fn a_jpeg_and_a_soft_mask_under_a_mark_are_painted_out() {
    use image::ImageEncoder as _;
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .write_image(&[200u8; 8 * 8 * 3], 8, 8, image::ExtendedColorType::Rgb8)
        .expect("encodes");
    let mut photo = format!(
        "<< /Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /DCTDecode /SMask 8 0 R /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    photo.extend_from_slice(&jpeg);
    photo.extend_from_slice(b"\nendstream");
    let mask = stream(
        "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 8",
        &[0; 64],
    );
    let content = "q 80 0 0 80 0 0 cm /Im1 Do Q";
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
           /Resources << /XObject << /Im1 7 0 R >> >> >>"
            .to_vec(),
        stream("", content.as_bytes()),
    ];
    objects.push(b"null".to_vec());
    objects.push(b"null".to_vec());
    objects.push(photo);
    objects.push(mask);
    let mut doc = open(pdf(&objects));
    let look = RedactionLook {
        fill: Some([1.0, 0.0, 0.0]),
        ..RedactionLook::default()
    };
    mark_region(&mut doc, 0, [0.0, 0.0, 40.0, 80.0], &look).expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");
    assert_eq!(applied.verification.images_checked, 1);

    let out = reopen(&applied.bytes);
    let page = out.page(0).expect("page");
    let resources = page.dict.get(b"Resources").and_then(Object::as_dict).expect("resources");
    let xobjects = out.resolve(resources.get(b"XObject").expect("xobjects")).expect("resolves");
    let (_, copy) = xobjects.as_dict().expect("dict").iter().next().expect("one image").clone();
    let copy = out.get(copy.as_reference().expect("a reference").number).expect("copy").object;
    let copy = copy.as_stream().expect("a stream");
    let (_, _, _, ImageData::Samples(samples)) = decode_image(&out, copy).expect("decodes") else {
        panic!("samples");
    };
    assert_eq!(&samples[..3], &[255, 0, 0], "the covered pixels take the red fill");
    let mask = out
        .get(copy.dict.get(b"SMask").and_then(Object::as_reference).expect("a mask").number)
        .expect("mask")
        .object;
    let (_, _, _, ImageData::Samples(alpha)) =
        decode_image(&out, mask.as_stream().expect("a stream")).expect("decodes")
    else {
        panic!("samples");
    };
    assert_eq!((alpha[0], alpha[7]), (255, 0), "the fill shows where it covers");
}

#[test]
fn an_image_that_cannot_be_decoded_is_not_drawn_under_a_mark() {
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /JPXDecode",
        b"not a jpx",
    );
    let content = "q 100 0 0 100 0 0 cm /Im1 Do Q";
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
           /Resources << /XObject << /Im1 5 0 R >> >> >>"
            .to_vec(),
        stream("", content.as_bytes()),
        image,
    ];
    let mut doc = open(pdf(&objects));
    mark_region(&mut doc, 0, [0.0, 0.0, 10.0, 10.0], &RedactionLook::default()).expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");
    assert!(!contains(&applied.bytes, b"not a jpx"), "the original is gone");
    assert_eq!(applied.verification.images_checked, 0, "a blank form stands in");
}
