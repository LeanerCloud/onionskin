//! Printing, end to end, because GPUI has none: renders pages through
//! `render` and drives the platform pipeline (NSPrintOperation on macOS,
//! CUPS on Linux, the Windows print APIs), with a print-to-file backend
//! behind the same trait so print behaviour is testable in CI. Acrobat
//! print-dialog parity - page ranges, scaling, N-up, booklet,
//! print-as-image. GPUI-free; `app` supplies only the dialog UI.
