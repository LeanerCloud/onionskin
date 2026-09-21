# M3 P20b verification: the comment surfaces around the pane

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain, GPUI's test platform. No macOS, Windows or hosted-CI run is
claimed.

P20b is the rest of P20 after the pane itself (P20a). This file grows with
each part as it lands.

## Quick actions: Comment, Highlight and Draw

Row flipped: Quick action toolbar, M2 `partial` to `implemented`.

**What the user gets.** The floating toolbar's Comment, Highlight and Draw
buttons are live. Comment places a sticky note, which is Acrobat's Add
Comment quick action. Highlight is the highlighter and Draw is the pencil.
Fill text fields and Add Sign stay disabled with their M5 reason.

**How it is built.** The buttons were already resolved through the
registry. What changed is which tool a button picks: the first tool made
for the action, meaning its first listed capability, and only then the
first that lists it at all. Without that, Comment picked the highlighter,
which lists Comment second because a highlight is a comment, and is
registered before the sticky note.

**Runs.**

- `quick_actions::tests::comment_highlight_and_draw_are_live_on_the_tools_acrobat_uses`
  reads the registry the app builds. It asserts each button is enabled with
  no reason, and that the tools are `sticky-note`, `highlight` and `ink`.
- `multiple_capabilities_enable_every_match_and_a_tool_made_for_the_action_wins`
  replaces the old "first matching tool wins" test. A later tool made for
  drawing now wins Draw over an earlier one that only also draws.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib --
  quick_actions`: 18 pass.

## Read and unread, and the context menu

Rows:

- Comments list: sort, filter, reply, set status, checkmark, read/unread
  moves from `partial` to `implemented`.
- Comments list context menu moves from `planned` to `partial`. Properties
  and Make Current Properties Default arrive with the properties inspector.

**What the user gets.**

- **Unread comments:** someone else's comment is unread until it is chosen
  in the list. An unread row has a dot before its heading and its text in
  semibold. A screen reader hears "Unread." first.
- **Your own comments:** a comment signed with your commenting name counts
  as read.
- **Mark as Read / Mark as Unread** switches the mark on the chosen
  comment.
- **Right-click on a row** chooses that comment and opens a menu with the
  same commands as the row's buttons:
  - Reply;
  - Edit Text;
  - the review statuses;
  - Check;
  - Mark as Read or Unread;
  - Delete.

  Running any entry closes the menu.

**How it is built.**

- **Read marks are the reader's own.** They live on the document's canvas
  (`canvas/comment_reads.rs`), so they survive switching tabs, and they go
  when the tab closes. They are never written to the file: marking a comment
  read is not an edit, does not move the edit epoch and does not make the
  document dirty.
- **On a document that may not be edited,** every command that writes is
  disabled with the reason. Mark as Read stays live, because it writes
  nothing.
- **One command list.** The row's buttons, the context menu and the
  accessibility tree are all built from `comments/commands.rs::commands`, so
  they cannot offer different things.

**Runs.**

- `a_comment_is_unread_until_opened_and_marking_it_writes_nothing`: someone
  else's note is announced "Unread.", choosing it reads it, and Mark as
  Unread makes it unread again. The edit epoch does not move.
- `the_context_menu_runs_the_rows_commands_on_the_comment_it_was_opened_on`:
  the menu is in the tree with Reply and Delete. Its Accepted entry sets the
  status, and the menu closes.
- Unit tests:
  - `comment_reads.rs`: someone else's comment is unread until marked, and
    your own is read unless you mark it unread.
  - `commands.rs`: the full command list, and the refusal rule that spares
    Mark as Read.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib --
  comment`: 21 pass.

## Every comment is signed

**What changed.** Until now only the stamp and attach-file tools put the
author on what they placed. Sticky notes, text boxes, typewriter text,
callouts, highlights, underlines, strikethroughs, inserted and replacement
text, lines, arrows, rectangles, ovals, polygons, connected lines, clouds
and ink were all unsigned. Acrobat signs every comment, and the Comments
pane lists by author, so an unsigned comment is one the author filter
cannot find.

**How it is built.** One `Signer` in `tools-comment/src/place.rs` is
configured from the shell's `ToolEnvironment` and puts the name on the
annotation's `/T`. Each tool holds one. With no name chosen the comment
stays unsigned; the operating system's account name is never used.

**Runs.**

- `tools-comment/tests/signing.rs`: a sticky note, a text box, a rectangle
  and an ink stroke are each signed "Ana Pop" when configured with that
  name, and unsigned when configured with none.
- `cargo test -p onionskin-tools-comment`: every test binary passes.
