# M3 P11 verification: `tools-organize`

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here; no macOS, Windows or hosted-CI
run is claimed.

Rows closed: 35 blank page, 41 rotate, 42 reorder/move, 43 insert, 44 delete,
45 extract, 47 replace, 48 copy/move between open documents, 49 renumber/page
labels. Row 50, the page grid, is P21's.

## Runs

- `cargo test -p onionskin-core --test organize`: 24 passing.
- `cargo test -p onionskin-core --test write_apis`: 2 passing.
- `cargo test -p onionskin-core --test pages`: 23 passing, including the
  external-corpus sweep.
- `cargo test -p onionskin-tools-organize`: 16 passing.
- `cargo test -p onionskin-plugin-api`: passing, including two new requirement
  tests.
- `cargo test -p onionskin-app --no-default-features --features tools-organize`:
  passing (the new CI line).
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`: 569
  pass, 6 fail - the environmental set in `known-issues.md` (it varies between
  runs), none touching this package.
- `cargo test --workspace --no-fail-fast`: everything passes except the three
  pre-existing `cos` failures recorded in `known-issues.md` (`lazy` on
  isartor-6-1-8, and the two `sections` tests that read a `hayro` fixture).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. The shell
  clippy (`--features shell,shell-test-support`) is clean apart from Linux-only
  dead code in `a11y/mod.rs`, a macOS-only path that CI lints on macOS.
  `cargo fmt --all -- --check`: clean.

## The importer, and the render comparison that decides it

`core::pages::import` copies a page and everything it transitively reaches -
content streams, resources, fonts and font programs, images, form XObjects,
annotations and their appearance streams - renumbering every reference through
one map. A number is reserved before its object is read, so a cycle terminates.
References to other pages of the source become `null`, unless that page is
being imported too, in which case they follow it; `/Parent`, `/B` and
`/StructParents` do not come.

It writes into the transaction's overlay through a `Sink`. Extract uses the
same copier into a fresh object list for `write_new`, so extract cannot be
shallower than insert.

**The fixture.** `corpus/organize/embedded-font.pdf`, committed and
byte-reproducible from `make-organize.py`: DejaVu Sans subset in a
`/FontFile2`, an image, a self-referencing form XObject and an annotation with
an appearance stream.

**The mutation the plan names** - the importer copies only the page dict,
leaving its references at the source's numbers - was run.
`an_inserted_page_renders_exactly_as_it_did_in_its_source` inserts into a
twelve-page destination with **more objects than the source has**, so every
stale reference resolves to *something*. Under the mutation the test's own
structural assertions (page count, `audit_references` clean) **pass**, and it
fails at the pixel comparison: `18344 pixels differ from the source's render`.
That is the plan's claim demonstrated - structure passes and only pixels catch
it.

The plan says the mutation should fail "the render comparison and nothing
else". It fails eight other tests too, and that is stronger rather than weaker:
the text-order tests extract glyphs through fonts that are no longer there,
the cycle test finds `/Self` naming the source's number, and the smaller
destinations dangle. The render comparison is the one built so that nothing
structural can catch it first.

Other importer mutations:

| Mutation | Fails |
| --- | --- |
| No reserve-before-read (cycle) | the cycle test and 8 others: the copy runs to the 200,000-object cap |
| Foreign pages followed | `a_reference_to_another_source_page_is_nulled_unless_that_page_came_too` only |
| Encrypted check removed | the insert, replace and extract refusal tests |

## The encrypted-source rule, in both shapes

- **Per input, at execution:** insert-from-file, replace and copy/move between
  open documents run `core::protection::read_out` inside the importer, because
  the source is picked after the command runs. Insert and replace are asserted
  **separately**, each on the saved output: the refused edit appends nothing, so
  the bytes are the original's.
- **Session-scoped:** extract. `plugin-api::CommandEffect` is new - `Reads`,
  `Edits`, `ReadsOut` - and `Session` gains `read_out_refusal`, so a command
  declaring `ReadsOut` is disabled by the same query that disables edits. P11
  registers no `ReadsOut` command, because extract needs a path and commands
  take none; `extract_pages_to` is the function P21's dialog calls, and it is
  refused by `core` with no file written. The requirement is tested in
  `plugin-api` over a stub.

## One transaction per operation, and whose undo

- **Replace is one undo step**, asserted: one `undo` empties the overlay and a
  second finds nothing.
- **Move between documents** is a copy in the destination and a delete in the
  source, each one step in its own document. The source's undo puts its page
  back and the destination keeps its copy. Moving every page out is refused
  before either document is touched.
- **Rotation writes `/Rotate`**, read back raw from a fresh parse. It composes
  with an inherited value: 270 plus a quarter turn is 0, a negative turn wraps,
  two half turns are none.

## Two bugs found on the way, and fixed

**Removing several tagged pages at once put the first one back.** P5 called
`remove_page` once per page, and each call rewrites the structure root's `/K`
and `/ParentTree` from the tree it was handed - the same tree every time. So
the second removal wrote back the first one's element, and then the reorder,
working from the same tree, appended every removed element at the end of the
reading order. `structure::check` passed over both, because an emptied element
is legal. Fixed with `remove_pages` (one rewrite for all pages) and a reorder
that drops kids on pages no longer in the order. Mutations: per-page removal
fails `deleting_several_tagged_pages_removes_every_one_of_their_elements` (on
the `/ParentTree`); the old reorder fails three tagged tests.

**A second page edit in one session was handed the file's structure tree.**
Same mechanism across edits: the tree is read once per edit, and reading it
from the file puts back what the previous edit removed. `Document::edit_pages`
reads it from the session's preview instead, and the page operations take the
tree as a parameter so no caller can quietly read the wrong one. Mutation:
reading from the file fails
`consecutive_page_edits_in_one_session_keep_the_structure_valid`. The comment
tools have the same class and are recorded in `known-issues.md`.

## No `cos` write API in `core` or a plugin

`Transaction::set_object` became `put_object`, so `core`'s own write shares no
name with `cos`'s. That lets `write_apis.rs` ban `cos`'s five write methods -
`add_object`, `set_object`, `delete_object`, `set_trailer_entry`,
`set_info_field` - by name, over the parsed syntax of every file in
`crates/core/src` and every `plugins/*/src`, with no allow-list guessing which
receiver is which. `verb.rs`'s private `set_info_field` became
`write_info_field` for the same reason. The scanner's own test proves it sees
method and path calls and not comments.

## The session and the shell follow the page count

`Document::page_count` walks the overlay when an edit is pending, cached per
edit epoch, so an inserted page counts before it is saved and an undone delete
gives its page back. A registry command that changes the edit epoch makes the
canvas rebuild its layout from the edited document, keeping mode, cover,
rotation and zoom and staying on the same page number, and drop every raster.
Tested both ways: a delete leaves the viewport one page shorter and a rotation
swaps the page's width and height; a no-op move leaves the view history
intact. Removing the relayout fails the first.

## The commands, and where they are reachable

Seven commands on the page the viewport is on: rotate either way, insert a
blank page (after it, at its size), move earlier or later, delete, and number
pages from 1. Each declares `CommandEffect::Edits` and carries no keybinding,
so T9's window test is not owed. They are in the Edit menu, disabled with the
document's own reason on an encrypted document; and the canvas context menu's
**Page Commands** entry, which could not have run anything, is now **Rotate
Page**, beside the view's Rotate Clockwise, running the registered command.

The selection-taking forms - several pages, another document, a file, a path -
are public functions for P21's grid and dialogs.

## Not done here

- **The page grid** (row 50) and every dialog: P21.
- **Insert from clipboard.** Nothing in the shell hands a PDF on the clipboard
  to a command yet; the function it would call is `copy_pages_between`.
- **Find after a page edit** searches the file as opened; recorded in
  `known-issues.md`.
