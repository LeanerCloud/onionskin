# M3 P8 verification: text markup, and guarantee 2 driven by a tool

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P1c, P3, P6 and P7.

Linux x86-64, stable toolchain. **No macOS, Windows or hosted-CI run is claimed
here.**

## Runs

- `cargo test -p onionskin-tools-comment`: 4 unit tests (`quads`) and 10
  integration tests (`markup`), all passing.
- `cargo test -p onionskin-app --test tool_edit_guarantee`: 2 passing.
- `cargo test -p onionskin-app --test guarantees`: 40 passing, 4 ignored (the
  M5/M6 guarantees).
- `cargo test -p onionskin-core`: all suites passing, including the 10 in
  `annots` after the blend-mode fix below.
- `cargo test -p onionskin-app`, `-p onionskin-app --no-default-features`, and
  `--no-default-features --features tools-comment`: all passing.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
  `cargo fmt --all -- --check`: clean.
- `cargo test --workspace`: one failure, `onionskin-cos --test lazy ::
  every_parsed_object_records_the_bytes_it_came_from`. Pre-existing, on
  `isartor-6-1-8-t01-fail-a.pdf`, recorded in `known-issues.md` and untouched
  by this package.

## Five tools, one gesture implementation

Highlight, Underline, Strikethrough, Insert Text and Replace Text differ only
in what they write when the drag ends, so they are one `MarkupTool` with a
`Writes` discriminant rather than five copies of the same drag handling. They
share one rail slot (`group() == "markup"`), the way Acrobat groups a toolset's
variants.

Replace Text is **one transaction**, not two annotations that happen to
overlap: the `/StrikeOut` and the `/Text` reply linked to it by `/IRT` commit
together, because undoing half of a replacement is not a state anyone wants.
`replace_text_writes_a_strikeout_and_a_reply_linked_to_it` asserts the link, not
just the pair.

## The selection logic moved to `core`

`glyph_order`, `nearest_glyph`, `selection_for` and `clip_run` were private to
`tools-basic`'s select tool. A second plugin needing the same selection would
have meant a second implementation of it, so they are now `core::textselect`,
with `select_between(page, from, to)` added; `tools-basic` delegates and keeps
its own tests. No behaviour changed, which its unchanged suite says.

## The quad merge is per line, and that is the claim worth testing

A selection hands back one quad per glyph. The merge groups them into one quad
per contiguous run on a line, never into a bounding rectangle over everything.
A bounding-rect implementation passes every single-line test, and on a real
two-column page it paints the gutter and every intervening line from top to
bottom.

`quads.rs` states the two thresholds it turns on - `SAME_LINE_OVERLAP` as a
fraction of the smaller glyph's height, so it means the same thing at 6pt and
60pt, and `MAX_GAP` as a multiple of that height, so a space joins and a gutter
does not.

## The bug the render assertion found: a highlight was painting out the text

`appearance.rs` carried the comment "Multiply is what a highlighter does: the
text underneath stays readable" above code that wrote no blend mode at all. The
fill was opaque. Every structural assertion in `crates/core/tests/annots.rs`
stayed green while it was: the `/QuadPoints` were right, the `/AP` was present,
the rect was right, and the rendered page showed a blank yellow band where the
sentence had been.

The fix is `/BM /Multiply` in the appearance's own `/ExtGState`, which is where
it has to be, because a reader composites the form as the form asks.
`a_highlight_lets_the_text_beneath_it_show_through` measures it in pixels:
707 dark pixels before the highlight, 707 after, and 55 with the blend mode
removed again.

The `/ExtGState` is now built once for both reasons a form needs one - opacity
and blend mode - rather than only when `/CA` is set.

## Rendered result, over the region rather than at a point

`the_highlight_covers_the_glyphs_it_was_dragged_over` renders page 0 before and
after the gesture, maps each written quad through `PageGeometry::user_to_device`
and classifies **every pixel** in the raster: inside a quad it must not be page
background, outside every quad it must be byte-identical to the render taken
before the gesture. Only the two-pixel antialiased ring is exempt. A single
sample point passes an appearance placed a whole quad-height off; this does not.

`a_highlight_on_a_rotated_page_still_lands_on_its_glyphs` makes the same
assertion on a `/Rotate 90` page. `/QuadPoints` never carries the rotation, so
the file looks identical either way and only a rendered result separates a
correct mapping from one a quarter turn out - the trap M2's P3 documented.

## Guarantee 2, driven by an edit a tool made

`crates/cos/tests/incremental.rs` proves the guarantee where the bytes are
written and `crates/core/tests/save.rs` through `EditSession`, and neither can
reach a plugin: `crates/core` depends on `content`, `cos` and `render` only. The
definition of done asks for an edit a **tool** made, so
`crates/app/tests/tool_edit_guarantee.rs` drives the real highlight through its
real gesture lifecycle, saves through `core`, and asserts all three clauses on
the bytes on disk. Ten gestures and one save is still one section.

The section count is taken by **parsing the cross-reference chain**, never by
scanning for `%%EOF`: an original may legitimately contain one, and a scan
reports two sections for a document nothing was appended to.

Its dev-dependencies (`onionskin-tools-comment`, `onionskin-cos`) are **not
optional and not behind the feature**. A file-level `cfg` is exactly what the
tripwire's reader refuses, because one can compile a whole suite away while
every test in it still reads as live.

### The tripwire edit

`enforcing_suite` was hard-coded to `crates/cos/tests`. It now delegates to
`enforcing_suite_at(directory, ...)`, and guarantee 2's tripwire names the
app-level suite as its second enforcing suite, with the three assertion markers
it must still contain and the parsed section count it must still take.
`assert_ci_reaches` is the generalized form of the cos-only membership check.

**Demonstrated, not asserted in prose.** Deleting
`crates/app/tests/tool_edit_guarantee.rs` and running the tripwire:

```
thread 'an_edit_appends_one_incremental_section_that_truncates_away' panicked at
crates/app/tests/guarantees.rs:4209:9:
.../crates/app/tests/tool_edit_guarantee.rs is unreadable
(No such file or directory (os error 2)), so guarantee 2 is unchecked
```

## Mutations run

| Mutation | Test that failed |
| --- | --- |
| The quad list replaced by its bounding rectangle | `a_drag_over_two_columns_produces_two_quads_not_one_bounding_box` (alone; the other nine markup tests pass) |
| The save emits two sections instead of one | `a_tool_edit_appends_one_section_that_truncates_away` and `ten_tool_edits_and_one_save_are_still_one_section`, both on the parsed count: left 3, right 2 |
| The tool-driven guarantee test deleted | `an_edit_appends_one_incremental_section_that_truncates_away` |
| `/BM /Multiply` removed from the highlight appearance | `a_highlight_lets_the_text_beneath_it_show_through`: 707 dark pixels became 55 |
| The appearance drawn in page space rather than the form's | both render tests, on the inside-the-quad clause |

## Review risks, answered

**Is the quad order written down and does it match what readers expect?**
`PageQuad`'s order - upper-left, upper-right, lower-left, lower-right - is
documented in `quads.rs` and asserted edge by edge in
`quad_points_are_written_in_the_vertex_order_readers_expect`: the upper edge is
level, the lower edge is level, the left and right edges are vertical, and the
upper edge is above the lower one. That is Acrobat's order, whatever
ISO 32000-1's prose says.

**Does a selection on a rotated page map correctly?** Covered above, by
rendering.

**Is the `/IRT` chain real?** Asserted as a reference to the strike-out's own
object, not as two annotations of the right subtypes.

**Does the tool hold a borrow of `PageText` across the commit?** No:
`select_between` returns an owned `TextSelection`, and the commit takes it out
of `self.pending` before asking for the document mutably. It would not compile
otherwise, which is the strongest form this can take.

## Shift-extend takes the previous markup back

Shift-dragging past the end of a highlight wrote a **second, overlapping**
annotation: a doubly-dark band and two rows in the comments pane for one thing
the user did once. `retract_previous` undoes the previous entry first, and only
when `history().undo_label()` is still this tool's own name, so a shift-extend
after some other edit extends nothing and takes nothing back.

## Not done here

- The fixtures for the render and two-column assertions are **hand-built, not
  `external/`**, which the plan's corpus bullet expected. The assertions are
  about a gutter and about where ink lands relative to one known run of glyphs;
  a corpus file can grow a second run on refetch and stop measuring either.
  Neither new suite is a corpus suite, so neither belongs in CI's
  `ONIONSKIN_CORPUS_REQUIRED` rerun list - `seed()` reads committed bytes and
  panics rather than skipping, so it cannot report a vacuous pass.
- Rows 56-60 are closed in code; `ACROBAT-PARITY.md`'s totals are **P15's** to
  recount, and this package does not touch them.
