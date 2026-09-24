//! The verifier: the redacted file read back, and nothing of what was
//! removed found in it.
//!
//! It is part of applying, not of the test suite: a file it finds anything
//! in is not handed back. It checks, on every redacted page:
//!
//! - **text**: no glyph inside an area, other than the overlay text the
//!   redaction wrote itself;
//! - **images**: every image still drawn over an area has the covered
//!   pixels painted out;
//! - **inline images and marks**: none left over an area, and no redaction
//!   mark left on the page;
//! - **bytes**: the page's old content streams, where no other page still
//!   draws them, are nowhere in the file.

use onionskin_content::redact::{covers_glyph, Area, NewResource, Rewritten};
use onionskin_content::{extract_page, Matrix};
use onionskin_core::images::{decode_image, ImageData};
use onionskin_cos::{BytesSource, Document as CosDocument, ObjRef, Object};

use crate::RedactError;

/// What the verifier has to find absent on one page.
#[derive(Debug, Clone)]
pub(crate) struct Expectation {
    pub(crate) page: usize,
    pub(crate) areas: Vec<Area>,
    /// The stream whose text is the overlay the redaction wrote.
    pub(crate) overlay: ObjRef,
    /// The raw bytes of the page's old content streams, by object number.
    pub(crate) originals: Vec<(u32, Vec<u8>)>,
}

/// What the verifier checked, and what it found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Verification {
    pub pages: usize,
    pub glyphs_checked: usize,
    pub images_checked: usize,
    pub streams_scanned: usize,
    /// Every trace found. Empty is a pass.
    pub problems: Vec<String>,
}

impl Verification {
    pub fn passed(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Reads `bytes` back and checks each page against its expectation, and,
/// when hidden information was removed, that none is left.
pub(crate) fn verify(
    bytes: &[u8],
    expected: &[Expectation],
    sanitized: bool,
) -> Result<Verification, RedactError> {
    let doc = CosDocument::open(Box::new(BytesSource::new(bytes.to_vec())))?;
    let mut out = Verification {
        pages: expected.len(),
        ..Verification::default()
    };
    for expectation in expected {
        check_text(&doc, expectation, &mut out)?;
        check_drawn(&doc, expectation, &mut out)?;
        check_marks(&doc, expectation, &mut out)?;
        check_bytes(&doc, bytes, expectation, &mut out);
    }
    if sanitized {
        check_sanitized(&doc, &mut out)?;
    }
    Ok(out)
}

/// The keys removing hidden information takes off every dictionary.
const HIDDEN_KEYS: [&[u8]; 4] = [b"Metadata", b"AA", b"PieceInfo", b"Thumb"];

/// The actions removing hidden information takes out: everything but going
/// to a page or a web address.
const RUNNING_ACTIONS: [&[u8]; 16] = [
    b"JavaScript",
    b"Launch",
    b"SubmitForm",
    b"ResetForm",
    b"ImportData",
    b"GoToR",
    b"GoToE",
    b"Rendition",
    b"Sound",
    b"Movie",
    b"Hide",
    b"Named",
    b"SetOCGState",
    b"Trans",
    b"GoTo3DView",
    b"RichMediaExecute",
];

/// Whether `object`, or any dictionary inside it, is such an action.
fn runs_something(object: &Object, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let dict = match object {
        Object::Dict(dict) => dict,
        Object::Stream(stream) => &stream.dict,
        Object::Array(items) => return items.iter().any(|item| runs_something(item, depth + 1)),
        _ => return false,
    };
    let is_action = dict
        .get(b"S")
        .and_then(Object::as_name)
        .is_some_and(|kind| RUNNING_ACTIONS.contains(&kind.as_bytes()));
    is_action
        || dict
            .iter()
            .any(|(_, value)| runs_something(value, depth + 1))
}

fn check_sanitized(doc: &CosDocument, out: &mut Verification) -> Result<(), RedactError> {
    if doc.trailer().contains(b"Info") {
        out.problems
            .push("the document information dictionary is still there".to_owned());
    }
    let root = doc.resolve(doc.trailer().get(b"Root").unwrap_or(&Object::Null))?;
    if let Some(root) = root.as_dict() {
        let names = root
            .get(b"Names")
            .map(|names| doc.resolve(names))
            .transpose()?
            .and_then(|names| names.as_dict().cloned())
            .unwrap_or_default();
        for (key, what) in [
            (b"JavaScript".as_slice(), "document JavaScript"),
            (b"EmbeddedFiles", "embedded files"),
        ] {
            if names.contains(key) {
                out.problems.push(format!("{what} are still there"));
            }
        }
        if root.contains(b"OCProperties") {
            out.problems.push("the layers are still there".to_owned());
        }
    }
    for number in doc.reachable_from_trailer() {
        let Ok(parsed) = doc.get(number) else {
            continue;
        };
        let dict = match &parsed.object {
            Object::Dict(dict) => dict,
            Object::Stream(stream) => &stream.dict,
            _ => continue,
        };
        if let Some(key) = HIDDEN_KEYS.iter().find(|key| dict.contains(key)) {
            out.problems.push(format!(
                "object {number} still has /{}",
                String::from_utf8_lossy(key)
            ));
        }
        if runs_something(&parsed.object, 0) {
            out.problems
                .push(format!("object {number} is an action that still runs"));
        }
        let attachment = dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|kind| kind.as_bytes() == b"FileAttachment");
        if attachment {
            out.problems
                .push(format!("object {number} is a file attachment"));
        }
    }
    Ok(())
}

fn check_text(
    doc: &CosDocument,
    expected: &Expectation,
    out: &mut Verification,
) -> Result<(), RedactError> {
    let text = extract_page(doc, expected.page)?;
    for run in text
        .runs
        .iter()
        .filter(|run| run.provenance.stream != expected.overlay)
    {
        for glyph in &run.glyphs {
            out.glyphs_checked += 1;
            if covers_glyph(&expected.areas, &glyph.quad) {
                out.problems.push(format!(
                    "page {}: text {:?} is still inside a redacted area",
                    expected.page + 1,
                    run.text
                ));
                break;
            }
        }
    }
    Ok(())
}

/// Runs the redaction again over the new page: an inline image it would
/// remove is a problem, and each image it would scrub must be scrubbed.
fn check_drawn(
    doc: &CosDocument,
    expected: &Expectation,
    out: &mut Verification,
) -> Result<(), RedactError> {
    let again = onionskin_content::redact_page(doc, expected.page, &expected.areas)?;
    if again.counts.inline_images > 0 {
        out.problems.push(format!(
            "page {}: an inline image is still drawn over a redacted area",
            expected.page + 1
        ));
    }
    check_images(doc, expected, &again.content, out);
    Ok(())
}

fn check_images(
    doc: &CosDocument,
    expected: &Expectation,
    content: &Rewritten,
    out: &mut Verification,
) {
    for resource in &content.resources {
        match resource {
            NewResource::Image {
                original,
                placement,
                ..
            } => {
                out.images_checked += 1;
                if !painted_out(doc, *original, placement, &expected.areas) {
                    out.problems.push(format!(
                        "page {}: image {} is not painted out under a redacted area",
                        expected.page + 1,
                        original.number
                    ));
                }
            }
            NewResource::Form { content, .. } | NewResource::GState { content, .. } => {
                check_images(doc, expected, content, out);
            }
        }
    }
}

/// Whether every pixel of `image` whose centre is in an area is one colour:
/// the fill it was painted with.
fn painted_out(doc: &CosDocument, image: ObjRef, placement: &Matrix, areas: &[Area]) -> bool {
    let Some(stream) = doc
        .get(image.number)
        .ok()
        .and_then(|parsed| parsed.object.as_stream().cloned())
    else {
        return false;
    };
    let Ok((width, height, color, ImageData::Samples(samples))) = decode_image(doc, &stream) else {
        return false;
    };
    let components = color.components();
    let mut first: Option<&[u8]> = None;
    for row in 0..height as usize {
        let v = 1.0 - (row as f64 + 0.5) / f64::from(height);
        for column in 0..width as usize {
            let u = (column as f64 + 0.5) / f64::from(width);
            let point = placement.apply(u, v);
            if !areas.iter().any(|area| inside(area, point)) {
                continue;
            }
            let at = (row * width as usize + column) * components;
            let Some(pixel) = samples.get(at..at + components) else {
                return false;
            };
            match first {
                None => first = Some(pixel),
                Some(first) if first != pixel => return false,
                Some(_) => {}
            }
        }
    }
    true
}

fn inside(area: &Area, (x, y): (f64, f64)) -> bool {
    let corners = area.corners();
    let side = |i: usize| {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
    };
    let sides = [side(0), side(1), side(2), side(3)];
    sides.iter().all(|s| *s >= -1e-9) || sides.iter().all(|s| *s <= 1e-9)
}

fn check_marks(
    doc: &CosDocument,
    expected: &Expectation,
    out: &mut Verification,
) -> Result<(), RedactError> {
    let page = doc.page(expected.page)?;
    let annots = match page.dict.get(b"Annots").map(|annots| doc.resolve(annots)) {
        Some(Ok(Object::Array(items))) => items,
        _ => return Ok(()),
    };
    for item in annots {
        let Ok(Object::Dict(dict)) = doc.resolve(&item) else {
            continue;
        };
        if dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|name| name.as_bytes() == b"Redact")
        {
            out.problems.push(format!(
                "page {}: a redaction mark is still on the page",
                expected.page + 1
            ));
        }
    }
    Ok(())
}

/// The old content streams no page draws any more must not be in the file,
/// under any object number.
fn check_bytes(doc: &CosDocument, bytes: &[u8], expected: &Expectation, out: &mut Verification) {
    let kept = doc.reachable_from_trailer();
    for (number, raw) in &expected.originals {
        if kept.contains(number) || raw.len() < MIN_SCANNED {
            continue;
        }
        out.streams_scanned += 1;
        if contains(bytes, raw) {
            out.problems.push(format!(
                "page {}: the bytes of old content stream {number} are still in the file",
                expected.page + 1
            ));
        }
    }
}

/// Streams shorter than this are not scanned for: a few bytes of `q` and
/// `cm` recur by chance.
const MIN_SCANNED: usize = 16;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    let probe = &needle[..needle.len().min(64)];
    haystack
        .windows(probe.len())
        .enumerate()
        .any(|(at, window)| window == probe && haystack[at..].starts_with(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page whose text sits inside the area, with a mark still on it and
    /// a JavaScript action: every check reports it.
    fn unredacted() -> Vec<u8> {
        let content = "BT /F1 12 Tf 20 200 Td (Secret) Tj ET BI /W 1 /H 1 /CS /G /BPC 8 ID A EI";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R /OpenAction << /S /JavaScript /JS (x) >> >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> /Annots [6 0 R] /Metadata 5 0 R >>"
                .to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            "<< /Type /Annot /Subtype /Redact /Rect [0 0 1 1] >>".to_owned(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size 7 /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }

    #[test]
    fn a_file_that_still_holds_what_was_removed_fails() {
        let bytes = unredacted();
        let expectation = Expectation {
            page: 0,
            areas: vec![Area::rect(0.0, 0.0, 300.0, 300.0)],
            overlay: ObjRef::new(99, 0),
            originals: vec![(42, b"(Secret) Tj ET BI /W 1".to_vec())],
        };
        let found = verify(&bytes, &[expectation], true).expect("reads");
        assert!(!found.passed());
        for expected in [
            "text \"Secret\" is still inside",
            "inline image",
            "a redaction mark",
            "old content stream 42",
            "information dictionary",
            "/Metadata",
            "action that still runs",
        ] {
            assert!(
                found
                    .problems
                    .iter()
                    .any(|problem| problem.contains(expected)),
                "{expected}: {:?}",
                found.problems
            );
        }
    }

    #[test]
    fn a_needle_is_found_whole_not_by_its_start() {
        let needle = b"0123456789abcdefXYZ";
        assert!(contains(b"....0123456789abcdefXYZ....", needle));
        assert!(!contains(b"....0123456789abcdefXY", needle));
        assert!(inside(&Area::rect(0.0, 0.0, 1.0, 1.0), (0.5, 0.5)));
        assert!(!inside(&Area::rect(0.0, 0.0, 1.0, 1.0), (1.5, 0.5)));
        assert!(Verification::default().passed());
    }
}
