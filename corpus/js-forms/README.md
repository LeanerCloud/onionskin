# JS forms corpus (not built yet)

Guarantee test 7 in [`PLAN.md`](../../PLAN.md) says every file in this set
"fills like Acrobat does: computed fields recalculate, formats apply, validation
fires, verified against Acrobat-produced expected values."

Nothing here yet, and it cannot be produced by `fetch.sh`. The PDFs are the easy
half; the expected values are the work, and they can only come from a licensed
Acrobat driving each file by hand.

## Why the fetched corpora do not cover this

A raw byte scan of everything `fetch.sh` pulls finds `/JavaScript` or `/JS` in
30 files across all sets, and most of those are open-action scripts rather than
AcroForm calculation, format and validation actions. Nobody publishes a form
corpus with recorded Acrobat behaviour, because the behaviour is the product of
a proprietary interpreter.

## Candidate sources for the PDFs

Concrete, in rough order of usefulness:

1. **IRS fillable forms**, `https://www.irs.gov/pub/irs-pdf/`. Real
   Acrobat-authored AcroForms with dense calculation and validation scripts, and
   published totals to check against. `f1040.pdf`, `f1040sc.pdf`,
   `f1040sse.pdf`, `f2441.pdf`, `f8949.pdf` are good starting points. Works of
   the US federal government, so public domain in the US. Stable enough at
   `f<name>.pdf`, but the IRS revises them yearly, so pin by downloading once
   and recording the revision date printed on the form.
2. **`mozilla/pdf.js`**, `test/pdfs/`. The entries in `test/test_manifest.json`
   carrying `"enableScripting": true` are exactly the scripting regressions, and
   the manifest doubles as the id list. Repository is Apache-2.0, but the
   individual PDFs have mixed third-party provenance and some are `.link` stubs
   pointing at external URLs. Local testing only.
3. **Apache PDFBox**, `pdfbox/src/test/resources/` plus the form and JavaScript
   issues in its Jira. Apache-2.0.
4. **PDF Association corpus index**, `github.com/pdf-association/pdf-corpora`
   (CC BY 4.0). An index of PDF-centric corpora rather than files, and the right
   place to look before assembling anything by hand.
5. **Adobe Acrobat SDK sample forms**. The closest thing to a reference, but the
   SDK license does not allow redistribution. Usable locally by whoever holds
   the licence, so treat as a private supplement, not part of the set.

## How the set gets produced

1. Pick a dozen or so files from the sources above, favouring breadth of
   scripting feature over page count: `AFNumber_Format`, `AFPercent_Format`,
   `AFDate_Format`, `AFSpecial_Keystroke`, field-level `/C` calculation actions,
   a `/CO` calculation order that matters, `/V` validation that rejects input,
   and at least one form whose calculation order is genuinely ambiguous.
2. Vendor them here under `js-forms/pdfs/`, with a `SOURCES.md` recording, per
   file, the exact URL, the download date, the form revision, and the licence.
   These are small and hand-picked, so vendoring beats a fetch script; unlike
   `external/`, the whole point is that the bytes never move under us.
3. For each PDF write `pdfs/<stem>.scenario.json`: an ordered list of field
   interactions, each one a field name, an action (`enter`, `blur`, `check`,
   `choose`) and a value.
4. Replay each scenario in Acrobat by hand, on a machine with a real licence,
   and record the resulting state as `pdfs/<stem>.expected.json`: every field's
   value and its formatted display value after the last step, plus which
   validation actions fired and what they reported. Note the Acrobat version and
   platform in the file, because this is the reference implementation and its
   behaviour is version-specific.
5. Cross-check each expectation on a second Acrobat version before trusting it.
   Where two versions disagree, record both and mark the field as
   version-dependent rather than picking a winner.

Step 4 is manual and is the reason this set does not exist yet. It is worth
scripting only if it turns out that Acrobat's JavaScript console can drive the
whole scenario, which would make replay repeatable; that is the first thing to
try.

## What the test does with it

For each PDF, load it, replay `<stem>.scenario.json` through Onionskin's forms
engine, and assert the resulting field values, formatted display values and
validation outcomes match `<stem>.expected.json` exactly. A field marked
version-dependent passes if it matches any recorded Acrobat version.
