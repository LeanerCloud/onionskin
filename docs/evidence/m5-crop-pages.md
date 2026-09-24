# M5 verification: Crop Pages and Set Page Boxes

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:**
  - Edit a PDF > Crop pages;
  - Organize pages > Crop pages (from Organize);
  - Use print production > Set Page Boxes (media, crop, bleed, trim, art).
- **Also changed:** the thumbnails context menu row. Its Crop Pages entry,
  the last one waiting on M5, is live. The judgment row about that menu
  crossing milestones now says every entry has shipped.
- **Headline:** 150 planned / 30 partial / 80 out-of-scope, 143
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **The Crop Pages tool** (tool rail, `tools-edit`). Drag a rectangle on a
  page, then double-click inside it or press Enter: the page's crop box
  becomes the rectangle. A double-click outside starts a new rectangle, and
  Escape or switching tools drops it. The rectangle is in the page's own
  coordinates whatever its rotation. Anything past the media box is left
  out, because it is not part of the page.
- **Edit > Crop Pages…**, the thumbnails menu's **Crop Pages**, and the
  Organize grid's **Crop Pages…** all open one dialog. It acts on the pages
  chosen where it was opened: the page on screen, or the grid's selection.
  The dialog offers:
  - which box to set: CropBox, BleedBox, TrimBox or ArtBox;
  - four margins in points (top, bottom, left, right). They are measured in
    from the media box as the page is shown, so "top" is the top of the
    screen on a turned page, as in Acrobat;
  - **Set To Zero**;
  - **Remove White Margins**: each page is fitted to what it draws, measured
    page by page, and the margin fields are hidden;
  - **Change Page Size**: a width and height as shown. The new media box is
    centred on the old one so the content stays in the middle, and the crop
    box becomes the whole new page. The margins are then measured from the
    new media box;
  - **Pages**: the chosen pages, or all pages.
- The margins open on the first chosen page's crop box and the size on its
  media box, so pressing Crop at once changes nothing.
- Every crop is one undo step, named "Crop Pages", across all the pages it
  touched. The canvas redraws at the new size, and so do print and export,
  which draw the crop box.
- A crop that would leave a page less than a point either way is refused
  for every page, and nothing is written. The dialog stays open and says
  which page, for example "These margins would leave page 1 200.0 by -0.0
  points…". A margin or size that is not a number says which field.
- On a document that may not be edited, the tool and every entry are
  disabled with the document's reason. In a build without `tools-edit` they
  say "The Edit PDF plugin is not installed".

## How it works

- **core, `pages/boxes.rs`.**
  - `set_page_box(tx, pages, which, margins)` works out every page's box
    first, from the media box and rotation each page inherits (an indirect
    `/MediaBox` is resolved), then writes `/CropBox`, `/BleedBox`,
    `/TrimBox` or `/ArtBox` on each page. A refusal leaves every page as it
    was.
  - `set_media_size(tx, pages, width, height)` is Change Page Size.
  - `boxed`, `shown_margins` and `resized` are the pure geometry.
    `shown_margins` is `boxed`'s inverse, so the dialog can open on a page's
    existing box.
  - Two new errors: `PageBoxTooSmall { page, width, height }` and
    `InvalidMargin`.
- **tools-edit** (no longer a stub):
  - `crop_pages`, `crop_to_content` and `crop_to_rect`, each one
    `Document::edit_pages` step;
  - `white_margins`, which renders the page once at one pixel a point
    through the screen's renderer and reads `BaseRaster::content_bounds`,
    plus the existing crop's offset;
  - the registered command `edit.crop-pages` (crop the page on screen to its
    content);
  - the `CropTool`, which declares a new capability, `EditPages`.
    `EditPages` edits the document, so a protected document disables the
    tool through the shared requirement query.
- **app.**
  - `chrome/crop_dialog/`: the form as plain data (`CropForm`, `request`)
    and one row list that both the accessibility tree and the drawing read.
  - `tabs/crop.rs`: opening the dialog, and running the crop through
    `tools-edit`.
  - Six new number fields reach the focus ring and the tree.
  - The Edit menu entry is registry-backed, as Split Document is: live when
    the plugin registers `edit.crop-pages`.

## Runs

- `cargo test -p onionskin-core --lib pages::boxes`: 9 pass. They cover the
  unturned and turned margins, too-small crops, bad margins, media box
  reading, rotation normalizing, the `shown_margins` round trip at all four
  rotations, resizing and the box keys.
- `cargo test -p onionskin-core --test page_boxes`: 8 pass, each on a fresh
  parse of the saved bytes:
  - only the pages asked for are written;
  - inherited rotation and media box are honoured;
  - every key is written;
  - one page that cannot take a crop refuses the whole edit;
  - bad margins and pages are refused;
  - undo works;
  - Change Page Size centres the page, shows all of it, and refuses a size
    under a point;
  - `Document::page_geometry` renders at the new size.
- `cargo test -p onionskin-tools-edit`: 15 pass.
  - `tests/crop.rs` (9): registration and the encrypted disable; one undo
    step for several pages; a named refusal; white margins measured on an
    unturned page, a turned page and a blank page; crop to content stable
    from an existing crop; any box; size and crop as one step; encrypted
    refused.
  - `tests/crop_tool.rs` (6): Enter crops and undoes; double-click inside
    or outside; a turned page and a rectangle past the edge; click, Escape,
    deactivate and a drag onto another page crop nothing; a rectangle on no
    page is refused by name.
- `cargo test -p onionskin-app --features shell-test-support --lib crop`:
  19 pass, including 7 on a real window:
  - the Edit menu crops the page on screen, reopens showing the crop, Set
    To Zero, and one Undo;
  - all pages;
  - a bad margin and a refused crop keep the dialog open;
  - Remove White Margins fits `letter_marked` to exactly
    `[199 164 485 401]` and hides the fields from both the tree and the
    focus ring;
  - the thumbnails menu and the Organize grid open the dialog on their
    pages, with the fields described as number inputs;
  - Change Page Size opens on the page's size and resizes about the centre.
- **Whole-suite runs.**
  - Workspace without the app: 1076 passed, 0 failed.
  - App (`--features shell-test-support`): every test passes except the
    known environmental ones: 2 to 4 timing-sensitive canvas tests (the set
    varies from run to run), the 3 export rollback tests that fail when run
    as root, and, in one run, the timing-sensitive auto-scroll test, which
    passes on its own.
  - App integration tests, guarantees included: all pass.

## Coverage

`cargo tarpaulin` with optimisation off, so small functions keep their
lines:

| File | Lines covered |
| --- | --- |
| `plugins/tools-edit/src/crop.rs` | 49 of 49 |
| `plugins/tools-edit/src/crop_tool.rs` | 49 of 50 |
| `plugins/tools-edit/src/lib.rs` | 8 of 8 |
| `crates/core/src/pages/boxes.rs` | 110 of 112, from the tools-edit suite and the core unit tests together |

The two lines left in `boxes.rs` are `set_media_size`'s too-small refusal,
which `tests/page_boxes.rs` covers. That suite was not run under tarpaulin:
the full core suite segfaults under ptrace in this container. The app
dialog's code is covered by its unit and window tests; the app crate is not
run under tarpaulin.

## Mutations

Each was caught, then reverted.

- `shown_margins` unturning by the page's rotation instead of its opposite:
  `shown_margins_are_what_boxed_takes_back_to_the_same_box` fails.
- The crop tool cropping on a double-click anywhere, not only inside the
  rectangle: `a_double_click_inside_crops_and_outside_starts_over` fails.
- Change Page Size not resetting the crop box:
  `change_page_size_centres_the_media_box_and_shows_all_of_it` fails.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support` reports only the existing
`a11y::Shared::record` warning. `cargo fmt --all --check` is clean.

## Not claimed

- **Units.** Margins and sizes are in points. Acrobat also offers inches,
  millimetres and picas.
- **Acrobat's dialog details.**
  - Its crop tool opens the dialog on a double-click; this one crops
    directly and leaves the dialog to Edit > Crop Pages.
  - The page range is the chosen pages or all of them. The grid's selection
    gives any set; Acrobat's From/To fields and even/odd filter are not
    offered.
  - Change Page Size takes a width and height centred on the page. Acrobat's
    fixed-size list and X/Y offsets are not offered.
- **Rendering.** Only the crop box changes what is drawn. Bleed, trim and
  art boxes are written for prepress and for other applications.
