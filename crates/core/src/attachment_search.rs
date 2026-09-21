//! Advanced Search's "Include PDF attachments": the text of PDFs attached
//! to the document, and of PDFs attached to those, two levels deep as
//! Acrobat searches them.
//!
//! Each attached PDF is opened from its bytes in memory and searched page by
//! page; nothing is written anywhere. One that cannot be opened or read is
//! named in [`AttachmentSearch::skipped`] and the rest are still searched,
//! so a damaged attachment does not hide the hits in its siblings.

use onionskin_content as content;

use crate::{Document, PageIndex, Result, SearchOptions};

/// How deep attachments of attachments are searched. Acrobat's depth.
pub const ATTACHMENT_SEARCH_DEPTH: usize = 2;

/// A bound on the hits collected, so a huge attachment cannot make one
/// search unbounded. The count past it is still reported.
pub const MAX_ATTACHMENT_HITS: usize = 1_000;

/// One hit inside an attached PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentHit {
    /// The attachment names from the document down: `["report.pdf"]`, or
    /// `["report.pdf", "annex.pdf"]` for a PDF attached to that one.
    pub path: Vec<String>,
    pub page: PageIndex,
    /// The matched text as the attachment has it.
    pub text: String,
}

/// What searching the attachments found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentSearch {
    pub hits: Vec<AttachmentHit>,
    /// Every hit found, including those past [`MAX_ATTACHMENT_HITS`].
    pub total: usize,
    /// One line per attachment that could not be searched, saying why.
    pub skipped: Vec<String>,
}

impl Document {
    /// Search the text of the PDFs attached to this document, two levels
    /// deep. Attachments that are not PDFs are passed over silently: Acrobat
    /// searches only PDF attachments too.
    pub fn search_attachments(
        &mut self,
        needle: &str,
        options: SearchOptions,
    ) -> Result<AttachmentSearch> {
        let mut found = AttachmentSearch::default();
        if !needle.trim().is_empty() {
            search_within(
                self,
                needle,
                options,
                &[],
                ATTACHMENT_SEARCH_DEPTH,
                &mut found,
            )?;
        }
        Ok(found)
    }
}

fn search_within(
    doc: &mut Document,
    needle: &str,
    options: SearchOptions,
    parents: &[String],
    depth: usize,
    found: &mut AttachmentSearch,
) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let attachments = doc.attachments()?.to_vec();
    for (index, attachment) in attachments.iter().enumerate() {
        let mut path = parents.to_vec();
        path.push(attachment.name.clone());
        let bytes = match doc.attached_bytes(index) {
            Ok(bytes) => bytes,
            Err(error) => {
                found.skipped.push(skip_line(&path, &error));
                continue;
            }
        };
        if !is_pdf(&bytes) {
            continue;
        }
        let mut attached = match Document::open_bytes(bytes) {
            Ok(attached) => attached,
            Err(error) => {
                found.skipped.push(skip_line(&path, &error));
                continue;
            }
        };
        if let Err(error) = search_pages(&mut attached, needle, options, &path, found) {
            found.skipped.push(skip_line(&path, &error));
            continue;
        }
        search_within(&mut attached, needle, options, &path, depth - 1, found)?;
    }
    Ok(())
}

fn search_pages(
    doc: &mut Document,
    needle: &str,
    options: SearchOptions,
    path: &[String],
    found: &mut AttachmentSearch,
) -> Result<()> {
    for page in 0..doc.page_count() {
        let hits = content::search(doc.page_text(page)?, needle, options);
        found.total += hits.len();
        let room = MAX_ATTACHMENT_HITS.saturating_sub(found.hits.len());
        found
            .hits
            .extend(hits.into_iter().take(room).map(|hit| AttachmentHit {
                path: path.to_vec(),
                page,
                text: hit.text,
            }));
    }
    Ok(())
}

fn skip_line(path: &[String], error: &dyn std::fmt::Display) -> String {
    format!("{} was not searched: {error}", path.join(" > "))
}

/// A PDF header in the first kilobyte, where readers look for it.
fn is_pdf(bytes: &[u8]) -> bool {
    bytes
        .get(..bytes.len().min(1024))
        .is_some_and(|head| head.windows(5).any(|window| window == b"%PDF-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_is_known_by_its_header_near_the_start() {
        assert!(is_pdf(b"%PDF-1.7\n"));
        assert!(is_pdf(b"\xef\xbb\xbf  %PDF-1.4"));
        assert!(!is_pdf(b"hello"));
        let mut late = vec![b' '; 2000];
        late.extend_from_slice(b"%PDF-1.7");
        assert!(!is_pdf(&late));
    }

    #[test]
    fn a_skipped_attachment_names_its_whole_path() {
        let line = skip_line(&["a.pdf".into(), "b.pdf".into()], &"broken");
        assert_eq!(line, "a.pdf > b.pdf was not searched: broken");
    }
}
