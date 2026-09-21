# M3 P5 verification: `core::pages`, the page-tree transformation

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P2 and P4, with
P1's reference auditor as the check that catches its mistakes.

Linux x86-64, stable toolchain. No macOS, Windows or hosted-CI run is claimed.

## Runs

- `cargo test -p onionskin-core --test pages`: 22 passing, including the sweep
  over every multi-page file in `corpus/external`.
- `cargo bench -p onionskin-core --bench pages`: both budgets met, numbers
  below.
- `cargo test -p onionskin-app --test guarantees`: 40 passing; the new suite is
  in CI's `ONIONSKIN_CORPUS_REQUIRED` rerun, and removing it fails
  `every_corpus_suite_is_rerun_with_the_corpus_required` (demonstrated).
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
  `cargo fmt --all -- --check`: clean.

## The fixture premise did not survive a sweep

The plan asks for three named `external/` fixtures of each shape, and says that
if the plan cannot name them the transformation is unproven. So the sweep ran
first, as the plan says it must, and it says the fixtures do not exist:

| Shape | veraPDF (2,907 files) | pdf.js (982 files) |
| --- | --- | --- |
| page tree more than one level deep | 1 | 17 |
| `/PageLabels` | 1 | 22 |
| intra-document `/Link` on a multi-page file | 1 | 10 |
| `/Names /Dests` on a multi-page file | 1 | 17 |
| `/AcroForm /Fields` on **more than one page** | 0 | 4 |
| `/Threads` | **0** | **1** |

The one veraPDF file in the first four rows is the same file - the Isartor test
suite manual. Every veraPDF form is a single page, so none can answer "delete
the page carrying this widget and check the others survive". Article threads
are close to extinct in real PDFs: one file in nearly four thousand.

pdf.js is not a set `corpus/fetch.sh` fetches, and a fixture CI cannot reach is
a fixture that reports a pass over nothing - guarantee 6's old failure mode. So
it was used here to size the problem, not as a fixture.

**What replaced three named files is stronger than them.** Each fix-up has a
minimal hand-built fixture carrying exactly its own shape, which is what makes
the mutation matrix below mean something: dropping one fix-up fails its own
test and no other. And the real-world breadth is a sweep over **every**
multi-page external file, asserting `audit_references` is clean and every
surviving page's four inheritable entries are unchanged after a delete.

## The mutation matrix: seven fix-ups, seven checks

The plan's sharpest requirement: dropping any one of the seven fix-ups must
fail **exactly its own fixture and no other**, which is what proves they are
seven checks rather than one. Each fix-up call was removed in turn from
`rewrite.rs` and the suite run:

| Dropped | Tests that failed |
| --- | --- |
| `/PageLabels` | `page_labels_follow_their_pages_through_a_delete_and_a_reorder` |
| destinations | `a_named_destination_on_a_deleted_page_is_dropped_and_the_rest_still_resolve` |
| outline chain | `the_outline_chain_walks_past_a_dropped_item`, `a_closed_outline_node_keeps_its_negative_count` |
| link annotations | `a_link_to_a_deleted_page_goes_and_one_to_a_surviving_page_stays` |
| `/AcroForm /Fields` | `a_form_field_on_a_deleted_page_goes_and_an_emptied_group_goes_with_it` |
| article threads | `an_article_ring_closes_over_the_beads_that_are_left`, `a_thread_whose_pages_all_went_leaves_the_catalog` |
| `/OpenAction` | `an_open_action_on_a_deleted_page_is_dropped_and_a_surviving_pages_aa_is_not` |

No row fails another row's test. Three more:

| Mutation | Tests that failed |
| --- | --- |
| Inheritance not materialized | `a_flattened_page_keeps_every_attribute_it_used_to_inherit`, `a_shared_resource_dictionary_stays_shared` |
| The flat node's `/Count` copied from the old root | `a_deleted_page_leaves_a_flat_tree_on_the_original_root_number` (and the two that read the page count) |
| The outline re-linked but not recounted | `the_outline_chain_walks_past_a_dropped_item`, failing on **the `/Count` half only**: the walk assertions before it pass, and the message is "the parent's /Count is recomputed, not copied", left 3 right 2 |
| P4's structure hooks not called | `a_tagged_document_keeps_a_valid_structure_tree_through_a_delete_and_a_reorder` |

### What `audit_references` cannot see

`every_fixture_survives_every_operation_with_no_dangling_reference` passed
under **every** mutation above. That is the finding worth stating: under T5's
free-nothing rule none of these bugs dangles. A dropped page's dictionary is
still there and still parses, so a bookmark, a link, a bead or an
`/OpenAction` naming it resolves cleanly - into a page no reader can reach. The
auditor proves the output is a well-formed file; only the per-fix-up fixtures
prove it is the same document. That is why they are seven tests and not one
sweep.

## Materialization reads the tree raw

`cos::Document::page` resolves inheritance for a reader and is lossy for the
two things materialization exists to preserve. `core::pages::inherit` walks the
dictionaries itself and carries each inheritable entry exactly as written:

- **A shared `/Resources` stays a reference.** `a_shared_resource_dictionary_stays_shared`
  asserts every rewritten page names the **same `Object::Ref`**, not an equal
  value; comparing resolved values passes on the inlined form. The bench backs
  it: 151 bytes per page is a page dict, not a page dict plus a font table.
- **A degenerate `/MediaBox` survives as written.** `cos`'s `rectangle()`
  filters a zero-area box to `None`; materializing through it would turn a page
  with a bad box into a page with none.

Materialized **before** `/Parent` changes, which is the ordering a reviewer
checks: afterwards, inheritance is read through the new flat node, finds
nothing above the page, and silently drops everything - on a deep tree only.

## Free-nothing, settled by the syntax tree

`nothing_in_this_module_frees_an_object` parses every file under
`src/pages/` with `syn` and walks for a `delete_object` call or path. The first
version grepped, and failed on `mod.rs`'s own doc comment, which states the
rule in those words - the self-matching trap this repository has now hit three
times. A comment is not a token. `syn` joins `core`'s dev-dependencies for it,
as it joined `app`'s for the same reason.

`a_removed_pages_objects_are_left_in_place_rather_than_freed` asserts the other
side: the removed page's dictionary still parses, unreachable, and nothing
dangles.

## The bench answers T5's open question

The plan refused to guess whether rewriting every surviving page dict makes the
appended section too large, and left the fallback to this number:

| Operation, 1000-page file (2.2 MB) | Time | Section |
| --- | --- | --- |
| delete the first page (999 rewritten) | 17 ms | 151,811 bytes |
| reverse every page (1000 rewritten) | 8 ms | 151,961 bytes |

**151 bytes per page, 6.8% of the file.** T5's fallback is not needed. The
budgets are 1.5 s and 1 KiB per page, so the bench fails if the rewrite stops
being linear or a page dict starts carrying an inlined resource dictionary.

## Decisions stated rather than discovered

- **`/OpenAction` naming a removed page is dropped, not retargeted.** The plan
  said to decide and assert. Retargeting has to choose a page and there is no
  honest choice; dropping makes a reader open at page one, the documented
  default.
- **A repeated page index is refused** with `Error::RepeatedPage`. Copying a
  page within a document goes through the importer, which renumbers; aliasing
  one object into two `/Kids` slots passes every structural check.
- **`/StructParents` is not renumbered.** `/ParentTree` is keyed on those
  values, not on page indices, so a surviving page's key still resolves wherever
  the page moves. P4's `remove_page` and `reorder_pages` do the rest, and P4's
  invariant is the assertion.
- **An outline heading whose children all go, and which names nothing itself,
  stays.** Removing a heading the user may still want is not this fix-up's
  business; a doomed item takes its own subtree with it.
- **`rewrite_page_tree` takes the structure tree as a parameter**, the way
  `add_annotation` does, rather than the plan's `&mut self` form: the tree is
  read once by the caller and every edit that needs it follows one convention.
- **The core error type carries the page-set refusals** rather than
  `core::pages` having its own, so a rewrite composes with `?` inside a
  transaction like every other edit.

## Not done here

- **`PageSource::Imported` is placed but not produced.** The importer that
  copies a page between documents with renumbered references is P11's; this
  module accepts its output and re-parents it.
- No `DocumentEdit` verb yet: P11 owns the user-facing delete, reorder and
  insert commands and will wrap `rewrite_page_tree` in them.
