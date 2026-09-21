# Onionskin test corpus

Every guarantee test in [`PLAN.md`](../PLAN.md) runs per corpus file, because
bytes are the product. This directory holds the corpus: what is fetched from
upstream, what is generated locally, and what is still to be built.

## Layout

| Path | Tracked in git | Produced by |
|---|---|---|
| `seeds/` | yes | `make-seeds.py`, committed output |
| `encrypted/` | yes | `make-encrypted.py` through qpdf, committed output |
| `malformed/` | no | `make-malformed.sh` from `seeds/` |
| `bench/` | no | `make-bench.py`, no inputs |
| `external/` | no | `fetch.sh` from pinned upstream revisions |
| `js-forms/` | README only | not yet, see `js-forms/README.md` |
| `tagged/` | README only | not yet, see `tagged/README.md` |

`external/`, `malformed/` and `bench/` are gitignored. Nothing under them is
vendored: they are reproducible from `fetch.sh`, `make-malformed.sh` and
`make-bench.py`.

## `encrypted/` (8 files)

One file per standard-security-handler revision - `/R` 2 through 6, RC4 and
AES-128 and AES-256, crypt filters, `/EncryptMetadata false`, object streams -
plus one `/R` 6 file with a user password that an empty password must not open.
Written by **qpdf** through pikepdf, so `cos`'s decryptor is checked against an
independent implementation of ISO 32000 7.6 rather than against itself; the
standard publishes algorithms and no test vectors.

Committed because they are small and because they are not reproducible byte
for byte: AES needs a random IV per string and stream, and the `/R` 6 file key
is random. Re-running `make-encrypted.py` produces equivalent fixtures with
different bytes, which is why the tests assert decrypted content and never
compare files.

## Quick start

```bash
./fetch.sh              # default sets, about 200 MB
./make-malformed.sh     # regenerate malformed/ from seeds/
./make-bench.py         # regenerate bench/ from nothing
./fetch.sh --list       # every set and whether it is present
```

`fetch.sh` skips stamped sets already on disk, so re-running costs well under a
second and needs no network. Sets with tracked checksums are revalidated before
that skip is accepted.

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

### `bench/` (1 file, 2235141 bytes)

One PDF written by `make-bench.py`: `pages-1000.pdf`, a thousand pages under a
page tree that branches by ten, so reaching page 500 parses the four nodes on
the path plus the siblings whose `/Count` lets them be skipped. Its text varies
per page from a seeded vocabulary, so a renderer cannot cache its way from page
1 to page 999, and its bytes are deterministic: no timestamps, no randomness,
no compression, so two runs produce the same file and a bench can assert on
numbers taken from it.

The rest of the corpus is almost all single-level page trees, whose `/Pages`
node has nothing but leaves under it and where the `/Count` subtree skip never
fires, so `crates/cos/tests/pages.rs` uses this file for both the "the skip
lands on the same pages a full walk does" check and the byte-count assertion
behind decision 11's time-to-first-page budget. Absent, those two tests skip;
under `ONIONSKIN_CORPUS_REQUIRED=1` they fail instead, rather than reporting a
pass they did not earn.

`crates/core/benches/` reads it through the same door, and every one of
decision 11's four budgets is stated over it: `open.rs` times the first page
and counts the bytes that reach it, `paint.rs` paints page 1000 of it cold,
and `scroll.rs` scrolls across two hundred of its pages for both the frame
budget and the memory one. The CI bench job generates it with
`make-bench.py` and sets `ONIONSKIN_CORPUS_REQUIRED=1`, so a run that cannot
find it fails instead of quietly measuring nothing.

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
| `hayro-corpus` | 41 | 159 MB, tracked by `checksums/hayro-corpus.sha256` | Subset of the [PDF Association large-scale PDF corpus](https://pdfa.org/new-large-scale-pdf-corpus-now-publicly-available/), the only real-world scanned material in reach |
| `hayro-pdfjs` | 679 | not measured | Ported from the pdf.js regression suite |
| `hayro-pdfbox` | 454 | not measured | From the Apache PDFBox issue tracker |
| `hayro-pdfium` | 127 | not measured | From the PDFium/Chromium issue tracker |

The cut: `hayro-corpus` is only 41 files but averages 3.9 MB each, one of them
67 MB, which alone would nearly double the default fetch. The other three add
about 1260 files whose total size is not known ahead of time because the bucket
publishes no index. All four are worth pulling on a machine that runs the full
suite; none is needed to work on the parser. `hayro-corpus` has tracked
SHA-256 values because the merge-gating first-paint benchmark depends on one of
its PDFs. The other R2-hosted optional sets remain unverified: their id lists are
pinned by hayro's manifest files, but their object bytes are not checked.

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
| 9, performance budgets | `bench/pages-1000.pdf` for the thousand-page budgets in decision 11, plus `hayro-corpus/0041790.pdf`, whose first page is the heaviest to rasterize and therefore the one the first-paint budget is stated against; `hayro-corpus` also carries the large real-world files, where the 67 MB scan is the natural worst case |

Tests 1, 2 and 6 walk directories, so a set added to `external/` is picked up
without touching test code. Tests 3, 7 and 8 need per-file expectations and
therefore name their inputs explicitly.

### What CI fetches, and why that list

The `test` job's Linux runner fetches the three default sets (`hayro`,
`pdf-association`, `verapdf`) and `hayro-corpus`, generates `malformed/` and
`bench/`, installs the `pdftotext` the extraction oracle scores against, and
then re-runs every suite that reads any of them under
`ONIONSKIN_CORPUS_REQUIRED=1`. Fetching a corpus is not proof anything measured
it: each of those suites returns early when its directory is absent, so the
ordinary `cargo test --workspace` before it would pass either way. The re-run
is what turns a silent skip into a failure, and therefore what makes removing
the fetch visible. `crates/app/tests/guarantees.rs` derives the re-run's
command list from the test targets that can reach a corpus lookup, following
the modules they declare, so a new suite that reads the corpus and never
reaches CI fails the build.

`hayro-corpus` is an opt-in set for a fetch on a laptop, and CI fetches it
anyway. Three live suites name it outright, `cos/tests/roundtrip.rs`,
`content/tests/corpus.rs` and `core/tests/search.rs`, plus one `#[ignore]`d
sweep in `content/tests/oracle.rs`; the first of those enforces guarantee 1.
Leaving it out would mean either a mandatory re-run that skipped guarantee 1's
own walk or a per-test skip list inside it, and both put the guarantee back
where it started. It costs 152 MB on a cold cache and nothing on a warm one.

The corpus steps run on the Linux runner only. Corpus assertions are byte and
structure work with no platform dimension, so putting the fetch on the
three-way matrix would buy three caches and three chances to flake for one
claim. The `test` and `bench` jobs cache `corpus/external` under separate keys
because they fetch different sets into it, and a shared key would make
whichever job ran first decide what the other one got.

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
| `seeds/`, `malformed/`, `bench/` | Same as Onionskin | Generated here, no third-party content |
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
id list, which comes from the manifests at the pinned hayro commit. For
`hayro-corpus`, this repository also tracks SHA-256 values for every fetched PDF
and verifies them before writing or accepting a stamp. The other R2 objects are
keyed by immutable id but carry no published checksum. If you need byte-level
reproducibility for those optional sets, mirror them yourself after the first
fetch.

Each fetched directory carries a `.fetch-stamp` recording its source and pinned
revision. `fetch.sh` uses that file to decide whether a set is complete. To pick
up a bumped revision, delete the set directory and re-run. A stamped
`hayro-corpus` directory is still checked against `checksums/hayro-corpus.sha256`
before `fetch.sh` skips it, so a stale or corrupted cache fails instead of being
trusted because the stamp is present.

The two kinds of set recover from an interrupted fetch differently, because a
tarball extraction has no meaningful partial state and a per-file download does:

- **Tarball sets** (`hayro`, `pdf-association/*`, `verapdf`) are staged under a
  `.partial` name and moved into place in one step. A set directory that exists
  without a stamp therefore cannot be resumed, and `fetch.sh` refuses to touch
  it rather than deleting it. Remove it by hand and re-run.
- **R2 sets** (`hayro-corpus`, `hayro-pdfjs`, `hayro-pdfbox`, `hayro-pdfium`)
  are downloaded into a per-run staging directory outside the final set
  directory, then copied into a private publication directory and moved into the
  final name with no-replace semantics. The final destination must be absent
  before download and again at publication time; an unstamped final directory is
  preserved but rejected rather than adopted or resumed. For `hayro-corpus`, the
  staged files are checked against the tracked SHA-256 manifest before anything
  is published, then the private copy is rechecked against the same manifest
  immediately before publication. The optional R2 sets without checksums use the
  same staging and publication path, but are explicitly reported as unchecked;
  they only pin the id list, not object bytes.

## Requirements

`fetch.sh` needs `curl`, `tar` and Python 3.10 or newer, resolved from `PYTHON`,
then `python3`, then `python`. `make-malformed.sh` needs `perl`. `make-seeds.py`
and `make-bench.py` need python3 with no third-party packages. Both shell
scripts run on bash 3.2, which is what macOS ships.
