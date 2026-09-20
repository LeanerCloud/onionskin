# M3 P4 verification: the tagged-PDF structure tree

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Built on P2.

Linux x86-64, stable toolchain. **No macOS, Windows or hosted-CI run is claimed
here.**

## Suite

`cargo test -p onionskin-core --test structure`: 9 tests, all passing.
`cargo clippy --workspace --all-targets -- -D warnings`: exit 0, which is the
gate P4 names. Reaching it needed three pre-existing findings cleared first, in
`crates/cos/src/document.rs`, `crates/content/src/interpret.rs` and
`crates/app/tests/guarantees.rs`; those are a separate commit and are not part
of this package's code.

## The oracle, and what it is not

veraPDF is a Java tool and is not available in this workspace, so the reference
implementation here is **pikepdf 10.5.1**, a binding over qpdf, independent of
everything in this repository. `corpus/tagged/derive.py` produced the numbers
and re-derives them on demand. The reader was written from the specification,
not from the script, so agreement between them is a real check rather than a
restatement.

| Fixture | Elements | Pages | Agreement |
| --- | --- | --- | --- |
| `Isartor test files/doc/Isartor test suite manual.pdf` | 374 | 20 | element count and the full 13-entry page-to-element mapping |
| `PDF_UA-1/7.2 Text/7.2-t27-pass-a.pdf` | 32 | 1 | both |
| `PDF_UA-1/7.2 Text/7.2-t15-pass-a.pdf` | 23 | 1 | both |
| `PDF_UA-2/8.2 …/8.2.5.26-t01-pass-a.pdf` | 23 | 1 | both |

The Isartor manual is the one carrying weight: 20 pages, an `/IDTree`, and 374
elements spread unevenly across 13 of the pages. The other three are
single-page and check the reader on ordinary tagged text and on a table.

## The invariant is not vacuous

Three mutations were applied, run, and reverted.

| Mutation | Tests failed |
| --- | --- |
| E. the invariant returns a clean report unconditionally | `the_invariant_fails_on_a_page_removed_without_the_hook` |
| F. the reorder hook does nothing | `reordering_rewrites_the_root_sequence_to_match_the_new_page_order` |
| G. the `/Pg` clause is dropped, leaving a `/K`-only invariant | `the_invariant_fails_on_a_page_removed_without_the_hook` |

E and F are the two the plan names. **G is not in the plan and was added to test
the plan's own claim**, which is that a `/K`-only invariant passes while the
tree points at a page that is gone. It does: with `check_element_pages` removed,
eight of the nine tests stay green and the broken fixture reports clean. That is
the clause earning its place, demonstrated rather than asserted.

## What the hook was checked against

A hand-built two-page tagged document, because the structure has to be the only
variable and `corpus/external/` is absent on a fresh clone. Page 2 removed from
the page tree **without** the hook leaves `ElementPageMissing`; the same removal
**with** the hook leaves a clean report. The pair is what makes the first result
a statement about the hook rather than about page removal.

Hostile shapes: a cyclic `/K` is walked once and terminates with both elements
read; an unsorted `/ParentTree` `/Nums` reads completely, because the walk
collects pairs into a map rather than trusting the order.

The untagged path asserts on the returned `Maintenance::Untagged` from all three
operations, not on the document being unchanged, which is the distinction the
review risk asks for: "nothing to do" and "done" must not look alike. A
`/StructTreeRoot` that is present but is not a dictionary is an error, which is
the third state.

## Limits and findings

- **A pre-existing `cos` failure surfaced.** Fetching `external/verapdf` turns
  on `crates/cos/tests/lazy.rs`'s
  `every_parsed_object_records_the_bytes_it_came_from`, which fails on
  `isartor-6-1-8-t01-fail-a.pdf`. It reproduces with `crates/cos` unmodified and
  has nothing to do with P4. Recorded in `known-issues.md`; not diagnosed here.
- **`corpus/tagged/` is half built.** P4 added the derivation over the veraPDF
  set. The vendored deep-structure documents with authored before-and-after
  expectations are still absent and still wait on `tools-accessibility`, for the
  reason that directory's README already gives.
- **`/ParentTree` keys are not renumbered** on a page removal; cleared slots are
  left in place. Renumbering would have to rewrite every `/StructParents` and
  `/StructParent` in the same transaction, and a mapping that is well-formed but
  off by one is the "well-formed and wrong" state this package's review risk
  names. A key with an empty slot list is well-formed and right.
- `/RoleMap` is read but not resolved, which is what the plan scopes out of M3.
