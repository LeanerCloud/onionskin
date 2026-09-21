# M3 P17 verification: the Print dialog and Page Setup

Date: 2026-09-21. Linux x86-64, stable toolchain, GPUI's test platform.
Save as PDF is exercised end to end. Printing to a printer goes through the
P16 macOS backend, which has not run on a Mac (see
`m3-p16-macos-print.md`). No macOS, Windows or hosted-CI run is claimed.

## Rows

- **To `implemented`:**
  - 4 Print button (global bar);
  - 15 File > Print;
  - 86 Print dialog;
  - 87 Page range and subset;
  - 88 Page sizing and handling;
  - 89 N-up;
  - 93 Orientation;
  - 94 Comments & Forms;
  - 95 Page Setup dialog;
  - 97 Print as image;
  - 98 Print to file;
  - 99 Advanced Print Setup (its stated subset).
- **To `partial`:**
  - 77 Print comments: Document and Markups prints them, but summary-only
    printing waits on row 96;
  - 92 duplex: sheet order is in every file, but the printer instruction
    is unrun on a Mac.
- **Stays `planned`:** 96 Summarize comments in the print output. Appending
  a summary as extra sheets needs a job that spans two documents, which
  `crates/print` does not do.
- **Headline:** 171 planned / 26 partial / 80 out-of-scope, 126
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Opening it.** File > Print…, Cmd/Ctrl+P, the global bar's printer
  button and the canvas context menu's Print all open the dialog on the
  active document.
- **Page Setup.** File > Page Setup… (Cmd/Ctrl+Shift+P) chooses paper
  (Letter, Legal, A4) and orientation.
- **The dialog's groups, in tab order:**
  - Printer: Save as PDF, then the platform's printers on macOS.
  - Copies, with Collate.
  - Pages to Print:
    - All, Current page, or Pages, which shows its box: `2-4, 7`;
    - All, odd or even pages in range;
    - Reverse pages.
  - Page Sizing & Handling: Fit, Actual size, Shrink oversized pages, or
    Custom Scale, which shows its percentage box.
  - Multiple Pages per Sheet: 1 to 16. Page order and borders appear once
    pages share a sheet.
  - Paper Size and Orientation, the same two Page Setup shows.
  - Comments & Forms, the four modes.
  - Print on Both Sides of Paper: off, or flip on the long or short edge.
  - Advanced: Print as Image.
- **The preview.** The shown sheet is drawn as an outline, each placed page
  as a numbered box where imposition put it. A line says "sheet 1 of 5,
  landscape, pages 1, 2", with Previous Sheet and Next Sheet. It is drawn
  from rectangles, not rasters, so it costs nothing on the UI thread
  whatever the resolution. This answers the review risk about rendering a
  1200 dpi sheet there.
- **Errors in words, in the dialog:**
  - a backwards range: "7-2 runs backwards; type 2-7, and use Reverse
    Pages to print it backwards";
  - a page past the end;
  - copies or scale outside 1 to 999.
- **Print.** Save as PDF asks where, suggesting "*name* (printed).pdf",
  writes the sheets, closes the dialog and says "Printed to …". A printer
  goes through the macOS backend; elsewhere the dialog lists none.
- **Encrypted documents:** Print as Image is shown ticked and locked, with
  the reason "Encrypted documents print only as images until M6", and the
  job prints as pictures.

## How it is built

- **`chrome/print_dialog/mod.rs`:** the model, with no GPUI in the logic.
  - `PrintSettings` holds every untyped choice.
  - `apply` and `apply_setup` change one choice.
  - `job(settings, setup, typed, printed, destinations)` builds the
    `onionskin_print::PrintJob` or says what is wrong.
  - `sheets(job, printed)` is `onionskin_print::impose`, and nothing else.
  - The dialog owns no imposition logic, which is the plan's first review
    risk.
- **`view.rs`:** one list of groups and controls (radio, checkbox, field)
  that both the drawing and the accessibility tree are built from. A
  control cannot be drawn without being operable from the keyboard,
  because click and keyboard dispatch the same `Activation::Print` value.
- **Page Setup:** `PageSetup` lives on the frame. Page Setup and the Print
  dialog both read and write it, so they cannot disagree about the paper
  (the plan's third review risk).
- **`crates/print` additions:**
  - `PageSelection` holds several ranges, printed in the order typed;
  - `parse_page_ranges` parses the Pages box and refuses backwards,
    out-of-range and non-numeric pieces with a message each.
- **Wiring:**
  - `MenuCommand::Print` (`file.print`, `cmd-p`) and
    `MenuCommand::PageSetup` (`file.page-setup`, `cmd-shift-p`);
  - the global bar button;
  - the canvas context menu's Print becomes a shell entry rather than a
    registry query;
  - `run_canvas_context_command` takes the window so it can open a
    dialog.

## Runs

- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support --lib tests::print`: 7 window tests pass.
  - `the_print_keystroke_opens_the_dialog_with_every_control_described`:
    - Cmd/Ctrl+P is pressed through `cx.simulate_keystrokes`, with the
      keystroke taken from the bindings;
    - every group is in the tree with its label;
    - every control has a label and a selected, toggled or value state;
    - Save as PDF is chosen;
    - the preview line is right.
  - `closing_the_dialog_takes_every_control_out_of_the_tree`: more than 20
    `print-` nodes before Cancel, none after.
  - `the_preview_is_the_printed_files_sheets`: with two-up, borders,
    reverse and duplex set, the preview's sheets are compared one for one
    with the PDF written through Save as PDF, on sheet count, each sheet's
    size and each sheet's XObject count.
  - `a_typed_range_prints_those_pages_and_a_backwards_one_is_refused`:
    `2-1` shows the backwards message and prints nothing (no save prompt);
    `2` previews page 2 only.
  - `page_setup_and_the_print_dialog_share_one_paper`:
    - Cmd/Ctrl+Shift+P opens Page Setup, where A4 and Portrait are chosen;
    - the context menu's Print opens the dialog with both selected, and a
      preview sheet A4 wide.
  - `an_encrypted_document_prints_as_images_and_the_box_says_why`.
  - `closing_…` and `the_preview_…` are also the plan's AccessKit and
    sheet-list assertions.
- `--lib print_dialog`: 8 model tests. They cover the defaults, every
  choice reaching the job, Page Setup holding only paper and orientation,
  the current page, each refusal in words, the encrypted override,
  preview-is-imposition, and destination names.
- `context_menu::tests::print_is_live_and_add_bookmark_goes_live_when_its_command_is_registered`:
  the plan's test, updated rather than deleted. Print is enabled; Add
  Bookmark's arm still asserts its own reason and goes live when its
  command registers.
- `cargo test -p onionskin-print`: 25 unit tests pass, including the
  range parser's three.
- **Full suites:**
  - `cargo test -p onionskin-app --features shell,shell-test-support
    --lib`: 706 pass and 6 fail, all six the known environmental set
    (snapshot turn, zoom raster, paint-nothing frame, three rollbacks);
  - integration tests pass;
  - `cargo test -p onionskin-app --no-default-features` passes.
- **Lint:**
  - `cargo clippy -p onionskin-app` with `--no-default-features` and with
    default features, `--all-targets -- -D warnings`: clean.
  - With `shell` on Linux, clippy stops on the pre-existing unused
    `a11y::Shared::record`, which is used only on macOS. With `-A
    dead_code` it is otherwise clean.
  - `cargo fmt --all --check`: clean.

## Mutations run

- Making the preview use default settings instead of the dialog's fails
  `the_preview_is_the_printed_files_sheets` (sheet 0: 1 placement against
  2) and the range test. This is the mutation the plan names.

## Not claimed

- **Coverage of the app half** is not measured: `cargo tarpaulin` cannot
  build `onionskin-app` (the `pulp` dependency fails its size assertion
  under tarpaulin's flags). `crates/print` is at 262 of 291 lines, 90%.
  The dialog's model is covered by the 8 unit tests and its frame half by
  the 7 window tests.
- **The macOS half** of the dialog's printer path (`print_to_printer`) is
  target-gated. It could not be type-checked from Linux, because the app's
  dependency graph builds C code (`ring`) that cross-checking cannot, and
  it has not run.
- **Summaries** in the print output (row 96) are not built.
