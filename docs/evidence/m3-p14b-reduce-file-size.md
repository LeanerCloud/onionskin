# M3 P14b verification: Compress a PDF and Reduce File Size

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain, GPUI's test platform. No macOS, Windows or hosted-CI run is
claimed.

Rows closed:

- 51 Compress a PDF.
- 52 Reduce File Size.

Both rows' notes carry the scope cut and the encrypted-source refusal.

## What the user gets

- **File > Reduce File Size…** opens a dialog that says what happens: "The
  copy keeps no editing history: every earlier version of the document is
  discarded from it and cannot be restored. This document is not changed."
- **Save a Reduced Copy…** asks where the copy goes, suggesting
  "*name* (reduced).pdf", and writes it there. The open document and its
  file are left exactly as they were.
- **A notice** says the new size, the old size, and again that the copy
  keeps no history.
- **Nothing to compress:** a document with no image above the target and
  nothing unreferenced says so and writes nothing. So does a document whose
  rewrite would not come out smaller.
- **Encrypted documents:** the menu entry is disabled with the
  encrypted-source reason.

## How it is built

- **`commands-core/src/compress.rs`:**
  - reads the document as it stands, edits included, through `structure()`;
  - keeps every object reachable from the catalog and the information
    dictionary;
  - downsamples each Gray or RGB image above 150 ppi to 150 ppi, and
    re-encodes it as JPEG at quality 75 when that is smaller;
  - writes a new file with `cos::write_new`: one section and a classic
    cross-reference table.
- **What an image's resolution is taken to be:** its pixels over the page
  it is first drawn on. An image drawn smaller than the page really has more
  pixels per inch than that, so the rule can keep more pixels than needed,
  never fewer.
- **Images left alone:**
  - CMYK images, whose colours a JPEG round trip through RGB would change;
  - image masks and images with `/Decode` or `/Mask`.
- **The scope cut the plan asks for:** no object streams and no
  cross-reference stream are written, because `cos` has no writer for
  either.
- **The command:** `file.reduce-size` is registered with effect `ReadsOut`.
  That makes the encrypted refusal the same `Requirement::Command` query as
  every read-out, and the encrypted-source sweep covers it. The command's
  own body writes the copy beside the document; the menu opens the dialog
  instead.

## Runs

- `commands-core/tests/compress.rs`:
  - `a_heavy_image_is_downsampled_into_a_smaller_rewrite_that_looks_the_same`:
    a 400 ppi page image comes out at most 750 pixels on its long side, in
    less than half the bytes. The output parses as one section with a
    classic `xref` and no `/Type /XRef`. Page count and text are unchanged,
    and the render at 25% is within a mean difference of 8 of the source's.
  - `a_document_with_nothing_to_compress_says_so_and_writes_nothing`.
  - `an_encrypted_document_is_refused`.
  - `the_seed_documents_compress_or_say_they_have_nothing_to`.
- Window tests in `tabs/tests/reduce.rs`:
  - `reduce_file_size_writes_a_smaller_copy_and_says_history_is_discarded`:
    - the dialog's text is in the tree and names the discarded history;
    - the chosen path gets a copy under half the size;
    - the original file's bytes are unchanged, and the tab stays on it,
      clean;
    - the notice repeats the history warning.
  - `save_appends_and_never_rewrites`: Rotate then Save leaves the file
    with two sections, one appended.
  - `reduce_file_size_is_refused_on_an_encrypted_document`.
- `encrypted_sweep` still passes with the new `ReadsOut` command
  registered.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib`:
  691 pass and 5 fail. The 5 are the environmental set in
  `known-issues.md`.
- `cargo clippy` for the app in both feature sets, and for
  `commands-core`, is clean apart from the Linux-only dead code in
  `a11y/mod.rs`.

## Mutations

- **Removing the downsampling step** fails the size-reduction test
  ("a 400 ppi image is something to compress"). The three correctness tests
  stay green.
- **Emitting an incremental section** instead of a rewrite would fail the
  one-section assertion in the same test.

## Not done here, and said

- **Compatibility target:** Acrobat's Reduce File Size asks which version
  of Acrobat the copy must open in. There is one output here, PDF 1.7 with
  a classic table, so there is nothing to choose.
- **The `external/` fixture** with real recompressible images is not
  fetched in this environment. The fixture here is generated: a 400 ppi
  photo-like image.
