//! Summarize Comments: a new document listing every comment, by page.
//!
//! Two layouts, as Acrobat offers them: the comments alone, or each page of
//! the document followed by its comments. Either way the result is a new
//! file (T8), put together by `core::pages::Assembly` - the source pages are
//! the importer's transitive copies, and the pages of comments are written
//! here and appended the same way - so this module sets text and nothing
//! else.
//!
//! **Encrypted sources are refused**, per the encrypted-source rule: the
//! summary copies the document's content into another file. The command is
//! registered as [`CommandEffect::ReadsOut`], so the shell disables it with
//! the document's reason, and [`summarize`] checks again itself.
//!
//! [`CommandEffect::ReadsOut`]: onionskin_plugin_api::CommandEffect::ReadsOut

use std::fmt;
use std::fmt::Write as _;
use std::ops::Range;

use onionskin_core::pages::Assembly;
use onionskin_core::{read_annotations, Document, ReadAnnotation};
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

use crate::text::{literal, wrap};

/// US Letter, the page the comments are set on.
const PAGE: (f64, f64) = (612.0, 792.0);
const MARGIN: f64 = 54.0;
const BODY: f64 = 10.0;
const HEADING: f64 = 13.0;
const INDENT: f64 = 16.0;

/// The subtypes a summary lists: the comments, not links, form fields or the
/// pop-ups that belong to other comments.
const COMMENTS: &[(&str, &str)] = &[
    ("Text", "Sticky Note"),
    ("FreeText", "Text Box"),
    ("Highlight", "Highlight"),
    ("Underline", "Underline"),
    ("Squiggly", "Squiggly Underline"),
    ("StrikeOut", "Strikethrough"),
    ("Caret", "Inserted Text"),
    ("Line", "Line"),
    ("Square", "Rectangle"),
    ("Circle", "Oval"),
    ("Polygon", "Polygon"),
    ("PolyLine", "Connected Lines"),
    ("Ink", "Drawing"),
    ("Stamp", "Stamp"),
    ("FileAttachment", "Attached File"),
    ("Sound", "Sound"),
];

/// Which of Acrobat's layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryLayout {
    /// The comments alone, page after page.
    CommentsOnly,
    /// Each page of the document, followed by its comments.
    DocumentAndComments,
}

/// A summary, ready to write.
#[derive(Debug, Clone)]
pub struct Summary {
    pub bytes: Vec<u8>,
    pub page_count: usize,
    pub comment_count: usize,
}

#[derive(Debug)]
pub enum SummaryError {
    /// The document has no comments to summarize.
    NoComments,
    /// Refused by the encrypted-source rule, or the document could not be
    /// read or assembled.
    Document(onionskin_core::Error),
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoComments => write!(f, "the document has no comments to summarize"),
            Self::Document(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SummaryError {}

impl From<onionskin_core::Error> for SummaryError {
    fn from(error: onionskin_core::Error) -> Self {
        Self::Document(error)
    }
}

/// The summary of `document`'s comments as the session sees them, pending
/// edits included.
pub fn summarize(document: &mut Document, layout: SummaryLayout) -> Result<Summary, SummaryError> {
    let page_count = document.page_count();
    let source = document.structure()?;
    onionskin_core::protection::read_out(source)
        .map_err(|refusal| SummaryError::Document(onionskin_core::Error::Protected(refusal)))?;
    let comments: Vec<ReadAnnotation> = read_annotations(source, page_count, &Default::default())?
        .into_iter()
        .filter(|annotation| kind(annotation).is_some())
        .collect();
    if comments.is_empty() {
        return Err(SummaryError::NoComments);
    }

    let sections = sections(&comments, page_count);
    let (listing, ranges) = listing(&sections, &comments)?;
    let (listing, _) = CosDocument::open_repairing(Box::new(BytesSource::new(listing)))
        .map_err(|error| SummaryError::Document(error.into()))?;

    let mut assembly = Assembly::new();
    match layout {
        SummaryLayout::CommentsOnly => {
            let all: Vec<usize> = (0..ranges.last().map_or(0, |(_, range)| range.end)).collect();
            assembly.append(&listing, &all)?;
        }
        SummaryLayout::DocumentAndComments => {
            for page in 0..page_count {
                assembly.append(source, &[page])?;
                if let Some((_, range)) = ranges.iter().find(|(owner, _)| *owner == page) {
                    assembly.append(&listing, &range.clone().collect::<Vec<_>>())?;
                }
            }
        }
    }
    let assembled = assembly.finish()?;
    Ok(Summary {
        bytes: assembled.bytes,
        page_count: assembled.page_count,
        comment_count: comments.len(),
    })
}

/// What a summary calls this annotation, or `None` if it is not a comment.
fn kind(annotation: &ReadAnnotation) -> Option<&'static str> {
    COMMENTS
        .iter()
        .find(|(subtype, _)| *subtype == annotation.raw_subtype)
        .map(|(_, name)| *name)
}

/// Each page with comments, and its comments' positions in `comments`.
fn sections(comments: &[ReadAnnotation], page_count: usize) -> Vec<(usize, Vec<usize>)> {
    (0..page_count)
        .filter_map(|page| {
            let on_page: Vec<usize> = comments
                .iter()
                .enumerate()
                .filter(|(_, comment)| comment.page == page)
                .map(|(index, _)| index)
                .collect();
            (!on_page.is_empty()).then_some((page, on_page))
        })
        .collect()
}

/// A source page, and the pages of the listing its comments took.
type Section = (usize, Range<usize>);

/// The pages of comments, one section per source page, each starting on a
/// fresh page; and which of those pages each section took.
fn listing(
    sections: &[(usize, Vec<usize>)],
    comments: &[ReadAnnotation],
) -> Result<(Vec<u8>, Vec<Section>), SummaryError> {
    let mut writer = PageWriter::default();
    let mut ranges = Vec::new();
    // The number each comment is listed under, for a reply to name.
    let numbers: Vec<(ObjRef, usize)> = sections
        .iter()
        .flat_map(|(_, indices)| indices.iter())
        .enumerate()
        .map(|(position, index)| (comments[*index].objref, position + 1))
        .collect();
    let mut number = 0;
    for (page, indices) in sections {
        let start = writer.page_break();
        writer.line(&format!("Page {}", page + 1), true, HEADING, 0.0);
        writer.gap(6.0);
        for index in indices {
            number += 1;
            writer.comment(number, &comments[*index], &numbers);
        }
        ranges.push((*page, start..writer.page_count()));
    }
    Ok((writer.finish()?, ranges))
}

/// Lines of text set down pages.
#[derive(Default)]
struct PageWriter {
    pages: Vec<String>,
    y: f64,
}

impl PageWriter {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Start a new page, returning its index.
    fn page_break(&mut self) -> usize {
        self.pages.push(String::new());
        self.y = PAGE.1 - MARGIN;
        self.pages.len() - 1
    }

    fn gap(&mut self, points: f64) {
        self.y -= points;
    }

    fn line(&mut self, text: &str, bold: bool, size: f64, indent: f64) {
        let leading = size * 1.3;
        if self.pages.is_empty() || self.y - leading < MARGIN {
            self.page_break();
        }
        self.y -= leading;
        let font = if bold { "HeBo" } else { "Helv" };
        let page = self.pages.last_mut().expect("a page was just started");
        let _ = writeln!(
            page,
            "BT /{font} {size} Tf {} {:.2} Td {} Tj ET",
            MARGIN + indent,
            self.y,
            literal(text)
        );
    }

    /// One comment: what it is, who, when; then its text.
    fn comment(&mut self, number: usize, comment: &ReadAnnotation, numbers: &[(ObjRef, usize)]) {
        let mut heading = format!("{number}. {}", kind(comment).unwrap_or("Comment"));
        if let Some(parent) = comment.in_reply_to {
            if let Some((_, parent)) = numbers.iter().find(|(objref, _)| *objref == parent) {
                let _ = write!(heading, ", replying to {parent}");
            }
        }
        if let Some(author) = comment
            .author
            .as_deref()
            .filter(|name| !name.trim().is_empty())
        {
            let _ = write!(heading, " by {author}");
        }
        if let Some(when) = comment.modified.as_deref().and_then(readable_date) {
            let _ = write!(heading, ", {when}");
        }
        let width = PAGE.0 - 2.0 * MARGIN;
        for line in wrap(&heading, true, BODY, width) {
            self.line(&line, true, BODY, 0.0);
        }
        let contents = comment.contents.as_deref().unwrap_or("").trim();
        if !contents.is_empty() {
            for line in wrap(contents, false, BODY, width - INDENT) {
                self.line(&line, false, BODY, INDENT);
            }
        }
        self.gap(6.0);
    }

    /// The pages as a PDF.
    fn finish(self) -> Result<Vec<u8>, SummaryError> {
        let count = self.pages.len();
        let (catalog, tree, fonts) = (1u32, 2u32, 3u32);
        let first_page = 4u32;
        let page_ref = |index: usize| ObjRef::new(first_page + 2 * index as u32, 0);
        let content_ref = |index: usize| ObjRef::new(first_page + 2 * index as u32 + 1, 0);

        let mut objects = vec![
            (
                ObjRef::new(catalog, 0),
                dict(&[
                    ("Type", Object::name("Catalog")),
                    ("Pages", Object::Ref(ObjRef::new(tree, 0))),
                ]),
            ),
            (
                ObjRef::new(tree, 0),
                dict(&[
                    ("Type", Object::name("Pages")),
                    (
                        "Kids",
                        Object::Array(
                            (0..count)
                                .map(|index| Object::Ref(page_ref(index)))
                                .collect(),
                        ),
                    ),
                    ("Count", Object::Integer(count as i64)),
                ]),
            ),
            (ObjRef::new(fonts, 0), font_resources()),
        ];
        for (index, content) in self.pages.into_iter().enumerate() {
            objects.push((
                page_ref(index),
                dict(&[
                    ("Type", Object::name("Page")),
                    ("Parent", Object::Ref(ObjRef::new(tree, 0))),
                    (
                        "MediaBox",
                        Object::Array([0.0, 0.0, PAGE.0, PAGE.1].map(Object::Real).to_vec()),
                    ),
                    (
                        "Resources",
                        dict(&[("Font", Object::Ref(ObjRef::new(fonts, 0)))]),
                    ),
                    ("Contents", Object::Ref(content_ref(index))),
                ]),
            ));
            let raw = content.into_bytes();
            let Object::Dict(stream_dict) = dict(&[("Length", Object::Integer(raw.len() as i64))])
            else {
                unreachable!("dict builds a dictionary");
            };
            objects.push((
                content_ref(index),
                Object::Stream(Stream {
                    dict: stream_dict,
                    raw,
                }),
            ));
        }
        let mut trailer = Dict::new();
        trailer.set(Name::new("Root"), Object::Ref(ObjRef::new(catalog, 0)));
        CosDocument::write_new(&objects, trailer)
            .map_err(|error| SummaryError::Document(error.into()))
    }
}

/// Helvetica and Helvetica-Bold, in WinAnsiEncoding, which is how
/// [`literal`] writes text.
fn font_resources() -> Object {
    let font = |base: &str| {
        dict(&[
            ("Type", Object::name("Font")),
            ("Subtype", Object::name("Type1")),
            ("BaseFont", Object::name(base)),
            ("Encoding", Object::name("WinAnsiEncoding")),
        ])
    };
    dict(&[
        ("Helv", font("Helvetica")),
        ("HeBo", font("Helvetica-Bold")),
    ])
}

fn dict(entries: &[(&str, Object)]) -> Object {
    let mut dict = Dict::new();
    for (key, value) in entries {
        dict.set(Name::new(key), value.clone());
    }
    Object::Dict(dict)
}

/// `D:20260921140500Z00'00'` as `2026-09-21 14:05`; `None` for anything
/// that does not start like a PDF date.
fn readable_date(date: &str) -> Option<String> {
    let digits = date.strip_prefix("D:").unwrap_or(date);
    let digits: String = digits.chars().take(12).collect();
    if digits.len() < 8 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut out = format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]);
    if digits.len() >= 12 {
        let _ = write!(out, " {}:{}", &digits[8..10], &digits[10..12]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_date_reads_as_a_date() {
        assert_eq!(
            readable_date("D:20260921140500Z00'00'").as_deref(),
            Some("2026-09-21 14:05")
        );
        assert_eq!(readable_date("D:20260921").as_deref(), Some("2026-09-21"));
        assert_eq!(readable_date("yesterday"), None);
    }

    #[test]
    fn lines_run_down_the_page_and_onto_the_next() {
        let mut writer = PageWriter::default();
        for index in 0..200 {
            writer.line(&format!("line {index}"), false, BODY, 0.0);
        }
        assert!(writer.page_count() > 2, "{} pages", writer.page_count());
    }
}
