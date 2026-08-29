# Onionskin test corpus

Every guarantee test in [`PLAN.md`](../PLAN.md) runs per corpus file, because
bytes are the product. This directory holds the corpus: what is fetched from
upstream, what is generated locally, and what is still to be built.

## Layout

| Path | Tracked in git | Produced by |
|---|---|---|
| `seeds/` | yes | `make-seeds.py`, committed output |
| `malformed/` | no | `make-malformed.sh` from `seeds/` |
| `external/` | no | `fetch.sh` from pinned upstream revisions |
| `js-forms/` | README only | not yet, see `js-forms/README.md` |
| `tagged/` | README only | not yet, see `tagged/README.md` |

`external/` and `malformed/` are gitignored. Nothing under them is vendored:
they are reproducible from `fetch.sh` and `make-malformed.sh`.

## Quick start

```bash
./fetch.sh              # default sets, about 200 MB
./make-malformed.sh     # regenerate malformed/ from seeds/
./fetch.sh --list       # every set and whether it is present
```

`fetch.sh` skips any set already on disk, so re-running costs well under a
second and needs no network.

## Sets

### `seeds/` (3 files, 1818 bytes)

Three tiny PDFs written by `make-seeds.py`: `minimal.pdf` (one empty page),
`hello.pdf` (one page, one text run, one standard-14 font) and `two-page.pdf`
(two pages, with a cropped and rotated second page).
They exist so `malformed/` has a known-good starting point where every byte is
accounted for.

Two properties are deliberate:

- Pure ASCII with uncompressed content streams, so `make-malformed.sh` can do
  line-oriented xref surgery without risking a match inside binary stream data.
- Xref entries end with `space LF`, never `CR LF`, so the files contain no
  carriage returns at all.

`make-seeds.py` verifies both, plus that every xref offset actually points at
its object, and fails loud otherwise. The generated PDFs are committed, so the
script only needs re-running when a seed definition changes.

### `malformed/` (15 files, 11416 bytes)

Five deterministic damage variants per seed. The suffix names the defect:

| Suffix | Damage | Noticed by poppler |
|---|---|---|
| `-xref-bad-offsets` | every in-use xref offset shifted by 137 bytes, so the table is found but nothing it points at is an object header | yes, "xref num 1 not found but needed, try to reconstruct" |
| `-junk-header` | 1024 bytes of junk before `%PDF-`, which also puts every xref offset short by 1024 | yes, "May not be a PDF file (continuing anyway)" |
| `-truncated` | tail cut at 60% of the original size, losing the xref table, the trailer and `%%EOF`, and leaving the last object incomplete | yes, "Couldn't find trailer dictionary" |
| `-no-eof` | trailing `%%EOF` removed and nothing else, isolating that one defect | no, silent |
| `-xref-count-mismatch` | subsection header declares 3 more entries than are present, so the declared range runs into the trailer | no, silent |

The last column is measured: `pdfinfo` on all 15 files, exit status and stderr
recorded. The six `-no-eof` and `-xref-count-mismatch` files pass with exit 0 and
empty stderr on all three seeds.

That silence is a property worth having in the set, not a defect in it. Both
variants violate ISO 32000 (7.5.5 requires the `%%EOF` marker; 7.5.4 requires the
subsection header's count to match the entries that follow), and a lenient reader
tolerating them is exactly the case where a repair pass can quietly disagree with
what the file claims. Guarantee test 6 asserts that Onionskin notices, repairs
and records the repair in the appended section, which is a stronger bar than
"another reader complained".

Each transform asserts that its output is non-empty and actually differs from
its seed, so a silently non-matching regex fails the run rather than producing a
valid file with a malformed name. Re-running produces byte-identical output.

For real-world damage rather than synthetic damage, `external/hayro/pdfs/load/`
is the complement: 80 crash-regression files, 62 of which do not have `%PDF-` in
their first 1024 bytes.

### `external/hayro` (360 PDFs, 28 MB, default)

Upstream: `LaurenzV/hayro` at `5a5f0e247c970df948505ee0bb36e2df2504bf86`, the
`hayro-tests/` subtree.

hayro's test PDFs are stored two ways, and only one of them is git:

- About 360 files are committed under `hayro-tests/pdfs/`, split into `custom/`
  (279 curated rendering cases), `load/` (80 crash regressions, mostly fuzzed
  and deliberately broken) and `other/`. There is no submodule and no git LFS;
  these are ordinary blobs. `fetch.sh` takes them from the codeload tarball of
  the pinned commit.
- The bulk, roughly 1300 more files, is not in the repository at all. It lives
  in a Cloudflare R2 bucket at `https://hayro-assets.dev/<kind>/<id>.pdf`, and
  upstream's `hayro-tests/sync.py` downloads it one object at a time using the
  id lists in `manifest_corpus.json`, `manifest_pdfjs.json`,
  `manifest_pdfbox.json` and `manifest_pdfium.json`. Those manifests come along
  with the subtree, so `fetch.sh` reproduces the same fetch for the sets below
  without needing upstream's Python dependencies.

The rendering snapshots hayro compares against are generated locally by its own
test run and are not published, so there is nothing to fetch there. Onionskin
wants the input PDFs, not the snapshots.

### `external/pdf-association` (70 PDFs, 6.6 MB, default)

Three PDF Association repositories, each pinned:

| Repo | Revision | PDFs | What it is |
|---|---|---|---|
| `pdf20examples` | `c20f2c17bfcc4baab7cfe62e70fae64caf14d5fa` | 7 | Minimal PDF 2.0 files, including one built by incremental save and one with an offset start |
| `safedocs` | `a6fd37308c91a0d2c17ebcace970367181bc0da7` | 26 | DARPA SafeDocs artifacts: compacted syntax, dialects, inline image abbreviations, targeted parser edge cases |
| `pdf-differences` | `26caa8795933269f3a38530369c70813486eabee` | 37 | Files where real viewers disagree: fill and stroke ordering, blend modes, degenerate dashing |

`pdf20examples/PDF 2.0 via incremental save.pdf` is the single most on-point
file in the whole corpus for Onionskin's core invariant, since incremental
update is exactly what the editor produces.

### `external/verapdf` (2907 PDFs, 164 MB, default)

Upstream: `veraPDF/veraPDF-corpus` at
`49de56cd987929932c9e4fbbbe67d052bf44ef83`, whole repository.

Atomic, self-documenting conformance files whose directory names map to clauses
of ISO 19005 (PDF/A), ISO 14289 (PDF/UA) and ISO 32000. Largest groups:
`PDF_A-2b` (986), `PDF_A-1b` (569), `PDF_A-4` (487), `PDF_UA-1` (296),
`Isartor test files` (205), `PDF_UA-2` (138), `TWG test files` (85).

The 434 files under `PDF_UA-1/` and `PDF_UA-2/` are the corpus's main supply of
tagged documents and the starting point for `tagged/`.

### Optional sets

These are hayro's R2-hosted extensions, left out of the default fetch so a first
run stays inside a few hundred megabytes. Fetch by name:

```bash
./fetch.sh hayro-corpus hayro-pdfjs hayro-pdfbox hayro-pdfium
```

| Set | Files | Size | Source |
|---|---|---|---|
| `hayro-corpus` | 41 | 159 MB | Subset of the [PDF Association large-scale PDF corpus](https://pdfa.org/new-large-scale-pdf-corpus-now-publicly-available/), the only real-world scanned material in reach |
| `hayro-pdfjs` | 679 | not measured | Ported from the pdf.js regression suite |
| `hayro-pdfbox` | 454 | not measured | From the Apache PDFBox issue tracker |
| `hayro-pdfium` | 127 | not measured | From the PDFium/Chromium issue tracker |

The cut: `hayro-corpus` is only 41 files but averages 3.9 MB each, one of them
67 MB, which alone would nearly double the default fetch. The other three add
about 1260 files whose total size is not known ahead of time because the bucket
publishes no index. All four are worth pulling on a machine that runs the full
suite; none is needed to work on the parser.

## How the guarantee tests consume the corpus

Guarantee test numbers refer to [`PLAN.md`](../PLAN.md), "Guarantee tests".

| Test | Input |
|---|---|
| 1, round-trip byte-identical | every file under `external/` and `seeds/`, except `hayro/pdfs/load/`, whose files are deliberately broken and only have to open without crashing |
| 2, onionskin incremental save | same set and same carve-out, plus `pdf20examples/PDF 2.0 via incremental save.pdf` as the shape reference |
| 3, redaction leaves no trace | files with extractable text: `hayro/pdfs/custom/`, `verapdf/PDF_UA-*`, `pdf20examples` |
| 4, signature preservation | see the gap below |
| 6, repair | all of `malformed/`, plus `hayro/pdfs/load/` for real-world damage |
| 7, forms compute | `js-forms/`, not built yet |
| 8, tag integrity | `tagged/`, not built yet; `verapdf/PDF_UA-1` and `PDF_UA-2` are the raw material |
| 9, performance budgets | `hayro-corpus` for large real-world files; the 67 MB scan is the natural worst case |

Tests 1, 2 and 6 walk directories, so a set added to `external/` is picked up
without touching test code. Tests 3, 7 and 8 need per-file expectations and
therefore name their inputs explicitly.

### Known gap: signed documents

A raw byte scan of the fetched sets finds `/Sig` or `/Adbe.pkcs7` in only 13
files (11 in `verapdf`, 2 in `hayro`), and none of them is a document signed by
a real certificate chain that would still validate. Guarantee test 4 needs
signed files whose signatures verify before the edit, so that "the signature is
still valid afterwards" means something. Producing that set is its own task; it
is not covered by any public corpus here. The counts above come from a raw byte
grep and undercount files whose catalog sits in a compressed object stream, so
treat them as indicative.

## Licensing

Nothing under `external/` is redistributed by this repository. It is fetched at
build time and gitignored, so these terms govern local use and any mirror you
choose to publish.

| Set | License | Notes |
|---|---|---|
| `seeds/`, `malformed/` | Same as Onionskin | Generated here, no third-party content |
| `external/hayro` | Apache-2.0 OR MIT for hayro's own code | The test PDFs are not covered by that grant. They were aggregated from the pdf.js, PDFBox and PDFium issue trackers and from user reports, and carry no unified license. Treat them as third-party material for local testing and do not redistribute the set. `external/hayro/LICENSE-APACHE` and `LICENSE-MIT` are symlinks to the hayro repository root and dangle here, because only the `hayro-tests/` subtree is extracted; read the license text upstream. |
| `external/hayro-corpus` and the other R2 sets | Unstated | Same caveat, plus the PDF Association large-scale corpus terms for `hayro-corpus`. Local testing only. |
| `external/pdf-association/pdf20examples` | CC BY-SA 4.0 | Attribute the PDF Association; share-alike applies to derivatives |
| `external/pdf-association/safedocs` | Apache-2.0 | DARPA SafeDocs program artifacts |
| `external/pdf-association/pdf-differences` | Apache-2.0 | |
| `external/verapdf` | CC BY 4.0 | Attribute the veraPDF consortium. Includes the Isartor suite, which carries its own upstream terms. |

If Onionskin ever publishes rendered output or extracted content from these
files, the attribution and share-alike obligations above apply to that output.
Running tests against them locally does not trigger either.

## Reproducibility

Every git-hosted set is pinned to a full commit SHA and fetched as the codeload
tarball of that SHA. A codeload tarball is not guaranteed byte-stable across git
server versions, so the pin is the commit, not an archive checksum. The commit
fixes the file list and every file's contents exactly.

The R2-hosted sets have no upstream version at all. What is pinned there is the
id list, which comes from the manifests at the pinned hayro commit. The objects
themselves are keyed by immutable id but carry no published checksum. If you
need byte-level reproducibility for those, mirror the set yourself after the
first fetch.

Each fetched directory carries a `.fetch-stamp` recording its source and pinned
revision. `fetch.sh` uses that file to decide whether a set is complete. To pick
up a bumped revision, delete the set directory and re-run.

The two kinds of set recover from an interrupted fetch differently, because a
tarball extraction has no meaningful partial state and a per-file download does:

- **Tarball sets** (`hayro`, `pdf-association/*`, `verapdf`) are staged under a
  `.partial` name and moved into place in one step. A set directory that exists
  without a stamp therefore cannot be resumed, and `fetch.sh` refuses to touch
  it rather than deleting it. Remove it by hand and re-run.
- **R2 sets** (`hayro-corpus`, `hayro-pdfjs`, `hayro-pdfbox`, `hayro-pdfium`)
  deliberately adopt an unstamped directory and resume into it. Each pdf is
  downloaded to `<id>.pdf.partial` and renamed on success, and a file already on
  disk is never re-fetched or overwritten, so a killed run costs at most the one
  file in flight. The stamp is written only when every id in the manifest is
  present.

## Requirements

`fetch.sh` needs `curl`, `tar` and `python3`. `make-malformed.sh` needs `perl`.
`make-seeds.py` needs python3 with no third-party packages. Both shell scripts
run on bash 3.2, which is what macOS ships.
