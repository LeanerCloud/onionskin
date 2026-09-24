# M5 verification: Check Spelling

Date: 2026-09-24. Linux x86-64, stable toolchain. No macOS, Windows or
hosted-CI run is claimed.

## Rows

- **To `partial`:** Edit > Check Spelling (in comments and form fields).
  The command works on the scope the row names. It has one dictionary, US
  English, and does not check as you type.
- **Headline:** 103 planned / 42 partial / 80 out-of-scope, 178
  implemented. `acrobat_parity_headline_matches_every_inventory_row`
  passes.

## What the user gets

- **Edit > Check Spelling…**, live on an open document. It goes word by
  word through the text of every comment and the value of every text
  field, in page order: a page's comments first, then its fields. Password
  fields are left out.
- **What it shows for each word:**
  - the word;
  - the passage it is in, named and with the word marked (`Comment on
    page 2: «Teh» meeting is at noon`);
  - a Change To field holding the likeliest suggestion;
  - the other suggestions, each of which fills Change To.
- **What each button does:**
  - **Ignore:** goes on to the next word.
  - **Ignore All:** passes over the word everywhere, until the dialog
    closes.
  - **Add to Dictionary:** passes over the word, and keeps it in
    `dictionary.txt` in the data folder, so every document is checked with
    it. If it cannot be saved, the dialog says so and accepts the word
    until it closes.
  - **Change:** puts Change To in place of the word, as one undo step
    (Check Spelling), then goes on from after the change. A comment is
    given its new text as the Comments pane gives it. A field is given its
    new value, with its appearance drawn again.
- **When no word is left**, the dialog says so, and how many words it
  changed.
- **Screen readers.** Every part of the dialog is in the accessibility
  tree, and Change To is a focusable text field.
- **Without the plugin.** In a build without the spelling plugin, the entry
  is off and says why.

## How it works

- **A new plugin crate, `plugins/spelling`.**
  - **The dictionary** is SCOWL's `en_US` in Hunspell form. It is bundled
    in `dictionary/` with its notice (`COPYING`: the SCOWL, Moby, 12Dicts
    and WordNet grants). It is read by `spellbook` (MPL-2.0, which
    `deny.toml` allows).
  - **`Checker`** checks words, accepts added ones, and suggests likely
    ones. For a capitalized word it also asks for the lower-case form and
    capitalizes what comes back: `spellbook` suggests "Eh" first for "Teh"
    but "the" first for "teh". Suggestions that split a word in two are
    dropped.
  - **`words`** leaves out what is not a word: single letters, acronyms,
    mixed-case names such as iPhone, and tokens joined to digits, `@`,
    paths or dotted names. A typographic apostrophe is read as the plain
    one.
  - **`passages`, `misspellings` and `correct`.** `correct` refuses a
    passage whose text has changed since it was read, rather than change
    the wrong word.
  - **`UserDictionary`** keeps one word to a line, sorted and each once.
- **app.**
  - `chrome/spelling_dialog.rs`: the dialog's state, view and
    accessibility nodes.
  - `tabs/spelling.rs`: opening the dialog, the actions, and moving on
    after a change.
  - `MenuCommand::CheckSpelling`, `ShellDialog::Spelling`,
    `Activation::Spelling` and `TextField::SpellingChangeTo`.
  - The `spelling` feature, on by default.

## Runs

- `cargo test -p onionskin-spelling`: 6 pass.
  - Unit tests cover words, what is not a word, the dictionary with
    suggestions (capitalized and not) and added words, and the user
    dictionary file (including a folder in the way).
  - `tests/passages.rs`:
    - comments and text fields read in page order, with no password, empty
      field or link, and the misspellings found in them;
    - a comment and a field corrected as one undo step each, a stale
      correction and a range past the text refused, and undo.
- `cargo test -p onionskin-app --features shell-test-support --lib`, on a
  real window (`tests::spelling`, 2 tests):
  - the dialog opening on "Teh" with "The" in Change To and the passage
    marked, with the tree carrying Change To and Add to Dictionary. Then
    Ignore All, Ignore, a suggestion picked, and Change writing "received"
    into the field. Done, with the count, in the dialog and the tree;
  - Add to Dictionary kept in `dictionary.txt`, and the next check passing
    over the word.
- **Whole-suite runs.**
  - App: 956 passed. The 6 failures are the known environmental ones: 3
    timing-sensitive canvas tests, which pass alone, and the 3 export
    rollback tests that fail when run as root.
  - App integration tests, guarantees included: 41 pass, 3 ignored.
  - `--no-default-features --test kernel_emptiness`: 4 pass.

## Coverage

`cargo tarpaulin -p onionskin-spelling` with optimisation off: 150 of 151
lines (99.3%).

| File | Lines covered |
| --- | --- |
| `lib.rs` | 72 of 72 |
| `passages.rs` | 57 of 58 |
| `user.rs` | 21 of 21 |

## Mutations

Each was caught, then reverted.

- Every token checked as a word: `what_is_not_a_word_is_left_alone` fails.
- A changed passage corrected anyway:
  `a_correction_is_one_undo_step_and_a_stale_one_is_refused` fails.
- Add to Dictionary not saved: `added_words_are_kept_for_the_next_check`
  fails.

## Clippy and format

The following report only the existing `a11y::Shared::record` warning:

- `cargo clippy --workspace --all-targets --features
  onionskin-app/shell-test-support`;
- the app built with `--no-default-features` and `shell`, and with
  `shell,spelling`.

`cargo fmt --all --check` is clean.

## Not claimed

- **Other languages.** Only US English is bundled, and Acrobat's choice of
  dictionaries is not offered.
- **Checking while typing.** Words are not underlined as a comment or
  field is typed.
- **Page text.** The page's own text is not checked. Acrobat checks it only
  while editing text.
- **Change All.** It is not offered: each occurrence is changed on its own.
