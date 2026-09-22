# M4 verification: Booklet and Poster printing

Date: 2026-09-22. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:** Booklet, and Poster / tile. Both were moved from M3
  to M4 by the M3 plan's ruling B: imposition over P15's sheet model, added
  without reopening it.
- **Headline:** 156 planned / 27 partial / 80 out-of-scope, 140
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

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

## Runs

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
- Booklet's "sheets from / to" range is not offered; the page range picks
  the pages instead.
