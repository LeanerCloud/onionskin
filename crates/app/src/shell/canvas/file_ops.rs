//! What the canvas does to its document's file and history: Save, Save As,
//! Revert, Undo, Redo, autosave and recovery (P18).
//!
//! Every one of them can change what the pages are, so every one that did
//! something rebuilds the layout the way a page command does. The dirty
//! state is never stored here: it is the history's cursor against its saved
//! mark, asked each time (T1), so undoing past a save makes the document
//! dirty again without anything setting a flag.

use std::path::{Path, PathBuf};

use onionskin_core::{DocumentFile, Recovered, RecoveryStore};

use super::{CanvasError, CanvasModel};

/// What Undo or Redo would do, for a menu entry to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryFacts {
    pub undo: Option<&'static str>,
    pub redo: Option<&'static str>,
    pub dirty: bool,
    /// Whether the document has a file Save can write to.
    pub has_path: bool,
}

/// A recovery file found for a document as it opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryOffer {
    pub document: PathBuf,
    /// The recovered bytes: the document's with the autosaved section.
    pub bytes: Vec<u8>,
    /// When the recovery was written, for ranking and for the prompt.
    pub written: Option<std::time::SystemTime>,
}

impl CanvasModel {
    pub fn history_facts(&self) -> HistoryFacts {
        let history = self.document.edit().history();
        HistoryFacts {
            undo: history.undo_label(),
            redo: history.redo_label(),
            dirty: self.document.is_dirty(),
            has_path: self.document.path().is_some(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.document.path()
    }

    pub fn undo(&mut self) -> Result<bool, CanvasError> {
        let undone = self.document.document_mut().undo()?;
        if undone {
            self.relayout_after_edit()?;
        }
        Ok(undone)
    }

    pub fn redo(&mut self) -> Result<bool, CanvasError> {
        let redone = self.document.document_mut().redo()?;
        if redone {
            self.relayout_after_edit()?;
        }
        Ok(redone)
    }

    /// Write the edits to the file the document came from.
    pub fn save(&mut self) -> Result<(), CanvasError> {
        self.document.save()?;
        self.relayout_after_edit()
    }

    /// Write the document to `path`, which it then belongs to.
    pub fn save_as(&mut self, path: &Path) -> Result<(), CanvasError> {
        self.document.save_as(path)?;
        self.relayout_after_edit()
    }

    /// Throw away every unsaved edit and read the file again. The history
    /// goes with it: it described edits to a document that is no longer
    /// open.
    pub fn revert(&mut self) -> Result<(), CanvasError> {
        let path = self
            .document
            .path()
            .ok_or(CanvasError::Core(onionskin_core::Error::NoPath))?
            .to_path_buf();
        let recovery = self.recovery.clone();
        let mut reopened = DocumentFile::open(&path)?;
        if let Some(store) = recovery.clone() {
            reopened.set_recovery(store);
        }
        let old = std::mem::replace(&mut self.document, reopened);
        // Nothing unsaved is worth keeping now, so the recovery goes too.
        old.close()?;
        self.relayout_after_edit()
    }

    /// Turn autosave on, writing into `store`.
    pub fn set_recovery(&mut self, store: RecoveryStore) {
        self.recovery = Some(store.clone());
        self.document.set_recovery(store);
    }

    /// Write the recovery file for the current edits, or remove it when
    /// there are none. A no-op without a store or a path.
    pub fn autosave(&self) -> Result<Option<PathBuf>, CanvasError> {
        Ok(self.document.autosave()?)
    }

    /// The recovery file for this document, if one applies to the file as it
    /// is on disk. One that no longer applies, because the file was saved
    /// after it was written, is removed: replaying it would apply its edits
    /// twice.
    pub fn recovery_offer(&self) -> Result<Option<RecoveryOffer>, CanvasError> {
        let (Some(store), Some(path)) = (&self.recovery, self.document.path()) else {
            return Ok(None);
        };
        let original = self.document.bytes();
        match store.recover(path, &original).map_err(core_recovery)? {
            Recovered::Nothing => Ok(None),
            Recovered::Stale => {
                store.discard(path).map_err(core_recovery)?;
                Ok(None)
            }
            Recovered::Bytes(bytes) => Ok(Some(RecoveryOffer {
                document: path.to_path_buf(),
                bytes,
                written: std::fs::metadata(store.path_for(path))
                    .and_then(|meta| meta.modified())
                    .ok(),
            })),
        }
    }

    /// Replay an offered recovery as one undoable edit.
    pub fn accept_recovery(&mut self, offer: &RecoveryOffer) -> Result<(), CanvasError> {
        self.document.document_mut().replay_recovery(&offer.bytes)?;
        self.relayout_after_edit()
    }

    /// Decline an offered recovery: its file goes.
    pub fn discard_recovery(&self) -> Result<(), CanvasError> {
        if let (Some(store), Some(path)) = (&self.recovery, self.document.path()) {
            store.discard(path).map_err(core_recovery)?;
        }
        Ok(())
    }
}

fn core_recovery(error: onionskin_core::RecoveryError) -> CanvasError {
    CanvasError::Core(onionskin_core::Error::Recovery(error))
}

/// Offers most recent first: when several documents open at once with
/// recoveries waiting, the one the user was working in last is asked about
/// first. An offer with no time goes last.
pub fn rank_offers(mut offers: Vec<RecoveryOffer>) -> Vec<RecoveryOffer> {
    offers.sort_by_key(|offer| std::cmp::Reverse(offer.written));
    offers
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn offer(name: &str, written: Option<u64>) -> RecoveryOffer {
        RecoveryOffer {
            document: PathBuf::from(name),
            bytes: Vec::new(),
            written: written.map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
        }
    }

    /// PLAN.md's testing strategy item 4, by name: the recovery ranking is a
    /// unit test with no window.
    #[test]
    fn recovery_offers_rank_most_recent_first_and_undated_last() {
        let ranked = rank_offers(vec![
            offer("old.pdf", Some(100)),
            offer("undated.pdf", None),
            offer("new.pdf", Some(300)),
            offer("middle.pdf", Some(200)),
        ]);
        let names: Vec<_> = ranked
            .iter()
            .map(|offer| offer.document.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["new.pdf", "middle.pdf", "old.pdf", "undated.pdf"]);
    }
}
