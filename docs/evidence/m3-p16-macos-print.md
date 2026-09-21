# M3 P16 verification: the macOS print backend

Date: 2026-09-21. Written and checked on Linux x86-64. The macOS code was
type-checked and linted for `aarch64-apple-darwin` from Linux (stable
toolchain, `-Zbuild-std` over the matching `library/` source), which
proves it compiles against AppKit's and PDFKit's bindings. **It has not
run on a Mac.** The manual acceptance run below is pending, and M3 is not
done until it has run.

## Rows

None flips. Row 92 (Print on both sides / duplex) and row 86 (Print dialog)
note the backend. Both stay `planned` until the P17 dialog makes printing
reachable and the manual run is recorded, by the same rule P15 follows.

## How it is built

- **One deviation from the plan, deliberately.** The plan sketched an
  `NSView` whose `drawRect:` renders each sheet through `crates/render`.
  Instead, `MacBackend` asks P15's `FileBackend` for the composed sheets and
  hands that PDF to PDFKit's
  `printOperationForPrintInfo:scalingMode:autoRotate:`. Three reasons:
  - the printer gets exactly the bytes `tests/file_backend.rs` checks,
    with no second sheet renderer to disagree with them;
  - sheets reach the printer as vectors;
  - no sheet is ever a printer-resolution raster, which answers the plan's
    review risk about unbounded memory (a 1200 dpi A3 sheet is about
    200 MB of RGBA).
  - Print as Image still sends our renderer's pixels when a job asks for
    them.
- **`backend/native.rs`:** `NativeSettings::new(job, sheets).entries()` is a
  pure function, compiled and tested on every platform. It gives each
  setting keyed by the `NSPrintInfo` attribute it sets:
  - paper name (PWG: `na-letter`, `na-legal`, `iso-a4`) and size;
  - orientation, taken from the imposed sheets rather than the request, so
    Auto on two-up tells the printer landscape;
  - zero margins, no centring and a scaling factor of 1, because each
    sheet is already the paper;
  - copies (at least one), collation, the printer by name when one is
    chosen;
  - `PMDuplexMode`: 1 off, 2 long edge, 3 short edge.
- **`backend/macos.rs`** (`#[cfg(target_os = "macos")]`) applies those
  entries key by key.
  - Copies and collation are set in the print info's dictionary under
    AppKit's own constants, whose strings (`NSCopies`) differ from the
    attribute names.
  - Duplex goes through PrintCore's `PMSetDuplex` on the print info's
    `PMPrintSettings`, then `updateFromPMPrintSettings`.
  - An unknown printer name, a PrintCore failure, a document PDFKit cannot
    read, and a cancelled or refused job each return
    `PrintError::Platform` with a reason. `runOperation` reports failure,
    so the user is not left with a spinner.
  - Printing started off the main thread is refused with a reason, rather
    than blocking a worker on a panel.
  - macOS's own print panel is not shown, because ours asks; its progress
    panel is.
  - `printers()` lists printer names for P17's dialog.
- **Encrypted sources:** `MacBackend` inherits `FileBackend`'s refusal, so
  an encrypted document prints only as images. On paper that is stricter
  than it needs to be, since the spool file is transient. P17 can turn
  Print as Image on for such documents rather than refuse.
- **Target gating:** the `objc2` family (`objc2` 0.6, `objc2-foundation`,
  `objc2-app-kit`, `objc2-pdf-kit` 0.3) is a
  `[target.'cfg(target_os = "macos")'.dependencies]` of `crates/print`, so
  no other build links AppKit. `native_backend()` returns `Some("macOS")`
  only there. **Job additions:** `PrintJob` gains `copies`, `collate`,
  `printer` and a `Duplex` of Off, LongEdge or ShortEdge, replacing the
  boolean.

## Runs

- `cargo test -p onionskin-print` (Linux): 21 unit tests, 14 file-backend
  tests and 1 platform test pass.
  - `a_job_maps_to_exactly_these_settings_in_this_order` compares the whole
    entry list. Swapping two keys fails it, which is the mutation the plan
    names.
  - `the_orientation_is_the_sheets_not_the_request`.
  - `defaults_print_one_collated_single_sided_copy_on_the_default_printer`.
  - `zero_copies_is_one_and_every_paper_and_duplex_mode_has_a_name`.
  - `a_build_without_a_platform_backend_says_so` and
    `only_macos_has_a_platform_backend_at_m3`.
- `tests/macos.rs` on macOS (compiled here for `aarch64-apple-darwin`, not
  run):
  - `macos_has_its_backend` reads the printer list;
  - `a_print_off_the_main_thread_is_refused_with_a_reason`: test threads
    are not the main thread, so it needs no printer.
- `RUSTC_BOOTSTRAP=1 cargo clippy -Zbuild-std=std,panic_abort --target
  aarch64-apple-darwin -p onionskin-print --all-targets -- -D warnings`:
  clean. The first check caught a missing `MainThreadMarker` argument,
  which is the value of doing it.
- `cargo clippy -p onionskin-print --all-targets -- -D warnings` and
  `cargo fmt --all --check` on Linux: clean.
- The CI matrix already runs `cargo clippy --workspace` and `cargo test
  --workspace` on `macos-latest`, which builds and runs the macOS half.
- **Coverage** (`cargo tarpaulin -p onionskin-print`, Linux): 241 of 265
  lines, 91%. The 4 lines tarpaulin attributes to `macos.rs` are the
  target-gated file, which Linux does not compile.

## Manual acceptance (pending)

To be run on a Mac once P17's dialog can start a print. The expected result
is given per step; the observed column is empty until it has run.

| Step | Expected | Observed |
|---|---|---|
| Print `corpus/seeds/two-page.pdf` to macOS's "Save as PDF" destination, default settings | A two-page Letter PDF whose pages match the on-screen pages | |
| Same, Copies 2, Collate on, to a real printer if one is available | Two collated sets | |
| Two-up a ten-page document | Five landscape sheets, pages in reading order | |
| Duplex, long edge, three pages | Two sheets, the second's back blank | |
| A commented document in each of the four Comments & Forms modes | Markups, stamps only, none, none | |
| An encrypted document | Refused unless Print as Image is on; with it on, prints | |
| Cancel from the progress panel | "Could not print: the print job was cancelled or the printer refused it" | |
