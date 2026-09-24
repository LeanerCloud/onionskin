//! What a user may do with a document, decided in one place, from its
//! security.
//!
//! A plain document, and an encrypted one opened with its owner password,
//! may be changed in every way. An encrypted document opened with its user
//! password, or with none, may be changed only as its `/P` permissions allow,
//! as Acrobat allows it:
//!
//! - **Editing.** Each change is one of four kinds ([`EditKind`]), and
//!   [`EditSession::transact`](crate::EditSession::transact) refuses a kind
//!   the permissions do not allow before the body runs, whichever tool,
//!   command or verb asked. What is allowed is written encrypted, with the
//!   document's own key.
//! - **Reading the object graph out into another document.** Compress,
//!   split, combine, extract, insert-from-file, the comment summary and
//!   print-to-file without Print as Image copy the source's objects into a
//!   new, unencrypted file. [`read_out`] allows that only where the
//!   permissions allow extracting content and changing the pages, and every
//!   such command calls it.
//!
//! Both are **derived from the document**, never from a flag someone could
//! forget to set.

use std::fmt;

use onionskin_cos::Document as CosDocument;
use onionskin_crypto::{Access, Permissions};

/// What kind of change an edit makes, which decides the permission it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Anything else: text, images, links, metadata. Needs "changes".
    Content,
    /// Adding and changing comments. Needs "comments" or "changes".
    Comments,
    /// Filling in form fields. Needs "form filling", "comments" or
    /// "changes".
    Forms,
    /// Inserting, deleting, moving and turning pages. Needs "document
    /// assembly" or "changes".
    Pages,
}

/// Why an operation on an encrypted document is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The permissions do not allow copying the document's content out.
    EncryptedSource,
    /// The permissions do not allow this kind of change.
    Restricted(EditKind),
    /// The document's security is changed only by one opened with its
    /// permissions password.
    ChangeSecurity,
}

impl Refusal {
    /// The short reason a disabled menu entry or tool shows. Fixed text, so it
    /// can sit in the same place as every other entry's reason.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::EncryptedSource => "Security: copying content out is not allowed",
            Refusal::Restricted(EditKind::Content) => "Security: changes are not allowed",
            Refusal::Restricted(EditKind::Comments) => "Security: commenting is not allowed",
            Refusal::Restricted(EditKind::Forms) => "Security: filling in forms is not allowed",
            Refusal::Restricted(EditKind::Pages) => "Security: changing pages is not allowed",
            Refusal::ChangeSecurity => {
                "Security: the permissions password is needed to change security"
            }
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; the document's permissions password lifts the restriction",
            self.reason()
        )
    }
}

impl std::error::Error for Refusal {}

/// What `document` allows: everything for a plain one or one its owner
/// opened, its `/P` for one opened with the user password.
pub fn permitted(document: &CosDocument) -> Permissions {
    match (document.access(), document.permissions()) {
        (Some(Access::User), Some(permissions)) => permissions,
        _ => Permissions::ALL,
    }
}

/// Whether `document`'s object graph may be read out into any other document.
///
/// **The encrypted-source rule**, which every package that can reach
/// `write_new`, or copy a page between documents, cites by name.
pub fn read_out(document: &CosDocument) -> Result<(), Refusal> {
    let permissions = permitted(document);
    if permissions.extract() && (permissions.modify() || permissions.assemble()) {
        Ok(())
    } else {
        Err(Refusal::EncryptedSource)
    }
}

/// Whether `document`'s security may be changed: a plain document's may,
/// and an encrypted one's only when its owner opened it.
pub fn change_security(document: &CosDocument) -> Result<(), Refusal> {
    match document.access() {
        Some(Access::User) => Err(Refusal::ChangeSecurity),
        _ => Ok(()),
    }
}

/// Whether `document` may take a change of any kind: the "changes"
/// permission, which every other kind also allows.
pub fn edit(document: &CosDocument) -> Result<(), Refusal> {
    edit_as(document, EditKind::Content)
}

/// Whether `document` may take a change of `kind`.
pub fn edit_as(document: &CosDocument, kind: EditKind) -> Result<(), Refusal> {
    let permissions = permitted(document);
    let allowed = permissions.modify()
        || match kind {
            EditKind::Content => false,
            EditKind::Comments => permissions.annotate(),
            EditKind::Forms => permissions.annotate() || permissions.fill_forms(),
            EditKind::Pages => permissions.assemble(),
        };
    if allowed {
        Ok(())
    } else {
        Err(Refusal::Restricted(kind))
    }
}

/// What to tell a user when a document with restrictions opens: what they
/// cannot do, and how to lift it. `None` for one that restricts nothing.
pub fn notice(document: &CosDocument) -> Option<String> {
    let permissions = permitted(document);
    let restricted: Vec<&str> = [
        (permissions.modify(), "changes"),
        (permissions.annotate(), "comments"),
        (
            permissions.annotate() || permissions.fill_forms(),
            "filling in forms",
        ),
        (
            permissions.assemble() || permissions.modify(),
            "changing pages",
        ),
        (permissions.extract(), "copying content"),
        (permissions.print(), "printing"),
    ]
    .into_iter()
    .filter(|(allowed, _)| !allowed)
    .map(|(_, what)| what)
    .collect();
    if restricted.is_empty() {
        return None;
    }
    Some(format!(
        "This document's security does not allow {}. Opening it with its \
         permissions password lifts the restrictions.",
        join(&restricted)
    ))
}

/// "a", "a or b", "a, b or c".
fn join(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}
