# M5 verification: filling forms, and their scripts

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:**
  - Clear form (Toolset: Prepare a form);
  - JavaScript preferences (enable or disable document JavaScript).
- **To `partial`:**
  - Fill in a form (as an end user). Guarantee test 7 is not run: the
    JS-forms corpus waits on values recorded in Acrobat.
  - Tab order / form field navigation. Tab moves between text fields and
    dropdowns in page order. It does not honour `/Tabs`, does not stop on
    buttons or list boxes, and no tab order can be set.
- **Notes changed, status kept:**
  - Field properties: Format, Validate, Calculate stays `planned`. The
    scripts run when a form is filled, but they cannot be set yet.
  - XFA forms stays `out-of-scope`. The open notice it asked for has
    shipped.
  - Document-level JavaScript beyond the forms API stays `out-of-scope`. A
    notice now names a field whose script cannot run.
- **Headline:** 124 planned / 33 partial / 80 out-of-scope, 166
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Filling with the Hand tool.** A click on a field fills it where it is.
  - **A text field** opens a text box over the widget. A password field is
    masked: its text is shown, read out and copied only as asterisks. Text
    past the field's maximum length is dropped.
  - **A dropdown** opens its options under the widget. An editable dropdown
    also takes typed text, and an option typed by its display name is kept
    as its export value.
  - **A list box** takes the row that was clicked. A multiple-choice list
    turns that row over.
  - **A check box** toggles. **A radio button** is chosen, and a radio group
    that cannot be turned off stays on.
  - **A button or a signature field** says why nothing happens: Onionskin
    does not run button actions, and signing with a digital ID is M6. A
    read-only field says it is read-only.
- **Committing a value.**
  - Enter commits and Escape leaves the field as it was.
  - Tab and Shift-Tab commit, then move to the next or previous text field
    or dropdown, going round at the end.
  - A press anywhere else on the page commits.
  - A value the field's scripts refuse leaves the editor open on what was
    typed, and the script's alert goes on the notice bar.
- **Scripts.** A committed value runs through the field's Keystroke and
  Validate scripts. Then every calculation runs in the form's `/CO` order,
  and each changed field is drawn through its Format script, with a new
  appearance. All of it is one undo step, labelled Fill Field.
  - `app.alert` goes on the notice bar.
  - A script that uses something outside the forms subset, throws, or runs
    out of its budget is named on the notice bar with the field. It is
    never silently skipped.
- **Edit > Clear Form.** Every field goes back to its default, as one undo
  step. On a document without fields, it says so.
- **Preferences > JavaScript > Enable Acrobat JavaScript.** On by default,
  and saved as `javascript`. Turned off, a value is kept as typed and no
  script runs. It applies to every open tab at once.
- **XFA.** A document with XFA says so when it opens:
  - a pure XFA form is shown as drawn, read-only;
  - a hybrid form is filled through its standard fields.
- **Screen readers.**
  - The open editor is in the accessibility tree under the page, with the
    field's name and kind ("qty (text field)"), where it is, and what is
    typed.
  - A dropdown lists its options as selectable items, each picking itself
    when activated.
  - Focusing the node focuses the text box.
  - Enter and Tab reach the field rather than the focus ring.

## How it works

- **`core` forms** (`crates/core/src/forms/`) reads the field tree, writes
  values with appearances, and resets them to defaults. It also gives the
  list box row under a point, from the same row geometry the list appearance
  draws, and the XFA notice. Filling itself never runs a script; `core`
  only stores.
- **`scripting`** runs one field script for one event in a fresh Boa
  context.
  - The context has no I/O and no network.
  - A loop runs at most a million iterations, and recursion goes 256 deep.
  - The forms API is JavaScript in `prelude.js`.
- **`tools-basic`**: the Hand tool raises a `FieldRequest` for a fillable
  field under the click, and otherwise follows a link.
- **`tools-form`.**
  - `fill` runs the event sequence and writes every changed field in one
    transaction.
  - `toggle` handles check boxes and radio buttons.
  - `clear_form` resets the form.
  - `replay` is guarantee 7's harness.
- **App.**
  - `canvas/forms.rs` answers the request: toggles, list rows, the prompt
    for an editor, commits, tab order, and notices.
  - `field_editor.rs` is the editor on the canvas: its keys, its position
    over the widget, its options, and its accessibility node.
  - The frame collects the notices when the canvas changes, and runs Clear
    Form.
  - The JavaScript preference reaches the canvas through the tool
    environment (`javascript_off`), so it is right for new tabs and windows
    too.

## Guarantee test 7

It stays ignored, and its reason now says what it waits on. Scripting and
filling have landed. The harness has too: `tools-form/src/replay.rs`, run
over `corpus/js-forms/pdfs/` by `tools-form/tests/guarantee.rs`.

- **What it compares.** It replays each `<stem>.scenario.json`, then
  compares every field's value and formatted display, and the alerts, with
  `<stem>.expected.json`. A list of values passes on any of them, for
  versions of Acrobat that disagree.
- **Where it is proved.** The harness is proved on the plugin's own form in
  `tests/replay.rs`, which checks both agreement and every kind of
  difference.
- **What is missing.** The corpus itself: the expected values can only be
  recorded in a licensed Acrobat. The formats, with examples, are in
  `corpus/js-forms/README.md`.

## Runs

- `cargo test -p onionskin-core --test forms`: 5 pass.
- `cargo test -p onionskin-scripting`: 8 pass.
- `cargo test -p onionskin-tools-form`: 7 pass. The corpus suite is
  ignored, and fails as it should when run with no set.
- `cargo test -p onionskin-tools-basic`: all pass.
- **App unit and window tests:**
  - `cargo test -p onionskin-app --features shell-test-support --lib`: 923
    pass and 5 fail.
  - The 5 failures are environmental. Two are canvas timing tests, which
    pass when run alone. Three are export rollback tests, which fail when
    run as root.
  - New in this change:
    - 10 canvas model tests (`canvas/forms/tests.rs`);
    - 8 window tests (`tabs/tests/forms.rs`), covering typing and Enter, a
      refused value, Escape, Tab and Shift-Tab, a press elsewhere
      committing, a dropdown picked from the accessibility tree, focusing
      the editor from the tree, a check box, Clear Form with and without a
      form, and the JavaScript preference;
    - one test for the preference row and one for the masked input.
- **App integration tests:** `--test '*'`, guarantees included, all pass.

## Coverage

`cargo tarpaulin` with optimisation off.

| Code | Lines covered |
| --- | --- |
| `plugins/tools-form/src` | 208 of 230 (90.4%) |
| `crates/scripting/src/lib.rs` | 55 of 55 (100%) |
| `crates/core/src/forms/*` | 524 of 571 (91.8%) |

In `core` forms, `appearance.rs` (175 of 196) and `read.rs` (183 of 202)
are the least covered: font resources that fail to resolve and malformed
`/Opt` entries. In `tools-form`, the lines left are the plugin manifest (3 lines, never
called in tests) and error paths in `fill.rs`. The forms API itself is
JavaScript and is covered by `scripting/tests/forms_api.rs` (8 tests over
every `AF` function family, `util`, `getField` and the error kinds). The app
crate is not run under tarpaulin; its new code is covered by the tests
above.

## Mutations

Each was caught, then reverted.

- A keystroke script's refusal ignored in `fill`:
  `a_keystroke_or_validate_script_refuses_a_value` fails.
- A multiple-choice list box row not turned off when clicked again:
  `a_list_box_takes_the_row_clicked_and_turns_it_over` fails.
- The editor closed on a refused value:
  `typing_into_a_text_field_commits_on_enter_and_its_script_can_refuse`
  fails.

## Clippy and format

`cargo clippy --workspace --all-targets --features
onionskin-app/shell-test-support` reports only the existing
`a11y::Shared::record` warning. So do these app builds with
`--no-default-features`:

- `--features shell`, the plugin-less build;
- `--features shell,tools-form`;
- `--features shell,tools-basic`.

`cargo fmt --all --check` is clean.

## Not claimed

- **Preparing a form.** Field tools, field properties, auto-detect and
  auto-complete are still planned.
- **Guarantee test 7.** See above.
- **Button actions and form submission.** A push button says it does not run
  its action. Submit and reset actions, and JavaScript beyond field events,
  are not run.
- **Keystrokes as they are typed.** Keystroke scripts run on commit, with
  `willCommit` true. They do not run on each key, so a character is not
  refused as it is typed.
- **Rich text, comb and multi-line layout while typing.** The editor is a
  single line; the appearance written afterwards lays out multi-line and
  comb fields.
- **Tab order.** `/Tabs` and the structure order are not honoured, and Tab
  does not stop on buttons or list boxes.
- **Signature fields.** They are not signed (M6).
- **Platform runs.** No macOS, Windows or VoiceOver run of the editor is
  claimed.
