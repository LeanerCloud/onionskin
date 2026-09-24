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
/// File > Reduce File Size: a smaller copy, rewritten, with its history
/// discarded. P14b. The menu entry opens a dialog that says so and asks
/// where; the command writes the copy beside the document.
pub const REDUCE_FILE_SIZE: &str = "file.reduce-size";
/// File > Print. P15 to P17.
pub const PRINT: &str = "file.print";
/// Split the open document at its top-level bookmarks, beside it. P12. The
/// File menu's Split Document entry asks for it, and opens the dialog that
/// offers the other ways to split.
pub const SPLIT_DOCUMENT: &str = "document.split";
/// Page organization, P11: each acts on the page the viewport is on.
pub const ROTATE_PAGE_CLOCKWISE: &str = "organize.rotate-page-clockwise";
pub const ROTATE_PAGE_COUNTERCLOCKWISE: &str = "organize.rotate-page-counterclockwise";
pub const DELETE_PAGE: &str = "organize.delete-page";
pub const INSERT_BLANK_PAGE: &str = "organize.insert-blank-page";
pub const MOVE_PAGE_EARLIER: &str = "organize.move-page-earlier";
pub const MOVE_PAGE_LATER: &str = "organize.move-page-later";
pub const RESET_PAGE_NUMBERING: &str = "organize.reset-page-numbering";
/// Crop Pages, M5 `tools-edit`. The command crops the page on screen to what
/// it draws (Acrobat's Remove White Margins); the menu entries, live when it
/// is, open the dialog that takes margins, a box and a page range.
pub const CROP_PAGES: &str = "edit.crop-pages";
/// Summarize the open document's comments into a new file beside it. P10.
/// The Edit menu's Summarize Comments entry asks for it, and opens the dialog
/// that offers both layouts.
pub const SUMMARIZE_COMMENTS: &str = "comment.summarize";
/// The selected image turned and flipped, M5 `tools-edit`: what the Edit
/// menu's image entries run.
pub const ROTATE_IMAGE_CLOCKWISE: &str = "image.rotate-clockwise";
pub const ROTATE_IMAGE_COUNTERCLOCKWISE: &str = "image.rotate-counterclockwise";
pub const FLIP_IMAGE_HORIZONTAL: &str = "image.flip-horizontal";
pub const FLIP_IMAGE_VERTICAL: &str = "image.flip-vertical";
