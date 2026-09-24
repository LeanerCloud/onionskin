# M5 verification: Fill & Sign

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`** (Toolset: Fill & Sign):
  - Add text;
  - Add checkmark / cross / dot;
  - Add circle / line;
  - Sign yourself: create signature (type, draw, image);
  - Sign yourself: add initials;
  - Save and reuse a signature locally.
- **Also changed:** the quick action toolbar row. Fill Text Fields and Add
  Sign are live in a build with `tools-fill-sign`. The row was already
  `implemented`.
- **Headline:** 136 planned / 30 partial / 80 out-of-scope, 157
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Seven tools on the rail**, in one Fill & Sign group:
  - **Add Text** types where the page is clicked: a borderless typewriter
    box, with the same in-place field the Typewriter opens. The page needs
    no form fields.
  - **Checkmark, Cross, Dot** each put a 12-point mark where clicked.
  - **Circle, Line** are dragged to size, with a preview while dragging. A
    click gives a 40 by 20 point circle or a 60 point line.
  - **Sign Yourself** places the saved signature or initials where clicked.
    It lists the kinds saved, so it can be switched between them.
- **Edit > Add Signature…** and **Add Initials…** open one dialog, made
  three ways:
  - **Type**: a name, set in Times Italic;
  - **Draw**: a pad drawn on with the pointer, with Clear;
  - **Image**: an image (PNG, JPEG, TIFF) or the first page of a PDF.

  **Save** keeps it and chooses the Sign tool, ready to click. Once one is
  saved, **Clear Saved Signature** (or Initials) forgets it. Something
  missing (no name, an empty pad, no file, a file that cannot be read) is
  said in the dialog, which stays open.
- **Saved locally.** The signature and the initials are one-page PDFs in the
  app's data folder. Every document's Sign tool offers them, and nothing
  leaves the computer.
- **Placing.** A signature is at most 150 points wide and initials 60,
  scaled down to fit and centred on the click.
- Everything placed is an annotation, as Acrobat writes it. It is one undo
  step, and it moves, deletes and shows in the Comments pane like a
  comment. A document that may not be edited is left as it was, and the
  menu entries are disabled with its reason. A build without the plugin
  says "The Fill & Sign plugin is not installed".

## How it works

- **`plugins/tools-fill-sign`** (no longer a stub):
  - `text.rs`: a FreeText annotation with `/IT /FreeTextTypeWriter`, border
    0, and the subject "Fill & Sign Text". The tool declares `takes_text`,
    so the shell opens its field on the box.
  - `symbols.rs`: each mark is a stamp whose appearance is a small drawing.
  - `shapes.rs`: Circle and Line annotations. A line's box is padded so its
    stroke is not clipped.
  - `gesture.rs`: tells a click from a drag by screen distance, shared by
    the tools.
  - `signature.rs`: `typed` and `drawn` make a one-page PDF the size of the
    ink. `SignatureLibrary` keeps `signatures/signature.pdf` and
    `initials.pdf` under the data folder. `save` checks the page first and
    writes through a partial file and a rename.
  - `sign_tool.rs`: reads the library from the tool environment's data
    folder, and places a stamp whose appearance is the saved page, as a
    custom stamp is placed.
- **app.**
  - `chrome/signature_dialog/`: the form as plain data (`SignatureForm`,
    `request`), and one row list read by both the accessibility tree and
    the drawing. The pad is described to a screen reader with its number of
    strokes; typing and an image are the keyboard ways to make one.
  - `tabs/signature.rs`: opening the dialog, the pad's pointer events,
    reading an image through the installed importer, saving, Clear Saved,
    and choosing the Sign tool.
  - Rail glyphs for the seven tools; the Edit menu's two entries.
  - Both are compiled only with `tools-fill-sign`.

## Runs

- `cargo test -p onionskin-tools-fill-sign`: 10 pass.
  - Unit (3): a typed page as wide as the name; a drawing's bounds turned
    upright; what is not a page refused by name.
  - `tests/tools.rs` (7), on a Letter page, reading annotations back and
    the rendered page:
    - Add Text's borderless typewriter box at the click, one undo step;
    - each mark drawn at the click and nowhere else;
    - circle and line dragged, and clicked to their usual size, with the
      drag preview;
    - the library keeping, replacing and forgetting each kind;
    - Sign placing the chosen saved page, scaled to fit each kind;
    - registration in one group;
    - a protected document left as it was.
- `cargo test -p onionskin-app --features shell-test-support --lib
  signature`: every test passes. That covers the form, the pad (strokes
  held to its edges, a press off it drawing nothing), the rows, and 5 tests
  on a real window:
  - a typed signature: the empty-name error, saved to the data folder, the
    Sign tool chosen, then a click placing it;
  - drawn initials: the empty-pad error, pad events turned into strokes,
    saved, reopened with Clear Saved Initials, cleared;
  - an image and a PDF made into a signature, and an unreadable file named;
  - no data folder: saving and clearing say so;
  - the quick actions choosing Sign and Add Text, and Add Text opening its
    field in place.
- **Whole-suite runs.**
  - Workspace without the app: 1138 passed, 0 failed.
  - App: 885 passed. The 6 failures are the known environmental ones: 3
    timing-sensitive canvas tests, and the 3 export rollback tests that fail
    when run as root.
  - App integration tests, guarantees included: all pass.
  - `cargo test -p onionskin-app --no-default-features --test
    kernel_emptiness`: 4 pass.

## Coverage

`cargo tarpaulin -p onionskin-tools-fill-sign` with optimisation off: 364
of 378 lines (96.3%).

| File | Lines covered |
| --- | --- |
| `gesture.rs` | 20 of 21 |
| `lib.rs` | 18 of 18 |
| `shapes.rs` | 61 of 65 |
| `sign_tool.rs` | 54 of 58 |
| `signature.rs` | 129 of 134 |
| `symbols.rs` | 56 of 56 |
| `text.rs` | 26 of 26 |

The lines left are `Default` constructors and I/O error paths. The app
dialog is covered by its unit and window tests; the app crate is not run
under tarpaulin.

## Mutations

Each was caught, then reverted.

- The Sign tool placing the saved page unscaled:
  `sign_places_the_chosen_saved_page_scaled_to_fit` fails.
- Add Text writing a bordered box:
  `add_text_places_a_borderless_typewriter_box_at_the_click` fails.
- A drag never told from a click:
  `circle_and_line_are_dragged_or_clicked_to_their_usual_size` fails.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support`, and `cargo clippy -p onionskin-app
--no-default-features` with `shell` alone and with `shell,tools-fill-sign`,
report only the existing `a11y::Shared::record` warning. `cargo fmt --all
--check` is clean.

## Not claimed

- **Not a cryptographic signature.** Sign places an appearance. Signing
  with a certificate is M6, `tools-protect`.
- **Acrobat's details.**
  - A typed signature is set in one font; Acrobat offers a choice of
    styles.
  - Acrobat's Fill & Sign text boxes have their own font and spacing
    controls, and its checkmark and shapes resize with handles. Here text
    takes the default comment text style, and a mark or shape is resized as a
    comment is.
  - Acrobat's Sign opens the create dialog when nothing is saved. Here the
    tool's hint points to Edit > Add Signature.
  - Signatures sync through an Adobe account there; that is out of scope.
- **Filling form fields.** Interactive forms are the forms package, still
  planned.
