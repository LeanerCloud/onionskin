# Tagged PDF corpus (not built yet)

Guarantee test 8 in [`PLAN.md`](../../PLAN.md) says editing a tagged document
"leaves its structure tree valid and consistent with the edited content, checked
by `tools-accessibility`'s own checker, eating its own dog food."

Nothing here yet. Unlike `js-forms/`, the raw material is already on disk after
`fetch.sh`; what is missing is the per-file before-and-after expectations, and
the checker that would grade them.

## Raw material already fetched

`external/verapdf/` carries 434 PDF/UA conformance files:

- `PDF_UA-1/` (296 files), organised by clause: `7.2 Text` (110), `7.18
  Annotations` (48), `7.21 Fonts` (46), `7.4 Headings` (14), and so on.
- `PDF_UA-2/` (138 files), including `8.4 Text representation for content` (67)
  and `8.2 Logical structure` (49).

CC BY 4.0, atomic, and self-documenting via their outlines. They are the right
input for "does the checker agree with veraPDF about which files are valid",
which is the first thing to build, since a checker that disagrees with veraPDF
on unedited files cannot be trusted about edited ones.

A byte scan finds `/StructTreeRoot` in 590 files under `external/verapdf/`, more
than the 434 in `PDF_UA-*`, so there is additional tagged material in the
PDF/A-1a, PDF/A-2a and PDF/A-4 groups. The count comes from a raw grep and
undercounts files whose catalog sits in a compressed object stream.

## Candidate sources for what is missing

The veraPDF files are conformance probes: mostly one page, one construct, pass
or fail. They do not exercise a structure tree deep enough that an edit has
somewhere interesting to go. Concrete public sources for that:

1. **`pdf-association/techniques-for-accessible-pdf`**, revision
   `9772af3c93ce6409b945c1c60a4cd7c8e74c8c40`, CC BY 4.0. 82 tagged PDFs laid
   out as matched pass/fail pairs under
   `fundamentals/<n>-<topic>/G<n>_<nn>-<description>/UA1_Tpdf-G<n>_<nn>.pdf`,
   with `F` in the id marking the failing variant. Directory names state the
   technique being demonstrated, which makes them usable as expectations almost
   as-is. This is the strongest single candidate and could be added to
   `fetch.sh` as a fourth `pdf-association` repo once the checker exists.
2. **PDF Association PDF/UA Reference Suite** and the **Matterhorn Protocol**
   test files, published at `pdfa.org/resource/` rather than on GitHub. The
   Matterhorn Protocol enumerates 31 checkpoints and 136 failure conditions, and
   is the standard rubric a checker is graded against. Not fetchable by commit
   SHA, so vendor with a recorded download date and version.
3. **`pdf-association/tagged-pdf-school`**, CC BY 4.0. Currently community
   feedback, no PDFs, but it is where new tagging examples land. Worth watching.
4. **Well-Tagged PDF (WTPDF) 1.0 examples**, published by the PDF Association
   alongside the specification. WTPDF is the tagging profile PDF 2.0 documents
   are expected to follow, so these are the forward-looking cases.
5. **`pdf-association/pdf-corpora`**, CC BY 4.0. The index to check before
   assembling anything by hand.

Everything above is CC BY 4.0 or published by the PDF Association under
comparable terms, so attribution is the only obligation for local use.

## How the set gets produced

1. Add `techniques-for-accessible-pdf` to `fetch.sh` at a pinned revision, so
   the pass/fail pairs land in `external/` like every other fetched set. This
   directory stays for the hand-authored part only.
2. Vendor a handful of documents with genuinely deep structure trees under
   `tagged/pdfs/`: a multi-level nested list, a table with header scope and a
   spanning cell, a figure with alt text inside a paragraph, and a document with
   an artifact region adjacent to real content. Record source, URL, date and
   licence per file in `SOURCES.md`. These have to be authored or picked by
   hand, because no public corpus targets structure depth.
3. For each vendored file write `pdfs/<stem>.edit.json`: one edit expressed in
   Onionskin's own terms, chosen so it forces the structure tree to move rather
   than just re-render. Delete a page containing a list item; insert a paragraph
   between two `<H2>` sections; redact a table cell; reorder two pages.
4. Alongside it, `pdfs/<stem>.expected-tree.json`: the structure tree the edit
   should produce, as a normalised nesting of structure types, MCIDs and
   attributes. Author this by reasoning from the spec, then confirm it against
   Acrobat Pro's reading order and tags panel, and record the Acrobat version
   used.
5. Only then wire the test: apply the edit, run `tools-accessibility`'s checker
   over the result, and assert both that the checker reports the document valid
   and that the tree matches `expected-tree.json`.

Step 4 is the reason this waits on `tools-accessibility`. Writing expectations
before there is a checker to express them in produces a format that will be
rewritten, so the ordering is: checker first, graded against `PDF_UA-*` and the
techniques pass/fail pairs, then these expectations on top of it.
