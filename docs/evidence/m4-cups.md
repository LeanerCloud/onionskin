# M4 verification: printing through CUPS

Date: 2026-09-23. Linux x86-64 (Ubuntu 24.04), stable toolchain, CUPS
2.4.7. No macOS, Windows or hosted-CI run is claimed, and no sheet came out
of a paper printer.

## Rows

- **To `implemented`:** Print on Linux (CUPS).
- **Note updated, still `partial`:** Print on both sides / duplex. The
  duplex choice now demonstrably reaches a printer driver through CUPS, but
  no duplex printer has turned a sheet over.
- **Headline:** 155 planned / 29 partial / 80 out-of-scope, 139
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

On Linux and the BSDs, the Print dialog's printer list is CUPS's
destinations (`lpstat -e`), after Save as PDF. Choosing one and pressing
Print sends the job; the dialog closes and the notice says "Sent to
<printer>". If CUPS refuses, the dialog stays open with `lp`'s own words,
for example "Could not print: lp: Error - The printer or class does not
exist." A system without CUPS lists no printers, and a print that somehow
reaches a missing `lp` says "CUPS is not installed: there is no lp
command".

Summarize Comments sends two jobs, as on macOS: the document, then the
summary titled "<document> - Comments". Both are imposed and preflighted
before either is sent, so a poster the summary cannot fit prints nothing
rather than half of what was asked.

## How it works

`crates/print/src/backend/cups.rs`. The printer receives the file backend's
PDF, the same bytes `tests/file_backend.rs` checks, on `lp`'s standard
input. The sheets already carry sizing, N-up, booklet and poster order, so
`lp` is told only what the hardware decides:

| Job setting | `lp` argument |
|---|---|
| Printer | `-d <name>` (omitted for the default destination) |
| Title | `-t <document name>` |
| Copies | `-n <copies>`, at least 1 |
| Paper | `-o media=na_letter_8.5x11in`, `na_legal_8.5x14in`, `iso_a4_210x297mm`, or `Custom.<w>x<h>` in points |
| Duplex | `-o sides=one-sided`, `two-sided-long-edge`, `two-sided-short-edge` |
| Collate | `-o collate=true` or `false` |
| (always) | `-o print-scaling=none`, `-o document-format=application/pdf` |

No orientation is sent: CUPS turns a landscape sheet onto portrait paper
itself. `lp` and `lpstat` are found on `PATH`; `CupsPrograms` lets tests
substitute stand-ins.

In the app, `tabs/print.rs` now has one printer path for every Unix. Only
`printer_backend` differs: `MacBackend` on macOS, `CupsBackend` elsewhere.
The macOS code path is the same code as before this change, reached
through that function.

## Runs

- `cargo test -p onionskin-print`: 54 unit tests, 6 `tests/cups.rs` (1
  ignored, below), 31 `tests/file_backend.rs`, 1 `tests/macos.rs`; all pass.
- `cargo test -p onionskin-app --features shell-test-support --lib print`:
  54 pass, including `tests/print_cups.rs`:
  - `printing_to_a_cups_printer_runs_lp_with_the_sheets`: the dialog lists
    Office from a stand-in `lpstat`; Print runs a stand-in `lp` with
    `-d Office -t two-page.pdf` and `media=na_letter_8.5x11in`, and its
    input parses as a two-sheet PDF.
  - `a_printer_that_refuses_keeps_the_dialog_open_saying_why`.
  - `summarize_comments_sends_the_summary_as_a_second_job`: the second
    `lp` run is titled `two-page.pdf - Comments`. This is the first
    automated run of the two-job path; on macOS it is still unverified.
- **Against a real scheduler.** `a_real_cups_queue_receives_the_sheets`,
  run with `--include-ignored`, passes. Setup, reproducible on any Linux
  host with CUPS:

  ```sh
  apt-get install cups cups-client cups-filters
  # A backend that keeps what the filter chain hands it:
  cat > /usr/lib/cups/backend/capture <<'EOF'
  #!/bin/sh
  if [ $# -eq 0 ]; then echo 'direct capture "Unknown" "Capture to a file"'; exit 0; fi
  out="${DEVICE_URI#capture:}"
  printf '%s\n' "$5" > "$out.options"
  if [ -n "$6" ]; then cat "$6" > "$out"; else cat > "$out"; fi
  EOF
  chmod 700 /usr/lib/cups/backend/capture
  mkdir -p /var/spool/cups-out && chown root:lp /var/spool/cups-out
  cupsd
  lpadmin -p Office -E -v capture:/var/spool/cups-out/office.pdf \
    -m lsb/usr/cupsfilters/Generic-PDF_Printer-PDF.ppd
  ONIONSKIN_CUPS_QUEUE=Office ONIONSKIN_CUPS_OUTPUT=/var/spool/cups-out/office.pdf \
    cargo test -p onionskin-print --test cups -- --include-ignored
  ```

  The job (two pages two-up, long-edge duplex) went through the real `lp`,
  scheduler and filter chain. The capturing backend received a two-sheet
  PDF wrapped in the Generic PDF driver's PJL, and these options, among
  others:

  ```
  media=na_letter_8.5x11in print-scaling=none sides=two-sided-long-edge
  Duplex=DuplexNoTumble PageSize=Letter
  ```

  The PJL header the driver wrote carries `@PJL SET DUPLEX=ON`,
  `@PJL SET BINDING=LONGEDGE` and `@PJL SET PAPER=LETTER`.

  Two setups that did not work, for whoever repeats this: a raw queue
  skips the filter chain, and Ubuntu's `cupsd` would not open a
  `file://` device path even with `FileDevice Yes`, hence the capturing
  backend.
- Real `lp`'s refusal for an unknown queue is `lp: Error - The printer or
  class does not exist.`; the stand-ins use exactly those words.

## Coverage

`cargo tarpaulin -p onionskin-print`: `backend/cups.rs` 46 of 51 lines
(90%); the print crate 481 of 537 (89.6%). The uncovered CUPS lines are
the `lp did not start` and `lp did not finish` I/O failures.

## Mutations

Each was applied alone, and each failed the named tests:

- `sides` always `Duplex::Off`: `a_job_becomes_lp_arguments`.
- The Summarize Comments second job skipped:
  `summarize_comments_sends_the_summary_as_a_second_job`.
- Linux lists no printers: all three `print_cups` tests.
- The app ignores its configured `lp` (uses the one on `PATH`): all three
  `print_cups` tests.

## Clippy and format

`cargo clippy -p onionskin-print -p onionskin-app --features
onionskin-app/shell-test-support --all-targets` reports one warning, the
existing `a11y::Shared::record` dead-code warning. Eight
`field_reassign_with_default` warnings in the print dialog's poster tests
were fixed in a separate commit. `cargo fmt --check` is clean.

## Not claimed

- No paper printer printed anything, and no duplex printer turned a sheet.
- No run on a BSD.
- The printer list is read once, when the dialog opens; a printer added
  while it is open appears on the next open.
- `lp`'s request id is not shown in the notice.
