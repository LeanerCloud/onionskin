# M3 P18 verification: save, undo/redo, dirty state and crash recovery

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here on GPUI's test platform; no
macOS, Windows or hosted-CI run is claimed.

Rows closed:

- 2 Undo / Redo icons on the global bar.
- 3 Save / Save As in the global bar.
- 7 Autosave and crash recovery.
- 10 File > Save.
- 11 File > Save As.
- 13 File > Revert.
- 17 Edit > Undo / Redo.

## What the user gets

- **File menu:** Save (`cmd-s`), Save As… (`cmd-shift-s`), Revert.
- **Edit menu:** Undo (`cmd-z`), Redo (`cmd-shift-z`).
- **Global bar:** Save, Save As, Undo and Redo buttons. Each button takes its
  availability from its menu entry, so the bar and the menu cannot disagree.
- **Dirty mark:** a tab with unsaved changes shows "●" before its title. A
  screen reader hears "Unsaved changes" instead of the dot.
- **Close confirmation:** closing a tab, closing the other tabs or closing
  all of them asks first when any of them has unsaved changes. The choices
  are Save, Don't Save and Cancel.
- **Autosave:** every 30 seconds, into an owner-only `recovery/` directory
  beside the other config files. When the directory can't be made private,
  the app says so once and leaves autosave off.
- **Recovery:** when a document opens and a recovery file for that exact file
  is waiting, the app offers Recover or Discard. Offers are ranked most recent
  first.

**Keybinding change.** `cmd-shift-z` is now Redo. A test that bound Dynamic
Zoom to it in a hand-written `keymap.json` now uses `cmd-shift-y`. Dynamic
Zoom still ships unbound.

## How it is built

- **The dirty state is derived, not stored.** It is the history cursor
  compared with the saved mark (`EditSession::is_dirty`, T1), read every time.
  No flag is set anywhere.
- **Undo, Redo, Save, Save As and Revert** go through `CanvasModel`
  (`canvas/file_ops.rs`). Each one that changes anything rebuilds the layout
  the way a page command does, because any of them can change the page count.
- **Revert reopens the file.** The history goes with the old document, and so
  does its recovery file.
- **Every keystroke runs deferred.** Keystrokes go through the one deferred
  `RunCommand` listener into `run_main_menu_command`, the route the menus
  already use.
- **Closes wait for an answer.** `request_tab_command` asks first;
  `run_tab_command` never asks, and it is what the dialog's answer runs. The
  dialog remembers the documents by canvas identity, not by tab index (B4.2).
- **Autosave cannot overlap a save.** Autosave runs on the UI thread, and save
  is synchronous on that thread.
- **Stale recoveries are removed.** A recovery written before the file's last
  save no longer matches the file (`Recovered::Stale`). It is deleted instead
  of offered, because replaying it would apply its edits twice.

## Recovery replay (core, committed as `aad3f5f`)

- `Document::replay_recovery` makes every object and trailer key the
  autosaved section defines into one undoable "Recover Unsaved Changes" step.
  After it, the document is dirty, the next save appends the same section,
  and Undo removes it.
- `Transaction::put_object` claims the number it writes. Without that, the
  first `reserve` after a replay handed out a number the recovery had already
  used (T3).

Tests in `core/tests/recovery.rs`:

- `a_replayed_recovery_is_one_undoable_edit_and_the_next_edit_does_not_collide`.
  With the claim removed from `put_object`, it fails.
- `bytes_that_do_not_extend_the_document_are_refused`.

## Runs

The window tests are in `tabs/tests/file.rs`:

| Test | What it asserts |
|---|---|
| `delete_save_undo_save_puts_the_page_back_everywhere` | T1, driven by keystrokes. After delete, `cmd-s`, `cmd-z`, `cmd-s`, the page is back in the canvas and in the file on disk. The tab is dirty after the first edit, clean after the save, and dirty again after the undo past the saved mark. `cmd-shift-z` deletes the page again. |
| `undo_restores_the_rendered_page` | After Rotate and then `cmd-z`, the page renders byte-identical to the render before the edit. The render uses the canvas's own worker. |
| `undo_and_redo_are_disabled_with_a_reason_when_there_is_nothing_to_do` | Undo, Redo and Save are in the tree, disabled, each with a reason. After an edit, Undo and Save become live. |
| `save_as_writes_the_chosen_file_and_the_tab_follows_it` | The chosen file gets the edit, the original file's bytes are unchanged, and the tab is retitled and clean. |
| `closing_an_unsaved_tab_asks_and_the_answer_lands_on_that_tab` | Cancel keeps the tab open. With the question still up, the user switches to the other tab, and Don't Save closes the edited document, not the tab that is now active. |
| `closing_a_clean_tab_does_not_ask` | A clean tab closes without the dialog. |
| `a_recovery_is_offered_for_its_own_document_only` | A recovery written for A is not offered when B opens. It is offered when A opens, and accepting it brings back the deleted page and leaves A dirty. |

Unit tests:

- `canvas::file_ops::recovery_offers_rank_most_recent_first_and_undated_last`,
  the ranking test PLAN.md's testing strategy item 4 asks for by name.
- Two tests in `file_dialogs`: the question text, and each button running its
  action.

Suite results:

- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  652 pass and 6 fail. The 6 are the environmental set in `known-issues.md`:
  the snapshot and zoom-raster tests, the unmeasured page test and the
  three rollback tests. That set varies a little from run to run.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support --lib -- file outline properties`: 69 pass. The
  window tests that edit through Delete Page or Rotate need `commands-core`
  and run only with the default features. The recovery test and the
  clean-close test run in both.
- `cargo clippy` over the workspace, and over the app in both feature sets,
  is clean apart from the Linux-only dead code in `a11y/mod.rs`.

## Mutations

- **Removing `cx.defer` from the command listener** (running
  `run_native_command` inline) fails 5 of the 7 window tests. Only the two
  that never press a keystroke still pass.
- **Replacing the saved-mark comparison with a count-based dirty flag** fails
  `delete_save_undo_save…`, at "undoing past the saved mark makes the
  document dirty again".

## Not done here, and said

- **Quit with unsaved documents does not ask.** Closing tabs asks; quitting
  the app does not. What was last autosaved is recoverable on the next open.
  A Quit prompt needs a hook on window close that GPUI's test platform can't
  drive, and it is left to P22's window-management work.
- **Autosave's interval is fixed at 30 seconds.** It is not a preference yet.
- **Save for a document with no file:** Save asks where, as Save As does. The
  close dialog can't stop to ask, so it names that document in its error
  instead.
