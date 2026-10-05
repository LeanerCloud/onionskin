# M6 WP4 verification: structure foundation

Date: 2026-10-05. macOS, stable toolchain, branch `feat/m6-wp4-structure`.
No VoiceOver session, Windows or hosted-CI run is claimed.

## Rows

- **To `implemented`:** Tags pane; Export to plain text / accessible text.
- **To `partial`:** Content pane (the page's drawing, view only); Reading a
  tagged PDF with a screen reader (tree tested, VoiceOver not run).
- **Text updated:** Bookmarks, and the Bookmarks pane context menu, for New
  Bookmarks From Structure.
- **Headline:** 403 rows: 80 planned / 43 partial / 80 out-of-scope, 200
  implemented.

## What the user gets

- **Tags pane.** The structure tree, folded to its top level. Each row shows
  the type as written, `(as X)` where a role map or the PDF 2.0 namespace
  resolves it to a different standard type, and the element's title, alt text or
  words. The triangle opens an element; choosing the row goes to its page and
  boxes its content, excluded and artifact content included.
- **Content pane.** The current page's text, images and paths in groups, each
  piece with the tag it sits under. Choosing one boxes it and goes to the page.
- **Screen reader.** A tagged page is published as its structure; an
  unreadable structure falls back to the text runs with an alert.
- **Plain-text export** reads a tagged document in structure order.
- **New Bookmarks From Structure** makes nested bookmarks from `H1` to `Hn` and
  `Title`, in one undo step, from the pane's button or its context menu.

## How it was checked

- Each of the eight commits was gated alone (fmt, clippy for the workspace and
  with `shell,shell-test-support`, tests for both) and reviewed by a fresh
  reviewer with a cold brief, two passes for the last three.
- Structure order against pikepdf walking the StructTreeRoot
  (`corpus/tagged/reading_order.py`, table in `crates/core/tests/structure.rs`).
- 195 veraPDF PDF/UA pass files through the AccessKit projection: no panic, no
  duplicate key, no hidden word, no collapsed Document.
- The outline written by New Bookmarks From Structure checked with `qpdf --check`
  and pikepdf (chains, `/Count` signs and magnitudes, `/Dest` pages), and undo
  and redo round-trip.
- Mutation runs over the new logic; the survivors that were not equivalent got
  tests.
- Review found a cos parser bug (a keyword cut by the 1024-byte read window was
  a syntax error), fixed in its own commit with a test that fails without it.

## Not verified

- VoiceOver. accesskit_macos 0.26.3 ignores heading level, language outside
  attributed text, and table indices, and a tree item's level and expanded
  state, so the Tags rows say those in words.
- Timing at scale is in `known-issues.md`.
