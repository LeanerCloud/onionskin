//! Turning the overlay into bytes.
//!
//! One section per save, not one per edit. Ten edits and one save append one
//! section, because the section is built from the overlay's *net* state rather
//! than from the history.
//!
//! **A save whose write succeeds and whose reopen fails is the case worth
//! designing for.** The bytes are on disk and correct, so the file is not lost,
//! but the session's `cos::Document` is still the pre-save one while the
//! overlay describes a state already written. The rule here: the overlay is
//! **not** cleared and the saved mark is **not** advanced, and the caller is
//! told the file was written but could not be reloaded. Refusing to clear is
//! the safe direction. Clearing on a failed reopen would lose the edits from a
//! session whose file on disk is perfectly fine, and a second save would then
//! append a second section carrying the same changes, which is untidy but not
//! destructive.
//!
//! Nothing here writes into cos's own edit map, so nothing has to be withdrawn
//! from it when a save fails.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use onionskin_cos::{BytesSource, Document as CosDocument};

use crate::edit::EditSession;
use crate::{Error, Result};

/// What a save did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveOutcome {
    /// Sections the save appended: one, or zero when the overlay was empty and
    /// the document was clean.
    pub sections_appended: usize,
    /// Whether the destination is a different file from the one the session
    /// opened, which is what makes the generations list describe a new history
    /// rather than the old one.
    pub saved_as: bool,
}

/// The net section for an overlay, or `None` when there is nothing to append.
///
/// This is the same call the preview makes, which is what keeps preview and
/// save from disagreeing: they cannot, because there is one builder.
pub(crate) fn section(base: &CosDocument, edit: &EditSession) -> Result<Option<Vec<u8>>> {
    Ok(base.section_for(&edit.pending_edits(), &edit.trailer_edits())?)
}

/// A save that has been written to disk but whose reopen failed.
///
/// Carried as an error because the session cannot continue as if the save
/// succeeded, and carrying the path lets the caller say which file is fine.
#[derive(Debug)]
pub struct WrittenButNotReloaded {
    pub path: PathBuf,
    pub cause: Box<Error>,
}

/// Write the overlay and reopen from what was written.
///
/// On success the caller gets the new bytes and document and is expected to
/// rebase the edit session against them; on a failed reopen it gets
/// [`Error::WrittenButNotReloaded`] and must leave the session's overlay and
/// saved mark exactly as they were.
pub(crate) fn write_and_reopen(
    original: &[u8],
    base: &CosDocument,
    edit: &EditSession,
    path: &Path,
) -> Result<(Arc<Vec<u8>>, CosDocument, usize)> {
    let section = section(base, edit)?;
    let appended = usize::from(section.is_some());

    // cos owns the temp-file-and-rename and the permission carry-over, so a
    // save that fails part way through does not leave a truncated document
    // where the original was.
    base.save_overlay_to_path(&edit.pending_edits(), &edit.trailer_edits(), path)?;

    let written = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(Error::WrittenButNotReloaded(WrittenButNotReloaded {
                path: path.to_path_buf(),
                cause: Box::new(Error::Io(error)),
            }))
        }
    };
    debug_assert!(
        written.len() >= original.len(),
        "a save appends; it never shortens the document"
    );

    let bytes = Arc::new(written);
    match CosDocument::open(Box::new(BytesSource::from_shared(Arc::clone(&bytes)))) {
        Ok(document) => Ok((bytes, document, appended)),
        Err(error) => Err(Error::WrittenButNotReloaded(WrittenButNotReloaded {
            path: path.to_path_buf(),
            cause: Box::new(Error::Cos(error)),
        })),
    }
}
