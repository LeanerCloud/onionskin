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

use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, Object};

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
    Ok(base.section_for(&edit.pending_edits(), &stamped(base, edit)?)?)
}

/// The trailer key that says a section is Onionskin's.
pub const SECTION_STAMP: &str = "OnionskinSection";

/// The session's trailer edits, plus the stamp that marks the section a save
/// writes as Onionskin's: where it starts, who wrote it, and when.
///
/// The start offset is what makes the stamp evidence rather than a guess.
/// Trailer keys are carried forward into later sections, by us and by other
/// writers, so a stamp alone would mark every later section as ours; a
/// stamp whose `/Start` is the section's own first byte cannot have been
/// copied from an earlier one.
///
/// Only a save stamps, not the preview: the preview is never written, and a
/// clock in it would make two previews of one state differ.
fn stamped(
    base: &CosDocument,
    edit: &EditSession,
) -> Result<std::collections::BTreeMap<Name, Option<Object>>> {
    let mut edits = edit.trailer_edits();
    let writes_something =
        !edit.pending_edits().is_empty() || !edits.is_empty() || !base.provenance().is_clean();
    if writes_something {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let mut stamp = Dict::new();
        stamp.set(
            Name::new("Start"),
            Object::Integer(base.next_section_start()? as i64),
        );
        stamp.set(
            Name::new("Producer"),
            Object::String(format!("Onionskin {}", env!("CARGO_PKG_VERSION")).into_bytes()),
        );
        stamp.set(
            Name::new("Date"),
            Object::String(crate::annots::pdf_date(now).into_bytes()),
        );
        edits.insert(Name::new(SECTION_STAMP), Some(Object::Dict(stamp)));
    }
    Ok(edits)
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
    base.save_overlay_to_path(&edit.pending_edits(), &stamped(base, edit)?, path)?;

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
