# M3 P20a verification: the Comments pane

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain, GPUI's test platform. No macOS, Windows or hosted-CI run is
claimed.

This is the first part of P20: the pane itself. The context menu, the
properties inspector, the quick actions, the find bar's Include Comments,
read and unread, and the encrypted-source sweep follow in P20b.

Rows closed:

- 29 Comments pane (list, sort, filter, reply, status).

Rows moved forward, still `partial`:

- 75 Comments list: sort, filter, reply, set status, checkmark,
  read/unread. Everything but read and unread.
- 78 Commenting preferences. The author name is read from
  `preferences.json` (`"commenting_author"`) and signs every new comment,
  reply and status. The Preferences dialog has no field for it yet.

## What the user gets

- **The pane:** a Comments button (❝) at the end of the navigation strip.
  The pane lists every comment in the document, including ones the file
  arrived with. Each comment shows:
  - its kind, author, status and checkmark;
  - its page and date;
  - its text;
  - its replies, indented under it.
- **Sort and filter:** controls above the list.
  - Sort steps through Page, Author, Date (newest first) and Type.
  - Type, Author and Status each step through the values present in the
    document, then back to All.
- **Commands:** clicking a comment chooses it and goes to its page. The
  chosen comment offers:
  - Reply;
  - Edit Text;
  - Accepted, Rejected, Cancelled, Completed, Clear Status;
  - Check or Uncheck;
  - Delete.
- **Typing a reply or an edit:** Enter saves, Escape cancels, and the keys
  go back to the shell afterwards, so Undo works straight away.
- **Undo:** every command is one undoable step.
- **Orphaned replies:** a reply whose comment is no longer in the document
  is listed at the end, with its own author and text.
- **Refusals:** a document that may not be edited disables every command
  with its reason.

## How it is built

- **Replies and statuses are Acrobat's.** A reply is a hidden `/Text` with
  `/IRT`. A status is a hidden `/Text` with `/IRT`, `/State` and
  `/StateModel` (`Review` or `Marked`). The comment's status is the newest
  such answer; nothing is written onto the comment itself.
- **The pane reads the edited document.** It reads through
  `Document::annotations`, which goes through `structure()`, so a comment
  made a moment ago is listed and one deleted a moment ago is not.
- **The pane follows edits made elsewhere.** The frame compares the active
  document's edit epoch each time the canvas notifies
  (`follow_document_edits`). A change makes the open pane read again, and
  refreshes the menus and the tab's dirty mark. That also closes the P18
  gap where an edit made with a tool did not redraw the dirty mark.
- **Undo, Redo, Save and Revert read the open pane again** instead of
  emptying it, so the chosen comment stays chosen.
- **Delete takes the comment's answers with it.** A reply left behind
  would answer nothing.
- **Code layout:** `panes/comments/` is split into `model.rs` (the list,
  sorting, filtering and status, with no drawing), `actions.rs` (what each
  command writes) and `view.rs` (drawing and the accessibility tree, both
  built from the same command list).

## Runs

Window tests are in `tabs/tests/comments.rs`:

| Test | What it asserts |
|---|---|
| `a_comment_is_replied_to_given_a_status_checked_edited_and_deleted_from_the_pane` | A note with an author is listed as "Note · Zoe". A reply typed and saved with Enter becomes an `/IRT` answer and is listed under the note. Accepted and the checkmark show on the row, and the note itself carries no `/State`. Undo, pressed with the platform keystroke, takes the checkmark back and leaves Accepted, without reopening the pane. Edit Text starts from the current text and saves the new one. Delete leaves no annotation at all. |
| `a_comment_made_while_the_pane_is_open_is_listed` | A note added to the document while the pane is open is in the list without reopening it. |
| `sorting_and_filtering_step_through_their_values` | The controls show their values and step through them. |

Unit tests:

- `model.rs`, 5 tests on a fixture of highlight, note, rectangle and link
  with replies and statuses:
  - the link is left out;
  - status answers are not replies, and the newest status wins;
  - an orphaned reply is listed;
  - each sort's order;
  - each filter's membership and its cycle back to All;
  - PDF dates.
- `view.rs`:
  - a row's wording;
  - the command list;
  - a refusal disabling every command with its reason.
- `comments/mod.rs`: the sort cycle, and a new document forgetting the
  choice.
- `preferences.rs`:
  - the author is trimmed;
  - a blank author is none;
  - a number is refused with "a name in quotes";
  - the round trip.

Suite results:

- `cargo test -p onionskin-app --features shell,shell-test-support`: the lib
  has 671 pass and 6 fail. The 6 are the environmental set in
  `known-issues.md`: the snapshot, zoom-raster and unmeasured-page tests and
  the three rollback tests. Every integration test binary passes.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support --lib -- comments file panes preferences`: 148
  pass. The comments tests add their notes through `core`, so they need no
  tool plugin.
- `cargo clippy` for the app, in both feature sets, is clean apart from the
  Linux-only dead code in `a11y/mod.rs`.

## Mutations

- **Not calling `follow_document_edits`** fails
  `a_comment_made_while_the_pane_is_open_is_listed`.
- **Leaving focus in the closed draft field.** Before focus was handed back
  to the shell, Undo after a reply did nothing, and the first window test
  failed at the checkmark assertion. That is how the bug was found.

## Not done here, and said

- **Read and unread**, the context menu, the properties inspector and
  "make current properties default", the quick actions, and the find bar's
  Include Comments: P20b.
- **A corpus file with comments Onionskin did not write** is not in the
  seeds. The model's fixture stands in for one; the `external/` run is
  P20b's.
- **Placement:** the pane is in the left navigation column. Acrobat shows
  its Comments list on the right, and the right-hand panel's comment content
  arrives with the properties inspector.
