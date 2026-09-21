# M3 P9a verification: notes, free text, and the overlay seam

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P0b, P6, P7 and P8.

Linux x86-64, stable toolchain. **The windowed tests ran here**, which is new;
see "The shell builds on Linux now" below for what that does and does not
establish. No macOS, Windows or hosted-CI run is claimed.

## Runs

- `cargo test -p onionskin-tools-comment`: 24 tests - 4 unit (`quads`), 10
  integration (`markup`), 10 integration (`notes`) - all passing.
- `cargo test -p onionskin-app`: all suites passing, including P7's contract
  suite over the real `build_registry()` with the four new tools in it.
- `cargo test -p onionskin-app --features shell --lib`: the windowed unit
  tests, including `every_overlay_shape_paints_where_the_viewport_puts_it`.
  One pre-existing failure, recorded below.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
  `cargo fmt --all -- --check`: clean.

## The overlay seam, corrected once rather than three times

The plugin API declares six overlay shapes and the canvas painted two. The
other four - `Rect`, `Polyline`, `Line`, `Circle` - reported "the canvas cannot
draw a {kind} overlay yet" on the status line. That string is what a user would
have seen in place of the shape they were drawing, and P9a's text box, P9b's
ink and P9c's four shapes all need one of the four, so the seam is P9a's to
fix.

Two of the six were the wrong shape as well:

- `Circle { center, radius }` cannot express an ellipse inscribed in a dragged
  rectangle, which is what Oval is. It becomes `Ellipse { bounds }`. It had no
  consumer to break: the search-hit marker its doc comment named is painted
  through `HighlightPaint`.
- `Polyline` was open-only, so a polygon's preview was missing the one edge
  that tells a user they have closed the shape. It gains `closed`.

`map_overlay` loses its error arm entirely. A variant added to `Overlay`
without a painter is now a compile error, which is the only form of
exhaustiveness that survives someone adding a variant in a hurry -
`every_overlay_shape_paints_where_the_viewport_puts_it` then adds that each
painter places its shape where the viewport says, and that **no shape puts a
status on screen**.

An unplaceable vertex drops the whole polyline rather than being skipped: a
path missing one vertex is a different shape, and drawing a different shape is
worse than drawing none.

A first draft of that test also scanned this file for the old status string.
It failed on its own text, which is the self-exclusion trap this repo has hit
before, and it was removed rather than taught to skip itself: the compiler
already carries that claim.

## A click is not a small drag

A sticky note is placed by a click and has no extent the user chooses: a reader
draws `/Text` at its own icon size whatever the `/Rect` says, so a rubber band
would change nothing. A text box is the opposite. Both are asserted, in both
directions: `a_sticky_note_dragged_across_the_page_writes_nothing` and
`a_drag_sizes_a_text_box_to_the_rectangle_dragged_out`.

The click's point is the note's **upper-left** corner, so the icon lands under
the pointer rather than above and to the right of it.

## `/DA` and the appearance stream cannot disagree

The review risk this package names. A `/FreeText`'s `/DA` string and its
appearance stream both name a font, and when they name different ones the
annotation renders one way in a reader that trusts `/AP` and another in one
that re-lays-out from `/DA` - which is what makes a text box look different in
Acrobat.

Both now come from one `TextStyle`: `default_appearance()` builds the `/DA`
string, and the appearance stream selects the same resource key at the same
size in the same colour. `the_default_appearance_and_the_stream_name_the_same_font`
asserts the `/DA`'s key, the form's `/Resources` `/Font` key, and the
`/BaseFont` behind it.

**Named, never embedded.** `/Helv` is a standard Type 1 name every reader
substitutes for, and the font dictionary carries no `/FontFile`, `/FontFile2`,
`/FontFile3` or `/FontDescriptor` - asserted, because legal posture rule 6 is
about shipping Adobe's font files and the way that rule gets broken is an
embedder added later. Acrobat itself writes `/Helv` by name.

## The callout's leader

`/CL` is three points - tail, knee, landing - because that is what Acrobat
writes and what a reader draws an elbow from; a two-point leader is legal and
looks like a stray line. The tail is what the callout points at, the landing is
on the edge of the box nearest the tail, asserted on the array rather than on
a render.

**The leader is also drawn into the appearance stream.** A reader with an `/AP`
draws what the stream says, so a leader living only in `/CL` is a leader nobody
sees; the test asserts the stream has the path operators as well as the array.

### The design decision, stated rather than discovered

One drag has to supply both what the callout points at and where the box goes.
The drag **starts at the target and ends at the box**, which is the order
Acrobat asks for. A click gets the default box offset from the target, because
a box on top of what it points at is a box the leader cannot reach.

## Mutations run

| Mutation | Test that failed |
| --- | --- |
| `/CL` dropped from the annotation dictionary | `a_callout_writes_a_leader_from_its_tail_to_its_box`: left 0, right 6 |
| An `Overlay` variant with no painter | does not compile; `map_overlay` has no catch-all |

A bug this package's own tests found before any mutation did: `commit` took
`self.anchor` out before computing the leader from it, so every committed
callout had an empty `/CL` while the preview - which ran before the take - drew
the leader correctly. Exactly the shape of failure that passes a preview test
and ships a broken file.

## The shell builds on Linux now

This is the deferral P7 recorded as "shell-feature test suite never run".
`cargo check -p onionskin-app --features shell` completes in about eight
minutes from cold, and after `apt-get install libxkbcommon-dev
libxkbcommon-x11-dev` the test binaries **link and run**: 96 of 98 canvas tests
pass.

What that establishes: the windowed code compiles, and its unit tests - which
are model-level and open no window - run. What it does not: nothing here opens
a window, renders through blade, or exercises AccessKit, so the macOS
acceptance work is untouched by this.

One pre-existing failure:
`an_unmeasured_page_is_described_without_words_and_says_so` expects page 1 of
`two-page.pdf` to still be unmeasured on the first frame and finds it measured.
It is a race with the geometry worker, it has nothing to do with overlays, and
it is recorded in `known-issues.md` rather than papered over here.

## Not done here

- **The tools write no text.** `ToolPlugin` has no key-input hook, and P9a does
  not add one: a `/FreeText` is created with its box, its `/DA` and its intent,
  and the text arrives when the comments pane (P20) can edit `/Contents`. The
  appearance stream already lays out whatever `/Contents` holds, so nothing
  about that is deferred except the typing.
- **Line breaking is on the text's own newlines only.** Wrapping to the box
  needs font metrics this crate does not have, and a wrap computed from a
  guessed advance is worse than none, because it looks deliberate.
- Rows 55, 61, 62 and 63 are closed in code; `ACROBAT-PARITY.md`'s totals stay
  **P15's** to recount.
