# M3 P13a verification: Document Properties, Initial View, Save as Other

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain. The windowed shell tests ran here; no macOS, Windows or hosted-CI
run is claimed, and the a11y probe (macOS only) was not run.

Rows closed: 12 File > Save as Other, 14 File > Properties, 32 Initial View
settings. The Layers pane's Layer Properties entry is live.

## The five-tab list: not confirmed against the corpus

The package's first task was to confirm Acrobat's tab list against the
screenshot corpus. **It could not be confirmed.**
`parity/reference/acrobat-reader-25.001.20438` has no capture of the
Document Properties dialog. The dialog is built with the five tabs the parity
row names: Description, Security, Fonts, Initial View, Custom. That list comes
from Adobe's documentation of the current unified UI and is recorded here as
unconfirmed. Adding a capture to the corpus is the follow-up. The tab list is
one constant (`PropertiesTab::ALL`), so correcting it is a one-line change
plus the test that pins it.

## Runs

- `cargo test -p onionskin-core --test metadata`: 9 pass. That is the 8 from
  the metadata commit plus the Fonts reader's test. `cargo test -p
  onionskin-core --lib metadata`: 7 pass.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  628 pass and 6 fail. The 6 are the environmental set in `known-issues.md`:
  - the zoom raster test;
  - the unmeasured page test;
  - the frame-open test;
  - the three rollback tests.

  All 9 Properties window tests pass.
- `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support --lib -- properties layers initial_view`: 30 pass.
  With no codecs installed, the Save as Other test takes its
  no-exporter branch.
- `cargo clippy` over core and over the app is clean in both feature sets,
  apart from the Linux-only dead code in `a11y/mod.rs`. `cargo fmt --all --
  --check` is clean.

## Metadata (core, committed earlier as 86a2ab2)

`/Info` and XMP are written in one transaction and read back by two
independent readers. The round trip, the no-`/Info` document with its trailer
undo, and the other-producer packet are that commit's tests. The mutation
that skips XMP fails 3 of them.

## Fonts (core)

- `metadata::document_fonts` lists every font the pages name. It reads the
  resource dictionaries, not extracted text, so a font that draws nothing
  extractable is still listed.
- It follows Form XObjects, with a visited set and a depth limit.
- It strips a subset prefix: six capitals and a plus.
- It reads embedding from the font descriptor, or from the descendant font
  for a composite font. A Type 3 font counts as embedded.
- The test covers a page font, a font reached only through a form, an
  embedded subset, and a composite font. It also checks that a font named on
  two pages is listed once.

## The dialog (app)

- **File > Properties…** (`cmd-d`, Acrobat's Ctrl+D) opens the dialog on the
  Description tab.
- **One Apply, one undo step.** Apply writes whichever halves changed:
  description and custom keys through `write_properties`, and the view
  through `write_initial_view`. Both go into one `edit_document` transaction.
  - The window test sets a title, an author and a custom key, then asserts
    both copies and a history reach of 1.
  - A dialog left unchanged writes nothing: reach stays 0.
- **Custom.** The dialog refuses a blank key, a key with spaces, and a
  duplicate. A standard key such as `Producer` is refused by `core` at Apply.
  The error stays in the dialog as a `Role::Alert` and nothing is written.
- **Security is read-only, and says so.**
  - The a11y `State` gains `read_only`, published as AccessKit's read-only
    flag.
  - Every Security row is a read-only field with no activation. The test
    asserts both on all six rows.
  - Removing `read_only` from the rows fails that test.
  - On an encrypted document the tab reports Password Security and the four
    permissions.
  - Apply is disabled, with `core`'s refusal as its description.
- **Fonts** is a read-only list.
- **Initial View** offers:
  - page layout: 6 layouts plus Default;
  - navigation tab: 6 modes plus Default;
  - magnification: the offered fits, plus the document's own zoom when it is
    not one of them, so opening the dialog cannot change it;
  - the page to open at, counted from 1 and checked against the page count.
- **The controls leave the tree on close.** The test asserts that the tab
  list is gone and the state dropped.
- **Files.** The plan named `shell/properties_dialog.rs`. The dialog lives in
  `chrome/properties_dialog/`, beside the other M3 dialogs, because it builds
  `SearchInput` fields, which are `chrome`'s. There is no
  `commands-core/src/properties.rs`: the dialog is shell behaviour, and no
  registered command has anything to run without it.

## Initial View honoured on open

- `apply_page_display` applies the preferences first and then the
  document's own view. Where the document says something, it wins, as it
  does in Acrobat.
- Layout mapping:
  - `OneColumn` is continuous single page;
  - `TwoColumn*` is continuous two-up;
  - `TwoPage*` is two-up;
  - `*Right` shows page one alone as a cover.
- The fit applies with an open page. Fit Visible falls back to Fit Page,
  because nothing is drawn at open for the visible-content fit to measure.
- The page mode opens its navigation pane: bookmarks, thumbnails,
  attachments or layers. This happens for File > Open and for documents given
  on the command line.
- Full Screen is not honoured. A document does not take over the screen on
  open.
- **Asserted through the session.** A file is written with page 2, Fit Width,
  TwoColumnRight and UseThumbs through `DocumentFile`, then saved and opened
  in a window. The canvas is on page 2 at Fit Width, two-up continuous with
  the cover, and the thumbnails pane is open.
- The plan's mutation, skipping `apply_initial_view`, fails that test.

## Save as Other

- The File menu entry opens a panel of its own. The panel lists one entry
  per export codec the registry has. A missing codec means a missing entry,
  not a disabled one.
- The test compares the panel against `build_registry().codecs()`, not
  against a list. It also asserts that no entry names PDF/X or Reader
  Extended.
- With no codecs installed, the File entry is disabled: "No installed codec
  exports".

## Layer Properties

The Layers pane entry is live. It opens a read-only dialog listing each layer
with its current visibility and whether the document locks it. A closing row
says that renaming a layer or changing its intent is layer editing, which
parity row 204 puts after 1.0.

## Not done here, and said

- **The corpus capture.** See the top of this document.
- **Full Screen page mode** is not honoured, for the reason given above.
- **The Description tab's file facts** omit PDF version, page size, tagged
  and fast web view. Nothing in `core` reports them yet.
