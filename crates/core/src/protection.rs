//! What M3 lets a user do with an encrypted document, decided in one place.
//!
//! Ruling A opens the encrypted documents Acrobat opens without a password,
//! read-only: render them, print them, search them. Writing to them is M6's
//! whole job - nothing in this codebase encrypts - so two things are refused,
//! and **both are derived from the document's encryption state**, never from a
//! flag someone could forget to set:
//!
//! - **Editing.** [`EditSession::transact`](crate::EditSession::transact)
//!   refuses on an encrypted base, so no tool, command or verb can begin an
//!   edit, whichever route it takes. Refused at the start of the work rather
//!   than at save, so a user never makes changes they cannot keep.
//! - **Reading the object graph out into another document.** Compress, split,
//!   combine, extract, insert-from-file, the comment summary and print-to-file
//!   without Print as Image all copy the source's objects into a new file. On an
//!   encrypted source both available outcomes are wrong: pass `/Encrypt`
//!   through and the output is plaintext under a trailer no reader can open;
//!   drop it and the output is a silently decrypted copy with the permission
//!   bits gone. So [`read_out`] refuses, and every such command calls it -
//!   for a session-scoped input through the command's requirement, and for a
//!   file the user picks afterwards at execution.
//!
//! [`read_out`] is **the one predicate**. The editing gate is the same answer
//! asked a different question; a second function that could drift from it is
//! the thing this module exists to prevent.

use std::fmt;

use onionskin_cos::Document as CosDocument;

/// Why an operation on an encrypted document is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The document is encrypted, and writing encrypted documents is M6.
    EncryptedSource,
}

impl Refusal {
    /// The milestone that lifts the refusal. Carried as data rather than only
    /// in prose, so a caller that reports it cannot quote the wrong one.
    pub fn milestone(self) -> &'static str {
        match self {
            Refusal::EncryptedSource => "M6",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::EncryptedSource => write!(
                f,
                "this document is encrypted; changing or copying out of encrypted \
                 documents arrives in {}",
                self.milestone()
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// Whether `document`'s object graph may be read out into any other document.
///
/// **The encrypted-source rule**, which every package that can reach
/// `write_new`, or copy a page between documents, cites by name.
pub fn read_out(document: &CosDocument) -> Result<(), Refusal> {
    if document.is_encrypted() {
        return Err(Refusal::EncryptedSource);
    }
    Ok(())
}

/// Whether `document` may be edited. The same answer as [`read_out`], because
/// the reason is the same: nothing here can write an encrypted document.
pub fn edit(document: &CosDocument) -> Result<(), Refusal> {
    read_out(document)
}

/// What to tell a user when an encrypted document opens, or `None` for a plain
/// one.
///
/// **In words, not a milestone number alone**, and saying the uncomfortable
/// part when it applies: a document whose own permissions allow changes is one
/// Acrobat would let the user edit and Onionskin will not yet. That residual
/// was weighed and accepted in ruling A; hiding it would make the refusal look
/// like the document's fault.
pub fn notice(document: &CosDocument) -> Option<String> {
    let permissions = document.permissions()?;
    let mut text = String::from(
        "This document is encrypted. You can read, search, print and export it, \
         but editing is turned off: saving changes to encrypted documents arrives \
         in M6.",
    );
    if permissions.modify() {
        text.push_str(
            " Its own permissions allow changes, so other PDF editors may let you \
             edit it now.",
        );
    }
    Some(text)
}
