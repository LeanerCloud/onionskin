//! The Windows backend: the file backend's sheets, rendered by our own
//! renderer and drawn onto a printer's device context through GDI.
//!
//! **Why pictures and not the PDF.** Windows has no built-in way to print a
//! PDF: handing the file to whatever application claims `.pdf` would print
//! it with that application's settings and renderer, not the job's. So each
//! sheet is rendered here, at the printer's resolution up to 300 dpi, and the
//! job's paper, orientation, copies, collation and sides go in the printer's
//! `DEVMODEW`.
//!
//! **What is proven where.** `settings.rs` (the `DEVMODEW` members) and
//! `raster.rs` (rendering, pixel format and placement on the paper) are
//! plain Rust, tested on every platform. `ffi.rs` is the Win32 calls; it
//! compiles on Windows, and on any host with `--cfg onionskin_check_windows`
//! for `cargo check`, which also checks that `settings.rs` agrees with
//! `windows-sys` on every constant. It has not been run on Windows.

pub mod raster;
pub mod settings;

#[cfg(any(windows, onionskin_check_windows))]
mod ffi;

#[cfg(any(windows, onionskin_check_windows))]
pub use ffi::{printers, WindowsBackend};
