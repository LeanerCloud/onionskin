//! A document's security, described and changed.
//!
//! **Changing it writes a new file.** Protecting a plain document, changing
//! the passwords or permissions of a protected one, or removing its
//! protection re-encrypts every object, so none of them can be a section
//! appended to the file. The whole document, with any unsaved changes, is
//! written afresh to the path given and the session reopened from it, with
//! the password that gives full access. The undo history goes with the old
//! file: what it would undo is now the file's own content.
//!
//! Only a plain document, or an encrypted one opened with its permissions
//! password, may have its security changed (`protection::change_security`).

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use onionskin_crypto::Method;
pub use onionskin_crypto::{Access, Permissions, Protection, Strength};

use crate::{Document, DocumentFile, Error, Result};

/// What Document Properties' Security tab says about a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityFacts {
    /// "No Security" or "Password Security".
    pub method: &'static str,
    /// The cipher and key length, such as "256-bit AES". `None` for a plain
    /// document.
    pub level: Option<&'static str>,
    /// Which password opened it.
    pub access: Option<Access>,
    /// What the permissions allow, as it was opened.
    pub permitted: Permissions,
    pub encrypts_metadata: bool,
}

impl Document {
    /// The document's security, for the Security tab.
    pub fn security_facts(&self) -> SecurityFacts {
        let encryption = self.cos.encryption();
        SecurityFacts {
            method: if encryption.is_some() {
                "Password Security"
            } else {
                "No Security"
            },
            level: encryption.map(|(revision, method, _)| level(revision, method)),
            access: self.cos.access(),
            permitted: self.permitted(),
            encrypts_metadata: encryption.is_none_or(|(_, _, metadata)| metadata),
        }
    }

    /// Why this document's security may not be changed, if it may not.
    pub fn security_refusal(&self) -> Option<crate::protection::Refusal> {
        crate::protection::change_security(&self.cos).err()
    }

    /// Write the whole document to `path` under `protection`, or with none,
    /// and reopen the session from it.
    pub(crate) fn write_security(
        &mut self,
        path: &Path,
        protection: Option<&Protection>,
    ) -> Result<()> {
        crate::protection::change_security(&self.cos).map_err(Error::Protected)?;
        let bytes = self.cos.rewrite(
            &self.edit.pending_edits(),
            &self.edit.trailer_edits(),
            protection,
        )?;
        write_atomically(path, &bytes)?;
        let password = protection.map_or_else(String::new, full_access);
        let reopened = Document::open_shared_with_password(Arc::new(bytes), &password)?;
        let previous = self.path.replace(path.to_path_buf());
        let recovery = self.recovery.take();
        let hairline = self.hairline_strokes;
        // Moved on from, never back: every cache and view keys on these.
        let generation = self.byte_generation + 1;
        let layer_epoch = self.layer_epoch + 1;
        *self = reopened;
        self.path = Some(path.to_path_buf());
        self.set_hairline_strokes(hairline)?;
        self.byte_generation = generation;
        self.layer_epoch = layer_epoch;
        if let Some(store) = &recovery {
            store.discard(path).map_err(Error::Recovery)?;
            if let Some(previous) = previous {
                store.discard(&previous).map_err(Error::Recovery)?;
            }
        }
        self.recovery = recovery;
        Ok(())
    }
}

impl DocumentFile {
    /// Save the document to `path` with `protection`, or with its security
    /// removed, and go on from the saved file. See [`crate::security`].
    pub fn save_with_security(
        &mut self,
        path: &Path,
        protection: Option<&Protection>,
    ) -> Result<()> {
        self.document_mut().write_security(path, protection)
    }
}

/// The password that opens `protection`'s document with full access.
fn full_access(protection: &Protection) -> String {
    let password = if protection.owner_password.is_empty() {
        &protection.user_password
    } else {
        &protection.owner_password
    };
    String::from_utf8_lossy(password).into_owned()
}

/// What a level of encryption is called.
fn level(revision: u8, method: Method) -> &'static str {
    match (method, revision) {
        (Method::Aes256, _) => "256-bit AES",
        (Method::Aes128, _) => "128-bit AES",
        (Method::Rc4, 2) => "40-bit RC4",
        (Method::Rc4, _) => "128-bit RC4",
        (Method::Identity, _) => "None",
    }
}

/// Write `bytes` to `path` through a temporary file beside it, renamed into
/// place once every byte is on disk, so a failure leaves the old file whole.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let directory = directory.unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = directory.join(format!(".{name}.onionskin-security.{}", std::process::id()));
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(written?)
}
