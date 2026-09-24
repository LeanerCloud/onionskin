# M5 verification: editing text, adding text, find and replace

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `implemented`:** Find toolbar > Replace text.
- **To `partial`:** Edit text (line-level); Add text (new text box);
  Change font, size, colour, alignment, spacing of edited text (see
  "Font, size and colour" below).
- **Also changed:** the page canvas context menu row. Its Edit Text now
  chooses the Edit Text tool. The row stays `partial`, because Copy With
  Formatting is still disabled.
- **Headline:** 104 planned / 41 partial / 80 out-of-scope, 178
  implemented when this was first written. It is 101 planned / 43
  partial / 179 implemented after the font, size and colour row, with
  Check Spelling and tag integrity landed in between. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **The Edit Text tool** (tool rail, `✎`; also the canvas context menu's
  Edit Text).
  - A click on a line of text outlines it and opens a text box over it,
    holding what the line says, with the focus in it.
  - Enter, or a press elsewhere on the canvas, rewrites the line with what
    was typed. The new text starts where the line began, in its font, size,
    colour and text state, and nothing else on the page moves. It is one
    undo step, Edit Text.
  - Escape leaves the line as it was.
  - A change that cannot be made is said on the canvas's status line, and
    the line stays as it was. That covers characters no available font can
    draw, and a line that says something else by the time it is kept.
- **The Add Text tool** (tool rail, `T+`). A click opens an empty text box
  there. Enter draws what was typed as a new line in Helvetica 12 pt,
  starting at the click, as one undo step (Add Text). Nothing typed adds
  nothing.
- **Replace in the find bar.** A Replace With row sits under the find
  bar's options.
  - **Replace** takes the match the find bar has on screen, or the first
    one when none is.
  - **Replace All** takes every match, as one undo step.
  - Match Case and Whole Word apply. The find runs again afterwards, and a
    notice says how many were replaced.
  - In a build without `tools-edit`, or on a document that may not be
    edited, both buttons are off and say why.

## How it works

- **content, `lines.rs`.** `text_lines` groups a page's runs into lines:
  runs drawn one after another on one baseline, with no gap wider than
  three line heights. Two columns on one baseline are two lines. A line
  knows each glyph's run, its place in that run, and what text it spelled.
- **content, `edit_text.rs`.** `edit_lines` rewrites some of a page's lines
  in one pass of the interpreter that extracts text:
  - **Taking the old text out.** The line's glyphs are stepped over, the
    way redaction steps over what it removes.
  - **Drawing the new text.** The operator that drew the line's first glyph
    draws the new text at that glyph's pen position, in the current font
    and graphics state. It then puts the pen back with a `TJ` number, so
    whatever that operator or the text object draws next lands where it
    did.
  - **Which font.** Each character is written with a code the page already
    drew it with in that font, which a subset is sure to hold. Failing
    that, when the font is not a subset, a code from its encoding is used.
    Failing both, a standard font of the same family, weight and slant is
    switched in with `Tf` and switched back after (for example,
    Garamond-Bold becomes Times-Bold), with the text in WinAnsiEncoding.
    Characters no font here can draw are refused, never drawn as question
    marks.
  - Marked content is copied as it was. A sequence whose glyphs changed
    loses its `/ActualText`, because that would still spell the old text.
- **core, `text_edit.rs`.**
  - `rewrite_lines` works out a page's new content from the document as it
    is.
  - `write_page_edit` gives the page that content as one new stream, and
    adds any standard font the new text needs under a name no font of the
    page starts with.
  - `find_in_lines` finds text line by line, with case folding and the
    whole-word rule; `replace_matches` rewrites every line holding a match,
    one page at a time.
  - `add_text` draws a new line after the page's content, guarded from the
    page's leftover graphics state.
  - The following are refused:
    - a line holding a glyph whose character is unknown;
    - text inside a form XObject, since the form may be drawn on other
      pages;
    - a match found on a line that is no longer there.
  - `Document::request_text_edit` and `take_text_edit_request` carry a
    tool's click to the shell, as link and form field requests do.
- **plugin-api.** `ToolCapability::EditText`, which edits the document, so
  a protected document disables both tools.
- **tools-edit.**
  - `text.rs` holds `line_at`, `edit_line` (checks the line still says what
    it said when chosen), `add_text`, `find`, `match_at` and `replace`.
  - `text_tool.rs` holds `EditTextTool` and `AddTextTool`.
  - The tool suites now share `tests/common`.
- **app.**
  - `line_editor.rs` is the text box. It is a text field to a screen reader
    (`TextField::LineText`), and it is focusable through the tree.
  - `tabs/replace.rs` and the find bar's Replace row.
  - `TextField::Replace`, `Activation::ReplaceText`, and the rail glyphs.

## Runs

- `cargo test -p onionskin-content --test edit_text`: 6 pass:
  - a line rewritten from where it began, the next line unmoved, and two
    lines rewritten in one pass;
  - part of a `TJ` rewritten, with the word after it not moving;
  - a subset writing what the page drew with it, then a standard font for
    what it did not, with `Tf` in and back out;
  - refusals: undrawable text, no glyphs, a glyph not found;
  - lines joining runs on a baseline and parting at a column gap and a new
    line;
  - a regression: after one line is edited, the operator that only moves
    the pen back must not be taken for the next line's. Without the fix,
    that test fails.
- `cargo test -p onionskin-core --test text_edit`: 6 pass:
  - a line rewritten on a tagged page, keeping `/MCID 0`, with the
    structure invariant clean;
  - every occurrence replaced across three pages in one edit, and none
    found case-sensitively;
  - a euro set in a standard font, with the inherited resources kept for
    the other pages;
  - refusals: undrawable, unknown glyph, stale match, text in a form;
  - new text drawn after the page, twice, under two font names;
  - the tool request taken once.

  The unit test covers case folding, whole words and non-overlap.
- `cargo test -p onionskin-tools-edit --test text`: 4 pass:
  - the Edit Text tool asking for the clicked line and outlining it, but
    not on a drag, on empty space or after Escape;
  - a line rewritten as one undo step, unchanged text doing nothing, a
    stale line and undrawable text refused, and undo;
  - `match_at`, then Replace on one match and Replace All on the rest as
    one step each;
  - the Add Text tool and `add_text`.
- `cargo test -p onionskin-app --features shell-test-support --lib`, on a
  real window (`tests::text_edit`, 3 tests):
  - Replace with no query, Replace, Replace All, and no match left; the
    find bar's Replace With and Replace All in the accessibility tree;
  - the Edit Text tool's box opened on the second line and focused, in the
    tree, and focusable. Escape leaves the line; Enter rewrites it (Edit
    Text in history); undrawable text goes to the status line;
  - the Add Text tool's empty box, and the new line drawn.

  Unit tests cover the find bar's Replace row and the context menu's Edit
  Text.
- **Whole-suite runs.**
  - Workspace without the app: 1289 passed, 0 failed.
  - App: 955 passed. The 5 failures are the known environmental ones: 2
    timing-sensitive canvas tests, which pass alone, and the 3 export
    rollback tests that fail when run as root.
  - App integration tests, guarantees included: 41 pass, 3 ignored.

## Coverage

`cargo tarpaulin` with optimisation off: 506 of 525 lines (96.4%).

| File | Lines covered |
| --- | --- |
| `content/edit_text.rs` | 89 of 92 |
| `content/lines.rs` | 67 of 71 |
| `content/redact/text.rs` | 91 of 99 |
| `core/text_edit.rs` | 157 of 159 |
| `tools-edit/text.rs` | 50 of 50 |
| `tools-edit/text_tool.rs` | 52 of 54 |

The app crate is not run under tarpaulin. Its text editing is covered by
the window tests above.

## Mutations

Each was caught, then reverted.

- The pen not put back after the new text:
  `part_of_an_operator_is_rewritten_and_the_rest_keeps_its_place` fails.
- A subset ignoring the codes the page drew with it:
  `a_subset_draws_what_it_drew_and_a_standard_font_the_rest` fails.
- Whole Word not applied: `occurrences_follow_case_and_whole_word` fails.

## Clippy and format

The following report only the existing `a11y::Shared::record` warning:

- `cargo clippy --workspace --all-targets --features
  onionskin-app/shell-test-support`;
- the app built with `--no-default-features` and each of `shell`,
  `shell,tools-edit`, `shell,tools-form` and `shell,codecs-common`.

`cargo fmt --all --check` is clean.

## Font, size and colour

Added later the same day. The change moves the "Change font, size, colour"
row to `partial`, and adds font, size and colour to Add Text.

- **What the user gets.** Under the line editor's text box are three short
  lists:
  - Font: Same font, Helvetica, Helvetica Bold, Times, Times Bold and
    Courier;
  - Size: Same size, 8, 10, 12, 14, 18 and 24;
  - Colour: Same colour, Black, Red, Blue, Green and Grey.

  The first of each keeps what the line has. For Add Text, the first
  entries mean Helvetica 12 pt in black. The line, or the new text, is set
  in what is picked. To a screen reader each list is a radio group whose
  entries pick themselves.
- **How it works.**
  - `LineEdit` carries a `TextStyle`. The new text is drawn with `rg` and
    `Tf` switched in, and then the line's font, size and fill colour are
    put back.
  - The interpreter keeps the operators that set the fill as written (a
    colour space with its colour, or a device colour), so the rest of the
    text object draws as it did.
  - The pen goes back by the new text's width at its own size, in units of
    the line's size.
  - `core` has `rewrite_styled_lines` and a styled `add_text`. `tools-edit`
    has `edit_styled_line` and `add_styled_text`.
- **Runs.**
  - `content --test edit_text`: 8 pass. The 2 new tests cover:
    - a standard font, size and colour switched in and back, with the next
      line's size and place unmoved;
    - a new size in the line's own font, where the word after it in the
      same `TJ` does not move and a colour space and its colour are put
      back.
  - `core --test text_edit`: 8 pass, 1 new: a styled rewrite written and
    read back. Add Text in Times and red is also asserted.
  - `tools-edit --test text`: 5 pass, 1 new: a restyled line and styled new
    text.
  - App, on a real window: the lists are in the tree, three picks are made
    through it, and the line is kept at 24 pt.
- **Coverage.**

  | File | Lines covered |
  | --- | --- |
  | `content/edit_text.rs` | 119 of 124 |
  | `core/text_edit.rs` | 166 of 169 |
  | `tools-edit/text.rs` | 54 of 54 |
  | `tools-edit/text_tool.rs` | 52 of 54 |
- **Not claimed.**
  - Alignment and spacing.
  - Any font but the standard ones. That waits on `text-engine`'s
    embedding and its fsType check.

## Not claimed

- **System fonts and fsType.** `text-engine` is still empty. Nothing is
  embedded, so a character the line's font cannot draw is set in a
  standard font, and one outside WinAnsi is refused. The fsType check has
  nothing to guard yet.
- **One font per line.** A rewritten line takes its first glyph's font. A
  bold word inside the line is not kept bold.
- **Reflow.** A longer line runs past where the old one ended, and a line
  is never wrapped. This is the next row, as the plan says.
- **Text in forms.** Text drawn inside a form XObject (headers stamped by
  some producers, for example) is refused.
- **Add Text's look.** It is Helvetica 12 pt in black unless the line
  editor's lists pick otherwise (see "Font, size and colour").
