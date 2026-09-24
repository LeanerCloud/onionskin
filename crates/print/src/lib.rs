//! Printing, end to end, because GPUI has none: a page-selection model, an
//! imposition engine that turns pages into sheets, and backends that put
//! sheets somewhere. The print-to-file backend is testable in CI; the macOS
//! backend (P16), the CUPS one and the Windows one (M4) sit behind the same
//! trait. GPUI-free; `app` supplies only the dialog.

pub mod appendix;
pub mod backend;
pub mod booklet;
pub mod impose;
pub mod job;
pub mod poster;
pub mod ranges;
pub mod sheet;

pub use appendix::{appendix_job, concatenate, print_with_appendix};
pub use backend::cups::{CupsBackend, Programs as CupsPrograms};
pub use backend::file::{print_to_file, FileBackend};
#[cfg(target_os = "macos")]
pub use backend::macos::{printers, MacBackend};
pub use backend::{native_backend, PrintBackend, PrintError};
pub use impose::{impose, PageSize};
pub use job::{
    Binding, Booklet, BookletSides, Duplex, Handling, NUp, NUpOrder, Orientation, PageSelection,
    PaperSize, Poster, PrintJob, Sizing, Subset,
};
pub use poster::{
    preflight as poster_preflight, sheet_count as poster_sheet_count, PosterError,
    MAX_POSTER_SHEETS,
};
pub use ranges::{parse_page_ranges, RangeError};
pub use sheet::{Placement, Sheet};
