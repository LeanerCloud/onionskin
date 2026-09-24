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
