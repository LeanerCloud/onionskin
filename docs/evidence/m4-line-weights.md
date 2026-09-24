# M4 verification: View > Show/Hide > Line Weights

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:** View > Show/Hide > Line Weights. P22 moved it from
  M3 to M4 because it needs an option in the renderer.
- **Headline:** 154 planned / 29 partial / 80 out-of-scope, 140
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

View > Show/Hide > Line Weights is a checked toggle, on by default as
Acrobat's is. Turned off, every stroked path on screen is drawn one device
pixel wide at any zoom, so a drawing whose heavy borders swamp its detail
becomes readable; turned on, strokes are drawn at the page's own widths.

It is the Page Display preference "Use line weights", as in Acrobat: the
Preferences dialog's Page Display category has the same switch, the
setting is saved, and a document opened later opens with it. It is
application-wide: toggling it in one window redraws every tab in every
window and updates every window's check mark. It has to be, since two
windows on one document draw from one session; a per-window setting would
have one window's menu contradict the other window's pages.

It changes the screen only. Printing, image export and SVG export keep the
page's own widths, and nothing reaches the saved file.

## How it works

- **The fork.** hayro's rasterizing device is private to the hayro crate,
  so no wrapper outside it can change a stroke. The fork gains
  `InterpreterSettings::hairline_strokes`: a stroked path is reported to the
  device with line width 0, which the PDF specification defines as the
  thinnest line the device can draw, and hayro's renderer already widens
  anything thinner than one device pixel to one pixel. Stroked text is left
  alone. The change is fork commit `257a9e33` on top of `67763e2e`, 14 lines,
  in the same upstreamable form as the fork's `ocg_overrides`; hayro's own
  `hayro` and `hayro-interpret` tests pass with it.
- **render.** `RenderOptions::hairline_strokes`, passed to the interpreter
  for rasters only; the SVG path builds its own settings and never sees it.
- **core.** The render worker takes `Request::SetHairlineStrokes`. The
  worker's options outlive byte reloads, so the setting survives edits.
  Renders answered on a caller's own channel (`render_page_now`, used by
  export) render with it forced off. `Document::set_hairline_strokes`
  reports whether anything changed; a second window's `RenderView` picks
  the setting up on its next request.
- **app.** `Preferences::line_weights` (key `line_weights`, default on);
  `ShellViewState` carries it for the menu's check mark; each
  `CanvasModel` tracks its own flag and drops its rasters and thumbnails
  when it changes. Per canvas rather than read from the session, because the
  first of two windows to hear of a change sets the shared session's flag
  and the second must still redraw. Toggling updates this window at once and
  every other window through a deferred update, the pattern `notify_peers`
  uses.

## Runs

- `cargo test -p onionskin-render --test render`: 8 pass, 4 of them new:
  - `hairline_strokes_draw_a_thick_line_one_pixel_wide`: a 20-point line
    is 20 pixels tall at 1x, 1 to 2 with hairlines;
  - `a_hairline_stays_one_pixel_wide_when_zoomed`: 80 pixels at 4x
    against 1 to 2;
  - `hairline_strokes_leave_fills_alone`;
  - `an_svg_export_keeps_its_line_weights`: identical SVG either way.
- `cargo test -p onionskin-core --test render`: 21 pass, 3 of them new:
  - the toggle on the primary queue, with no-op reporting;
  - `an_export_render_keeps_its_line_weights`;
  - `a_second_view_takes_up_line_weights`.
- `cargo test -p onionskin-app --features shell-test-support --lib`: 819
  pass. New or rewritten:
  - `tests/line_weights.rs` (4, on real windows): the menu toggle redraws
    and saves, a second window follows the first both ways, a document
    opened while off opens without line weights, and the Preferences
    dialog switch does what the menu does;
  - `line_weights_is_a_checked_toggle_that_runs` and
    `no_menu_entry_is_deferred_to_a_milestone` (global bar);
  - `page_display_offers_line_weights_as_a_switch` (dialog);
  - `every_setting_round_trips_through_the_file` (preferences).

  The 6 failures are the environmental ones recorded before this change:
  - three of the four timing-sensitive canvas raster tests. Which three
    varies between runs; two runs failed
    `a_zoom_change_keeps_the_raster_the_paint_will_scale` and
    `an_unmeasured_page_is_described_without_words_and_says_so`, with
    `a_snapshot_turns_with_the_view` in one and
    `an_update_that_paints_nothing_leaves_no_frame_open` in the other;
  - three export rollback tests that fail when run as root.
- Rerun as committed, with the fork fetched at its pinned rev and
  `--locked`: render and core 467 pass, 0 fail; the app library 819 pass
  with the same 6; guarantees 40 pass.
- `cargo test --workspace`: 1171 pass, 0 fail.
- `cargo test -p onionskin-app --test guarantees`: 40 pass.
- The lockfile as committed was verified with `--locked`, fetching the
  fork commit itself through a git URL rewrite, so it resolves once that
  commit is on GitHub. The earlier runs above used a path override of the
  same fork tree.

## Mutations

Each was applied alone, and each failed the named tests:

- The render crate stops passing the flag to hayro: both render stroke
  tests.
- The fork's override removed: both render stroke tests.
- Export renders with the canvas's options: `an_export_render_keeps_its_line_weights`.
- A second view never re-syncs the flag: `a_second_view_takes_up_line_weights`.
- Other windows are not told: `a_second_window_follows_the_first`.
- The open-time preference is not applied:
  `a_document_opened_while_line_weights_are_off_opens_without_them`.
- The canvas reads the session's flag instead of its own:
  `a_second_window_follows_the_first`.

## Coverage

`cargo tarpaulin`: every new line in `render/src/base.rs` is covered. In
core, every new line is covered except the worker-already-stopped error
branch of `WorkerHandle::set_hairline_strokes`; tarpaulin also lists that
function's signature line and the `hairline_strokes` getter, which the new
tests do call. At this optimization level those two lines are inlined, and
tarpaulin misattributes inlined lines.

## Clippy and format

`cargo clippy -p onionskin-render -p onionskin-core -p onionskin-app
--features onionskin-app/shell-test-support --all-targets` reports only the
existing `a11y::Shared::record` warning. `TabError::CommandUnavailable`,
which Line Weights was the last user of, is removed. `cargo fmt --check` is
clean.

## Not claimed

- Stroked text (text rendering modes 1, 2, 5 and 6) keeps its width.
- No default keystroke: Acrobat's default for this entry is not settled
  here.
- Thumbnails follow the setting, as the page does; Acrobat's may not.
