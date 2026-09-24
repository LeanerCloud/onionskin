//! Apply Redactions: every mark applied, and the document written as a new
//! file with nothing of what the marks covered in it.
//!
//! The new file is a flattening rewrite, never an incremental section: an
//! incremental save keeps every old byte underneath, which is exactly what
//! a redaction must not do. It holds only the objects the document still
//! reaches, so the old content streams, the images replaced and every
//! earlier revision are gone. The verifier reads it back before it is
//! handed over, and a file it finds anything in is refused.

mod objects;
mod overlay;
mod page;
mod sanitize;
mod scrub;

use std::collections::BTreeMap;

use onionskin_content::redact::Counts;
use onionskin_core::redactions::RedactionMark;
use onionskin_core::Document;
use onionskin_cos::{Document as CosDocument, Object};

use crate::verify::{verify, Expectation, Verification};
use crate::RedactError;
use objects::Objects;
use page::{apply_page, Beyond, PageApplied};
pub use sanitize::Sanitized;

/// What applying took out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// The pages redacted, from 0.
    pub pages: Vec<usize>,
    pub marks: usize,
    pub counts: Counts,
    /// Annotations other than the marks removed because they reached an
    /// area.
    pub annotations: usize,
    /// What the removed glyphs spelled, page by page, for the notice and
    /// the log. Glyphs with no Unicode mapping spell nothing.
    pub text: Vec<(usize, String)>,
    /// What removing hidden information took out, when it was asked for.
    pub sanitized: Option<Sanitized>,
}

/// How to apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyOptions {
    /// Also remove hidden information: metadata, scripts, attachments,
    /// comments, hidden layers and cropped content.
    pub remove_hidden_information: bool,
}

/// A redacted file, verified.
#[derive(Debug, Clone)]
pub struct Applied {
    pub bytes: Vec<u8>,
    pub report: Report,
    pub verification: Verification,
}

/// Applies every redaction mark `doc` has and writes the result as a new
/// file. `doc` itself is not changed.
pub fn apply_redactions(doc: &mut Document) -> Result<Applied, RedactError> {
    apply_with(doc, &ApplyOptions::default())
}

/// Removes hidden information, applying any marks too: Sanitize Document.
pub fn sanitize(doc: &mut Document) -> Result<Applied, RedactError> {
    apply_with(
        doc,
        &ApplyOptions {
            remove_hidden_information: true,
        },
    )
}

/// Applies the marks, and removes hidden information when `options` say.
pub fn apply_with(doc: &mut Document, options: &ApplyOptions) -> Result<Applied, RedactError> {
    if let Some(refusal) = doc.read_out_refusal() {
        return Err(RedactError::Refused(refusal));
    }
    let marks = doc.redactions()?;
    let sanitizing = options.remove_hidden_information;
    if marks.is_empty() && !sanitizing {
        return Err(RedactError::NothingMarked);
    }
    let page_count = doc.page_count();
    let source = doc.structure()?;
    let mut objects = Objects::from_source(source)?;
    let hidden = if sanitizing {
        sanitize::hidden_layers(&objects)
    } else {
        Vec::new()
    };
    let mut report = Report {
        marks: marks.len(),
        sanitized: sanitizing.then(Sanitized::default),
        ..Report::default()
    };
    let marked = by_page(&marks);
    let mut expected = Vec::new();
    for page in 0..page_count {
        let on_page = marked.get(&page).cloned().unwrap_or_default();
        if on_page.is_empty() && !sanitizing {
            continue;
        }
        let beyond = if sanitizing {
            let node = source.page(page)?;
            Beyond {
                areas: sanitize::outside_crop(
                    node.media_box.unwrap_or([0.0, 0.0, 612.0, 792.0]),
                    node.crop_box,
                ),
                hidden: hidden.clone(),
            }
        } else {
            Beyond::default()
        };
        let Some(applied) = apply_page(source, &mut objects, page, &on_page, &beyond)? else {
            continue;
        };
        if let Some(sanitized) = report.sanitized.as_mut() {
            sanitized.hidden_layers += applied.counts.hidden;
            if !beyond.areas.is_empty() {
                sanitized.cropped_pages += 1;
            }
        }
        expected.push(expectation(source, &applied));
        record(&mut report, applied);
    }
    unspell(&mut objects, &report.text);
    if let Some(sanitized) = report.sanitized.as_mut() {
        sanitize::sweep(&mut objects, &hidden, sanitized);
    }
    let bytes = objects.write()?;
    let verification = verify(&bytes, &expected, sanitizing)?;
    if !verification.passed() {
        return Err(RedactError::NotVerified(verification.problems));
    }
    Ok(Applied {
        bytes,
        report,
        verification,
    })
}

fn by_page(marks: &[RedactionMark]) -> BTreeMap<usize, Vec<&RedactionMark>> {
    let mut pages: BTreeMap<usize, Vec<&RedactionMark>> = BTreeMap::new();
    for mark in marks {
        pages.entry(mark.page).or_default().push(mark);
    }
    pages
}

fn expectation(source: &CosDocument, applied: &PageApplied) -> Expectation {
    let originals = applied
        .originals
        .iter()
        .filter_map(|objref| {
            let parsed = source.get(objref.number).ok()?;
            Some((objref.number, parsed.object.as_stream()?.raw.clone()))
        })
        .collect();
    Expectation {
        page: applied.page,
        areas: applied.areas.clone(),
        overlay: applied.overlay,
        originals,
    }
}

fn record(report: &mut Report, applied: PageApplied) {
    let counts = &mut report.counts;
    counts.glyphs += applied.counts.glyphs;
    counts.paths += applied.counts.paths;
    counts.inline_images += applied.counts.inline_images;
    counts.images += applied.counts.images;
    counts.forms += applied.counts.forms;
    counts.fontless += applied.counts.fontless;
    report.annotations += applied.annotations;
    report.pages.push(applied.page);
    let text: String = applied
        .removed
        .iter()
        .map(|glyph| glyph.text.as_str())
        .collect();
    if !text.trim().is_empty() {
        report.text.push((applied.page, text));
    }
}

/// Words shorter than this are not looked for in the structure tree: a
/// two-letter word recurs by chance.
const MIN_WORD: usize = 3;

/// Takes `/ActualText`, `/Alt` and `/E` off any dictionary, a structure
/// element most often, that still spells a word the redaction removed.
fn unspell(objects: &mut Objects, removed: &[(usize, String)]) {
    let words: Vec<&str> = removed
        .iter()
        .flat_map(|(_, text)| text.split_whitespace())
        .filter(|word| word.chars().count() >= MIN_WORD)
        .collect();
    if words.is_empty() {
        return;
    }
    for number in objects.reachable() {
        let Some(Object::Dict(dict)) = objects.get(number) else {
            continue;
        };
        let spelled: Vec<&[u8]> = [b"ActualText".as_slice(), b"Alt", b"E"]
            .into_iter()
            .filter(|key| match dict.get(key) {
                Some(Object::String(bytes)) => {
                    let text = onionskin_content::pdf_text_string(bytes);
                    words.iter().any(|word| text.contains(word))
                }
                _ => false,
            })
            .collect();
        if spelled.is_empty() {
            continue;
        }
        let mut dict = dict.clone();
        for key in spelled {
            dict.remove(key);
        }
        objects.set(number, Object::Dict(dict));
    }
}
