# M3 P21 verification: the Organize Pages grid

Date: 2026-09-21. Linux x86-64, stable toolchain, GPUI's test platform. No
macOS, Windows or hosted-CI run is claimed.

## Rows

- **To `implemented`:**
  - 50 Page thumbnail zoom and multi-select in the Organize grid;
  - the M2 Page thumbnails pane context-menu row, where every page entry
    now runs and only Crop Pages stays disabled on its M5 reason;
  - Extract pages;
  - Replace pages. Both were closed in code by P11 and waited on this
    package's grid.
- **Stays `planned`:** Copy or move pages between open documents. The grid
  reorders within one document; dragging between two documents is not
  built.
- **Headline:** 168 planned / 25 partial / 80 out-of-scope, 130
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Opening it.** Edit > Organize Pages puts a grid of the document's
  pages in place of the page, with the page on screen chosen. Choosing it
  again puts the page back.
- **Choosing pages:**
  - a click chooses one page;
  - Shift-click chooses the range from the last plain click;
  - Cmd/Ctrl-click adds or removes one;
  - dragging between pages draws a marquee that chooses what it touches,
    and with Shift or Cmd it adds to what was chosen;
  - a plain click on a page inside a larger selection, without dragging,
    narrows the selection to it.
- **Dragging chosen pages** shows where they will go (a bar in the gap) and
  moves them there on release: one reorder and one undo step. Released
  outside the grid, nothing moves.
- **The toolbar**, acting on the chosen pages:
  - Rotate Counterclockwise and Rotate Clockwise;
  - Delete;
  - Insert Blank Page, after the last chosen page;
  - Insert From File…, Replace… and Extract…, which asks where and then
    opens the extracted pages;
  - Select All;
  - Smaller and Larger Thumbnails, shared with the pane;
  - Close.
- **Refusals.** The editing buttons are disabled with the document's own
  reason when it may not be edited (an encrypted document), and with "The
  Organize Pages plugin is not installed" in a build without it.
- **Selection after edits.** After an Undo that takes pages away, the
  selection keeps to pages that exist and the grid says so, for example "1
  selected page is no longer in the document". After Delete, the page that
  took the first deleted page's place is chosen.
- **The thumbnails pane's menu** runs Insert Pages…, Extract Pages…,
  Replace Pages…, Delete Pages and Rotate Pages on the page on screen, or on
  the grid's selection while it is open. It also offers:
  - **Page Properties**, a read-only list of page number, size in points
    and inches, rotation, media box and crop box;
  - **Embed All Page Thumbnails** and **Remove All Page Thumbnails**, each
    one undo step;
  - **Crop Pages**, still disabled with its M5 reason.

## How it is built

- **One thumbnail cache (the review risk about duplicating
  `ThumbnailsState`).** The grid draws from, and asks through, the pane's
  `ThumbnailsState`.
  - The state now holds two bands, the pane's and the grid's.
  - `request_band` asks for the missing pages of both, deduplicated.
  - Eviction keeps the pages nearest either band.
  - There is no second thumbnail path, so the pane's poll-rearm and
    stale-size fixes (APP-004, APP-012) cover the grid too.
  - An edit (a new edit epoch) now drops the cached pictures, because a
    page index may name a different page. The grid and the page therefore
    agree on order after a reorder with no refresh.
- **Arithmetic, not layout.** `organize::GridLayout` places fixed-size
  cells: the pane's row height, 12-point gaps. It answers:
  - the page under a point, and where a drop lands (before a cell's left
    half, after its right half, at the end below the last row);
  - what a marquee touches;
  - which rows are on screen, plus one row of slack either side.
  - The grid's drawn bounds are recorded at prepaint for the pointer
    arithmetic between frames.
- **A drag reorders once.** `OrganizeState::release` returns at most one
  `Reorder`, and only when the pointer moved and the drop changes the
  order. Nothing is applied while the pointer moves, so a cancelled drag
  (released outside) cannot leave a partial reorder.
- **Every edit is `tools-organize`'s.** The functions are called with an
  explicit page list through the new `CanvasModel::edit_pages`, which
  rebuilds the layout the way a registered command does.
- **`core::pages::{embed_thumbnails, remove_thumbnails}`:**
  - embedding renders each page to at most 96 pixels on its longer side and
    stores it as `/Thumb`, a Flate RGB image composited on white;
  - removing strips `/Thumb` from every page;
  - each is one edit.

## Runs

- `cargo test -p onionskin-core --test thumbnails`: 2 pass.
  - Embedding gives both pages of `two-page.pdf` a `/Thumb` no larger than
    96 pixels whose decoded samples are width x height x 3 and include
    ink. One Undo removes both.
  - Removing counts 0 and then 2.
- `--lib pages::thumbs`: the image stream's fields and samples.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`
  covering the grid, organize and thumbnails tests: 18 pass. The model
  tests (`shell::organize::tests`, 9) cover:
  - cells found again under the pointer, and gaps and past-the-end finding
    nothing;
  - drop slots, including below the last row;
  - the visible band (a 1000-page grid scrolled to row 25 asks for rows 24
    to 29);
  - click, Shift and Cmd rules;
  - a marquee, plain and additive, and cancelled;
  - a drag over 19 intermediate positions giving one `Reorder` on release,
    and a cancelled one giving none;
  - a drop that changes nothing, which is not a reorder;
  - clamping after an undo, with its message;
  - window-to-grid coordinates and scroll limits.
- The window tests (`tabs::tests::page_grid`, 7):
  - `organize_pages_puts_a_described_grid_in_the_pages_place`:
    - the document node leaves the tree and the grid's pages are listed
      "Page 1" to "Page 6" with the current page selected;
    - the toolbar is live, or says the plugin is missing without it;
    - choosing again puts the document back.
  - `click_shift_click_and_cmd_click_select_on_the_grid`: through the
    frame's pointer handlers at window coordinates computed from the drawn
    bounds, including a marquee across two rows.
  - `a_drag_is_one_reorder_and_one_undo_step`: 30 intermediate positions,
    then release after page 3. The order becomes 2, 3, 1, 4, 5, 6, read
    back from the pages' text. One Undo restores 1 to 6 and leaves no
    further undo step.
  - `a_drag_released_outside_changes_nothing`.
  - `the_toolbar_edits_the_selection_and_an_undo_clamps_it_loudly`:
    - Insert Blank Page chooses the new page;
    - Undo clamps the selection with its message;
    - Delete removes page 2 and chooses the page after.
  - `a_thousand_page_grid_asks_for_a_screenful`: the grid's band equals
    `GridLayout::visible` for its drawn size and is under 100 pages of
    1000. The pages are asked for through the pane's cache.
  - `the_thumbnail_menus_page_entries_run_and_crop_still_waits`:
    - through the open menu in the tree, every page entry is live and Crop
      Pages is disabled naming M5;
    - Rotate and Delete run on the page on screen, and Embed writes
      `/Thumb`;
    - Page Properties opens with "Page: 1 of 2".
- `panes::thumbnails::tests::page_entries_follow_the_documents_edit_refusal_and_crop_waits_on_m5`
  replaces the M2 test that asserted every entry was disabled on an M3
  reason. It proves the mechanism: live without a refusal, refused with
  the document's reason, and Crop alone waiting on M5.
- `global_bar` Edit menu list updated for Organize Pages.
- **Full suites.**
  - `cargo test -p onionskin-app --features shell,shell-test-support
    --lib`: 738 pass and 4 fail, the known environmental set.
  - `--no-default-features --features shell,shell-test-support --lib`: the
    same known failures only.
  - `cargo test -p onionskin-core`: passes.
- **Lint:**
  - `cargo clippy -p onionskin-app`, with `--no-default-features` and with
    default features, `--all-targets -- -D warnings`: clean.
  - With `shell` it is clean apart from the pre-existing macOS-only
    `a11y::Shared::record` dead-code warning.
  - `cargo clippy -p onionskin-core --all-targets -- -D warnings` and
    `cargo fmt --all --check`: clean.

## Mutations run

- Reordering at every intermediate drag position fails both drag tests:
  the one-undo test (undo no longer restores the order) and the
  released-outside test. This is the plan's mutation.

## Not claimed

- The pointer handlers are driven through the frame at computed window
  coordinates, not through GPUI's simulated mouse events. The drawn bounds
  those coordinates come from are the real prepaint bounds.
- Keyboard selection in the grid is the accessibility tree's: each page
  item's activation chooses that page. There are no arrow-key moves yet.
- Coverage of the app half is not measured (`cargo tarpaulin` cannot build
  `onionskin-app`).
