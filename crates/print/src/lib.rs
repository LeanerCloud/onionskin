//! Printing, end to end, because GPUI has none: a page-selection model, an
//! imposition engine that turns pages into sheets, and backends that put
//! sheets somewhere. The print-to-file backend is testable in CI; the macOS
//! backend (P16) and the CUPS and Windows ones (M4) sit behind the same
//! trait. GPUI-free; `app` supplies only the dialog.

pub mod backend;
pub mod impose;
pub mod job;
pub mod sheet;

pub use backend::file::{print_to_file, FileBackend};
pub use backend::{PrintBackend, PrintError};
pub use impose::{impose, PageSize};
pub use job::{NUp, NUpOrder, Orientation, PageSelection, PaperSize, PrintJob, Sizing, Subset};
pub use sheet::{Placement, Sheet};
