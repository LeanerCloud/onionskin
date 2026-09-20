# M3 P2 verification: the edit graph

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Base: `7937655`.

This records what was run against P2's verification list, and what the results
do and do not claim. Everything below was run on Linux x86-64 with the stable
toolchain, against `corpus/seeds/minimal.pdf` for the suite and a locally
generated `corpus/bench/pages-1000.pdf` for the bench. **No macOS, Windows, or
hosted-CI run is claimed here.**

## Suite

`cargo test -p onionskin-core`: 193 tests, all passing, of which 15 are
`tests/edit.rs`. `cargo fmt --all -- --check` clean. `cargo clippy -p
onionskin-core --all-targets` produces no warnings in `crates/core`; the two
warnings the run prints are pre-existing, in `crates/cos/src/document.rs:2078`
and `crates/content/src/interpret.rs:330`, and were not introduced or touched
here.

Every bullet in the plan's verification list has a test named after it. The one
that carries the package is
`second_edit_undone_restores_the_first_edit_not_the_base`: it asserts on the
intermediate state, which is the only thing that distinguishes the right
implementation from the one that reads `before` through the base alone.

## Mutation

The plan names four mutations that must break these tests. Each was applied to
`crates/core/src/edit/overlay.rs`, the suite was run, and the source restored.

| Mutation | Tests failed |
| --- | --- |
| A. `capture_object` reads the base and never the overlay | `second_edit_undone_restores_the_first_edit_not_the_base`, `an_aborted_transaction_leaves_the_overlay_and_the_counter_unchanged` |
| B. `collapse` is a no-op | `setting_a_description_then_undoing_leaves_the_trailer_as_it_was`, `undoing_every_edit_leaves_the_overlay_empty` |
| C. `before` is `None` for an object the base has | `before_is_captured_from_the_base_by_value_at_edit_time`, `no_change_carries_before_none_for_a_number_the_base_has`, `setting_a_catalog_entry_rewrites_the_catalog_and_nothing_else`, `two_producers_in_one_transaction_make_one_change_per_object` |
| D. `Change::TrailerKey`'s undo is a no-op | `setting_a_description_then_undoing_leaves_the_trailer_as_it_was`, `a_description_set_saved_undone_and_saved_again_leaves_the_file_without_one` |

All four are killed. Mutation A is the one the plan predicts every other test
survives, and it does: 13 of the 15 still pass under it.

**One deviation from the plan's wording.** It says mutation D must fail the
no-`/Info` test "and nothing else". It fails two, and both are trailer tests:
the pre-save case and the across-a-save case. The property the wording is after
holds, which is that the trailer's undo is carried by trailer-specific tests and
by nothing object-level. No object test moved under D.

## The bound

`cargo bench -p onionskin-core --bench edit_entry`, against the generated
thousand-page file:

- one 1000-page reorder: **1,440,040 bytes in a single entry**
- a hundred annotations: **207,700 bytes across a hundred entries**
- `MAX_HISTORY_BYTES`: 268,435,456 bytes

So the bound holds roughly 186 worst-case entries, and the reorder is about
seven times the whole hundred-annotation session while being one undo step,
which is the asymmetry the byte-based bound exists for. The bench asserts all
three relations rather than printing them.

## Limits

- `cargo test -p onionskin-app --no-default-features` was **not** run here: the
  app crate was not built in this environment. It is unaffected by this change
  in source terms, but that is an argument and not a run.
- The bench file is generated and gitignored, so the bench skips on a fresh
  clone. `ONIONSKIN_CORPUS_REQUIRED=1` turns that skip into a failure, which is
  what the CI bench job sets.
- Nothing here exercises a save path end to end beyond appending one section to
  a byte buffer and reopening it. P3 owns the save, and the `edit, save, undo,
  save` object-graph comparison the plan assigns to P3 is still P3's to write.
