//! Split Document: the open document into several new files, by page count,
//! by file size, or at its top-level bookmarks.
//!
//! Reads the document as its session has it, unsaved edits included, and is
//! refused for an encrypted one before anything is planned - the
//! encrypted-source rule's session-scoped shape, the same predicate the menu
//! entry asks through `CommandEffect::ReadsOut`. Every part is written, or
//! none: see [`crate::publish`].

use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::{Path, PathBuf};

use onionskin_core::pages::Assembly;
use onionskin_core::{Document, OutlineItem};
use onionskin_cos::Document as CosDocument;

use crate::publish::{publish, PublishError};

/// How to cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitBy {
    /// At most this many pages per file.
    PageCount(NonZeroUsize),
    /// At most this many bytes per file, best effort: a single page larger
    /// than the target gets a file of its own rather than being refused.
    FileSize(u64),
    /// A new file at every top-level bookmark.
    TopLevelBookmarks,
}

/// Where the cuts fall, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPlan {
    pub parts: Vec<Range<usize>>,
    /// Top-level bookmarks that name no page this document has. Reported
    /// rather than skipped: a bookmark the user expected to start a file and
    /// that silently did not is a split they will not notice is wrong.
    pub unresolved: Vec<String>,
}

/// What a split wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub files: Vec<PathBuf>,
    pub unresolved: Vec<String>,
}

#[derive(Debug)]
pub enum SplitError {
    /// The document may not be read out: encrypted, at M3.
    Refused(onionskin_core::protection::Refusal),
    /// Splitting at bookmarks, and the document has no top-level bookmark
    /// that names a page.
    NoBookmarks,
    Document(onionskin_core::Error),
    Publish(PublishError),
}

impl std::fmt::Display for SplitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "{refusal}"),
            Self::NoBookmarks => write!(f, "the document has no top-level bookmarks to split at"),
            Self::Document(source) => write!(f, "{source}"),
            Self::Publish(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for SplitError {}

impl From<onionskin_core::Error> for SplitError {
    fn from(source: onionskin_core::Error) -> Self {
        Self::Document(source)
    }
}

/// Split `doc` by `by` into files named `<stem> - Part <n>.pdf` in `folder`.
pub fn split(
    doc: &mut Document,
    by: SplitBy,
    folder: &Path,
    stem: &str,
) -> Result<Split, SplitError> {
    let (plan, parts) = split_bytes(doc, by)?;
    let outputs: Vec<(PathBuf, Vec<u8>)> = parts
        .into_iter()
        .enumerate()
        .map(|(index, bytes)| (part_path(folder, stem, index + 1), bytes))
        .collect();
    publish(&outputs).map_err(SplitError::Publish)?;
    Ok(Split {
        files: outputs.into_iter().map(|(path, _)| path).collect(),
        unresolved: plan.unresolved,
    })
}

/// The plan, and each part's bytes, without writing anything.
pub fn split_bytes(
    doc: &mut Document,
    by: SplitBy,
) -> Result<(SplitPlan, Vec<Vec<u8>>), SplitError> {
    if let Some(refusal) = doc.read_out_refusal() {
        return Err(SplitError::Refused(refusal));
    }
    let count = doc.page_count();
    let plan = match by {
        SplitBy::PageCount(size) => SplitPlan {
            parts: by_count(count, size),
            unresolved: Vec::new(),
        },
        SplitBy::TopLevelBookmarks => by_bookmarks(doc.outline()?, count)?,
        SplitBy::FileSize(target) => {
            let source = doc.structure()?;
            return by_size(source, count, target);
        }
    };
    let source = doc.structure()?;
    let parts = plan
        .parts
        .iter()
        .map(|range| assemble(source, range.clone()))
        .collect::<Result<_, _>>()?;
    Ok((plan, parts))
}

/// `<folder>/<stem> - Part <n>.pdf`.
pub fn part_path(folder: &Path, stem: &str, number: usize) -> PathBuf {
    folder.join(format!("{stem} - Part {number}.pdf"))
}

/// Consecutive runs of `size` pages, the last one short.
pub fn by_count(count: usize, size: NonZeroUsize) -> Vec<Range<usize>> {
    (0..count)
        .step_by(size.get())
        .map(|start| start..(start + size.get()).min(count))
        .collect()
}

/// A part at every top-level bookmark that names a page, in page order. Pages
/// before the first such bookmark belong to the first part, so none is lost.
pub fn by_bookmarks(outline: &[OutlineItem], count: usize) -> Result<SplitPlan, SplitError> {
    let mut starts: Vec<usize> = Vec::new();
    let mut unresolved = Vec::new();
    for item in outline {
        match item.page.filter(|page| *page < count) {
            Some(page) => starts.push(page),
            None => unresolved.push(item.title.clone()),
        }
    }
    starts.sort_unstable();
    starts.dedup();
    if starts.is_empty() {
        return Err(SplitError::NoBookmarks);
    }
    starts[0] = 0;
    let ends = starts.iter().skip(1).copied().chain([count]);
    let parts = starts
        .iter()
        .zip(ends)
        .map(|(start, end)| *start..end)
        .collect();
    Ok(SplitPlan { parts, unresolved })
}

/// Greedy by measured size: grow each part a page at a time while the
/// assembled part still fits. Measured rather than estimated, because pages
/// share resources and their sizes do not add. Quadratic in the pages of a
/// part, which is the price of the promise being true.
fn by_size(
    source: &CosDocument,
    count: usize,
    target: u64,
) -> Result<(SplitPlan, Vec<Vec<u8>>), SplitError> {
    let mut parts = Vec::new();
    let mut outputs = Vec::new();
    let mut start = 0;
    while start < count {
        let mut end = start + 1;
        let mut bytes = assemble(source, start..end)?;
        while end < count {
            let grown = assemble(source, start..end + 1)?;
            if grown.len() as u64 > target {
                break;
            }
            bytes = grown;
            end += 1;
        }
        parts.push(start..end);
        outputs.push(bytes);
        start = end;
    }
    let plan = SplitPlan {
        parts,
        unresolved: Vec::new(),
    };
    Ok((plan, outputs))
}

fn assemble(source: &CosDocument, pages: Range<usize>) -> Result<Vec<u8>, SplitError> {
    let mut assembly = Assembly::new();
    assembly.append(source, &pages.collect::<Vec<_>>())?;
    Ok(assembly.finish()?.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("nonzero")
    }

    /// The off-by-one the plan names: ten pages in threes is four files, and
    /// the fourth holds the tenth page.
    #[test]
    fn ten_pages_in_threes_is_four_parts_the_last_holding_page_ten() {
        assert_eq!(by_count(10, size(3)), [0..3, 3..6, 6..9, 9..10]);
        assert_eq!(by_count(9, size(3)), [0..3, 3..6, 6..9]);
        assert_eq!(by_count(2, size(5)), vec![(0..2)]);
    }

    fn item(title: &str, page: Option<usize>) -> OutlineItem {
        OutlineItem {
            title: title.to_owned(),
            page,
            children: vec![OutlineItem {
                title: "nested, never a boundary".to_owned(),
                page: Some(1),
                children: Vec::new(),
            }],
        }
    }

    #[test]
    fn bookmarks_at_pages_one_four_and_nine_cut_there() {
        let outline = [
            item("Intro", Some(0)),
            item("Middle", Some(3)),
            item("End", Some(8)),
        ];
        let plan = by_bookmarks(&outline, 10).expect("plans");
        assert_eq!(plan.parts, [0..3, 3..8, 8..10]);
        assert!(plan.unresolved.is_empty());
    }

    #[test]
    fn a_bookmark_naming_no_page_is_reported_not_skipped() {
        let outline = [
            item("Intro", Some(0)),
            item("Gone", None),
            item("Past the end", Some(40)),
            item("End", Some(5)),
        ];
        let plan = by_bookmarks(&outline, 10).expect("plans");
        assert_eq!(plan.parts, [0..5, 5..10]);
        assert_eq!(plan.unresolved, ["Gone", "Past the end"]);
    }

    /// Pages before the first bookmark are not lost.
    #[test]
    fn pages_before_the_first_bookmark_join_the_first_part() {
        let plan = by_bookmarks(&[item("Late", Some(4))], 6).expect("plans");
        assert_eq!(plan.parts, vec![(0..6)]);
    }

    #[test]
    fn no_usable_bookmark_is_refused() {
        assert!(matches!(
            by_bookmarks(&[item("Gone", None)], 3),
            Err(SplitError::NoBookmarks)
        ));
    }
}
