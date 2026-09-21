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
