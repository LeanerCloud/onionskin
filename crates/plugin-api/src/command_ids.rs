//! Command ids the shell asks the registry about before their plugins exist.
//!
//! A menu entry that waits on a later package queries the registry for that
//! package's command by id. **The id is a constant here, and the package that
//! lands the command registers exactly this constant**, so a misspelling on
//! either side is a compile error rather than an entry that quietly never goes
//! live. A string literal in the menu and another in the plugin would be two
//! guesses that happen to agree today.

/// Copy the selection as rich text. `ACROBAT-PARITY.md` rich-text export, M3.
pub const COPY_WITH_FORMATTING: &str = "edit.copy-with-formatting";
/// Export the selection to a file. Same row.
pub const EXPORT_SELECTION: &str = "file.export-selection";
/// Add a bookmark at the current view. P13b.
pub const ADD_BOOKMARK: &str = "document.add-bookmark";
/// File > Print. P15 to P17.
pub const PRINT: &str = "file.print";
/// The page-organization commands. P11.
pub const PAGE_COMMANDS: &str = "organize.page-commands";
