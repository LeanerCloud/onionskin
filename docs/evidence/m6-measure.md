# M6 verification: the Measure tools

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed. No file was opened in Acrobat for this entry.

## Rows

- **To `implemented`:**
  - Distance tool;
  - Perimeter tool;
  - Area tool;
  - Enable Measurement Markup;
  - Measurement Info panel.
- **To `partial`:**
  - Scale ratio and units: there is no field to type a scale in;
  - 2D snap settings: sensitivity and the hint's colour are fixed.
- **Headline:** 94 planned / 45 partial / 80 out-of-scope, 184
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Three tools** in one rail slot, Measure:
  - **Distance:** click where the distance starts and again where it ends,
    or drag from one end to the other.
  - **Perimeter:** click each point in turn. Double-click the last one, or
    press Enter, to finish.
  - **Area:** click each corner in turn. Click the first corner again,
    double-click the last one, or press Enter, to finish.
  - Escape abandons the measurement being made.
- **Snapping.** While a tool is in use, the pointer snaps to the page's
  line art within 8 view pixels. A small box marks the point snapped to.
  It tries, in this order:
  - the ends of straight edges;
  - where two edges cross;
  - the middles of edges;
  - the nearest point along an edge.
- **Measurement Info**, in the side panel under the tool's name:
  - the scale;
  - the measurement being made, or the last one made;
  - for a distance, how far it runs across (ΔX) and up (ΔY), and its
    angle;
  - what the point was snapped to.
- **Settings**, in the side panel. The three tools share one set, so a
  scale chosen for one is the scale of all three.
  - **Scale:** eight scales, from `1 in = 1 in` (the page as it is) to
    `1 cm = 1 km`. Nine units are understood: pt, in, mm, cm, m, km, ft,
    yd and mi.
  - **Snap to:** endpoints, intersections, midpoints and paths, each on or
    off.
  - **Keep as a comment:** Acrobat's measurement markup. On by default.
- **What is kept.** With markup on, each measurement becomes a comment, one
  undo step named for its tool. It is written as Acrobat writes one:
  - a `/Line`, `/PolyLine` or `/Polygon`;
  - `/IT` `/LineDimension`, `/PolyLineDimension` or `/PolygonDimension`;
  - a `/Measure` dictionary of subtype `/RL`, with the scale in words in
    `/R` and the `/X`, `/D` and `/A` number formats;
  - the value in `/Contents`, and `/Cap true` on a line.

  The value is also drawn as the shape's caption, in Helvetica: above the
  middle of a distance and along it, above the last side of a perimeter,
  and in the middle of an area. A caption never reads upside down.
  Changing the comment's colour or opacity draws the caption again.
- **A document that may not be commented** can still be measured. Only the
  comment is refused.

## How it works

- **content:** each painted path now carries its straight edges: a line, a
  rectangle's sides, and the edge a close draws back. Curves are left out.
- **core, `annots::measure`:**
  - `Unit`, `Scale` (its words, parsed back), `Kind` and `Measure`;
  - `Measure::annotation` builds the comment, with a `/Rect` that holds
    the caption;
  - the caption is placed once and drawn by the Line, PolyLine and Polygon
    appearances;
  - `Intent` gains the three dimension intents. Redrawing a comment reads
    its intent and `/Measure` back, but only for the shape its kind names.
- **plugin-api:**
  - `Reading`, and `ToolPlugin::readings`;
  - `ToolPlugin::settings` and `picked`, for settings that are on several
    at a time;
  - `ToolCapability::Measure`, which does not count as editing the
    document.
- **tools-measure:** `Settings` and `Shared`, `Snapper` and `MeasureTool`.
- **app:**
  - the side panel shows the active tool's readings and settings;
  - `Activation::ToolSetting`;
  - rail glyphs for the three tools.

## Runs

- `cargo test -p onionskin-tools-measure`: 13 pass.
  - Unit tests: the settings, by name and by words; each kind of snap, in
    order, with kinds turned off; crossings; a page's edges read once.
  - `tests/measure.rs`, through the real gesture lifecycle on a page with a
    rule and a square:
    - a distance of two clicks snapped to the rule's end, its middle, and
      onto the rule, with its readings and the comment it keeps;
    - a dragged distance at a scale chosen on the Area tool;
    - an area closed on its first corner, snapped to the square's corners;
    - a perimeter ended by a double click;
    - markup off; Escape; another page; too few points;
    - the tools' names, hints, capability and settings.
- `cargo test -p onionskin-core`: every suite passes. New: the measurement
  unit tests, a measurement read back by the redraw path, and
  `a_measurement_keeps_its_scale_and_draws_its_value`, which renders the
  page and finds the caption's ink above the line, before and after a
  change of colour.
- `cargo test -p onionskin-content`: every suite passes, with
  `straight_edges_are_kept_and_curves_left_out`. `tools-form`'s field
  detection, which reads the same shapes, passes unchanged.
- `cargo test -p onionskin-plugin-api`: 19 pass.
- On a real window (`tests::measure`): the Distance tool chosen, the scale
  set from the side panel's settings, two clicks, and Measurement Info
  reading `Distance: 30.00 ft` and `Angle: 0.0°`, with one undo step,
  Distance.
- **Whole-suite runs.**
  - App: 960 passed. The 6 failures are the known environmental ones: 3
    timing-sensitive canvas tests, which pass alone, and the 3 export
    rollback tests that fail when run as root.
  - App integration tests, guarantees included: 42 pass, 2 ignored.
  - `--no-default-features --test kernel_emptiness`: 4 pass.

## Coverage

`cargo tarpaulin -p onionskin-tools-measure` with optimisation off: 99.4%
of the crate's own lines (328 of 330).

| File | Lines covered |
| --- | --- |
| `lib.rs` | 7 of 7 |
| `settings.rs` | 45 of 45 |
| `snap.rs` | 90 of 90 |
| `tool.rs` | 186 of 188 |
| `core/src/annots/measure.rs` | 176 of 215, under this crate's tests alone |

`core`'s own unit tests cover the rest of `measure.rs`: reading a
`/Measure` back, the units and the caption stream.

## Mutations

Each was caught, then reverted.

- The caption left out of the appearance:
  `a_measurement_keeps_its_scale_and_draws_its_value` fails.
- Any `/IT` read as a measurement whatever the subtype:
  `a_measured_area_reads_back_with_its_scale_and_a_free_text_keeps_its_intent`
  fails. It was found this way: the first version did it.

## Clippy and format

The following report only the existing `a11y::Shared::record` warning:

- `cargo clippy --workspace --all-targets --features
  onionskin-app/shell-test-support`;
- the app built with `--no-default-features` and `shell`, and with
  `shell,tools-edit`.

`cargo fmt --all --check` is clean.

## Not claimed

- **Typing a scale.** Only the eight listed scales can be chosen in the
  side panel. Acrobat's scale dialog takes any ratio.
- **Precision.** Values are shown to two decimal places; Acrobat lets the
  user choose.
- **Snap sensitivity and hint colour.** Both are fixed.
- **Scales read from the document.** A scale in the page's viewport
  dictionary (`/VP`) or in an existing measurement is not picked up.
- **Rulers, grids and guides.** The View > Show/Hide row is still planned.
- **Editing a kept measurement.** Moving a vertex does not measure again.
