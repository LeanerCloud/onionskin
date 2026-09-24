# M5 verification: editing images

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:** Replace image; Add image; Extract / save image.
- **To `partial`:** Edit images and objects (move, resize, rotate, flip,
  crop, align). Images move, resize, turn and flip; crop, align and vector
  objects are missing.
- **Headline:** 107 planned / 39 partial / 80 out-of-scope, 177
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **The Edit Image tool** (tool rail, `⧉`).
  - A click selects the topmost image under it, outlined; a click on
    nothing lets the selection go, and so do Escape and another tool.
  - A drag on the selected image moves it. A drag on one of its corners
    resizes it about the opposite corner, keeping its proportions. The
    outline follows the pointer until release.
  - Edit > Delete takes the selected image off its page.
- **Edit menu, on the selected image:**
  - **Rotate Image Clockwise** and **Rotate Image Counterclockwise** turn it
    a quarter about its centre.
  - **Flip Image Horizontal** and **Flip Image Vertical** mirror it about
    its centre.
  - **Replace Image…** asks for a PNG, JPEG or TIFF, or a PDF (its first
    page), and draws it fitted and centred in the image's frame.
  - **Save Image As…** writes the image as Export All Images does: a JPEG
    as its bytes, gray and RGB as PNG, CMYK as TIFF. The name suggested is
    the one Export All Images would give it.
  - With nothing selected, each entry says to click an image with the Edit
    Image tool. On a protected document the edits are disabled with its
    reason, and Save Image As with the read-out refusal.
- **The Add Image tool** (tool rail, `⊕`). Choosing it asks for a picture,
  as Replace Image does. A click places the picture at its own size, its
  top left at the click; a drag places it fitted and centred in the
  rectangle. The new image is selected.
- Every change is one undo step: Move Image, Resize Image, the four turns
  and flips, Replace Image, Delete Image, Add Image. The selection follows
  the image through each.

## How it works

- **content, `placements.rs`.** `page_images` walks a page's content, and
  the forms it draws, and records each image `Do`: the image, its resource
  name, the CTM it was drawn with, and where the operator sits in its
  stream (the stream's object and the decoded byte range). `Matrix::inverse`
  is new.
- **core, `image_edit.rs`.** Every edit rewrites the one `Do`, in the
  stream it is in:
  - move, resize, turn and flip write `q M cm /Im Do Q`, where `M` carries
    the page-space change into the image's own space
    (`ctm · change · ctm⁻¹`), so nothing else the stream draws moves;
  - replace imports the picture's first page as a form XObject, names it
    in the stream's resources (a form's own, or the page's), and draws it
    fitted in the old image's unit square;
  - delete writes nothing where the `Do` was.

  The image XObject is never changed, so another placement of the same
  image stays. A placement whose bytes are no longer where it was found is
  refused, never guessed at. `add_image` appends a guarded content stream
  after the page's own. `selection.rs` gains `ImageSelection`: a selection
  is now a region, text or an image, never two.
- **plugin-api.** `ToolCapability::EditImages` marks the tool the menu's
  image entries need; `PlacesImage` marks a tool the shell hands a
  one-page PDF. `command_ids` names the four turn and flip commands.
- **tools-edit.** `images.rs` holds the selection-level edits, each
  re-selecting the image by its place in drawing order afterwards.
  `image_tool.rs` holds `EditImageTool` and `AddImageTool`. The plugin
  registers the four commands.
- **codecs-common.** `extract_image` writes one image XObject, sharing the
  naming and format choice with `extract_images`.
- **app.**
  - `chrome/image_commands.rs`: `ImageCommand`, whose four turns and flips
    are registry-backed menu entries (live only when `tools-edit` registered
    them) and whose Replace and Save As are the shell's.
  - `tabs/images.rs`: Replace Image, Save Image As, and `tool_file`, which
    makes an image file picked for a `PlacesImage` tool into a PDF under the
    data folder's `placed-images/` before the tool is given it.

## Runs

- `cargo test -p onionskin-content --test placements`: 2 pass: every image
  found where it is drawn (in the page's content, and twice through a form
  drawn twice, one turned), a flat image covering nothing, an unknown name
  drawing nothing, and a matrix undone by its inverse.
- `cargo test -p onionskin-core --test image_edit`: 4 pass:
  - moved, resized, turned and flipped, the other placement untouched, and
    one inside a form moved inside it;
  - replaced in its frame on the page and inside a form, and removed;
  - added after the page, fitted, then a second under a name of its own;
  - refusals for a placement that cannot be located, moved since it was
    found, or drawn flat.
- `cargo test -p onionskin-tools-edit`: every test passes, 6 of them in
  `tests/images.rs`: the selection-level edits with undo; adding; the Edit
  Image tool's select, move and corner resize; Escape and deactivation;
  the Add Image tool's click and drag; the registered commands.
- `cargo test -p onionskin-codecs-common --test images`: 13 pass, one new:
  a single image comes out with the same name and bytes as Export All
  Images gives it, and a non-image or missing object is refused.
- `cargo test -p onionskin-app --features shell-test-support --lib`, on a
  real window (`tests::images`, 4 tests):
  - the image entries live; Rotate refused with nothing selected, then
    turning a 100 by 50 image to 50 by 100 and flipping it in place, the
    selection kept;
  - Replace Image refused with nothing selected, then a 4 by 4 PNG fitted
    as a 50 by 50 square, and a missing file named;
  - Save Image As through the simulated save prompt, decoded back as a 100
    by 50 PNG, and a folder that does not exist named;
  - Add Image given a PNG (made into `placed-images/logo.pdf`), a PDF (as
    it is), and a text file (refused, the picture kept).
- **Whole-suite runs.**
  - App: the new tests pass. The failures are the known environmental
    ones: the timing-sensitive canvas tests, and the 3 export rollback
    tests that fail when run as root.
  - App integration tests, guarantees included: 41 pass, 3 ignored.

## Coverage

`cargo tarpaulin` with optimisation off.

| File | Lines covered |
| --- | --- |
| `tools-edit/image_tool.rs` | 131 of 135 |
| `tools-edit/images.rs` | 75 of 77 |
| `core/image_edit.rs` | 138 of 144 |
| `core/selection.rs` | 23 of 23 |

That is 367 of 379 lines (96.8%). The lines left are error arms: a
stream that is gone, and a stream that does not decode. The app crate is
not run under tarpaulin; its image code is covered by the window tests
above.

## Mutations

Each was caught, then reverted.

- Rotate Image Clockwise turning anticlockwise:
  `the_selected_image_is_turned_flipped_replaced_and_deleted` and
  `the_edit_menu_turns_and_flips_through_the_registry` fail.
- The change applied outside the image's space rather than carried into
  it: `an_image_is_moved_resized_turned_and_flipped_where_it_is` fails.
- An image file handed to Add Image without being made a PDF:
  `the_add_image_tool_is_handed_a_pdf_of_the_picked_picture` fails.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support`, and the app built with
`--no-default-features` and `shell`, `shell,tools-edit` and
`shell,codecs-common`, report only the existing `a11y::Shared::record`
warning. `cargo fmt --all --check` is clean.

## Not claimed

- **Crop, align, arrange, and vector objects.** Acrobat's object editing
  also crops an image, aligns several, changes stacking order and edits
  paths. None of it is offered.
- **Inline images.** An image drawn inline (`BI` … `EI`) is not found, so
  it cannot be selected.
- **A drag across pages.** A move or resize stays on the page it started
  on.
- **Rotation by any angle.** Turns are quarter turns; Acrobat's free
  rotation handle is not offered.
