# M5 verification: header and footer, watermark, background, Bates numbering

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`** (Edit a PDF):
  - Header and footer: add, update, remove;
  - Watermark: add, update, remove;
  - Background: add, update, remove;
  - Bates numbering: add, remove, add to file names.
- **Headline:** 146 planned / 30 partial / 80 out-of-scope, 147
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

The Edit menu has **Watermark…**, **Background…**, **Header & Footer…** and
**Bates Numbering…**. Each opens one dialog on the document in front. It
applies to all pages, or to the chosen ones: the page on screen, or the
Organize grid's selection.

- **Header & Footer.**
  - Six lines: left, centre and right, at the top and the bottom. They may
    use `[page]`, `[pages]` and `[date]`.
  - A standard font, size and colour; four margins; a start number.
  - It opens with "Page [page] of [pages]" in the centre footer.
- **Watermark.** Either text (font, size, colour, several lines centred) or
  page 1 of a chosen PDF at a scale. Both take a rotation (45° by default),
  an opacity (50%), a horizontal and vertical alignment, and Appear Behind
  Page.
- **Background.** Either a colour filling the page, or a PDF page with a
  scale, rotation, opacity and alignment. A background is always behind the
  page's content.
- **Bates Numbering.**
  - Prefix, suffix, number of digits (6) and start number, in any of the six
    places, with its font, size, colour and margins.
  - **Also Number Other Files…** adds files that are numbered after this
    document, the numbers running on. Each is written as a new file beside
    its original: its name gets the text typed under "Add to file names"
    and, unless unticked, its first and last Bates numbers. For example,
    `exhibit.pdf` becomes `exhibit-bates_EX000003-EX000005.pdf`. Nothing is
    written unless every file numbers, and no file is overwritten.
- **Update and Remove.** When the document already has that kind of mark,
  the button reads **Update** and **Remove** is offered.
  - Update replaces the mark, and the dialog opens on the settings it was
    made with.
  - Remove takes that kind off every page and leaves the other kinds:
    removing Bates numbers keeps the header.
  - Bates numbering is added again rather than updated, and a second
    numbering replaces the first.
- Every change is one undo step, named "Add Watermark", "Update Header &
  Footer", "Remove Background" and so on. The notice bar says what was done,
  for example "Added the header and footer on 2 pages." or "Removed the
  watermark from 3 pages.".
- A document that may not be edited disables the four entries with its
  reason. A build without `tools-edit` says the plugin is not installed.

## How it works

- **core, `pages/marks/`.** A mark is written in three parts:
  - a **Form XObject** drawn in the page's *shown* space, whose `/Matrix`
    maps it onto the crop box at the page's rotation, so a header is at the
    top of a turned page as shown;
  - Acrobat's `/PieceInfo /ADBE_CompoundType /Private` name (`/Watermark`,
    `/Background` or `/HeaderFooter`) plus `/OnionskinMark` naming the exact
    kind;
  - a **content stream of its own** drawing the form inside an `/Artifact`
    marked-content sequence, so the structure tree stays valid.

  How the pieces are placed:
  - Marks drawn over the page follow its own content, which is first
    wrapped in a `q`/`Q` pair of guard streams, so the page's leftover
    graphics state cannot move the mark.
  - A background, or a watermark set behind, is put first.
  - The page's resources are copied onto the page before the form is named
    in them, so a dictionary other pages share is not changed.

  Removing and settings:
  - Removing recognises our streams by `/OnionskinMark`. It recognises
    Acrobat's by shape: a stream that only draws forms whose `/PieceInfo`
    names the kind.
  - It drops the form names only those streams used, and drops the guards
    once nothing is drawn over the page.
  - `/OnionskinSettings` on the form keeps what the dialog was set to, for
    Update to open on.
- **content.** `encode_win_ansi` and `standard_text_width` set a line in a
  standard font from its published widths, so text is centred and
  right-aligned without embedding a font.
- **tools-edit, `marks/`.** Lays out each kind and writes it:
  - `add_header_footer`, `add_bates`, `add_watermark`, `add_background`;
  - `remove_marks`, `marked_pages`, `saved_settings`;
  - `number_files`, which numbers other files and writes them all or none,
    each linked into place rather than overwriting.
- **app.** The dialog is `chrome/marks_dialog/`:
  - the form as plain data (`MarkForm`, `request`);
  - the settings kept for Update (`settings.rs`);
  - one row list read by both the accessibility tree and the drawing.

  `tabs/marks.rs` runs it. The dialog is compiled only with `tools-edit`;
  without it the entries say so.

## Runs

- `cargo test -p onionskin-content --lib standard`: 2 pass (WinAnsi
  encoding, published widths).
- `cargo test -p onionskin-core --lib pages::marks`: 3 pass (the shown
  space at all four rotations, the kinds' names, form naming).
- `cargo test -p onionskin-core --test page_marks`: 10 pass. Each reads a
  fresh parse or the renderer:
  - a watermark is at the shown bottom-left, over the page, between guards,
    with shared resources untouched;
  - a mark follows an inherited `/Rotate 270`;
  - a background is behind the page's text;
  - a watermark set behind is prepended;
  - Replace keeps one mark, and Remove restores the page and its resources;
  - each kind is removed on its own;
  - Acrobat's watermark shape is recognised and removed, while a stream that
    also paints is not taken for one;
  - pages out of range are refused;
  - undo works;
  - settings are kept for Update.
- `cargo test -p onionskin-tools-edit`: 35 pass, including 9 in
  `tests/marks.rs`:
  - header and footer text and position read back through text extraction,
    on every page, with one undo step;
  - Update and Remove, including saved settings;
  - a turned page's header in the shown top band;
  - Bates numbers running on and removed apart from the header;
  - a half-opaque text watermark drawn in pink over the middle;
  - a scaled PDF-page watermark placed from the top-left and behind;
  - a background colour behind the page's own drawing, updated and removed;
  - a protected document refusing every kind by name;
  - Bates across two files: names, running numbers, no overwrite, the
    original untouched, and a missing file named.
- `cargo test -p onionskin-app --features shell-test-support --lib marks`:
  every test passes. That covers the dialog's form, settings and rows, and
  4 tests on a real window:
  - Header & Footer is added, reopened on its settings, updated and
    removed;
  - a watermark needs its text and marks only the chosen page;
  - a background comes from a chosen file, and a file gone by the time Add
    is pressed is named;
  - Bates numbering covers this document and an added file, with the new
    file named as expected.
- **Whole-suite runs.**
  - Workspace without the app: 1115 passed, 0 failed.
  - App: 854 passed. The 6 failures are the known environmental ones: 3
    timing-sensitive canvas tests, and the 3 export rollback tests that fail
    when run as root.
  - App integration tests, guarantees included: all pass.
  - `cargo test -p onionskin-app --no-default-features --test
    kernel_emptiness`: 4 pass.

## Coverage

`cargo tarpaulin -p onionskin-tools-edit` with optimisation off, over the
marks code in content, core and tools-edit: 538 of 578 lines (93.1%).

| File | Lines covered |
| --- | --- |
| `content/font/standard.rs` | 14 of 14 |
| `core/pages/marks/mod.rs` | 117 of 120 |
| `core/pages/marks/contents.rs` | 51 of 58 |
| `core/pages/marks/recognise.rs` | 91 of 112 |
| `tools-edit/marks/*` | every file above 88% |

Most lines left in `recognise.rs` are the Acrobat-shape path, which
`crates/core/tests/page_marks.rs` covers. That suite was not run under
tarpaulin: the full core suite segfaults under ptrace in this container.

## Mutations

Each was caught, then reverted.

- The quarter-turn shown-space matrix given the three-quarter one:
  `the_shown_space_covers_the_crop_box_at_every_rotation` fails.
- Marks appended without guarding the page's content:
  `a_watermark_is_drawn_over_the_page_at_the_shown_bottom_left` and
  `replacing_keeps_one_mark_and_removing_leaves_the_page_as_it_was` fail.
- Headers placed at the bottom margin: `each_place_is_at_its_edge` fails.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support` and `cargo clippy -p onionskin-app
--no-default-features --features shell` report only the existing
`a11y::Shared::record` warning. `cargo fmt --all --check` is clean.

## Not claimed

- **Fonts.** Text is set in the standard fonts only (Helvetica, Times,
  Courier), in WinAnsiEncoding. A character outside it is drawn as `?`. No
  font is embedded.
- **Sources.** A watermark or background from an image file needs the image
  made into a PDF first (Create PDF From File). Only page 1 of a chosen PDF
  is used.
- **Options Acrobat has and this does not:**
  - Acrobat's page-number and date format lists: `[page]` is a plain number
    and `[date]` is `YYYY-MM-DD`;
  - even/odd page subsets;
  - offsets beyond alignment;
  - "shrink document to avoid overwriting".
- **Acrobat's own marks.** Update and Remove recognise the shape Acrobat
  writes, a stream of its own drawing a form tagged in `/PieceInfo`. Updating
  one of Acrobat's opens on the defaults, since its settings are Acrobat's
  XML, which is not read. A mark Acrobat has merged into the page's own
  content stream is not recognised.
- **Bates across files.** Copies go beside their originals; there is no
  separate output folder choice in the dialog.
