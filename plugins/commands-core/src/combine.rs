//! Combine Files: a list of PDFs, each whole or in part, into one new PDF.
//!
//! Create PDF From Multiple Files is this with a different entry point, and
//! says so: both build an [`Input`] list and call [`combine`]. An input that is
//! not a PDF - a scan, a photo - is made into one by an [`Importer`], which
//! the shell backs with the registry's codecs, so this crate names no image
//! format.
//!
//! Inputs are opened one at a time and dropped once their pages are copied,
//! so a hundred-file combine holds one input and the output, never a hundred
//! inputs. The output is written only once every input has been copied, so a
//! refused input - an encrypted one, one that will not open - leaves no file
//! behind.

use std::path::{Path, PathBuf};

use std::io::Read as _;
use std::sync::Arc;

use onionskin_core::pages::{Assembly, Tagging};
use onionskin_cos::{BytesSource, Document as CosDocument};
use onionskin_plugin_api::{CodecPlugin, ImportError, PluginRegistry};

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

/// What turns a file that is not a PDF into one.
pub trait Importer {
    /// Whether `bytes` - a file's start is enough - are a format it imports.
    fn reads(&self, bytes: &[u8]) -> bool;
    /// A document made from the whole file.
    fn import(&self, bytes: &[u8]) -> Result<Vec<u8>, ImportError>;
}

/// PDFs only: for a caller with no codecs to hand.
pub struct PdfOnly;

impl Importer for PdfOnly {
    fn reads(&self, _bytes: &[u8]) -> bool {
        false
    }

    fn import(&self, _bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        Err(ImportError::NotImported)
    }
}

/// The codecs a registry holds, cloned out of it so a background job can
/// own them.
pub type Codecs = Vec<Arc<dyn CodecPlugin + Send + Sync>>;

/// Every codec `registry` holds.
pub fn codecs(registry: &PluginRegistry) -> Codecs {
    registry
        .codecs()
        .filter_map(|codec| registry.codec(codec.id()))
        .collect()
}

impl Importer for Codecs {
    fn reads(&self, bytes: &[u8]) -> bool {
        self.iter().any(|codec| codec.reads(bytes))
    }

    fn import(&self, bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
        self.iter()
            .find(|codec| codec.reads(bytes))
            .ok_or(ImportError::NotImported)?
            .import(bytes)
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
    /// The file is an image, or another format a codec reads, and it did
    /// not become a document.
    Import {
        path: PathBuf,
        source: ImportError,
    },
    /// The file could not be read at all.
    Read {
        path: PathBuf,
        source: std::io::Error,
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
            Self::Import { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Read { path, source } => {
                write!(f, "{} could not be read: {source}", path.display())
            }
            Self::Assemble(source) => write!(f, "the combined document: {source}"),
            Self::Publish(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for CombineError {}

/// Combine `inputs`, in order, into a new file at `output`.
pub fn combine(
    inputs: &[Input],
    output: &Path,
    importer: &dyn Importer,
) -> Result<Combined, CombineError> {
    let assembled = assemble(inputs, importer)?;
    publish(&[(output.to_path_buf(), assembled.bytes)]).map_err(CombineError::Publish)?;
    Ok(Combined {
        page_count: assembled.page_count,
        tagging: assembled.tagging,
    })
}

/// The combined document's bytes, without writing them anywhere.
pub fn assemble(
    inputs: &[Input],
    importer: &dyn Importer,
) -> Result<onionskin_core::pages::Assembled, CombineError> {
    if inputs.is_empty() {
        return Err(CombineError::NoInputs);
    }
    let mut assembly = Assembly::new();
    for input in inputs {
        // Opened here and dropped at the end of the iteration: one input at a
        // time, however long the list.
        let source = open(&input.path, importer)?;
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
pub fn page_count(path: &Path, importer: &dyn Importer) -> Result<usize, CombineError> {
    let source = open(path, importer)?;
    all_pages(&source)
        .map(|pages| pages.len())
        .map_err(|source| CombineError::Input {
            path: path.to_path_buf(),
            source,
        })
}

/// The files directly inside `folder` that combine can take, by name: what
/// Add Folder adds. PDFs by extension, and any other file whose signature the
/// importer reads - a folder of scans. Not recursive, which is what Acrobat
/// does, and sorted so the order is the one the user sees in a file browser
/// rather than the file system's.
pub fn files_in_folder(folder: &Path, importer: &dyn Importer) -> std::io::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file() && (is_pdf(path) || importable(path, importer)))
        .collect();
    found.sort();
    Ok(found)
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// How much of a file its signature needs.
const SIGNATURE: usize = 64;

fn importable(path: &Path, importer: &dyn Importer) -> bool {
    let mut head = Vec::with_capacity(SIGNATURE);
    std::fs::File::open(path)
        .and_then(|file| file.take(SIGNATURE as u64).read_to_end(&mut head))
        .is_ok_and(|_| importer.reads(&head))
}

/// A PDF as itself; anything the importer reads, through it. A file neither
/// recognises is opened as a PDF, so the error is the parser's own account of
/// why it is not one.
fn open(path: &Path, importer: &dyn Importer) -> Result<CosDocument, CombineError> {
    let opened = if !is_pdf(path) && importable(path, importer) {
        let pdf = import(path, importer)?;
        CosDocument::open_repairing(Box::new(BytesSource::new(pdf)))
    } else {
        CosDocument::open_path_repairing(path)
    };
    opened
        .map(|(document, _)| document)
        .map_err(|source| CombineError::Open {
            path: path.to_path_buf(),
            source,
        })
}

fn import(path: &Path, importer: &dyn Importer) -> Result<Vec<u8>, CombineError> {
    let bytes = std::fs::read(path).map_err(|source| CombineError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    importer
        .import(&bytes)
        .map_err(|source| CombineError::Import {
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

#[cfg(test)]
mod tests {
    use onionskin_plugin_api::{
        Document, ExportError, ExportOutputKind, ExportRequest, PageIndex, PluginManifest,
    };

    use super::*;

    /// Reads files starting with its own tag and imports each as that tag.
    struct Tagged(&'static str);

    impl CodecPlugin for Tagged {
        fn id(&self) -> &'static str {
            self.0
        }
        fn name(&self) -> &'static str {
            self.0
        }
        fn extension(&self) -> &'static str {
            self.0
        }
        fn output_kind(&self) -> ExportOutputKind {
            ExportOutputKind::Single
        }
        fn export_page(
            &self,
            _doc: &mut Document,
            _request: &ExportRequest,
            _page: PageIndex,
            _first_in_request: bool,
        ) -> Result<Vec<u8>, ExportError> {
            Ok(Vec::new())
        }
        fn reads(&self, bytes: &[u8]) -> bool {
            bytes.starts_with(self.0.as_bytes())
        }
        fn import(&self, _bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
            Ok(self.0.as_bytes().to_vec())
        }
    }

    struct TwoCodecs;

    impl PluginManifest for TwoCodecs {
        fn id(&self) -> &'static str {
            "test.two-codecs"
        }
        fn name(&self) -> &'static str {
            "Two codecs"
        }
        fn register(&self, registry: &mut PluginRegistry) {
            registry.register_codec(Box::new(Tagged("png")));
            registry.register_codec(Box::new(Tagged("tif")));
        }
    }

    #[test]
    fn the_registrys_codecs_import_through_whichever_reads_the_file() {
        let mut registry = PluginRegistry::new();
        registry.install(&TwoCodecs);
        let codecs = codecs(&registry);
        assert_eq!(codecs.len(), 2);

        assert!(codecs.reads(b"tif...."));
        assert_eq!(codecs.import(b"tif....").expect("imports"), b"tif");
        assert_eq!(codecs.import(b"png").expect("imports"), b"png");
        assert!(!codecs.reads(b"%PDF"));
        assert!(matches!(
            codecs.import(b"%PDF"),
            Err(ImportError::NotImported)
        ));
        assert!(matches!(
            PdfOnly.import(b"png"),
            Err(ImportError::NotImported)
        ));
    }
}
