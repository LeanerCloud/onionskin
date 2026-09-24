# M5 verification: tag integrity (guarantee test 8)

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed. The PDF/UA files were checked out from
`veraPDF/veraPDF-corpus` at the revision `corpus/fetch.sh` pins, because
the container's proxy refuses the tarball download `fetch.sh` uses.

## Rows

- **To `implemented`:** Keep the structure tree valid through every edit.
- **Headline:** 102 planned / 42 partial / 80 out-of-scope, 179
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.
- **Guarantee 8** is no longer ignored in `crates/app/tests/guarantees.rs`.
  It names its enforcing suite, as guarantees 3 and 7 do.

## What is guaranteed

Editing a tagged document leaves its structure tree valid and consistent
with the edited content, as `tools-accessibility`'s own checker judges it.

## How it works

- **The checker** (`plugins/tools-accessibility/src/checker.rs`) reports
  four kinds of finding:
  - `core`'s invariant, the tree against the document's objects: pages and
    objects it names exist, the `/ParentTree` resolves, and the rest;
  - an element naming marked content its page does not open;
  - a page opening marked content no element names;
  - a page opening marked content without `/StructParents`.

  `Report::new_since` gives what a report finds that an earlier one did
  not.
- **content** gains `page_mcids`: the `/MCID`s a page's own content opens,
  written in the `BDC` or named in `/Properties`, leaving out those inside
  forms.
- **The enforcing suite** (`plugins/tools-accessibility/tests/guarantee.rs`)
  puts each document through the edits below, as the plugins make them.
  After each one it checks that the checker finds nothing it did not find
  before:
  1. Edit Text: a line rewritten;
  2. Replace All: a word replaced by itself everywhere, rewriting every line
     that holds it;
  3. Add Text;
  4. Create Link: an annotation with its `/Link` element;
  5. Rotate Page;
  6. Move Pages: the last page to the front;
  7. Delete Page: the last page.
- **Two sets.**
  - A three-page tagged fixture, with a document element over a heading
    and two paragraphs a page. It must be whole before, take every edit,
    and be whole after.
  - Every tagged file in `external/verapdf/PDF_UA-1` and `PDF_UA-2`: at
    least 400 files and four applied edits a file. A skip is loud, and it
    fails when `ONIONSKIN_CORPUS_REQUIRED` is set. CI reruns the suite that
    way.
- `corpus-testing` gains `pdfs_in`, the recursive walk.

## What it found, and the fixes

Each fix has its own regression test in `core`, and each was confirmed to
fail without the fix.

- **A deleted page's new content dropped from the section.** Replace a
  page's text, then delete the page. The page's changed dictionary stayed
  in the incremental section, but the content stream it names was dropped
  as unreachable, so the file named an object it did not hold and no later
  read could open it. The overlay's reachability now starts from every
  changed object the base has
  (`a_page_deleted_after_its_text_was_edited_leaves_a_whole_file`).
- **Every surviving element's marked content dropped on a page delete.**
  The removal hook dropped every bare MCID from every element that stayed,
  not only from elements on the removed pages. Deleting or extracting a
  page from a tagged document left the other pages' content named by
  nothing. The hook's test now asserts that the surviving element keeps
  its marked content.
- **Structure spanning a removed page dropped.**
  - A parent on a removed page was emptied even when a child sat on a page
    that stays. Removal is now worked out children first
    (`a_parent_on_a_deleted_page_stays_for_a_child_that_does`).
  - The reorder after a removal dropped any root kid whose first page went,
    such as a document element over every page. It is now ranked by its
    first surviving page
    (`deleting_a_page_keeps_an_element_that_spans_it_and_a_page_that_stays`).
- **A `/ParentTree` key taken twice.** A new annotation's element took the
  next key the tree did not list, even when a page held that key. Keys held
  by pages and annotations now count as taken
  (`a_key_a_page_holds_is_taken_even_when_the_tree_lost_it`).

## Runs

- `cargo test -p onionskin-tools-accessibility`: 5 pass.
  - `checker.rs`: an untagged file; each kind of finding on a file written
    to break each rule; `new_since`; the plugin's manifest.
  - `guarantee.rs`: the fixture through every edit, whole throughout; the
    PDF/UA sets, 432 tagged files and 2117 edits checked.
- `cargo test -p onionskin-content --test marked`: 1 pass.
- `cargo test -p onionskin-core`: every suite passes, the four new
  regression tests included, with `structure.rs` running against the
  fetched PDF/UA and Isartor files.
- `cargo test -p onionskin-corpus-testing`: 1 pass (`pdfs_in`).
- App integration tests, guarantees included: 42 pass, 2 ignored
  (guarantees 4 and 7).

## Coverage

`cargo tarpaulin -p onionskin-tools-accessibility` with optimisation off:

- `checker.rs`: 37 of 38 lines, before the manifest test was added.
- `core/structure/maintain.rs`: 198 of 249 lines under this crate's tests
  alone. Its own suite, `core --test structure`, covers the rest.

## Mutations

Each was caught, then reverted.

- An element's bare MCIDs dropped on any page removal: both guarantee
  tests fail.
- The checker not reporting content no element names:
  `each_way_the_tree_and_the_content_disagree_is_found` fails.
- Before their fixes, the reorder and parent-key bugs each failed the
  PDF/UA walk, on `7.18.1-t03-pass-f.pdf` and `7.20-t02-fail-a.pdf`.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support` reports only the existing
`a11y::Shared::record` warning. `cargo fmt --all --check` is clean.

## Not claimed

- **The right tree, not only a whole one.** The guarantee proves that
  edits break nothing the checker can see. It does not prove that the tree
  after an edit is the one a careful author would write. For example, new
  text from Add Text is left untagged, and a replaced line keeps its
  element. Authored before-and-after trees (`corpus/tagged/README.md`,
  steps 2 to 5) would add that.
- **The checker is structural.** It is not a PDF/UA or Matterhorn checker:
  alt text, headings, tables and language are not judged. The checker Acrobat
  users run from the Accessibility tools is the M6 row.
- **Forms' own marked content.** A form XObject's `/StructParents` content
  is not compared with the tree.
