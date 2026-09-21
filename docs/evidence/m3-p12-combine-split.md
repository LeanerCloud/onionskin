# M3 P12 verification: combine and split

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here; no macOS, Windows or hosted-CI
run is claimed.

Rows closed: 34 Create from multiple files, 37 Combine files into a single
PDF, 38 Add files / add folders, 39 Reorder, preview and remove entries, 40
Expand a file and combine at page granularity, 46 Split.

## Runs

- `cargo test -p onionskin-core --test assemble`: 10 passing.
- `cargo test -p onionskin-core --test organize`: 24 passing (extract now goes
  through the assembly).
- `cargo test -p onionskin-commands-core`: 12 unit and 17 integration tests
  passing.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`: 587
  pass, 6 fail, and the 6 are the environmental set in `known-issues.md`. The
  eight new dialog tests pass.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support,commands-core --lib` (the plan's run): 568 pass, 7
  fail. The seventh is `the_thumbnails_pane_asks_only_for_the_rows_it_shows`,
  which passes alone and twice more in a row. It is the same race family, and
  it is recorded.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. The shell
  clippy, in both feature sets, is clean apart from the Linux-only dead code
  in `a11y/mod.rs` that CI lints on macOS. `cargo fmt --all -- --check`: clean.

## The primitive: `core::pages::Assembly`

An ordered list of `(document, pages)` goes in and one new document comes out.
Combine, Split and Extract all call it. So will P10's summary and P14a's
create-from-images when they land.

- **Every page is the importer's transitive copy.** The combine test compares
  every output page against its source page, by extracted text and by pixels.
  The inputs are three documents, one of them the embedded-font fixture with
  its image, cyclic form and annotation.
- **Each append is its own copy.** A document combined with itself gets
  disjoint object sets for the two copies. The mutation that aliases a repeated
  source fails `a_document_combined_with_itself_gets_independent_copies`, which
  lists the 14 shared objects.
- **Inputs are streamed.** `append` borrows a source and is done with it when
  it returns. `combine` opens each file, appends it and drops it, so a
  100-file combine holds one input and the output. That answers the review
  risk.
- **Metadata is fresh, and the choice is stated.** The output's `/Info` names
  the producer and nothing else. Taking the first input's title would be a
  guess, and merging several has no right answer. The test uses an input whose
  own `/Info` has nine keys.
- **Half-tagged is detected, not produced.** The output keeps a structure tree
  only when every input was tagged and came whole. The trees are merged under
  one root:
  - top-level elements stay in input order;
  - `/ParentTree` keys move past the previous inputs' keys, and the test
    asserts they are distinct;
  - role and class maps are merged, with the first definition winning.

  P4's invariant is clean on the merged tree. If any input is untagged or
  partial, the output is untagged. `Assembled::tagging` says which input caused
  it, and every structure-only object and key is removed. A mutation that
  skips the removal leaves orphan `StructElem` objects, and the test catches
  them.

## The encrypted-source rule, in both shapes

- **Combine checks each input at execution.** The check runs inside `append`,
  once per input. The plan's position-2-of-3 case is covered twice:
  - in `core`, the refused append copies nothing;
  - through `combine`, the error names the encrypted file and no output file
    exists.
- **Split is refused per session.** `SPLIT_DOCUMENT` is registered with
  `CommandEffect::ReadsOut`, and the File menu's Split Document entry asks the
  registry about it.
  - On an encrypted document, the entry is disabled with the document's own
    reason. This is asserted on a real window.
  - `split` itself refuses before planning, and no file is written.

## Split

| Case | Asserted |
| --- | --- |
| 10 pages at 3 | Four files; the fourth holds page 10 alone. The plan's off-by-one mutation fails it. |
| Bookmarks at pages 1, 4, 9 | Three files with those boundaries. |
| A bookmark naming no page | Reported in `Split::unresolved` and in the notice, not skipped. |
| Pages before the first bookmark | Joined to the first part, so no page is lost. |
| By size | No part over the target unless it is a single page; every page appears once and in order. The sizes are measured by assembling, because shared resources mean page sizes do not add. |
| A part name already taken | Nothing is written: every output is staged beside its destination and moved into place only when all are complete, so it is all or nothing. |

## The surface

- **File menu entries.**
  - **Combine Files…** and **Create PDF From Multiple Files…** open one dialog.
    The Create entry opens it under its own title, with a line saying it is the
    same as Combine.
  - **Split Document…** opens the split dialog.
- **Availability.** Combine is live whenever `commands-core` is compiled in. The
  dialog calls the plugin's functions directly, and a list of files is not
  something a command context can carry, so there is no registry command to
  ask. Split is live through the registry query.
- **The combine dialog.**
  - Rows 38–40: Add Files and Add Folder (PDFs directly inside the folder,
    sorted by name), Move Up, Move Down, Remove, and a page field that expands
    the selected file to a page list such as "9-10, 1, 3".
  - Each row's label is its preview: the file, its page count, and the pages
    that will be used. A file that will not open stays in the list and says so.
  - Combine runs on the background executor, writes where the user chose, and
    opens the result in a tab.
- **The split dialog** chooses between page count, size in MB, and top-level
  bookmarks. The value field leaves the dialog and the tab order when
  bookmarks are chosen. The parts are written beside the document.

Both dialogs are described in the accessibility tree in paint order. Every
control carries the same `Activation` a click sends, and the window tests
drive them through `run_activation`.

## Not done here, and said

- **The preview is text, not a thumbnail.** Each row shows the page count and
  the selected pages. Drawing a thumbnail of a file that is not open needs a
  render worker per row, which is P21's page-grid machinery.
- **Split runs on the UI thread.** A session's `Document` is not `Send`, so
  splitting it on the background executor would need a reopen from bytes. For
  now the window is busy while a large document is split. Combine opens its
  own inputs, so it runs in the background.
- **No outline in a combined document.** Acrobat can add a bookmark per input
  file. The rows do not ask for it, and `Assembly` carries no outline.
- **The dialogs sit in `shell/chrome/`**, beside `export_dialog.rs`, rather than
  in `shell/` as the plan's file list says. The text field they use is private
  to `chrome`.
