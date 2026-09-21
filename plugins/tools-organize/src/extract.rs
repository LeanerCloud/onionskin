//! Extract Pages: to a new document, written by `cos`'s `write_new`, and
//! optionally deleted from this one afterwards.
//!
//! Refused for an encrypted document by the importer - an extracted copy is
//! exactly the silently decrypted file the encrypted-source rule exists to
//! prevent - and, ahead of time, by `Document::read_out_refusal`, which the
//! shell's requirement query reads for any command declaring
//! `CommandEffect::ReadsOut`.

use std::path::Path;

use onionskin_core::{pages, Document, PageIndex};
use onionskin_plugin_api::CommandError;

const LABEL: &str = "Extract Pages";

/// `pages` of `doc`, as its session has them, as the bytes of a new PDF.
pub fn extract_pages(doc: &mut Document, pages: &[PageIndex]) -> Result<Vec<u8>, CommandError> {
    let source = doc.structure().map_err(failed)?;
    pages::extract_pages(source, pages).map_err(failed)
}

/// Extract `pages` of `doc` to a new file at `path`, then, when
/// `delete_after`, delete them from `doc` as one undo step. The file is
/// written before anything is deleted, so a failed write loses nothing.
pub fn extract_pages_to(
    doc: &mut Document,
    pages: &[PageIndex],
    path: &Path,
    delete_after: bool,
) -> Result<(), CommandError> {
    let bytes = extract_pages(doc, pages)?;
    std::fs::write(path, bytes).map_err(|error| failed(error.into()))?;
    if delete_after {
        crate::delete_pages(doc, pages)?;
    }
    Ok(())
}

fn failed(source: onionskin_core::Error) -> CommandError {
    CommandError::Edit {
        label: LABEL,
        source,
    }
}
