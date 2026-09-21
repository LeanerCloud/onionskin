//! Combine Files: a list of PDFs, each whole or in part, into one new PDF.
//!
//! Create PDF From Multiple Files is this with a different entry point, and
//! says so: both build an [`Input`] list and call [`combine`].
//!
//! Inputs are opened one at a time and dropped once their pages are copied,
//! so a hundred-file combine holds one input and the output, never a hundred
//! inputs. The output is written only once every input has been copied, so a
//! refused input - an encrypted one, one that will not open - leaves no file
//! behind.

use std::path::{Path, PathBuf};

use onionskin_core::pages::{Assembly, Tagging};
use onionskin_cos::Document as CosDocument;

use crate::publish::{publish, PublishError};

/// One entry in the list the user builds: a file, and which of its pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub path: PathBuf,
    /// `None` for every page, in order; otherwise these, in this order - the
    /// dialog's per-file expansion, where a user picks pages out of one file.
    pub pages: Option<Vec<usize>>,
}

impl Input {
    pub fn whole(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            pages: None,
        }
    }
}

/// What a combine produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combined {
    pub page_count: usize,
    pub tagging: Tagging,
}

/// Why a combine produced nothing. Every variant that concerns one input
/// names it, because in a list of forty files "a file is encrypted" is not
/// something the user can act on.
#[derive(Debug)]
pub enum CombineError {
    NoInputs,
    /// The file could not be opened as a PDF.
    Open {
        path: PathBuf,
        source: onionskin_cos::Error,
    },
    /// The file opened and its pages could not be copied: encrypted, or a
    /// page the list names that the file does not have.
    Input {
        path: PathBuf,
        source: onionskin_core::Error,
    },
    Assemble(onionskin_core::Error),
    Publish(PublishError),
}

impl std::fmt::Display for CombineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoInputs => write!(f, "there are no files to combine"),
            Self::Open { path, source } => write!(f, "{} did not open: {source}", path.display()),
            Self::Input { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Assemble(source) => write!(f, "the combined document: {source}"),
            Self::Publish(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for CombineError {}

/// Combine `inputs`, in order, into a new file at `output`.
pub fn combine(inputs: &[Input], output: &Path) -> Result<Combined, CombineError> {
    let assembled = assemble(inputs)?;
    publish(&[(output.to_path_buf(), assembled.bytes)]).map_err(CombineError::Publish)?;
    Ok(Combined {
        page_count: assembled.page_count,
        tagging: assembled.tagging,
    })
}

/// The combined document's bytes, without writing them anywhere.
pub fn assemble(inputs: &[Input]) -> Result<onionskin_core::pages::Assembled, CombineError> {
    if inputs.is_empty() {
        return Err(CombineError::NoInputs);
    }
    let mut assembly = Assembly::new();
    for input in inputs {
        // Opened here and dropped at the end of the iteration: one input at a
        // time, however long the list.
        let source = open(&input.path)?;
        let pages = match &input.pages {
            Some(pages) => pages.clone(),
            None => all_pages(&source).map_err(|source| input_error(input, source))?,
        };
        assembly
            .append(&source, &pages)
            .map_err(|source| input_error(input, source))?;
    }
    assembly.finish().map_err(CombineError::Assemble)
}

/// How many pages a file has, for the dialog's expansion and preview.
pub fn page_count(path: &Path) -> Result<usize, CombineError> {
    let source = open(path)?;
    all_pages(&source)
        .map(|pages| pages.len())
        .map_err(|source| CombineError::Input {
            path: path.to_path_buf(),
            source,
        })
}

/// The PDFs directly inside `folder`, by name: what Add Folder adds. Not
/// recursive, which is what Acrobat does, and sorted so the order is the one
/// the user sees in a file browser rather than the file system's.
pub fn pdfs_in_folder(folder: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file() && is_pdf(path))
        .collect();
    found.sort();
    Ok(found)
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn open(path: &Path) -> Result<CosDocument, CombineError> {
    CosDocument::open_path_repairing(path)
        .map(|(document, _)| document)
        .map_err(|source| CombineError::Open {
            path: path.to_path_buf(),
            source,
        })
}

fn all_pages(source: &CosDocument) -> onionskin_core::Result<Vec<usize>> {
    let count = source.page_count()?;
    Ok((0..usize::try_from(count).unwrap_or(0)).collect())
}

fn input_error(input: &Input, source: onionskin_core::Error) -> CombineError {
    CombineError::Input {
        path: input.path.clone(),
        source,
    }
}
