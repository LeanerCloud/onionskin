# M5 verification: redaction, and removing hidden information

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`** (Toolset: Redact a PDF):
  - Mark text for redaction;
  - Mark images / regions for redaction;
  - Mark whole pages for redaction;
  - Find text and redact (search and redact, including patterns);
  - Redaction code sets and the Redaction Code Editor;
  - Apply redactions;
  - Sanitize document / remove hidden information.
- **To `partial`:** Redaction properties. Overlay text is set in Helvetica
  only, and fill opacity is not offered.
- **Also changed:** the page canvas context menu row. Its Redact Text is
  live. The row stays `partial`: Copy With Formatting and Edit Text are
  still disabled.
- **Guarantee test 3 runs.** It was an ignored stub; `guarantees.rs` now
  enforces `plugins/redact/tests/guarantee.rs`.
- **Headline:** 128 planned / 31 partial / 80 out-of-scope, 164
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Marking.** Nothing is removed until the marks are applied. Each mark is
  an ordinary undoable edit, outlined in red. Marks are made:
  - with the **Redact Text & Images** tool: a drag that starts on text marks
    the text it selects, and a drag that starts anywhere else marks a
    rectangle;
  - with the canvas menu's **Redact Text**, over the selection. With
    nothing selected, it chooses the tool;
  - with **Edit > Mark Pages for Redaction…**, over page numbers and ranges
    such as `1-3, 5`;
  - with **Edit > Find Text & Redact…**. It looks for words or a phrase
    (whole words, case sensitive) or a pattern: phone numbers, e-mail
    addresses, credit card numbers (Luhn checked), social security numbers,
    dates. Every hit is listed, checked, and can be unchecked before
    **Mark Checked for Redaction**.
- **Redaction Properties.**
  - A mark has a fill (black, white, red, gray, or no fill) and an outline
    colour.
  - Its overlay text has a colour, a size (empty fits the text to the area),
    an alignment, and a repeat.
  - From the Edit menu, the dialog sets the look of new marks, kept in the
    preferences. Clicking a mark with the tool opens it on that mark, with
    **Remove Mark**.
- **Code sets.** The U.S. FOIA and U.S. Privacy Act exemptions are built
  in.
  - **Use *code* as Overlay Text** writes the chosen code over the mark.
  - A set of the user's own is saved from a name and a comma-separated list.
    It can be renamed, removed, imported from a text file and exported to
    one.
- **Edit > Apply Redactions…** says what it will do and asks where to save.
  - It writes a new file, suggested as `name_Redacted.pdf`, and opens it in
    a tab. The open document is not changed.
  - Under each mark it removes text, vector drawings, images (painted out),
    inline images, forms and annotations, then fills the area and writes the
    overlay text.
  - **Also remove hidden information** adds the sweep below.
  - The notice counts what went and says the check found nothing left. When
    the check fails, the dialog stays open, says why, and writes nothing.
- **Edit > Remove Hidden Information…** removes, without needing any mark
  (marks there are applied too):
  - metadata, document scripts, and actions that run something;
  - attachments, comments and hidden layers;
  - content outside the crop box;
  - private application data and thumbnails.

  Links and form fields stay.
- The marking entries are disabled on a document that may not be edited.
  Apply and Remove Hidden Information are disabled on one whose content may
  not be read out. A build without the plugin says "The Redact plugin is not
  installed".

## How it works

- **content, `redact`.** Redaction is a mode of the extraction interpreter,
  so a glyph goes exactly where extraction and search place it.
  - **Glyphs** at least a fifth covered by an area go. The operator is
    rewritten as a `TJ` array stepping over them, so the rest of the line
    stays put. `'` and `"` keep their line move and spacing. A run where a
    removed glyph has no width loses every glyph.
  - **Paths** wholly inside an area go, keeping any clip.
  - **Inline images** touching an area go.
  - **Images, forms and soft-mask groups** that need changing are renamed
    to copies the caller writes.
  - A **marked-content sequence** that lost a glyph loses `/ActualText`,
    `/Alt` and `/E`.
  - Text with **no font** starting in an area, and an **unreadable form**
    over one, go whole.
  - Given **hidden layers**, everything drawn in them goes and their
    sequences are not written; state operators stay.
- **core.**
  - `redactions`: `/Redact` annotations with `/QuadPoints`, `/IC`, `/OC`,
    `/OverlayText`, `/DA`, `/Q` and `/Repeat`, drawn as their outline.
  - `Document::request_redaction_properties` carries the tool's click to
    the shell.
  - `images::decode_image` hands back pixels.
- **plugins/redact.**
  - `apply` gathers every object the document reaches, rewrites each marked
    page and writes a new file of only what the trailer still reaches, with
    a dangling reference written as `null`. Resources a rewrite stopped
    naming are pruned, so what it replaced is not kept by name.
  - `scrub` decodes an image (JPEG through `image`), paints the covered
    pixels through its `/Decode`, and makes its soft mask opaque there. An
    image it cannot decode is replaced by a blank form where it was drawn.
  - `sanitize` holds the hidden-information sweep.
  - `verify` reads the new file back:
    - no glyph in an area but the overlay's;
    - each image over an area painted out, found by running the redaction
      again;
    - no inline image and no mark left;
    - the old content streams and replaced objects nowhere in the bytes
      unless still drawn;
    - after the sweep, none of what it removes.
  - `mark`, `find`, `codes`, `look` (the preference form of a look) and
    `RedactTool` make up the rest.
- **plugin-api.** `ToolCapability::Redact`. `ToolEnvironment::redaction`
  carries the default look, as whole numbers so the preferences file
  round-trips exactly.
- **app.**
  - `chrome/redact_dialog/` is one dialog with four panels (Properties,
    Find, Pages, Apply), each a row list read by both the accessibility tree
    and the drawing.
  - `tabs/redact.rs` runs the dialog, the menu entries and Redact Text.
  - `preferences/redaction.rs` keeps the default look.

## Runs

- `cargo test -p onionskin-content --test redact`: 12 pass. They cover:
  - glyphs removed in `Tj`, `TJ`, `'` and `"`, with the next word's position
    unchanged;
  - untouched text;
  - fontless text;
  - paths and clips, and an unpainted path;
  - inline images, images and forms;
  - an unreadable form;
  - an `/ActualText` sequence;
  - a soft-mask group;
  - hidden layers, including an `/OCMD` and an image's `/OC`.

  Unit tests cover the geometry, the output, the `TJ` arithmetic (vertical
  writing too) and paths.
- `cargo test -p onionskin-core --test redactions`: 5 pass. They cover
  marks written, read back, restyled and removed, the outline appearance,
  refusals, and marks as Acrobat writes them.
- `cargo test -p onionskin-redact`: 31 pass. The integration suites:
  - `apply` (9):
    - text gone from the text and the bytes, with one revision and no
      `/Prev`;
    - an image painted out and an annotation removed;
    - overlay text;
    - patterns and whole pages;
    - a structure element unspelled;
    - an earlier revision not carried;
    - a JPEG with a soft mask;
    - an undecodable image;
    - nothing marked.
  - `sanitize` (2): a document carrying every kind of hidden information.
  - `guarantee` (2): see below.
  - `tool` (4), `codes` (3).

  Unit tests cover the verifier failing on a file that still holds text, an
  inline image, a mark, old bytes, metadata and a running action; also the
  patterns, Luhn, the fill through `/Decode`, covered pixels, the overlay
  layout, crop strips, the look round trip and the error messages.
- **Guarantee test 3** (`plugins/redact/tests/guarantee.rs`). It redacts
  every occurrence of a word:
  - on `hello.pdf`;
  - on the turned, cropped second page of `two-page.pdf`;
  - on a page drawing it split across a `TJ`, as a hex string, and inside a
    form drawn twice.

  It then reads the output independently: every page's text extracted,
  every stream decoded and searched, the raw bytes scanned for the word and
  for the redacted pages' original content streams. `guarantees.rs`
  enforces that the suite exists, runs, asserts each clause, and is reached
  by CI.
- `cargo test -p onionskin-app --features shell-test-support --lib`, on a
  real window (`tests::redact`, 5 tests):
  - Find Text & Redact through patterns, then Apply Redactions writing and
    opening a copy without the number;
  - Properties setting the default look, Mark Pages using it, a clicked mark
    restyled and removed;
  - code sets saved, used, renamed, exported, imported (a second import
    refused) and removed;
  - Redact Text on the selection and Remove Hidden Information;
  - a failed apply staying in the dialog.

  Unit tests cover the dialog's forms and rows, the notices, the preference
  round trip and a malformed preference, and Redact Text being live.
- **Whole-suite runs.**
  - Workspace without the app: 1202 passed, 0 failed.
  - App: 902 passed. The 7 failures are the known environmental ones: 4
    timing-sensitive canvas tests, and the 3 export rollback tests that fail
    when run as root.
  - App integration tests, guarantees included: all pass.
  - `cargo test -p onionskin-app --no-default-features --test
    kernel_emptiness`: 4 pass.

## Coverage

`cargo tarpaulin` with optimisation off.

| Code | Lines covered |
| --- | --- |
| `plugins/redact` | 994 of 1076 (92.4%) |
| `content/src/redact/*` and `interpret.rs` | 709 of 823 (86.2%), before hidden layers |

In `plugins/redact`, `apply/page.rs` and `verify.rs` are the least covered
files, at 88% and 91%.

## Mutations

Each was caught, then reverted.

- Resources a rewrite stopped naming not pruned: the original form stayed in
  the file, and guarantee test 3 failed. This one was found by the guarantee
  test while it was being written, and fixed.
- The verifier not looking at text in an area:
  `a_file_that_still_holds_what_was_removed_fails` fails.
- Annotations and marks left on the page: every `apply` test fails, because
  the verifier refuses the file.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support` reports only the existing
`a11y::Shared::record` warning, and so do the plugin-less
`--no-default-features --features shell` build and `--features
shell,redact`. `cargo fmt --all --check` is clean.

## Not claimed

- **Properties.** Overlay text is set in Helvetica, in WinAnsi characters.
  There is no font choice and no fill opacity.
- **Partial coverage by line art.** A path only partly inside an area is
  kept whole: a rule or a box crossing a redaction stays. Text drawn as
  vector outlines is removed only when each glyph's path lies wholly inside.
- **Patterns.** Tiling patterns and shadings are not looked into. Text
  drawn inside a pattern cell under a mark is not removed.
- **Images.** Only images in the device, calibrated and ICC colour spaces,
  at 8 bits or 1 bit, are painted out. Any other image under a mark is not
  drawn there at all. Stencil masks and colour-key masks on a painted-out
  image are dropped.
- **Hidden information.** Bookmarks, named destinations, form field values
  and hidden form fields are kept, and so are search indexes other than
  `/PieceInfo`. Hidden text (render mode 3) is kept unless it is under a
  mark.
- **A structure element's words.** `/ActualText`, `/Alt` and `/E` are
  removed wherever they spell a removed word of three or more letters. That
  can take one off text that was not redacted but spells the same word.
- **Apply in place.** Acrobat applies to the open document and asks to
  save. Here the result is always a new file, and the open document keeps
  its marks.
