//! The one handle that can write, truncate or recover a document.
//!
//! Every tool is handed `&mut Document`. Before this type existed, P3 put save,
//! Save As, `revert_to` and autosave on `Document` itself, which meant every
//! tool held a reference that could truncate the user's file in the middle of a
//! gesture. P7's review risk names exactly that, and it is a question of what
//! the type system allows rather than of what any tool happens to do today.
//!
//! So the file operations live here. The app holds a `DocumentFile`; tools and
//! commands are handed the `&mut Document` inside it, and there is no path from
//! a `Document` back to the file that owns it. A plugin that wanted to call
//! `revert_to` would not compile.
//!
//! `DocumentFile` dereferences to `Document`, because a document with its file
//! is still a document: everything a reader or an editor needs is reachable
//! through it unchanged. The narrowing runs one way only, which is the direction
//! that matters.

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use crate::recovery::RecoveryStore;
use crate::{Document, Result, SaveOutcome};

/// A document together with the file it came from.
pub struct DocumentFile {
    document: Document,
}

impl DocumentFile {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(DocumentFile {
            document: Document::open_path(path)?,
        })
    }

    /// Wrap a document opened some other way, such as from bytes, which has no
    /// file until Save As gives it one.
    pub fn from_document(document: Document) -> Self {
        DocumentFile { document }
    }

    /// What tools and commands are handed.
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    pub fn save(&mut self) -> Result<SaveOutcome> {
        self.document.save()
    }

    pub fn save_as(&mut self, path: &Path) -> Result<SaveOutcome> {
        self.document.save_as(path)
    }

    /// Drop the trailing generation `target` by truncating the file. See
    /// `generations.rs` for why the target is the generation being dropped.
    pub fn revert_to(&mut self, target: usize) -> Result<()> {
        self.document.revert_to(target)
    }

    /// Keep generation `keep` and everything older, and truncate away every
    /// newer one. The skins panel's Roll Back.
    pub fn roll_back_to(&mut self, keep: usize) -> Result<()> {
        self.document.roll_back_to(keep)
    }

    pub fn set_recovery(&mut self, store: RecoveryStore) {
        self.document.set_recovery(store);
    }

    pub fn autosave(&self) -> Result<Option<PathBuf>> {
        self.document.autosave()
    }

    /// A clean close, which removes the document's recovery file.
    pub fn close(self) -> Result<()> {
        self.document.close()
    }
}

impl Deref for DocumentFile {
    type Target = Document;

    fn deref(&self) -> &Document {
        &self.document
    }
}

impl DerefMut for DocumentFile {
    fn deref_mut(&mut self) -> &mut Document {
        &mut self.document
    }
}
