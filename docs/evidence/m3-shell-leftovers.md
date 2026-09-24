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
