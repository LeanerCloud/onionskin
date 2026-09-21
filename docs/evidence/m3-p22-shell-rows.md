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
