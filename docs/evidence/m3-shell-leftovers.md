# M2/M3 shell leftovers (WP2)

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed. Each section is one row closed or narrowed, in
the order they landed; the headline is updated with each.

## Zoom To: a typed magnification (M2)

- **Row to `implemented`:** View > Zoom > Zoom In / Zoom Out / Zoom To.
- **Headline:** 83 planned / 45 partial / 80 out-of-scope, 195
  implemented.
- **What the user gets:** Zoom To has a Magnification field above its 12
  presets, holding the magnification in force when the dialog opens.
  **Zoom** applies what is typed, with or without a `%` sign. A value
  outside 5% to 3200%, the range the viewport offers, or one that is not
  a number, is refused in the dialog with the range, and the dialog stays
  open. Acrobat's own field goes to 6400%; the viewport's 3200% ceiling
  is unchanged here.
- **How:** `dialog::parse_magnification`, the field in the frame's page
  entry state, `Activation::SubmitZoomPercent`.
- **Runs:** `cargo test -p onionskin-app --features shell-test-support
  --lib`: 981 pass, the 3 failures the known rollback tests that fail as
  root. New: the parser's accepted and refused inputs, and on a real
  window the field showing the zoom in force, 9000 refused with the
  range, and " 137 % " applied as 1.37.

## Layer Properties: name, intent and default state (M2)

- **Row to `implemented`:** Layers pane context menu (Layer Properties,
  visibility and default-state commands). Merging and flattening layers
  stay post-1.0 layer editing, as the row says.
- **Headline:** 83 planned / 44 partial / 80 out-of-scope, 196
  implemented.
- **What the user gets:** Layer Properties lists the document's layers to
  choose from; for the chosen one it shows its name in a field, its
  intent (View or Design) and its default state (On or Off), as the file
  has them rather than as the pane was toggled. **Apply** writes them as
  one undoable step, and the pane and the page show the new defaults. An
  empty name is refused in the dialog; on a document that may not be
  edited, Apply is off and the dialog says why.
- **How:** `core::set_layer_properties` sets the group's `/Name` and
  `/Intent`, and puts the group in `/OCProperties /D /ON` or `/OFF` and
  out of the other, whether `/OCProperties`, `/D` or the lists are inline
  or objects of their own. `Session::set_layer_properties` then hands the
  renderer the new defaults. `LayerIntent::of` reads an intent written as
  a name or an array (View unless it names only Design).
- **Runs:**
  - `cargo test -p onionskin-core --test layer_properties`: 1 pass, over
    an inline and a referenced default configuration: both layers
    renamed (one to a non-ASCII name), intent and default state changed,
    saved, reopened and read back the same by us and by pikepdf 10.5
    (`/Name`, `/Intent`, `/D /ON`, `/D /OFF`); `qpdf --check` finds no
    errors.
  - `cargo test -p onionskin-app --features shell-test-support --lib`:
    980 pass, the 4 failures the known environmental ones. New on a real
    window: the dialog opens on the first layer at its defaults, refuses
    an empty name, and applies a new name, Design and Off, which the pane
    reads back, and Undo takes back.

## Dynamic stamps: local time (M3)

- **Claim:** a dynamic stamp reads the author's local time from the platform's
  own timezone conversion and prints its offset, rather than saying "UTC"
  because the crate carried no timezone database. Offset and colour of a
  bookmark are separate work below.
- **Why:** PLAN.md's parity row offered local time as a gap, and a stamp a
  reader elsewhere sees has to say what clock it was taken against. Reading the
  platform's conversion is both smaller than a bundled timezone database and
  correct about DST, which a fixed offset would not be.
- **How:** `dynamic_line_at` renders from an offset, and `local_offset_seconds`
  reads it from `localtime_r`, so the rendering is a pure function and a
  half-hour zone is a unit test rather than something that needs the machine's
  zone set to half past five. `libc` is already in the lockfile as a transitive
  dependency, so this is a manifest edge rather than a new build.
- **Runs:** `cargo test -p onionskin-tools-comment --lib`: 22 pass. Run under
  `TZ=Asia/Kolkata` (a half-hour zone), `TZ=UTC`, `TZ=America/New_York` and
  `TZ=Europe/Berlin`, all 8 stamp tests pass. The offset assertion is pinned
  per named zone, and was checked to fail when its expectation is deliberately
  wrong: under `TZ=Asia/Kolkata` a whole-hour expectation fails 19800 against
  18000. The half-hour case asserts `2026-09-21 19:35 +05:30`, which a
  `offset / 3600` formatting gets wrong.

## Description: the file's own facts (M3)

- **Claim:** the Description tab reports the effective PDF version, the current
  page size, whether the document is tagged, and whether it is in fast web
  view, which it previously left out.
- **How:** `metadata::document_facts` reads all four and `Document::document_facts`
  exposes them beside the existing `info` and `fonts`, so the dialog does not
  reach into the COS layer. The version is the *effective* one: a catalog's
  `/Version` outranks the `%PDF-` header, which is ISO 32000-1 7.5.2 rather
  than a guess, and `cos::Document::header_version` was added for the header
  half since nothing read the bytes at the front of the file. Linearization is
  read from the file's FIRST object, because a linearized file's first object
  is the linearization dictionary and not the catalog. Tagged means
  `/MarkInfo /Marked true` and not the presence of `/MarkInfo`, so a document
  carrying the dictionary with the flag absent reads untagged.
- **Runs:** `cargo test -p onionskin-core --test document_facts`: 4 pass, one
  per fact, each with a fixture that states it and one that does not. The
  tagged case runs four shapes, of which three must read untagged.

## Attachments: making the oracle real (M3)

- **Claim:** the Edit Description coverage was not proving what it appeared to.
  The core test read `/Desc` back with pypdf and checked the file with qpdf,
  but both halves skipped silently when the tool was missing.
- **How:** pypdf and pikepdf in a venv, qpdf installed, and the existing
  `ONIONSKIN_REQUIRE_ATTACHMENT_ORACLE` set so an absent oracle is a failure
  rather than a skip. qpdf immediately found what the skip had hidden: the
  fixture's page carried no `/Resources`, so `qpdf --check` exited 3 with
  "succeeded with warnings" and the test's success assertion failed. The same
  defect turned up a second time in the `layer_properties` fixture, fixed the
  same way. That is twice an oracle that had never run found a real problem
  behind a skip.
- **Runs:** `cargo test -p onionskin-core --test attachment_description` and
  `--test layer_properties` with the oracle mandatory: both pass, no skips. The
  WIP commit's own two "not done" items were in fact already done: the window
  test `edit_description_rewrites_what_the_pane_shows` (outline.rs) drives the
  pane, the menu, the typed value and the submit, and passes.

## Bookmark style: `/F` and `/C` (M3, core half)

- **Claim:** the PDF half of Bookmark Properties exists: a title's `/F` bit
  position and `/C` colour are written, undoable, and readable by an
  independent reader. The dialog and the two menu entries are NOT done, so the
  parity row stays partial.
- **How:** `core::outline::set_bookmark_style` writes `/F` as the bit POSITION
  ISO 32000-1 12.3.3 defines, where italic is 1 and bold is 2, and `/C` as
  three 0..1 floats. The bit field is the trap: reading the two as independent
  flags writes 1 for "bold, not italic" and switches italics on instead, which
  is invisible on a document where nothing is bold and wrong on every one where
  it is. A colour of `None` removes `/C` rather than writing black.
- **Runs:** `cargo test -p onionskin-core --test bookmark_style`: 4 pass, all
  four bold/italic combinations pinned, colour added and removed, one undoable
  step. pikepdf reads `/F` and `/C` back and `qpdf --check` passes, both under
  `ONIONSKIN_REQUIRE_OUTLINE_ORACLE`. The pikepdf half matters more than it
  looks: our reader and our writer could agree on the same misreading of `/F`,
  and an independent reader cannot.
