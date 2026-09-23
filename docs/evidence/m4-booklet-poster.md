# M4 verification: Booklet and Poster printing

Date: 2026-09-22 historical baseline; interval evidence added 2026-09-23 on
the macOS GPUI test host. No native print-driver or hosted-CI run is claimed.

## Rows

- **Historical promotion, 2026-09-22:** Booklet and Poster / tile were marked
  `implemented`. Both had been moved from M3 to M4 by the M3 plan's ruling B:
  imposition over P15's sheet model, added without reopening it.
- **Historical headline:** 156 planned / 27 partial / 80 out-of-scope, 140
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## Correction, 2026-09-23

The Sept 22 row promotion and headline are historical evidence. The current
parity inventory keeps both rows `partial` because these named behaviours are
not fully accepted across all targets:

- **Booklet:** physical-sheet **From / To** selection is implemented after
  complete padded composition. Native print-driver/default-retention and
  Binding dropdown, Tall binding variants, and auto-rotate-per-page remain
  missing; native print-driver/default-retention acceptance is deferred.
- **Poster:** custom scale and overlap entry is not offered. Acrobat's printed
  tile labels are also not drawn.

The revised headline is 156 planned / 29 partial / 80 out-of-scope, 138
implemented. This correction adds no runtime or native-print verification
claim. The Sept 22 test and coverage results below remain historical.

## What the user gets

The Print dialog's **Page Sizing & Handling** offers Size and Multiple (as
before), Booklet and Poster. The groups under it change with the choice,
and the preview and the printed file are the chosen handling's sheets.

- **Booklet.**
  - **Order.** Two pages to a landscape side, in saddle-stitch order: for
    eight pages the sides are 8|1, 2|7, 6|3, 4|5. Printed on both sides,
    folded and stapled on the fold, they read 1 to 8.
  - **Blanks.** A page count short of a multiple of four is padded with
    blanks at the end.
  - **Booklet subset:** Both sides, Front side only, or Back side only, for
    a printer that cannot print both sides.
  - **Binding:** Left, or Right, which mirrors each side for right-to-left
    documents.
  - **Page range.** The range and odd/even choices pick which pages go into
    the booklet.
  - **Sheets.** `Sheets from` and `To` select an inclusive physical-sheet
    interval after composition; invalid values are rejected at submit time.
- **Poster.**
  - **Tiles.** Each page is enlarged by the tile scale (150, 200, 300 or 400
    percent) and split into as many sheets as it takes. The tiles run left
    to right, top to bottom, page after page.
  - **Overlap.** None, 0.25 in or 0.5 in: neighbouring tiles repeat that
    much of the page for gluing.
  - **Cut marks.** They frame each tile's area in an 18-point margin.
  - **Clipping.** Each tile's page is clipped to the tile, so nothing prints
    into the margin.
  - **Paper.** Auto orientation turns the paper to the page's shape.

## How it is built

- `print::job::Handling` is `Pages`, `Booklet(Booklet)` or
  `Poster(Poster)`, on `PrintJob`. `impose` dispatches on it: the grid
  (`impose_pages`, unchanged), `booklet::impose_booklet` or
  `poster::impose_poster`.
- **The one clip in the model.** `sheet::Placement` gains `clip`, the part
  of the sheet the page may draw in; only a poster tile sets it.
  - The file backend writes it as `re W n` inside the placement's `q ... Q`,
    before the page's transform. The macOS backend prints the file
    backend's PDF, so it inherits the clip.
  - `Placement::visible` is the footprint cut to the clip, which the
    dialog's preview outlines.
- **Dialog.** `HandlingChoice` and the booklet and poster settings live in
  `PrintSettings`. The actions are `Handling`, `BookletSides`, `Binding`,
  `TileScale`, `Overlap` and `CutMarks`. `handling_groups` shows the groups
  the choice needs.
- **Physical interval.** `Booklet.sheets` filters the original padded sheet
  loop, preserving source selection, blank padding, side mode and binding.
  Comment-summary appendix jobs reset this document-specific interval.

## Physical-sheet interval evidence, 2026-09-23

- `cargo test --locked -p onionskin-print --lib --test file_backend
  booklet_sheet_range`: 8 passed, including exact side counts, transforms,
  blank padding, invalid-range rejection, and appendix reset.
- `cargo test --locked -p onionskin-app --features shell-test-support
  shell::chrome::print_dialog::tests`: 14 passed, including selected-source
  totals, Odd bounds, strict fields, Current revalidation and Pages/Poster isolation.
- `cargo test --locked -p onionskin-app --features shell-test-support
  shell::chrome::tabs::tests::print::booklet_sheet_range`: 5 passed, including
  actual Save-as-PDF output, pending chooser tab/source edit, field keyboard
  entry, invalid-to-fixed submit, and preview narrowing with drawn-window AX.
- `cargo clippy --locked -p onionskin-print -p onionskin-app
  --features onionskin-app/shell-test-support --all-targets -- -D warnings`
  and `cargo fmt --all -- --check`: passed.
- Native macOS print-window/driver checks remain deferred. Binding dropdown
  shape, Tall variants and auto-rotate-per-page remain missing from this row.
- Baseline-compatible control proof used the preserved worktree
  `/Users/cristi/Dropbox_Maestral/devel/onionskin-booklet-sheet-range-baseline`
  at `62b8830`, with only
  `booklet_sheet_range_publishes_physical_sheet_inputs` copied into the
  existing test file. The exact `cargo test --locked -vv -p onionskin-app
  --features shell-test-support --lib
  shell::chrome::tabs::tests::print::booklet_sheet_range_publishes_physical_sheet_inputs`
  compiled the baseline app crate and failed at runtime on the missing
  `print-booklet-from` control. Log:
  `/Users/cristi/Dropbox_Maestral/devel/onionskin-booklet-sheet-range-baseline/verification/booklet-sheet-range-20260923/baseline.log`.
- The same test passed against the frozen treatment source after forcing source
  compilation with package-scoped test profile overrides; the shared target had
  otherwise reused the baseline artifact. The final verbose treatment log
  contains the treatment `CARGO_MANIFEST_DIR` rustc invocation and 10 matching
  tests passed:
  `/Users/cristi/Dropbox_Maestral/devel/onionskin-booklet-sheet-range/verification/booklet-sheet-range-20260923/final-treatment.log`.
  This is control-presence evidence only, not a baseline saved-output claim.
- The pre-fix Tab regression (focus landed on `print-binding` instead of
  `print-booklet-to`) is preserved in session `35491`; the corrected test now
  covers forward Tab, reverse Shift-Tab, group entry/exit, actual field focus
  and keyboard entry.
- Final focus correction verification: print-dialog tests 14 passed, print-tab
  tests 21 passed, `a11y::focus::tests` 15 passed, strict clippy passed, and
  `cargo fmt --all -- --check` passed. The focused final treatment transcript
  is `verification/booklet-sheet-range-20260923/final-treatment.log`.

## Historical runs, 2026-09-22

- `cargo test -p onionskin-print --lib`: 34 pass, 9 of them new.
  - **Booklet (4):**
    - eight pages in saddle-stitch order on landscape Letter;
    - five pages padded to eight with the blanks at the back;
    - front only; back only with right binding; an empty selection;
    - every page fitting its half.
  - **Poster (4):**
    - full size with no margin is one tile;
    - 200% with no overlap is four tiles that show the page's four
      quarters exactly, top row first;
    - overlap 36 with cut marks is nine tiles, each framed and stepping 540
      points;
    - a landscape page prints on landscape tiles, and pages follow one
      another.
  - **Sheet (1):** a clipped placement shows only its clip.
- `cargo test -p onionskin-print --test file_backend`, 2 new:
  - a 200% poster writes four sheets, each drawing one page clipped by
    `0 0 612 792 re W n`;
  - an 8-page booklet writes four landscape sides of two pages.
- **App.**
  - `print_dialog::tests::booklet_and_poster_choices_reach_the_job_and_the_preview`:
    ten pages as a booklet are six sides; two pages as a 200% poster without
    overlap or marks are eight tiles.
  - Window test `choosing_booklet_shows_its_controls_and_prints_its_sides`:
    choosing Booklet puts the booklet and binding groups in the tree and
    takes Multiple out; the file printed from a two-page document is one
    sheet's two landscape sides.
- **Coverage.** `cargo tarpaulin -p onionskin-print`: 333 of 370 lines,
  90%, with `booklet.rs` 18/18 and `poster.rs` 25/27.
- **Lint.** `cargo clippy` for print and the app (shell features, with and
  without defaults): clean. `cargo fmt --all --check`: clean.
- **Full app suite:** all pass but the known environmental set.

## Mutations run

- Swapping the outside and inside pages of a booklet front fails three of
  the four booklet tests.
- Dropping the clip from the file backend fails the poster file test.

## Not claimed

- Acrobat's poster tile labels (the page and tile number printed in the
  margin) are not drawn.
- Native macOS print-window, printer-driver and screenshot acceptance remains
  deferred. The Booklet parity row remains partial for binding dropdown shape,
  Tall variants, auto-rotate-per-page and those native gaps.
- The interval proof is source-based and macOS-host GPUI-test-based; it does not claim
  Acrobat's native default-retention or printer-driver behavior.
