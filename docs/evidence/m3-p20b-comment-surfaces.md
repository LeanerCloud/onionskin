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

## Commenting preferences: the author name

Rows:

- Commenting preferences moves from `partial` to `implemented`.
- The Preferences dialog row (M2, `partial`) now lists Commenting among its
  live categories.
- The dynamic stamps row (still `partial`, for local time) now takes its
  name from here.

**What the user gets.**

- **Where:** Preferences has a Commenting category, first in Acrobat's
  alphabetical order. It holds an "Author name" field and a Save Name
  button, and Enter in the field saves too.
- **What it signs:** the name signs every new comment, reply, status and
  dynamic stamp, in every open tab at once, with no reopening.
- **An empty name** signs nothing.

**How it is built.**

- **Saving:** Save writes `commenting_author` to `preferences.json`. It then
  hands the shell's `ToolEnvironment` to every open canvas, whose tools'
  `Signer`s take the name, and the canvas keeps it for the Comments pane's
  replies and statuses.
- **The field's own key context** binds Enter to save the name, so Enter
  does not go to the focus ring.
- **Not offered:** Acrobat's display-only Commenting options (pop-up font,
  pop-up opacity, printing notes).

**Runs.**

- `the_author_name_saved_in_preferences_signs_the_next_comment`, on a
  window:
  - the field is in the tree;
  - "  Ana Pop " typed and saved with Enter is stored trimmed, and written
    to the preferences file;
  - the open canvas signs as Ana Pop;
  - a status set from the Comments pane is signed Ana Pop.
- The preferences dialog tests now cover five categories. Commenting's
  setting is the typed name, not a choice.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  675 pass and 4 fail. The 4 are the environmental set in
  `known-issues.md`: the snapshot test and the three rollback tests.

## Comment properties, and Make Current Properties Default

Rows:

- Comment properties (colour, opacity, author, subject, default) moves from
  `planned` to `implemented`.
- Comments list context menu moves from `partial` to `implemented`: it now
  has Properties and Make Current Properties Default.
- Right-hand side panel: its note now names the inspector as its first
  tool-specific content.

**What the user gets.**

- **Opening it:** choose a comment in the Comments pane, then use
  Properties from its row or its right-click menu. The side panel shows
  Comment Properties.
- **Colour and opacity:** eight colour swatches and four opacities (100,
  75, 50 and 25 percent). Each applies on the click, as one undoable edit,
  and the comment is drawn again in its new colour and opacity.
- **Author and subject:** two fields, filled from the comment, which wait
  for Save Author and Subject.
- **Make Current Properties Default:** the comment's colour and opacity
  become the look of the next comment of its kind, in every open tab. The
  default is saved to `preferences.json` under `comment_defaults`, as
  `{"Text": {"color": "#e53935", "opacity": 50}}`.
- **On a document that may not be edited,** every control that writes is
  disabled with the reason. Properties and Make Current Properties Default
  stay live, because they change the panel and the preferences, not the
  document.

**How it is built.**

- **`core::properties::set_properties`** writes `/C`, `/CA`, `/T` and
  `/Subj` and draws the appearance again, in one edit. Full opacity and
  blank text remove their keys instead of writing defaults.
- **`core::annots::rebuild`** reads the drawable model back from the
  dictionary for every subtype `core` draws: shapes, lines with their
  endings, polygons and clouds, ink, text markup, notes and free text.
  `set_contents` now shares it. A stamp or attachment is not redrawn from
  its dictionary; its appearance is left as it was.
- **`ReadAnnotation` carries `/CA`,** so the inspector shows the opacity in
  force.
- **Defaults reach the tools through `ToolEnvironment::comment_defaults`.**
  The shared `Signer` that signs each comment also applies its kind's
  default colour and opacity. A kind with no default keeps the tool's own
  look.

**Runs.**

- `core/tests/properties.rs`:
  - colour, opacity, author and subject are written;
  - the appearance is drawn in the new colour with an opacity state;
  - Undo takes all of it back;
  - full opacity and blank text remove their keys.
- `rebuild::tests`: an arrow keeps its line, endings, colour and opacity; a
  cloud keeps its vertices and intensity; a stamp is not redrawn; `/DA`
  reads back.
- `tools-comment/tests/signing.rs::a_default_look_applies_to_its_own_kind_only`.
- `the_inspector_changes_a_comment_and_makes_its_look_the_default`, on a
  window:
  - the panel shows the inspector with the author filled in;
  - red and 50 percent are written, and red is announced as chosen;
  - the author and subject are saved;
  - the default is stored for `Text`.
- `the_next_sticky_note_takes_the_default_the_inspector_made`: end to end
  through the tool, as the plan asks. After Make Current Properties Default,
  a sticky note placed with the tool has the default's colour and 75
  percent opacity.
- `preferences.rs`: the defaults round-trip, and one malformed entry
  refuses the whole table with a message that shows the shape.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  678 pass and 6 fail.
  - 4 of the failures are the environmental set in `known-issues.md`.
  - The other 2, `an_update_that_paints_nothing_leaves_no_frame_open` (also
    listed in `known-issues.md`) and
    `snapshot_tool_drag_copies_a_background_encoded_png`, pass when run on
    their own. They are timing-sensitive under a loaded two-core machine.

## Find: Include Comments

Row: Edit > Find stays `implemented`. Its note now says Include Comments is
live and Include Bookmarks is not.

**What the user gets.** The find bar's Include Comments checkbox, which was
disabled with "Comments arrive with the comment tools in M3", is live.

- **On:** Find also looks in every comment's text and every reply's text.
- **A comment hit** is counted and highlighted over the comment's rectangle
  on its page.
- **A reply's hit** is at the comment it answers, because a reply is drawn
  nowhere.
- **Status answers** are not searched.

**How it is built.**

- **The option:** `SearchOptions::include_comments`. The page-text walk
  ignores it.
- **Finding comment hits:** when a walk begins with it on, the session
  reads the edited document's comments, so a comment typed a moment ago is
  found. It matches their text with `content::text_matches`, which uses the
  page search's own case folding, whole-word rule and Phrase, Any and All
  modes.
- **Placing them:** the hits are held in `SearchState` and joined to each
  page's hits as the walk reports that page. So they are in document order,
  and the cursor lands on the first hit on either kind.
- **Not done:** `SearchResult::Unavailable::reason` stays `&'static str`.
  Nothing in this package needed it widened, because the checkbox went live
  instead of carrying a dynamic reason.

**Runs.**

- `core/tests/search.rs::include_comments_finds_text_that_is_only_in_a_comment`:
  - text only in a note is not found without the option;
  - with the option it is found, case-insensitively, over the note's
    rectangle;
  - a reply's text is found at the note it answers.
- `find_bar::tests::include_comments_is_a_live_checkbox_that_changes_the_search`.
- `find_with_include_comments_finds_a_comments_text`, on a window: 0 hits,
  then 1 after the checkbox.

