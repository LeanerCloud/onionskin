# M3 P3 verification: save, preview and generations (stage A)

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

- **`structure()` is built but not yet the referent for every structural read.**
  `Document::structure()` exists and returns a document opened from the preview
  bytes, but `page_geometry`, `page_text`, `outline`, `attachments`,
  `signatures`, `layers` and `page_count` still read `&self.cos`. Until they are
  routed through it, the stale-read defect P3 describes is still present: after
  deleting a page, `page_count()` reports the old count. The mechanism exists;
  the rewiring does not.
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

## Mutations not yet run

The plan names five for this package. Three are covered by tests that exist
(`edit_then_undo_then_save_writes_nothing`,
`ten_edits_and_one_save_are_still_one_section`,
`the_preview_cache_key_includes_the_filter`); the reopen-skip mutation needs the
two-saves test and is covered; `Overlay::default()` in place of the reseed is
covered by `create_save_create_save_puts_the_two_annotations_at_two_numbers`.
They have **not** been run as mutations here, which stage B owes.
