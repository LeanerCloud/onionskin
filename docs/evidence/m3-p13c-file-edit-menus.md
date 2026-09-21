# M3 P13c verification: Attach to Email, Copy File to Clipboard, Edit verbs

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain, GPUI's test platform. No macOS, Windows or hosted-CI run is
claimed.

Rows closed:

- 16 File > Attach to Email.
- 18 Edit > Cut / Copy / Paste / Delete.
- 19 Edit > Copy File to Clipboard.

## What the user gets

- **File > Attach to Email…** hands the saved file to the operating system's
  mail client: Mail on macOS, the freedesktop `xdg-email` request elsewhere
  on Unix.
  - It is disabled with a reason for a document that was never saved.
  - It is disabled with a reason while there are unsaved changes ("Save
    first, so the email carries your changes").
  - On Windows it says there is no mail client it can ask yet.
- **File > Copy File to Clipboard** puts the saved file's `file://` URI on
  the clipboard. A file manager or a mail client resolves the URI back to
  the file.
- **Edit > Cut, Copy, Paste and Delete** (`cmd-x`, `cmd-c`, `cmd-v`,
  Delete) mean what the active tool says they mean.
  - The text tool answers Copy: the selected text goes on the clipboard.
  - A verb the tool does not answer is disabled with a reason, such as "The
    active tool has nothing to cut".
  - With no tool active, every verb says "Choose a tool first".

## How it is built

- **No shell anywhere.** `shell/share.rs` builds a program and an argument
  vector, with the path as one argument, and runs it directly with
  `std::process::Command`. A file name full of `;`, `$()`, backticks, quotes
  and a newline is a file name, because nothing ever parses the vector as a
  command line.
- **The Edit verbs go through the plugin API, not the shell.** `ToolPlugin`
  gains:
  - `claims(verb)`, which says whether the tool answers a verb;
  - `edit(ctx, verb, pasted)`, which runs it and returns the text for the
    clipboard.

  The menu reads `claims` for the active tool through
  `CanvasModel::edit_verb_availability`, so there is no per-tool special
  case in the shell and no list of which tools copy.
- **The file URI** percent-encodes everything outside the unreserved set,
  so a name with spaces, quotes or a newline is still one URI.

## Runs

- `share::tests::a_crafted_file_name_runs_nothing`: the runner is given a
  path whose name would create a marker file under any shell, and no marker
  appears.
- `share::tests::the_path_is_one_argument_and_no_shell_is_involved`: the
  program is not a shell, and the path is the last argument, unchanged.
- `share::tests::a_file_uri_encodes_everything_a_name_can_hide`.
- `edit_verbs::tests::every_verb_says_why_it_is_off`.
- Window tests in `tabs/tests/edit_menu.rs`:
  - `the_edit_verbs_are_what_the_active_tool_answers`: with the text tool
    active and text selected, the menu offers Copy and disables the rest
    with the tool's reasons. The Copy keystroke puts the text on the
    clipboard, and the Cut keystroke leaves the clipboard alone.
  - `copy_file_to_clipboard_names_the_saved_file`: the URI resolves to the
    open file.
  - `attach_to_email_waits_for_unsaved_changes`: the command is live for a
    clean saved file, and disabled with its reason after a rotate.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  every failure is in the environmental set in `known-issues.md`.
- `cargo clippy` for the app in both feature sets is clean apart from the
  Linux-only dead code in `a11y/mod.rs`.

## Mutations

- **Passing the path through `sh -c`** (the plan's named mutation) fails
  `a_crafted_file_name_runs_nothing`: the marker file appears.

## Not done here, and said

- **Only the text tool claims a verb.** No comment tool keeps a selected
  comment yet, so Cut, Paste and Delete are offered by nothing, and say so.
  The Comments pane's Delete removes a comment.
- **Windows mail:** asking Windows for a message with an attachment needs
  MAPI, which is not wired.
