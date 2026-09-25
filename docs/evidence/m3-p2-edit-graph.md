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

## macOS headless rollback verification (2026-09-25)

This section records the uncommitted candidate at base `27a9953` with source
diff `7cd231cea2fd44b61b1208a20230dd528acd99face4ad4b70235e700ba336cb9`.
It preserves the historical Linux evidence above; it does not replace it or
claim GUI, native-app, full-corpus, hosted-CI, or Windows verification.

The rollback correction restores the exact overlay representation captured at
transaction entry. It restores whether each object or trailer slot was absent,
present, or explicitly cleared, while also restoring the reservation counter.
The retained history, redo tail, epoch, dirty state, and original bytes remain
unchanged after closure errors and orphan-scan errors.

The retained macOS headless evidence is:

- Baseline `/tmp/claude/onionskin-transaction-slice1-baseline.z8Eh7I`: one
  exact test passed and the document projection test produced the intended
  unchanged-production RED at the exact overlay assertion.
- Supplemental baseline
  `/tmp/claude/onionskin-transaction-supplemental-baseline.2uJqe6`: both exact
  tests produced the intended REDs. The first exposed leaked base-object and
  absent-trailer membership; the second exposed the orphan-scan rollback leak.
- GREEN run `/tmp/claude/onionskin-transaction-abort-green.HtNZTv`: all four
  exact rollback tests passed, and the bounded 13-target core suite passed 136
  tests, including those same four tests. The exact tests cover redo and counter restoration,
  epoch and preview stability, fresh section projection, strict COS reopen,
  and the public save/reopen path. The orphan fixture separately proves that
  the closure body completed before the malformed reachable object caused the
  orphan scan error.

The tests are headless. They do not exercise native UI behavior or establish a
full-corpus acceptance claim. The bounded save sweep recorded 192 clean and 5
repaired cases; optional and capped corpus inputs were not a full-corpus run.
The optional `qpdf --check` attachment check was unavailable, while the
mandatory pypdf 6.10.0 readback ran and passed.

## Slice 2 null-Info save and reopen verification (2026-09-25)

This slice was verified against unchanged production at integrated base
`e00eacea330349ae9917d2cd47c69d6c72a4b79f`. The literal fixture and the
scan-repaired fixture both cover a null `/Info` entry. Each test groups the
properties that matter: exact undo and redo, all saved objects, trailer
equality after removing only the validated `OnionskinSection` stamp, `Info`
reference identity, and `Info` and XMP field readback after save and reopen.

The initial EPG0SA result rejected the planned save oracle because it required
byte identity and treated the intentional save stamp as unchanged content. It
was an oracle correction, not a production bugfix. The final save oracle checks
the saved object graph and trailer while validating the stamp separately.

The bounded run passed 63 tests in the six-target surrounding suite, including
the two new scenarios, plus one exact generation-stamp test, for 64 distinct
tests. The two new scenarios also passed in separate exact runs. The run was
headless and made no GUI or full-corpus claim.
The full revised-baseline evidence is
`/tmp/claude/onionskin-transaction-slice2-revised-baseline.mCO84O`.
The optional `qpdf --check` attachment check was unavailable. The mandatory
pypdf 6.10.0 readback ran and passed.
