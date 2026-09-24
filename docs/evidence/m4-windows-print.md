# M4 verification: printing on Windows

Date: 2026-09-24. Linux x86-64, stable toolchain. **No Windows run is
claimed**: this environment cannot download the Windows standard library,
so nothing here has been compiled for Windows, linked, or run there.

## Rows

- **To `partial`:** Print on Windows. It stays partial until the checks in
  "What is left" pass on a Windows machine.
- **Headline:** 153 planned / 30 partial / 80 out-of-scope, 140
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets, once it runs

On Windows the Print dialog lists the spooler's printers, local and
connected, after Save as PDF. Printing to one sends the job's sheets (the
same ones the preview and Save as PDF show) as one spooler job, titled with
the document's name, with the job's paper, orientation, copies, collation
and duplex. A failure keeps the dialog open with the step that failed and
Windows' error code, for example "Could not print: Windows could not open
the printer Office (error 1801)". With no printer chosen the job goes to
the default printer, or says there is none.

## How it works

`crates/print/src/backend/windows/`:

- **Why pictures.** Windows has no built-in way to print a PDF, and handing
  the file to whatever application claims `.pdf` would print it with that
  application's settings and renderer, not the job's. So each sheet the
  file backend wrote is rendered by our renderer, the one the screen uses,
  and drawn with `StretchDIBits`. One sheet is held at a time, rendered at
  the printer's resolution capped at 300 dpi: 34 MB for a Letter sheet,
  where 1200 dpi A3 would be about 200 MB.
- **`settings.rs` (all platforms).** The job as `DEVMODEW` members:
  - paper by number (Letter 1, Legal 5, A4 9), or `DMPAPER_USER` with the
    size in tenths of a millimetre;
  - orientation from the sheets;
  - copies, at least 1; collation;
  - duplex: long edge is `DMDUP_VERTICAL`, short edge `DMDUP_HORIZONTAL`,
    Windows naming the flip by the axis of a portrait sheet;
  - `dmFields` naming exactly the members set.
- **`raster.rs` (all platforms).**
  - `Sheets` renders the file backend's output one sheet at a time.
  - `SheetImage` turns premultiplied RGBA into opaque, top-down BGRA on
    white, the format `StretchDIBits` takes.
  - `device_rect` places the whole sheet at the paper's corner, which is
    up and left of GDI's origin by the printable area's offset, so every
    mark lands where the sheet put it.
- **`ffi.rs` (Windows, and `--cfg onionskin_check_windows`).**
  - Printer list: `EnumPrintersW`, level 4.
  - Default printer: `GetDefaultPrinterW`.
  - Settings: the driver's own `DEVMODEW` from `DocumentPropertiesW`,
    with ours merged in and validated by the driver.
  - Printing: `CreateDCW`, then `StartDocW`, then per sheet `StartPage`,
    `StretchDIBits` (HALFTONE) and `EndPage`, then `EndDoc`; `AbortDoc`
    on any failure.
  - Handles are closed by `Drop`.
  - A `const` block asserts every number in `settings.rs` equals
    `windows-sys`'s.
- **app.** On Windows, `tabs/print.rs` lists the spooler's printers and
  prints through `WindowsBackend`, sharing the printer path macOS and CUPS
  use. `native_backend()` answers "Windows".

## Runs

- `cargo test -p onionskin-print`: 60 unit tests pass, including 6 new
  (DEVMODE mapping, custom paper, every value; resolution cap, pixel format,
  placement). `tests/windows_raster.rs`: 3 pass. On real file-backend
  output of `two-page.pdf` two-up:
  - one sheet, sized as imposition made it, at 72 and 144 dpi;
  - opaque, with ink in both cells;
  - non-PDF bytes are refused.
- `cargo test -p onionskin-app --features shell-test-support --lib print`:
  54 pass. This is the Linux CUPS path, unchanged.
- **Type-checking the Win32 layer on Linux.**
  `RUSTFLAGS="--cfg onionskin_check_windows" cargo clippy -p onionskin-print
  -p onionskin-app --features onionskin-app/shell` passes with only the
  existing `a11y::Shared::record` warning. `windows-sys` declares its
  functions on every platform, so this checks every call's types, every
  struct literal and the constant assertions. Two deliberate breakages
  prove the check is real:
  - `DMDUP_VERTICAL` set to 3 in `settings.rs` fails the build at its
    `const` assertion;
  - a `u32` for `biBitCount`, a `u16` in `windows-sys`, fails it too.

  In this mode the app builds its Windows path in place of CUPS; swapping
  `WindowsBackend::new`'s arguments fails the build as well.
- `acrobat_parity_headline_matches_every_inventory_row` passes.

## Coverage

`cargo tarpaulin -p onionskin-print` with optimisation off for the
workspace crates, so small functions are not inlined away from their lines:
`windows/raster.rs` 35 of 35 lines, `windows/settings.rs` 23 of 23; the print
crate 739 of 766 (96.5%). At the default opt-level 1 tarpaulin reports
88.6% for the crate, and misses lines the unit tests call directly (for
example `device_rect`'s body), because they are inlined. `ffi.rs` is not
compiled in a Linux test build.

## Mutations

- `SheetImage` writing RGB order instead of BGR:
  `pixels_become_opaque_bgra_on_white` fails.
- The two build-time breakages above.

## Clippy and format

As above for Windows mode. In the normal Linux build, `cargo clippy -p
onionskin-print -p onionskin-app --features onionskin-app/shell-test-support
--all-targets` reports only the existing `a11y` warning. `cargo fmt --check`
is clean.

## What is left: the Windows run

On a Windows machine with a printer (a PDF printer such as Microsoft Print
to PDF is enough for the first four):

1. `cargo build -p onionskin-app` compiles and links.
2. File > Print lists the installed printers.
3. Printing `two-page.pdf` to Microsoft Print to PDF writes a file whose
   pages match Save as PDF's output.
4. A failure, such as a printer removed after the dialog opened, keeps the
   dialog open with the error.
5. On a duplex printer, long and short edge flip as chosen, and copies and
   collation come out as asked.

## Not claimed

- Vector output: sheets reach the printer as pictures, at up to 300 dpi.
- The printable-area offset is honoured by placing the sheet at the
  paper's corner; how a driver clips marks outside the printable area is
  the driver's.
- No Windows run of any kind.
