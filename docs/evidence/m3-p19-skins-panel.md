# M3 P19 verification: the skins panel

Date: 2026-09-21. Linux x86-64, stable toolchain, GPUI's test platform. No
macOS, Windows or hosted-CI run is claimed.

## Rows

None. `ACROBAT-PARITY.md` does not count Onionskin-only surface, and the
skins panel is the first thing it names as such. The panel is still the
identity of the release.

## What the user gets

- **Opening it.** The rail's 🧅 Skins button (below the tools) or File >
  Skins (Version History) opens the panel in the side panel on the active
  document. The same button puts it away.
- **One row per version, newest first:**
  - the newest is marked "(current)" and the original is at the bottom;
  - each row says who wrote it and when, and how big it is, for example
    "Version 2, saved by Onionskin 0.1.0, 2026-09-21 20:10:00 UTC, 312
    bytes";
  - a section another program appended reads "added by Acrobat, not by
    Onionskin".
- **Open a Copy of This Version…** asks where the copy goes, suggesting
  "*name* (version N).pdf" or "(original)", writes the file as it was then,
  and opens it in its own tab. The open document and its file are not
  touched.
- **Roll Back to This Version…:**
  - it first asks, saying what goes, for example "Roll back to version 1?
    2 newer versions (1.2 KB) will be removed from the file. This cannot
    be undone.";
  - Roll Back truncates the file to the end of that version, and the tab
    shows the file as it now is;
  - a notice says how many versions were removed.
- **Refusals, in words:** Roll Back is disabled with its reason on the
  current version ("This is the current version") and while there are
  unsaved edits ("Save or undo your changes first").
- **Following the file.** A save while the panel is open adds the new
  version to it. The panel reads the file's versions when its bytes change
  (a save or a roll back) or the active tab changes, never per frame.

## How it is built

- **What "ours" means, derived and not guessed.**
  - Every section a save writes carries a trailer entry, `/OnionskinSection
    << /Start n /Producer (Onionskin x.y) /Date (D:…) >>`.
  - `/Start` is the section's own first byte, from the new
    `cos::Document::next_section_start`.
  - Trailer keys are carried forward into later sections, by other writers
    too, so a stamp alone would mark every later section as ours. A section
    is ours only when its trailer's stamp names its own start.
  - A carried-forward stamp names an earlier start and does not count.
    `/Producer` is never consulted for this.
- **Only a save stamps**, not the preview. The preview is never written,
  and a clock in it would make two previews of one state differ.
  `the_preview_equals_what_the_following_save_writes` still passes, because
  it compares object graphs and the stamp is a trailer entry.
- **`core::Document::generation_details`** opens the file as it was when
  each generation ended, over the same shared buffer. The new
  `cos::BytesSource::prefix` does this without copying the file once per
  generation. Each generation's details are:
  - who wrote it: the stamp's producer for ours, or `/Info /Producer` as of
    that generation;
  - when: the stamp's date, or `/Info /ModDate` (`/CreationDate` for the
    original).
- **`roll_back_to(keep)`** (`DocumentFile`, `CanvasModel`) truncates at the
  start of the generation after `keep`.
  - Every newer generation is trailing, so it is always a truncation.
  - It is refused with unsaved edits (`UnsavedEdits`), on the newest
    (`AlreadyCurrent`) and out of range (`NoSuchGeneration`).
  - It shares one `truncate_at` with P3's `revert_to`, which bumps the byte
    generation before truncating: the render worker's lifetime across a
    truncation stays P3's decision.
- **One deviation from the plan.** The plan previews an older generation in
  place through `adopt`. Here an older version opens as a copy in its own
  tab instead. The review risk asked whether a preview "opens a second
  document or mutates the current one", and this answers it by
  construction: the current document is never the one that changes. It
  also gives the user something to keep.
- **`shell/skins.rs`** holds the state, the row labels (reusing the
  Properties dialog's date and size formats), the panel and the
  confirmation. **`chrome/tabs/skins.rs`** is the frame's half. The side
  panel now takes generic content: the skins first, a chosen comment's
  properties otherwise.

## Runs

- `cargo test -p onionskin-core --test generations`: 4 pass.
  - Three saves are four generations whose ranges partition the file, with
    all three saves ours, and each has an Onionskin producer and a date.
  - A section appended after ours with our stamp carried forward, asserted
    present in its trailer, is not ours.
  - Rolling back to the middle leaves the file byte for byte a truncation
    at the next generation's start, and the session equals a fresh reopen.
  - Refusals: current version, out of range, unsaved edits. A refusal
    writes nothing.
- `cargo test -p onionskin-cos --test write_new`: reachability tests still
  pass. `cargo test -p onionskin-core`: all pass.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support --lib skins`: 12 pass (5 unit, 1 frame unit, 6
  window). The window tests:
  - `the_rail_opens_the_panel_listing_every_version_newest_first`:
    - the rail button is found and activated through the tree;
    - four rows in order;
    - Roll Back is disabled on the current version;
    - after closing, no `skins-` node is left in the tree.
  - `rolling_back_asks_first_then_truncates_the_file`:
    - the question names what goes;
    - the file is unchanged while asking;
    - after Roll Back the file equals `before[..sections[2].start]`, the
      panel shows two versions, and the notice is right.
  - `cancelling_a_roll_back_leaves_the_file`.
  - `roll_back_waits_for_unsaved_edits_to_be_saved_or_undone`: disabled
    with the reason, and the action opens no dialog.
  - `opening_a_copy_of_the_original_leaves_the_file_alone`: the copy is
    the original's bytes, the file hashes the same, and a second tab
    opened.
  - `a_save_adds_a_version_to_the_open_panel`.
- **Full suite:** `cargo test -p onionskin-app --features
  shell,shell-test-support --lib`: 720 pass and 4 fail, all four the known
  environmental set. The rail tests were updated for the new last child.
- **Lint:** `cargo clippy` for `onionskin-core`, `onionskin-cos` and
  `onionskin-app` (default and `--no-default-features`), `--all-targets --
  -D warnings`: clean. With `shell` on Linux it is clean apart from the
  pre-existing `a11y::Shared::record` dead-code warning, which is used only
  on macOS. `cargo fmt --all --check`: clean.

## Mutations run

- Rolling back one byte short (`next.start - 1`) fails the core
  byte-identity test and the window roll-back test. This is the plan's
  mutation.
- Counting any stamp as ours, ignoring `/Start`, fails the
  carried-forward test.

## Not claimed

- The plan names an `external/` file with a pre-existing incremental
  section from another producer. This run uses a section appended by the
  test after ours, with the stamp carried forward, which is the harder
  case for the "ours" rule. The external-file case is not run here.
- `cargo test -p onionskin-cos` has one failure in this container that is
  not this package's: `lazy::every_parsed_object_records_the_bytes_it_came_from`
  on an external veraPDF file. It fails identically with this package's
  `source.rs` reverted.
- Coverage of the app half is not measured (`cargo tarpaulin` cannot build
  `onionskin-app`). The model is covered by the unit tests above and the
  frame half by the window tests.
