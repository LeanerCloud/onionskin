# M3 P3 verification: save, preview and generations (stages A and B)

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P2, P4 and P6.

Linux x86-64, stable toolchain. **No macOS, Windows or hosted-CI run is claimed
here.**

## Scope: this is stage A of P3, not all of it

P3 is the largest package in M3 and it is landing in stages so each one is
independently green. What is here:

- `save.rs`: save, Save As, and the write-then-reopen ordering.
- `preview.rs`: the preview buffer, one entry, keyed on `(generation, filter)`.
- `generations.rs`: the generations list and `revert_to`.
- `session.rs`: the surface, the generation counter, cache invalidation.
- `tests/save.rs`: 13 tests.

**What is not here, and is not claimed:**

- **Three reads still go to `&self.cos`, each for a stated reason** (stage B
  routed the rest; see below). `page_geometry` and the async geometry response
  pair a cos read with the render worker's own geometry, which comes from the
  **original** bytes; routing the cos half alone would make the two disagree
  after a page edit, which is worse than both being stale together. They move
  when the worker does. `page_count` is still a field: nothing in M3 can change
  the page count until P5 lands, so it is correct today, and P5 owes either
  routing it or forcing a structure build. `export_snapshot` is documented as
  exporting the original bytes and is left alone.
- **The render worker still renders from the original bytes**, not from the
  preview. The plan is explicit that feeding it preview bytes is a restructure
  of `render.rs` rather than a new `Arc`, and it has not been done.
- **Autosave and the recovery file do not exist**, including the permission
  checks the plan specifies and which are a privacy requirement rather than a
  nit.
- **`benches/save.rs` does not exist.**
- The **failed-reopen** path is implemented and its rule is enforced in code,
  but there is no test for it: provoking a write that succeeds and a reopen that
  fails needs a fault-injection seam this package does not have.

## Stage B: structural reads answer from the edits

`page_text`, `outline`, `attachments`, `attachment_bytes`, `signatures`,
`layers` and `reset_layer_visibility` now read through `structure()`, the
document opened from the unfiltered preview.

**Two design corrections came out of doing it.**

**The preview has two slots, not one.** Structural reads need the unfiltered
document for the current generation; the print dialog asks for filtered
previews. In one slot, every mode change evicted the buffer every structural
read depends on. The unfiltered buffer is now persistent per generation and
filtered previews get one transient slot of their own, which is what the plan's
"filtered previews are transient" means.

**An edit invalidates the caches without anyone remembering to.** The first cut
of the routing tests called `bump_generation()` by hand after each edit, because
an edit through `edit_mut()` told the document nothing. That is a convention
every caller must follow, and some caller would not. `EditSession` now carries
an epoch bumped by every commit, undo, redo, rebase and forget, and every read
that caches checks it first. The two routing tests pass with the manual bumps
removed, which is the proof.

## Runs

- `cargo test -p onionskin-core --test save`: 13 tests, all passing.
- `cargo test -p onionskin-core`: 14 suites, all passing.
- `cargo test -p onionskin-app --no-default-features`: passing, 40 guarantee
  tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.

## What the tests found

Writing them surfaced four real defects, three in code and one in the plan's own
arithmetic.

1. **The filter walked the base, not the merged view.** An annotation the
   session had just authored lives only in the overlay, so a filter walking the
   base found nothing to hide and returned a buffer identical to the unfiltered
   one. Hiding a comment the moment after writing it is the first thing anyone
   tries. Fixed in `filter.rs`, which now resolves each annotation the way the
   annotation reader does.
2. **Collapse rule 2 did not exist.** Add an annotation and delete it again in
   one session: the page's `/Annots` collapses back to the base under rule 1,
   and the annotation dictionary and its appearance stream stay in the overlay
   with nothing naming them, so the save appends a section of orphans for a
   document the user changed and changed back. `Overlay::collapse` now drops
   overlay-only objects that the merged document does not reach. The walk is
   skipped entirely unless the overlay holds an object the base does not, so an
   ordinary editing session does not pay for it.
3. **Removing the last annotation left `/Annots []` behind**, which is legal but
   is not what the base holds, so the page could never collapse. The key is now
   removed rather than emptied. This is what made defect 2 visible.
4. **`revert_to`'s argument was ambiguous in the plan and wrong in the first
   implementation.** "Refused if the target is not a trailing section" only
   makes sense if the target is the generation being *dropped*; read as the
   generation being *returned to*, reverting to generation 0 would be the most
   ordinary case and the refusal rule would be unreachable. The argument is now
   documented as the generation being dropped, and 0 is refused because the
   original document is not a section anyone can remove.

## Two assertions worth naming

**Guarantee 1 is partitioned by `Provenance`, never by filename.** A repaired
document's no-op save legitimately appends its repair, so it is a separate test
rather than an exception, and both halves run. One scoping note: the repaired
half skips fixtures whose `%PDF-` header is itself damaged, because an appended
section reaches the end of the file and can never repair byte 0. That is a
property of incremental update, not a carve-out for a failing case.

**The save-boundary test compares object graphs, not bytes**, in all four shapes
M3 can produce, with `audit_references` asserted empty in each. Comparing bytes
would fail on two serializations of the same graph and pass on a restored object
whose referrer was forgotten; comparing the graph catches exactly the second.

## Mutations

Applied, run and reverted.

| Mutation | Tests failed |
| --- | --- |
| I. `section_for`'s empty-overlay short circuit removed | `edit_then_undo_then_save_writes_nothing`, `adding_an_annotation_and_deleting_it_in_one_session_writes_nothing`, `a_no_op_save_of_a_well_formed_document_is_byte_identical` |
| J. the reopen after save skipped | `two_saves_produce_two_sections_and_the_second_points_at_the_first`, `create_save_create_save_puts_the_two_annotations_at_two_numbers`, `revert_refuses_…` |
| K. the `next_number` reseed replaced with `Overlay::default()` | `create_save_create_save_puts_the_two_annotations_at_two_numbers` **and nothing else** |
| L. the filter dropped from the preview cache key | `two_hiding_filters_at_one_generation_do_not_share_a_buffer` |

K fails exactly one test, which is the plan's own prediction and the reason
that test exists: every reference still resolves and every other test stays
green while the second annotation silently overwrites the catalog.

**L survived the first time.** The two-slot correction above moved the
unfiltered and filtered requests into different slots, so the original
cache-key test, which compares those two, passed whether or not the key
included the filter. The collision the key actually guards is two *hiding*
modes sharing the one transient slot. A new test compares Document-Only against
Document-and-Stamps on a document with a stamp, and it kills L. Recorded because
it is the clearest case in this package of a refactor silently weakening a test
that still passed.

The plan's fifth, "one section per edit instead of per save", is an alternative
architecture rather than a local mutation, and is covered by
`ten_edits_and_one_save_are_still_one_section` asserting on parsed sections.

## An environment note

`crates/app/tests/parity_privacy.rs` shells out to `git check-ignore`, so it
fails in a checkout that is not a git repository, such as one produced by
`git archive`. With the build copy initialised as a repository it passes, along
with every other app suite.
