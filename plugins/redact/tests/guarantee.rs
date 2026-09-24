//! Guarantee test 3: redaction. After redacting text T, the verifier
//! extracts all text and images from the output and finds no trace of T,
//! and a raw byte scan finds no trace of the original object bytes.
//!
//! Driven through the plugin as the shell drives it: every occurrence of T
//! found by Find Text & Redact, marked, and applied. The output is then read
//! back independently of the verifier: every page's text extracted, every
//! decoded stream searched, and the old content streams looked for in the
//! raw bytes.

use onionskin_content::extract_page;
use onionskin_core::redactions::RedactionLook;
use onionskin_core::{Document, SearchOptions};
use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Document as CosDocument, Object};
use onionskin_redact::apply_redactions;
use onionskin_redact::find::{find, Query};
use onionskin_redact::mark::mark_found;

/// Redacts every occurrence of `word` in `bytes`, and proves it is gone.
fn redact_and_prove(bytes: Vec<u8>, word: &str) {
    let original = CosDocument::open(Box::new(BytesSource::new(bytes.clone()))).expect("opens");
    let mut doc = Document::open_bytes(bytes).expect("opens");
    let options = SearchOptions {
        case_sensitive: true,
        ..SearchOptions::default()
    };
    let found = find(&mut doc, &Query::Text(word.to_owned(), options)).expect("finds");
    assert!(
        !found.is_empty(),
        "{word:?} is in the document to begin with"
    );
    mark_found(&mut doc, &found, &RedactionLook::default()).expect("marks");
    let applied = apply_redactions(&mut doc).expect("applies");

    assert!(
        applied.verification.passed(),
        "the verifier must pass the redacted file: {:?}",
        applied.verification.problems
    );
    let output = CosDocument::open(Box::new(BytesSource::new(applied.bytes.clone())))
        .expect("the redacted file opens");
    let pages = output.page_count().expect("pages") as usize;
    for page in 0..pages {
        let text = extract_page(&output, page).expect("extracts");
        for run in &text.runs {
            assert!(
                !run.text.contains(word),
                "the redacted word must not be extracted from the output: page {} says {:?}",
                page + 1,
                run.text
            );
        }
    }
    for (number, raw) in &content_streams(&original, &applied.report.pages) {
        assert!(
            !contains(&applied.bytes, raw),
            "the original content stream bytes must not be in the output: object {number}"
        );
    }
    let kept = output.reachable_from_trailer();
    for number in kept {
        let Ok(parsed) = output.get(number) else {
            continue;
        };
        if let Object::Stream(stream) = parsed.object {
            let decoded = output.decode_stream(&stream).unwrap_or_default();
            assert!(
                !contains(&decoded, word.as_bytes()),
                "the redacted word's bytes must not be in the output: stream {number}"
            );
        }
    }
    assert!(
        !contains(&applied.bytes, word.as_bytes()),
        "the redacted word's bytes must not be in the output"
    );
}

/// The content streams of `pages`, by object number, with their raw bytes.
fn content_streams(doc: &CosDocument, pages: &[usize]) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    for &page in pages {
        let node = doc.page(page).expect("page");
        let refs: Vec<_> = match node.dict.get(b"Contents") {
            Some(Object::Ref(objref)) => vec![*objref],
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_reference).collect(),
            _ => Vec::new(),
        };
        for objref in refs {
            if let Some(stream) = doc.get(objref.number).expect("stream").object.as_stream() {
                out.push((objref.number, stream.raw.clone()));
            }
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[test]
fn a_word_on_the_seeds_is_gone_after_redaction() {
    redact_and_prove(
        std::fs::read(seed("hello.pdf")).expect("readable"),
        "Onionskin",
    );
    // On the second page, which is turned a quarter and cropped.
    redact_and_prove(
        std::fs::read(seed("two-page.pdf")).expect("readable"),
        "two",
    );
}

/// A word written in every way a content stream can hide it: split across a
/// `TJ` array, in a hex string, and inside a form XObject drawn twice.
#[test]
fn a_word_however_it_is_drawn_is_gone_after_redaction() {
    let content = "BT /F1 12 Tf 20 250 Td [(Conf) -5 (idential memo)] TJ ET \
                   BT /F1 12 Tf 20 220 Td <436F6E666964656E7469616C> Tj ET \
                   q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 -60 cm /Fm0 Do Q \
                   BT /F1 12 Tf 20 100 Td (Public) Tj ET";
    let form = "BT /F1 12 Tf 20 180 Td (Confidential footer) Tj ET";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> /XObject << /Fm0 6 0 R >> >> >>"
            .to_vec(),
        stream("", content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >>",
            form,
        ),
    ];
    redact_and_prove(pdf(&objects), "Confidential");
}

fn stream(dict: &str, data: &str) -> Vec<u8> {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
    .into_bytes()
}

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
