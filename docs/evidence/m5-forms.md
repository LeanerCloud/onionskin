# M5 verification: filling and preparing forms, and their scripts

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Filling forms: rows

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
- **Headline after filling:** 124 planned / 33 partial / 80 out-of-scope,
  166 implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes at each step.

## Preparing forms: rows

- **To `implemented`** (Toolset: Prepare a form):
  - Text field;
  - Check box;
  - Radio button;
  - List box;
  - Dropdown;
  - Date field;
  - Digital signature field (the field; signing is M6).
- **To `partial`:**
  - Button (push button). Its actions are neither set nor run.
  - Field properties: General, Appearance, Position, Options, Actions. There
    is no Actions tab, Helvetica is the only font, and the border's width
    and style are fixed.
  - Field properties: Format, Validate, Calculate. Simplified field notation
    and editing the calculation order are not offered.
- **To `partial`, with Auto-Complete:** Auto-Complete form entries. Basic
  and the entry list are offered; Advanced is not.
- **To `implemented`, afterwards:** Image field.
- **Still `planned`:** auto-detect form fields.
- **Headline after every part:** 112 planned / 37 partial / 80
  out-of-scope, 174 implemented.

## Preparing forms: what the user gets

- **Field tools.** One tool for each kind of field, sharing a rail slot:
  Text Field, Check Box, Radio Button, List Box, Dropdown, Button, Date
  Field and Signature Field.
  - Drag to draw a field, or click to place one at Acrobat's usual size,
    hanging from the click.
  - Fields are numbered as Acrobat numbers them: Text1, Check Box1, Group1.
  - A radio button placed while another is selected joins its group.
  - Every field is outlined while a field tool is chosen, so one with no
    border can be found.
  - A click selects a field. A double click, or Enter, opens its
    Properties. Edit > Delete takes it away.
  - Each is one undo step, labelled Add Text Field, Delete Field and so on.
- **Properties.** The dialog is titled per kind, as Acrobat titles it
  ("Text Field Properties"), and its tabs are:
  - **General:** name, tooltip, hidden, read-only, required.
  - **Appearance:** border, fill and text colour, font size.
  - **Position:** left, bottom, width and height in points.
  - **Options,** for each kind:
    - a text field's alignment, default value, limit of characters,
      multi-line, password and comb;
    - a check box's or radio button's export value and whether it is on by
      default, and whether clicking the chosen radio button leaves it
      chosen;
    - a list's or dropdown's items, with export values, order, default,
      custom text or multiple selection;
    - a button's label.
  - **Format, Validate and Calculate,** for text fields and dropdowns, each
    choice written as the `AF` call Acrobat writes.
  - A name that is empty, dotted or already taken is refused in the
    dialog, and so is a size under a point or a number that is not one. A
    field that went away says so.
- **Written as Acrobat writes.**
  - A new field is a field merged with its widget, on the page and in
    `/AcroForm /Fields`, with its appearance.
  - On a tagged document it gets a `/Form` structure element.
  - The form dictionary is made when there is none, with Helvetica and
    ZapfDingbats in `/DR`.
  - A field that gains a calculation joins `/CO`, and one that loses it
    leaves.
  - Changing a check box's export value renames its on state and draws its
    appearances again.
- **Image fields.** The Image Field tool places a button named as Acrobat
  names them (Image1_af_image), icon only. It carries the click action
  Acrobat's image field runs, `event.target.buttonImportIcon()`, so it works
  in Acrobat as well.
  - With the Hand tool, a click asks for an image file. A PDF's first page
    works too, and so does any image the Create entries import.
  - The image becomes the button's `/MK /I` form XObject, drawn fitted and
    centred inside the frame, as one undo step.
  - A file no codec reads is said on the notice bar.
- **One drag gesture.** The marquee and the click-or-drag threshold, written
  four times across the tool plugins, are one module in `plugin-api` now.

## Auto-Complete

- **Preferences > Forms.**
  - Auto-Complete: Off or Basic, on by default.
  - Remember numerical data: off by default, so a number typed into a form,
    which may be an account's or a card's, is not kept. (judgment)
  - The remembered entries, each with Remove, and Clear All.
- **Remembering.** What is typed into a text field and kept is
  remembered, most recent first, up to 500 entries.
  - Never a password field.
  - Never a value the field's scripts refused.
  - Never a dropdown's choice.
- **Offering.** Typing into a text field offers the entries that start with
  what was typed, ignoring case, up to five, under the text box. A click
  puts one in the box, to be committed as typed text is. A screen reader
  gets the suggestions as options of the field's node, each of which picks
  itself.
- **Kept locally.** The entries are kept in `autocomplete.json` beside the
  other settings, written owner-only. The list is read again from the file
  before every change, so two windows do not lose each other's entries.

## Filling forms: what the user gets

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

Preparing forms and Auto-Complete, run afterwards:

- `cargo test -p onionskin-core --test forms`: 13 pass. These cover adding
  every kind of field to a document without a form, names, removal
  (including from `/CO`), and properties written and read back for each
  kind, plus the refusals.
- `cargo test -p onionskin-plugin-api --test marquee`: 2 pass.
- `cargo test -p onionskin-tools-form`: 21 pass:
  - fill 4;
  - replay 3;
  - field tools 6;
  - the Format, Validate and Calculate scripts 5;
  - field edits 3.

  The corpus suite stays ignored.
- `cargo test -p onionskin-tools-basic -p onionskin-tools-edit -p
  onionskin-redact -p onionskin-tools-fill-sign`: all pass after the shared
  gesture moved.
- App lib: 942 pass. Six fail, all environmental: three canvas timing tests
  that pass alone, and three export rollback tests, which fail when run as
  root.
  - New for preparing forms: 8 tests of the dialog as data, and 3 window
    tests (place, double-click, change and save; a refused name and Delete;
    a field gone away).
  - New for Auto-Complete:
    - 5 tests of the entry list;
    - 1 of the Forms preference rows;
    - 1 of the canvas model;
    - 2 window tests: remembering, offering, picking from the tree, and
      removing, then Off, numbers, and Clear All.

## Coverage

`cargo tarpaulin` with optimisation off.

| Code | Lines covered |
| --- | --- |
| `plugins/tools-form/src`, after preparing forms | 568 of 598 (95.0%) |
| `crates/scripting/src/lib.rs` | 55 of 55 (100%) |
| `crates/core/src/forms/*`, after preparing forms | 1037 of 1120 (92.6%) |

In `core` forms, the least covered files are `appearance.rs` (199 of 222),
`author.rs` (239 of 264) and `read.rs` (196 of 217). What is left is font
resources that fail to resolve, malformed `/Opt` entries, and a `/Fields`,
`/DR` or `/AcroForm` held in an object of its own. In `tools-form`, the
lines left are the plugin manifest (5 lines, registered only by the app)
and error paths in `fill.rs`. The forms API itself is
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
- A field's calculation not put into `/CO`:
  `a_text_field_s_properties_are_written_and_read_back` fails.
- A radio button placed with one selected not joining its group:
  `a_radio_button_placed_with_one_selected_joins_its_group` fails.

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

- **Preparing a form.** These are still planned:
  - auto-detecting fields;
  - auto-complete;
  - moving or resizing a field by dragging it, rather than through
    Position;
  - setting a tab order.
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
