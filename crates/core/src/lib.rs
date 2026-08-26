//! Kernel: the document model over `cos` - page tree, the edit graph
//! (base nodes are the original objects, overlay nodes are pending
//! edits), history, selection and save. Owns the tagged-PDF structure
//! tree, which every edit that touches tagged content must leave valid.
//! Contains no features; those live in `plugins/`.

/// An open document.
///
/// The page tree, edit graph, history and save path land with M1. The
/// type exists now because `plugin-api` hands it to every tool and
/// command, and that signature is the contract M0 pins down.
#[derive(Debug, Default)]
pub struct Document;
