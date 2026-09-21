# M3 P13b verification: bookmark and attachment authoring

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here; no macOS, Windows or hosted-CI
run is claimed.

Rows closed:

- 25 Bookmarks: create, rename, nest, set destination, delete.
- 26 Attachments: add and delete.
- 27 Bookmarks pane context menu.
- 28 Attachments pane context menu.

## Runs

- `cargo test -p onionskin-core`: every suite passes, including:
  - 8 new tests in `tests/bookmarks.rs`;
  - 2 new tests in `tests/embedded.rs`;
  - 1 new test in `tests/edit.rs`.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  637 pass and 6 fail. All six new window tests in `tabs::tests::outline`
  pass. The 6 failures are the environmental set in `known-issues.md`:
  - the snapshot rotation test;
  - the unmeasured page test;
  - the frame-open test;
  - the three rollback tests.
- `cargo clippy --workspace --all-targets` and the app in both feature sets:
  clean, apart from the Linux-only dead code in `a11y/mod.rs`. `cargo fmt
  --all -- --check`: clean.

## An edit-model defect found and fixed (P2)

Undoing a delete of something made earlier in the same session failed.

**How it happened.**
1. A transaction's commit collapses the overlay.
2. Collapse's rule 2 drops any object this session created that nothing
   names any more.
3. That drop was not recorded in the history entry.
4. So undoing the delete restored the reference and not the object. The
   outline then named a dictionary the overlay no longer had, and
   `structure()` failed with `DanglingReference`.

**Who else was exposed.** The same sequence reaches any object made and
then orphaned in separate steps: a comment added and then deleted, or an
attachment added and then deleted.

**The fix.** `Transaction::drop_orphans` runs at commit. It records each such
drop as an ordinary change, with the object as its `before` and nothing as
its `after`:

- undo puts the object back;
- redo takes it away again;
- an object created and orphaned in one transaction merges to a no-op, so
  nothing changes for it.

Collapse still runs afterwards. It now drops nothing that was not already
recorded.

**Tests.**
- `edit::an_object_orphaned_by_a_later_edit_comes_back_on_undo_and_goes_on_redo`
  asserts this directly on the overlay. With `drop_orphans` removed, it fails.
- So does the bookmark test that found the defect.

## Bookmarks (core)

**Addressing.** `core::outline::write` addresses a bookmark by its path: its
index among its siblings at each level, which is the shape
`Document::outline` returns.

**Five operations.**

| Function | What it does |
|---|---|
| `add_bookmark` | Adds at an index under a parent. |
| `rename_bookmark` | Changes `/Title` and nothing else. |
| `set_bookmark_destination` | Sets or clears the destination, and drops any action. |
| `delete_bookmark` | Deletes the bookmark and its subtree. |
| `move_bookmark` | Moves it. Nesting and un-nesting are both moves. |

**One traversal.**
- `outline::chain::Walk` is the only walk over sibling chains. It visits
  each item once and within a budget.
- The reader was refactored onto it, and the writer loads its tree through
  it, so the two cannot disagree about which items exist or where a cycle
  stops.
- P5's page fix-up (`pages/outline.rs`) keeps its own transaction-side walk.
  Moving it onto `Walk` is a refactor of P5's code and is not done here.

**Stored from the tree.**
- Every `/Parent`, `/Prev`, `/Next`, `/First`, `/Last` and `/Count` is
  derived from the tree.
- Only dictionaries that changed are written.
- A removed subtree is never written. Its dictionaries stay in the file,
  unreferenced, and nothing is freed.

**Decided, and asserted.**
- **Destinations are explicit**: `[page /XYZ null null null]`. The test that
  deletes the destination page runs through P5's fix-up. The bookmark is
  dropped, and `bookmarks_dropped` counts it and its child, so the choice
  survives the fix-up.
- **Deleting a parent removes its children.** This matches Acrobat and P5's
  own rule. Undo restores the whole subtree.
- **`/Count` keeps its sign.** A closed item stays closed after a sibling is
  added. The root counts only what is visible: 2 in the test, not 3.
- **Moving a bookmark into its own subtree is refused.**
- **A path with no bookmark is refused** with `NoSuchBookmark`, and no edit
  is recorded.
- **A document with no outline gets one.** Undo takes `/Outlines` out of the
  catalog again.

**The round trip.** A nested outline is saved, reopened and read back through
`outline::read` with its shape and pages. `audit_references` is clean.

**Mutation.** Making `add_bookmark` ignore its parent flattens the outline.
It fails 6 of the 8 bookmark tests, including the nesting round trip.

## Attachments (core)

**Delete.** `embedded::remove_attachment(tx, stream)` deletes every name-tree
entry whose file specification embeds that stream, and takes every file
attachment comment carrying it off its page.
- It returns how many places named the file. A second delete finds 0.
- The name tree is rewritten through the same helper Add uses, so there is
  one writer for it.

**What the delete test asserts.** It deletes one of two entries, saves and
reopens, then checks:
- only the other entry is listed;
- `audit_references` is clean;
- the stream is still in the file;
- the appended section, which is a classic xref, has no free entry.

**Refactor.** `mime_for` moved from `tools-comment` into `core::embedded`, so
Attach File and the pane's Add share one table.

## The panes (app)

**The bookmarks context menu.**
- Six entries: New Bookmark, Rename Bookmark…, Set Destination To Current
  Page, Nest Under Bookmark Above, Move Out One Level, and Delete Bookmark.
- It opens on the right-clicked row, or on none.
- The entries that need a row say so. Nesting needs a bookmark above, and
  moving out needs a parent.
- On a document that may not be edited, every entry is disabled with that
  document's reason, which the test asserts on the encrypted fixture.
- New and Rename ask for a title in a small dialog (`BookmarkTitle`). New
  first adds an "Untitled" bookmark after the chosen row, at the current
  page, then opens the dialog on it. A blank title is refused in the dialog.
- The window test makes two bookmarks, nests one, renames it, and deletes
  the parent. It checks the outline through the session after each step,
  and checks that the pane re-reads.

**The attachments pane.**
- It gains an Add Attachment… button, which is there even when the list is
  empty, and a Delete on each row.
- Its context menu has Add Attachment…, Open, Save and Delete. Each entry
  runs the same activation as the matching button.
- The window test adds a file with `attach_file`: the frame's half after the
  file dialog, which the test platform cannot show. It then reads the row's
  name and detail ("text/plain · 17 bytes"), deletes the file through the
  menu, and asserts that the list and the session are both empty.
- A file that cannot be read is named in the pane's feedback.

**Both menus in the accessibility tree.** Both menus are `Role::Menu` nodes
while open. The test asserts that each one leaves the tree after an entry is
run.

## Not done here, and said

- **No registered commands.** The plan listed
  `commands-core/src/{bookmarks,attachments}.rs`. Every authoring operation
  needs an argument the registry's `CommandCtx` cannot carry: a path, a
  title, or a file. So these are pane behaviour over `core`, like the
  Properties dialog. A bare "Add Bookmark at this page" command could still
  be registered for a keystroke; it is not added here.
- **Dragging bookmarks to reorder them** is not built. Nest and Move Out
  cover the tree operations.
- **New Bookmarks From Structure** is M6, as the plan says.
- **Editing an attachment's description** is not offered.
- **Open** stays disabled until M5's trust list.
