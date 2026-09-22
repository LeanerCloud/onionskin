# M3 P22 verification: the remaining shell rows

Date: 2026-09-21. Linux x86-64, stable toolchain, GPUI's test platform. No
macOS, Windows or hosted-CI run is claimed. One section per row, each
landed in its own commit.

## Home view: Starred (row 5)

**What the user gets.**

- Every recent document's row on Home has a star: ☆, or ★ once starred.
  Choosing it stars or unstars the document without opening it.
- A Starred section under the recents lists the starred documents in the
  order they were starred, each opening its document and each with its own
  Unstar. With nothing starred it says "Star a recent document to keep it
  here."
- Stars live in the recents file on this machine, beside the recents and
  owner-only like them. Acrobat keeps them in its cloud storage.

**How it is built.**

- `Recents` gains a `starred` list next to the recents rather than a flag
  on each recent, so the recents limit never unstars a document and
  starring never changes what is recent.
- The file's `starred` key defaults to empty, so a file written before
  Starred still loads.
- `toggle_star` refuses a path JSON cannot write, as `record` does, because
  an entry that cannot be written would block every later save of the list.
- Home's rows are unchanged for a screen reader. Each gains a Star/Unstar
  button child with its toggled state, and the Starred section is its own
  list.
- **Residual, stated rather than solved.** Stars are absolute paths, like
  the recents. On screen they read with `~` for the home directory, and the
  file is owner-only.

**Runs.**

- `cargo test -p onionskin-app --lib recents`, 3 new tests:
  - stars are kept apart from the recents and survive truncation to zero;
  - they round-trip through the file, and a pre-Starred file loads with
    none;
  - a non-UTF-8 path is never starred.
- `--features shell,shell-test-support --lib home`, 2 new tests: a starred
  recent is marked on its row (label, toggled state, activation) and listed
  under Starred; an empty section says how to star.
- Window test `a_star_on_home_is_saved_and_opens_its_document`:
  - the star is activated through the accessibility tree;
  - the recents file read back from disk has the star;
  - the Starred row opens the document in a tab.

## View > Show/Hide > Line Weights (row 23): moved to M4

**Decision, carried as one branch.**

- Acrobat's toggle draws every stroke at one constant hairline width when
  it is off, which needs a constant-hairline option in the renderer.
- The hayro fork this build pins (`cristim/hayro` at `67763e2`) has no such
  option, and no fork commit landed in this package.
- So, as the plan rules, the row moves to M4 and the menu entry stays,
  disabled, with its reason: "Line Weights arrive in M4 with the renderer's
  constant-hairline option".
- The `By milestone` headline becomes M3 97, M4 5.
  `acrobat_parity_headline_matches_every_inventory_row` passes, and
  PLAN.md's M3 paragraph says the headline is the count.

**Runs.** `line_weights_is_disabled_naming_m4_and_has_no_action_route`: the
entry is disabled with that reason and the reason names M4. It has no view
action, no shell action and no native action, and the native menu item
shows the reason. Only this test exists for the row; the rendered-widths
test the plan describes for the other branch does not.

**Stale test fixed after the move.** `every_deferred_entry_names_its_own_delivery_stage`
still required every deferred View entry to name M3, and failed once Line
Weights named M4. It now accepts M3 or M4, so an entry naming no milestone
still fails. Fixed in its own commit.

## Manage Tools (row 6)

**What the user gets.**

- View > Manage Tools… opens a dialog listing every tool that has a rail
  button, in rail order, each a checkbox that is checked while the rail
  shows it. With no tools installed the entry is disabled, as Tools is.
- Clearing a tool takes its button off the rail at once, in the collapsed
  and the expanded rail. A collapsed group whose chosen member is hidden
  shows its next member instead.
- A hidden tool still runs from its menu entry, its shortcut and Tool
  Search. The tool in use stays on the rail while it is selected, so the
  rail never hides what is active.
- The choice is kept in `preferences.json` as `hidden_tools`, a list of
  tool ids, written only when something is hidden. An id no installed tool
  has is kept, since it may be a plugin's that is not installed today. A
  list with anything but strings in it is refused whole and named, like
  every other preference.
- Acrobat also lets the user reorder the rail. This does not, and the
  parity note says so.

**How it is built.**

- `chrome::manage_tools` holds the pure part: the tool list taken from the
  registry when the dialog opens (so drawing does not build a registry), the
  toggle, and the dialog's accessible rows and render.
- `RailState::entries` takes the hidden set and filters before grouping, so
  grouping and remembering work on what is shown.
- `ShellDialog::ManageTools`, `MenuCommand::ManageTools`
  (`view.manage-tools`, unbound) and `Activation::ToggleToolShown(id)`,
  which saves the preferences through the same `save_preferences` the
  Preferences dialog uses.

**Runs.**

- `preferences::tests`: the round trip now carries two hidden tools;
  `hidden_tools_are_a_list_of_ids_and_a_bad_list_is_named` covers a good
  list with an unknown id, a list with a number in it and a bare string.
- `rail::tests::hidden_tools_leave_the_rail_unless_selected`.
- `chrome::manage_tools::tests`, 3: the toggle, the rows as checkboxes with
  their state and activation, and the listed tools equal the registry's
  rail tools.
- Window tests `tabs::tests::manage_tools`, 3:
  - `a_cleared_tool_leaves_the_rail_and_the_choice_is_kept`: opened from
    the View menu, one checked row per rail tool; clearing the last one
    through its accessible activation takes it off the rail, unchecks the
    row and writes it to the preferences file; checking it again restores
    it and empties the list.
  - `the_selected_tool_stays_on_the_rail_when_hidden`.
  - `a_hidden_tool_in_the_file_is_off_the_rail_at_start`.
- **Full suites.** `cargo test -p onionskin-app --features
  shell,shell-test-support --lib`: 749 pass and 7 fail, all in the known
  environmental set (the canvas raster tests and the three export rollback
  tests). The integration tests (`guarantees`, `registry`, `contract`,
  `kernel_emptiness`, `parity_privacy`) pass, including the headline
  recount at 166 planned / 132 implemented.
- **Lint.** `cargo clippy -p onionskin-app --all-targets -- -D warnings`
  with default features, with `--no-default-features`, and with
  `shell,shell-test-support` (with and without default features, allowing
  the pre-existing macOS-only dead code): clean. `cargo fmt --all --check`:
  clean.

**Mutation run.** Dropping the "selected stays" rule (filtering hidden tools
even when active) fails `hidden_tools_leave_the_rail_unless_selected` and
`the_selected_tool_stays_on_the_rail_when_hidden`.

## View > Page Display > Automatically Scroll (row 22)

**What the user gets.**

- View > Automatically Scroll, or Cmd+Shift+H (Acrobat's Ctrl+Shift+H),
  scrolls the document in front down at a reading pace. The entry is
  checked while it runs, and choosing it again stops it.
- While it runs, Up is one speed faster and Down one slower (nine speeds,
  15 to 350 view pixels a second), minus reverses the direction, and Escape
  stops it. With no scroll running those keys are the focus ring's again.
- A wheel scroll or a press on the page pauses it. It carries on by itself
  two seconds after the last touch.
- It stops by itself at the end of the document, or at the start when
  reversed, and the menu's check goes with it.

**How it is built.**

- **The timing is in core, without a clock** (`core::AutoScroll`). The
  caller passes the time of each frame; the first frame after a start or a
  pause sets the clock and moves nothing, and one frame accounts for at
  most 100 ms, so a window coming back from being hidden does not jump.
- **No timer runs.** The canvas advances the scroll when it draws and asks
  for the next frame with `request_animation_frame`. GPUI runs a window's
  display link only while the platform reports it visible, so a hidden or
  occluded window draws nothing and scrolls nothing. This answers the
  review risk about a timer running for an invisible window.
- `CanvasModel` holds the running scroll: `toggle_auto_scroll`,
  `change_auto_scroll`, `stop_auto_scroll`, and `advance_auto_scroll`, which
  pans the viewport and stops when the pan no longer moves it. Its `scroll`
  and `pointer_down` pause it.
- **Keys.** The frame adds `OnionskinAutoScroll` to its key context only
  while the tab in front scrolls. Its Up, Down and minus are bound after the
  shell's own keys, so at the frame's depth they win over the focus ring's
  Up and Down, and a text field's own context still wins over both. A key
  with nothing to steer is propagated.
- `MenuCommand::AutoScroll` (`view.automatically-scroll`, bound to
  `cmd-shift-h`), in the View menu's page display entries.
- The read-mode test bound Read Mode to `cmd-shift-h` in its test keymap;
  it now uses `cmd-shift-r`, since the default binding took that key.

**Runs.**

- `cargo test -p onionskin-core --lib autoscroll`, 5 tests: the first frame
  sets the clock and later frames move at the level's speed; a 60-second gap
  moves one 100 ms step; speed steps clamp at both ends and say when they
  did nothing; reversing moves toward the start; a pause holds for
  `AUTO_SCROLL_RESUME_AFTER` and restarts the clock without paying out the
  paused time.
- Window tests `tabs::tests::auto_scroll`, 4:
  - `the_keystroke_starts_it_frames_move_it_and_escape_stops_it`: the
    default keystroke starts it and checks the menu entry, 30 frames move
    the view down, Escape stops it and clears the check.
  - `the_arrow_keys_and_minus_steer_a_running_scroll`: "up up", "down",
    "-" leave it one level faster and reversed. Stopped, the frame's key
    context is the shell's alone and Down moves the focus ring again.
  - `touching_the_document_pauses_the_scroll`: a wheel scroll and a press
    each pause it, and the pause ends after `AUTO_SCROLL_RESUME_AFTER`.
  - `reaching_the_end_stops_it`: at the fastest speed the frames run until
    it stops by itself, and the menu is unchecked.
- **Full suites.** `cargo test -p onionskin-app --features
  shell,shell-test-support --lib`: all pass but the known environmental
  set. With `--no-default-features`: the same. `cargo test -p
  onionskin-core`: passes. The app's integration tests pass, the headline
  recount at 165 planned / 133 implemented included.
- **Lint.** `cargo clippy` for `onionskin-core` and for `onionskin-app` in
  the four feature sets used above: clean. `cargo fmt --all --check`: clean.

**Mutations run.**

- Dropping the pause from `CanvasModel::scroll` fails
  `touching_the_document_pauses_the_scroll`.
- Never stopping at the end fails `reaching_the_end_stops_it`.

**Not claimed.** The frames in the window tests are driven by calling
`advance_auto_scroll` with computed times, not by GPUI drawing frames; the
drawn path is one call from `Canvas::render`. That a hidden window draws no
frames is GPUI's behaviour on macOS, read from its source, not a test here.

## Advanced Search: include attachments and property criteria (rows 20, 21)

**What the user gets.**

- Edit > Advanced Search…, or Cmd+Shift+F (Acrobat's Ctrl+Shift+F), opens
  a dialog over the document in front. It starts with the find bar's words
  and options.
- It offers:
  - the words;
  - Return Results Containing (the three modes);
  - Whole words only, Case-Sensitive and Include Comments;
  - **Include PDF Attachments**;
  - **Use These Additional Criteria**, one line of property, test and
    value. The property steps through Title, Author, Subject, Keywords,
    Creator, Producer, Date Created and Date Modified. The test is
    contains / does not contain for text, or is before / is after / is on
    for dates, which are written YYYY-MM-DD.
- **Search**:
  - The criterion is checked first. A document that does not meet it is
    not searched, and the dialog says so. A criterion that cannot apply
    (a date that is not one) says how to write it.
  - The pages are searched by the find bar's walk with the dialog's words
    and options, and the Search Results pane opens on them.
  - With Include PDF Attachments, the PDFs attached to the document, and
    the PDFs attached to those, are searched: two levels, Acrobat's depth.
    Each hit is listed in the dialog as "annex.pdf > appendix.pdf, page 3:
    word". An attachment that cannot be opened is named ("broken.pdf was
    not searched: …") and its siblings are still searched. Non-PDF
    attachments are passed over, as in Acrobat.
- Changing any option drops the last outcome, which no longer describes the
  form.

**How it is built.**

- `core::metadata::criteria`: `PropertyCriterion::matches(info, xmp)`. A text
  field matches when either `/Info` or the XMP packet has the value, ignoring
  case; a date is read from `/Info` first and XMP second and compared by day.
  A test the field does not take, or a value that is not a date, is a
  `CriterionError` with a message.
- `core::Document::search_attachments`: opens each attached PDF from its
  bytes in memory, searches every page with `content::search`, and recurses
  one level. Hits are capped at 1,000 with the true total kept. It reads
  through a new crate-private `attached_bytes`, which does not apply the
  encrypted-source refusal: nothing leaves the process. `attachment_bytes`,
  the door to a file, still refuses.
- `chrome::advanced_search`: a pure `AdvancedForm` (options, attachments,
  criterion) with `apply`, one row list that both the accessibility tree and
  the drawing read, and `outcome_lines`. The frame's half
  (`tabs/advanced_search.rs`) runs criteria, then the page walk, then
  attachments. `FindBarState::set_options` hands the dialog's options to the
  walk.
- `MenuCommand::AdvancedSearch` (`edit.advanced-search`, `cmd-shift-f`),
  `ShellDialog::AdvancedSearch`, `Activation::AdvancedSearch`, and
  `TextField::{AdvancedQuery, AdvancedValue}` wired into the focus ring.

**Runs.**

- `cargo test -p onionskin-core --lib criteria`, 4 tests: either copy
  matches, case ignored; dates from `/Info` then XMP by day; errors say why;
  every criterion must pass.
- `cargo test -p onionskin-core --lib attachment_search`, 2 tests: a PDF by
  its header in the first kilobyte; a skipped attachment names its path.
- `cargo test -p onionskin-core --test advanced_search`, 4 tests over a
  document attaching a PDF attaching a PDF attaching a PDF:
  - "heron", only in the first attachment, is found with path `annex.pdf`
    and page 0, and the document's own page walk does not find it;
  - "pelican", in the second level, is found with path `annex.pdf >
    appendix.pdf`; "walrus", in the third, is not;
  - a broken attached PDF is named in `skipped` and a blank query searches
    nothing;
  - property criteria over the document's own `/Info`.
- `chrome::advanced_search` unit tests, 6: criterion cycling, an option that
  changes nothing, outcome lines, attachment summaries, the criterion rows
  appearing only while used, and every control's state following its action.
- Window tests `tabs::tests::advanced_search`, 3:
  - `a_word_only_an_attachment_has_is_found_when_attachments_are_included`:
    opened by its keystroke, the Include PDF Attachments checkbox is
    unchecked; a search lists no attachment hits but runs the page walk and
    opens the Search Results pane; with the option on, "annex.pdf, page 1:
    heron" is listed.
  - `a_document_that_misses_the_criterion_is_not_searched`: Author contains
    "Radu" holds the document out and runs no page walk; "ana" matches;
    Date Created with "March" says YYYY-MM-DD; "2025-03-02" is before.
  - `searching_for_nothing_says_to_type_words`.
- **Full suites.** `cargo test -p onionskin-app --features
  shell,shell-test-support --lib` and with `--no-default-features`: all
  pass but the known environmental set. `cargo test -p onionskin-core`:
  passes. The app's integration tests pass, including the headline recount
  at 163 planned / 135 implemented.
- **Lint.** `cargo clippy` for core and for the app in the four feature
  sets: clean. `cargo fmt --all --check`: clean.

**Mutations run.**

- Recursing without decrementing the depth (three levels searched) fails
  `the_second_level_is_searched_and_the_third_is_not`.
- Searching the pages even when the criterion fails fails
  `a_document_that_misses_the_criterion_is_not_searched`.

**Not claimed.**

- Clicking an attachment hit does not open the attachment; the row says
  where the hit is.
- The attachment search runs on the main thread when Search is pressed.
  It is bounded by two levels and 1,000 hits, but a very large attachment
  would pause the window while it is read.
- One criterion line, where Acrobat allows several. Stemming, and searching
  across folders or indexes (the post-1.0 row), are not offered.

## Copy with formatting / Export Selection As (row 31, `partial`)

**What the user gets.**

- **Export Selection As**, in the page's context menu, is live whenever text
  is selected.
  - It asks where to save and suggests `Selection.rtf`.
  - A `.rtf` name gets Rich Text. Each stretch of one face and size is a
    group with that family, its size, and bold or italic when the face's
    name says so.
  - Any other name gets the selected text as plain UTF-8.
  - It says "Exported the selection to …" on the notice bar.
  - On an encrypted document it refuses before asking where, with the
    document's reason. Writing the selection to a file is reading the
    document out, as every export is.
- **Copy With Formatting** stays disabled, with the reason "After 1.0: the
  clipboard has no rich-text format yet; use Export Selection As".
  `gpui::ClipboardEntry` has only text and image variants. This is the plan's
  section 8 item 12, and the row is `partial` for it.

**How it is built.**

- `core::TextSelection` gains `spans: Vec<TextSpan>` (text, font, size).
  `core::textselect::styled_text` cuts a page's flattened text where the face
  or size changes. A separator the join added belongs to the span before it,
  so the spans joined are exactly the selection's text. `content::Flattened`
  exposes its `pieces()` for this. Both the drag selection and
  `commands-core`'s Select All fill the spans.
- `codecs-common::rtf::selection_rtf` writes RTF 1.x:
  - a font table of families (`Helvetica-Bold` and `Arial,Bold` are
    Helvetica and Arial);
  - `\fs` in half points, with 12 points for a size the text did not state;
  - the control characters escaped, line breaks as `\par`, tabs as `\tab`;
  - anything outside ASCII as `\uN?`, surrogate pairs for characters outside
    the Basic Multilingual Plane.

  It is a writer the shell calls, not a registered codec. Whole-document RTF
  export is the post-1.0 row, so `kernel_emptiness`'s codec count does not
  change. The plan expected a registered codec; this is the recorded
  deviation.
- The shell's `export_selection` (`tabs/export_selection.rs`) chooses RTF
  or text by the name. In a build without `codecs-common` it suggests
  `Selection.txt` and refuses a `.rtf` name, saying the plugin is missing.
  `DocumentFile::document()` lets the shell ask the read-out refusal without
  a mutable borrow.

**Runs.**

- `cargo test -p onionskin-codecs-common --lib rtf`, 3 tests: each span's
  family, size and style; escaping of controls, breaks and Unicode including
  a surrogate pair; defaults for names, sizes and an empty selection.
- `cargo test -p onionskin-core --test selection_spans`, 2 tests: "Plain" in
  12-point Helvetica, "Bold" in 18-point Times-Bold, then "Next" on the line
  below, give three spans whose text joins to "Plain Bold\nNext". A dragged
  selection carries spans that join to its text.
- `tabs::export_selection::tests`, 2: a non-RTF name gets the plain text and
  a `.rtf` name gets the faces.
- Window tests `tabs::tests::export_selection`, 3:
  - a `.txt` name writes the selection's text and says so;
  - a `.rtf` name writes RTF containing the text and a size;
  - an encrypted document refuses with the document's reason and asks for
    no path.
- The context-menu test now derives the live set with a selection from
  every `TextSelection` requirement, so Copy and Export Selection As both go
  live with a selection.
- **Full suites.** App `--lib` with and without default features: all pass
  but the known environmental set. The app's integration tests pass, the
  headline recount at 162 planned / 26 partial / 135 implemented included.
  `cargo test` for core, content, codecs-common and commands-core: passes.
- **Lint.** `cargo clippy` for those crates and the app in four feature
  sets: clean. `cargo fmt --all --check`: clean.

**Not claimed.** Colour, position and paragraph layout are not carried; the
file is the selected text with its type.

## New Window and the Window menu (rows 24 and 8)

**Decision.** Two windows over one document were built as one session with
two viewports, the plan's correct model, rather than opening the file twice
or mirroring one canvas. That meant splitting the session out of the canvas.

**What the user gets.**

- **Window > New Window** opens a second window on the document in front,
  titled with its name.
  - An edit in either window shows in both: a deleted page leaves both
    layouts.
  - One Undo, in either window, takes it back once. There is one history.
  - Each window has its own scroll, zoom and layout.
  - Closing one of two windows on an edited document asks nothing, because
    the other still has the edits. Closing the last one asks as before.
- **Window > Minimize** (Cmd/Ctrl+M), **Zoom** and **Bring All to Front** act
  on the windows. Menu commands and keystrokes now go to the window in
  front, not to the first window opened.
- **Not offered: Cascade and Tile.** GPUI can size a window but has no call
  to move one, so arranging windows needs a platform addition. Row 8 is
  `partial` for that.

**How it is built.**

- **`core::RenderView`** (`render_view.rs`, a child of `session`). A second
  viewport's render queue: its own worker thread and pending geometry.
  - It is handed the session's preview bytes and layer visibility whenever
    `(byte generation, edit epoch, layer epoch)` moved past what it holds.
  - Its methods mirror the primary queue's: `request_render_in`,
    `request_render_with_geometry_in`, `try_render_response_in`, the
    geometry and thumbnail pairs.
  - A view is needed because the worker's queue belongs to one viewport: a
    newer generation replaces every older request, so two viewports sharing
    one queue would cancel each other's pages.
  - `Document` gains a `layer_epoch`, bumped by every visibility change.
- **The canvas model shares its session.**
  - `CanvasModel.document` is `Rc<RefCell<DocumentFile>>` (`SharedFile`).
    It is borrowed for each use and never held across a call that borrows
    again.
  - `view: Option<RenderView>` picks the queue: `None` for a document's
    first window, a view for each window New Window opens. Six small
    routing functions (`queue_render`, `next_render_response` and so on) are
    the only place that choice is made.
  - Accessors that returned references into the document now return owned
    values or `Ref`/`RefMut` guards. The app's callers were updated to
    match, including two that held the document across `handle_change`,
    which the borrow checker caught.
- **Following the session.**
  - `CanvasModel::update` compares the session's `(byte generation, edit
    epoch)` with the one its layout was built for.
  - When they differ, and the page count or a visible page's size changed,
    the layout is rebuilt as after a page command. A comment changes neither
    and leaves the view alone.
  - The view history is still reset only by a rebuild.
- **Telling the other windows.** `Canvas::handle_change` sends each new
  session stamp to every other canvas on the same `SharedFile`, in every
  window, from a deferred callback. Those canvases follow the session and
  repaint, and each is marked as told so the message does not bounce back.
- `MenuCommand::{Minimize, ZoomWindow, BringAllToFront}`, with `NewWindow`
  now live, and `ShellSettings::for_new_window` so the second frame starts
  with the same files and keys. `ShellFrame::close_loses_changes` is "dirty
  and no other window has it".

**Runs.**

- `cargo test -p onionskin-core --test render`, 2 new tests:
  - `a_second_view_has_its_own_queue`: the primary queues generation 5 and a
    view queues generation 1; both rasters arrive, which one queue would
    refuse.
  - `an_edit_to_the_session_reaches_the_view`: a page rotated in the session
    measures turned through the view; a pending request is not repeated; a
    page past the end is refused.
- Window tests `tabs::tests::new_window`, 3:
  - `an_edit_in_one_window_shows_in_the_other_and_one_undo_takes_it_back`:
    - both windows on `two-page.pdf` lay out 2 pages;
    - deleting page 2 through the first window leaves both at 1 page with
      Undo labelled "Delete Pages";
    - Undo run from the second window's Edit menu leaves both at 2 pages
      with nothing left to undo.
  - `each_window_keeps_its_own_view`: Zoom In in the second window leaves
    the first window's zoom unchanged.
  - `closing_one_of_two_windows_asks_nothing_and_the_last_one_asks`: with an
    unsaved edit, neither window's close would lose changes while both are
    open; after the second window is removed, the first's would.
- `tabs::windows::tests`: a window's title. The Window menu test lists
  Minimize, Zoom, Bring All to Front and New Window, live with a document.
- The whole app suite passed unchanged after the session split, before New
  Window was built on it: 770 passing, with only the known environmental
  failures.
- **Full suites.** App `--lib` with and without default features: all pass
  but the known environmental set. The app's integration tests pass, the
  headline recount at 160 planned / 27 partial / 136 implemented included.
  `cargo test -p onionskin-core`: passes.
- **Lint.** `cargo clippy` for core and for the app in the four feature
  sets: clean. `cargo fmt --all --check`: clean.

**Mutations run.**

- The plan's mutation: New Window opening a second `core::Document` from the
  same bytes (two sessions) fails the shared-undo test and the close test.
- Not telling the other windows (`tell_peers` removed) fails the shared-edit
  test: the second window keeps laying out the deleted page.

**Not claimed.**

- The find results live in the document's one session, so a find in either
  window replaces the other's.
- The selection is the session's too, so text selected in one window is
  selected in both.
- Closing a window with the window's own close button does not ask about
  unsaved changes, as before this package; the question is asked when a tab
  is closed.
- The platform behaviour of Minimize, Zoom and Bring All to Front is
  GPUI's; the tests run on its test platform, where those calls do not
  change anything visible.
