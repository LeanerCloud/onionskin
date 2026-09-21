# M3 P9b verification: ink

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain; no macOS, Windows or hosted-CI run is claimed.

Rows closed: 64 Draw freehand (ink), 65 Erase ink. `ToolCapability::Draw` now
has tools, so the Draw quick action is live.

## Runs

- `cargo test -p onionskin-tools-comment`: 7 new ink integration tests, 4 new
  unit tests for the eraser's splitter and the pen width, and the tagged-session
  test. The earlier suites pass unchanged.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`: 585
  pass. The 8 failures are the environmental set in `known-issues.md`. This
  run's set included `a_standalone_snapshot_failure_still_reaches_status`,
  which then passed alone on three reruns. The set varies, as recorded.
- `cargo clippy` over the touched crates with `-D warnings`: clean.

## Pressure reaches the page

Each point keeps the pen width its pressure gives: `base * (0.25 + 0.75p)`. A
mouse reports 1.0 and draws at the base width. A light touch draws at a
quarter of it, never at nothing. The appearance stream draws each segment at
the mean width of its two ends. PDF has no key for per-point width, so, as in
Acrobat, pressure lives in `/AP` and `/InkList` carries the geometry.

`a_harder_stroke_covers_more_of_the_page` renders the same path at pressure
0.2 and at pressure 1.0 and compares covered pixels. It does not read the
stream text. **The mutation the plan names**, pressure accepted and ignored,
fails it.

## One gesture, one undo entry

A 400-event stroke is one transaction, committed when the pointer lifts, and
it keeps its points. **The mutation the plan names**, one transaction per
pointer event, fails that test and five others.

## Erase splits, and never writes an empty stroke

The eraser removes the part of each stroke under it. What is left stays as
separate strokes of the same annotation, with a `/Rect` recomputed for the
remainder. An annotation with nothing left is removed from its page.

- **Strokes and the eraser's own path are both densified** to half the
  eraser's radius. Without that, a two-point stroke crossed between its ends,
  or a fast swipe that reports two pointer events, erases nothing. The first
  run caught exactly this.
- **Tested cases:**
  - erasing the middle leaves two strokes, and the `/Rect` fits them exactly;
  - erasing the end shrinks the `/Rect`;
  - erasing the whole stroke removes the annotation;
  - erasing nothing makes no edit and no undo entry;
  - a click draws a one-point dot, never a zero-point stroke.
- **A rewritten stroke is drawn at `/BS /W`.** The pen widths it was drawn with
  are not in the file. Recorded as a limitation, not a defect: Acrobat has the
  same constraint.

## Found on the way: every comment tool read the file's structure tree

This was recorded in P11's known issues and is fixed here. Every comment tool
read the structure tree from the file, not from the session. On a tagged
document, the second comment in a session got the first comment's
`/ParentTree` key and wrote the tree without it. `Document::edit_annotations`
hands the session's tree to every comment tool: sticky note, free text,
shapes, markup and ink.

`every_comment_in_a_session_keeps_its_own_structure_element` places three
notes and finds four `/ParentTree` entries and three distinct `/StructParent`
keys. With the file's tree restored, it finds two entries.
