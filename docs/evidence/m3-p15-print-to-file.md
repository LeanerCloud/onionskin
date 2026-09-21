# M3 P15 verification: imposition and the print-to-file backend

Date: 2026-09-21. Linux x86-64, stable toolchain. No window, display or
printer is involved, and no macOS, Windows or hosted-CI run is claimed.

## Rows

No row flips to `implemented` here. P15 is the engine; the print dialog that
makes it reachable is P17, and this file's rule is that a row closed in code
but reachable only through a later package's dialog stays `planned`. The
seven rows the plan names for P15 carry a note pointing here:

- 87 Page range and subset.
- 88 Page sizing and handling.
- 89 Multiple pages per sheet (N-up).
- 93 Orientation.
- 94 Comments & Forms.
- 97 Print as image.
- 98 Print to file / print to PDF.

Ruling B is applied in the same commit: Booklet (90) and Poster / tile (91)
move to M4, and the `By milestone` headline becomes M3 98, M4 4.
`acrobat_parity_headline_matches_every_inventory_row` recounts and passes.
(The plan's "M3 97" predates a later M3 row; the recount is authoritative.)

## How it is built

- **`crates/print/src/job.rs`:** what a print is asked to do.
  - `PageSelection`: an inclusive range (it can run backwards), an
    all/even/odd subset counted in page numbers, and reverse order.
  - `Sizing`: Fit, Actual size, Shrink oversized pages, Custom scale.
  - `NUp`: 1, 2, 4, 6, 9 or 16 a sheet, Acrobat's four page orders, and
    borders.
  - Orientation (portrait, landscape, auto), paper size, duplex, the
    Comments & Forms filter, and Print as Image with its resolution.
- **`impose.rs`:** pure arithmetic from page sizes to `Vec<Sheet>`.
  - Fit fits to the paper, not a printer's printable area: the file
    backend has no hardware margins, and a platform backend with them
    passes a smaller paper.
  - Auto orientation turns the sheet when that makes a cell the shape of
    the first page, so two portrait pages go side by side on landscape.
  - Duplex with an odd sheet count adds a blank back, here rather than in a
    backend, so the file backend proves it.
  - Borders are sheet frames, drawn by the backend from imposition's
    rectangles.
- **`sheet.rs`:** `Placement { source, transform }` and `Sheet`. There is no
  `clip` field, since only poster/tile (M4) clips. A hand-built irregular
  sheet (a quarter-turned placement beside a half-size one) is expressible,
  which is ruling B's concession.
- **`backend/file.rs`:**
  - `print_to_file(doc, job)` takes the document as it stands, edits
    included, through `preview_bytes(job.comments)`. It imposes, then writes
    one page per sheet with `cos::write_new`.
  - **Vector pages:** each source page becomes one Form XObject, made by
    `core::pages::import_page_for_print`. That form draws the page form,
    then each annotation whose `/F` says print and not hidden, in its
    `/AP /N` appearance (the `/AS` state when `/N` is a state dictionary),
    placed by ISO 32000-1 12.5.5.
  - Each page is written once, however many sheets use it.
  - `/Rotate` is applied inside the placement, from the form's `/BBox`, so
    the transform is built from the crop box when there is one, and
    rotation happens before scaling.
  - **Print as Image:** each page is rendered at the job's resolution,
    composited on white, and placed as one Flate RGB image. Nothing of the
    source's own objects is written.
- **Encrypted sources:** printing without Print as Image is refused with
  `PrintError::Refused(Refusal::EncryptedSource)`. The message names M6 and
  says to turn on Print as Image.
- **`cos::Document::reachable_from(roots)`:** added, with
  `reachable_from_trailer` built on it. The file backend's object closure
  and Reduce File Size's garbage drop both use it.

## Runs

- `cargo test -p onionskin-print`: 15 unit tests and 14 integration tests
  pass.
  - The plan's named imposition tests:
    - `four_up_on_a_landscape_sheet_places_pages_left_to_right_then_down`
    - `shrink_oversized_leaves_a_page_that_fits_at_actual_size`
    - `custom_scale_of_fifty_percent_halves_both_axes`
    - `an_odd_page_count_in_duplex_leaves_the_last_back_blank`
    - `an_even_and_odd_subset_of_a_five_page_document_selects_one_three_five`
  - Also vertical order, auto orientation, borders, selection and the
    irregular sheet.
- `tests/file_backend.rs` reads every output back with `cos` and the
  content tokenizer: sheet count, sheet size, each `Do`'s XObject subtype,
  and the `cm`s around it multiplied out.
  - **Structural:**
    - one page;
    - four-up with borders, five pages onto two sheets in reading order;
    - custom 50% centred at (153, 198);
    - a 90-degree page shown 100 x 200;
    - duplex blank back;
    - a page used twice written once;
    - an empty selection refused.
  - **Pixels, once per mode:**
    - vector two-up: the black square renders where the matrix says, and
      the rest of the page is paper;
    - the rotated page: the square lands near the top of the sheet, not
      where the unrotated page had it;
    - Print as Image: the square is in the right place.
  - **Transparency:** a half-transparent square in a transparency group
    prints as exactly one image XObject, and its grey is within 8 levels
    of the source's direct render composited on white.
  - **Comments & Forms:** `hello.pdf` gets a blue rectangle and a stamp.
    Each of the four modes is asserted on the annotation appearances the
    printed form draws (2, 1, 0 and 0), and on whether blue renders where
    the rectangle's border is.
  - **Encrypted (`corpus/encrypted/r4-aes-128.pdf`):**
    - off: refused with the typed reason, naming M6;
    - on: the output is unencrypted, and every object in it is the
      catalog, the page tree, a page, a sheet content stream that draws
      only with `q`/`Q`/`cm`/`Do`, or an image;
    - the filtered path refuses the same way.
- `cargo test -p onionskin-core --lib print_form`: the 12.5.5 placement, and
  a page of four annotations where only the printing, unhidden one and the
  stateful one in its `/AS` state are drawn.
- `cargo test -p onionskin-cos --test write_new`: reachability keeps what
  the trailer reaches and not an orphan, and from a page reaches the tree
  but not the catalog.
- `cargo test -p onionskin-commands-core`: Reduce File Size is unchanged on
  the shared reachability.
- `cargo test -p onionskin-app --test guarantees acrobat_parity`: the
  headline matches the rows.
- `cargo clippy -p onionskin-print -p onionskin-core -p onionskin-cos
  -p onionskin-commands-core --all-targets -- -D warnings` and
  `cargo fmt --all --check` are clean.
- **Coverage** (`cargo tarpaulin -p onionskin-print`): 236 of 251 lines in
  `crates/print/src`, 94%. The uncovered lines are error `Display` arms and
  the reversed N-up orders.

## Mutations run

- Transposing the Horizontal N-up order fails the four-up test.
- Identity sizing for Custom and Shrink fails both sizing tests.
- Dropping the page-to-form matrix fails the rotated test and both Print as
  Image tests.
- Removing the hidden check in `printed_appearances` fails the core
  flags test.

## Not claimed

- The encrypted fixture is in `corpus/encrypted`, which is committed. No
  `external/` corpus file is used, so P1c's fetch step is not involved.
- There is no printer, dialog or platform backend: those are P16 and P17.
- No `/OC` on annotations is honoured. M3 authors none, and print filters
  through `/F` as the plan says.
- Form fields are not printed specially: M3 authors none, and Form Fields
  Only hides every markup.
