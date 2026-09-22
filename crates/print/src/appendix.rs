//! Summarize Comments in the print output: the document's sheets, then the
//! comment summary's pages printed on sheets of their own after them, as
//! Acrobat's Print dialog does with "Summarize Comments" on.
//!
//! The summary is a PDF of its own (`tools-comment` makes it), so a print
//! with it spans two documents. Each is imposed with the job's paper, sizing
//! and pages per sheet. The summary prints every one of its pages in order:
//! the page range, odd/even and reverse choices are about the document, not
//! about a summary of it. The two outputs are joined into one PDF.

use onionskin_core::pages::insert_pages_from;
use onionskin_core::{AnnotationFilter, Document};

use crate::backend::file::print_to_file;
use crate::backend::PrintError;
use crate::job::{PageSelection, PrintJob};

/// `doc` printed by `job`, followed by `appendix` (a PDF's bytes) printed
/// on the job's paper.
pub fn print_with_appendix(
    doc: &mut Document,
    job: &PrintJob,
    appendix: &[u8],
) -> Result<Vec<u8>, PrintError> {
    let printed = print_to_file(doc, job)?;
    let mut summary = Document::open_bytes(appendix.to_vec())?;
    let appended = print_to_file(&mut summary, &appendix_job(job))?;
    concatenate(printed, appended)
}

/// The job the appendix prints with: the same paper, sizing and layout, all
/// of its pages in order, nothing filtered.
pub fn appendix_job(job: &PrintJob) -> PrintJob {
    PrintJob {
        selection: PageSelection::all(),
        comments: AnnotationFilter::DocumentAndMarkups,
        ..job.clone()
    }
}

/// One PDF: `first`'s pages, then `second`'s.
pub fn concatenate(first: Vec<u8>, second: Vec<u8>) -> Result<Vec<u8>, PrintError> {
    let mut joined = Document::open_bytes(first)?;
    let mut tail = Document::open_bytes(second)?;
    let pages: Vec<usize> = (0..tail.page_count()).collect();
    let at = joined.page_count();
    let source = tail.structure()?;
    joined.edit_pages("Append", |tx, structure| {
        insert_pages_from(tx, structure, source, &pages, at).map(|_| ())
    })?;
    Ok(joined
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)?
        .as_ref()
        .clone())
}
