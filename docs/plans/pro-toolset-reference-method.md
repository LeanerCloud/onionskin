# Method: building the Pro-only toolset reference without a Pro licence

Status: method, confidence model and a proof run. No source changes.

Companions: `docs/plans/parity-goal.md` (candor list item 1, the position this
document implements), `docs/evidence/pro-surface-reader.md` (the results of
running this method), `docs/evidence/parity-reference.md` (the B7 private
evidence ledger this extends), `parity/README.md` (the B7 capture protocol),
`PLAN.md` (Legal posture), `ACROBAT-PARITY.md` (the row denominator).

---

## 1. What Onionskin builds, stated before anything else

**Onionskin implements these features fully functional, with no upsell and no
gate.** The reference product's paywall states are scaffolding around the parts
being replicated, not part of them.

From a locked panel we take its structure, its naming, its grouping, its
iconography in the grid-and-metaphor sense, and its layout. We explicitly do not
take the lock affordance or the purchase call to action. Every measurement in
this method's evidence stops at that line: where a captured frame contains an
upsell caption or a trial button, the evidence file records that the space is
accounted for and records nothing further about it, precisely so that a later
reader cannot mistake it for something to build.

This is worth stating first because the failure mode is subtle. A person working
from a faithful reference, in good faith, reproduces what the reference shows.
If the reference shows a rail with a pinned trial button under a separator, a
faithful implementation grows a pinned button under a separator. The rule is
that fidelity to the reference stops at the gate.

---

## 2. The method in one paragraph

Read the reference product's own interface with two instruments and a fallback,
then record only what the instrument used can actually establish. The first
instrument is the **accessibility tree** of the installed Reader, queried through
System Events: it yields exact control titles, their index within their
container, separators, submenu nesting, enabled state and keyboard shortcuts, as
text, from the pinned build, with no interpretation step in between. The second
is a **private screen capture** of a Reader window driven to a named state, from
which pane widths, insets, row pitches, control sizes and element order are
measured numerically and the numbers, not the image, are committed. The
fallback, for states the paywall genuinely closes, is **public Adobe
documentation prose**, which frequently names controls, dialog fields and
workflow step order in words. Every fact is written down with the source that
produced it, the date, the depicted version and how that version was determined,
what was read off it, and a confidence class; and the confidence model in
section 4 refuses to let a fact of one class be established by a source of a
weaker one. Captures are audited before they are measured, because a
click-and-capture run fails silently and produces a directory of plausible
filenames over one stuck frame.

---

## 3. Legal posture as it applies here

Nothing in this method changes `PLAN.md`'s Legal posture; it applies it.

- **Rule 1, spec-first clean room.** Everything here is black-box observation of
  a running product and reading of a published manual. No decompilation, no
  disassembly, no inspection of Acrobat's internals. The accessibility tree is
  the operating system's published interface to any application's controls; it
  is not Acrobat's internals.
- **Rule 2, trademarks and artwork.** Icons are redrawn in-house. Iconography in
  this method means grid, stroke weight, metaphor and optical size. Adobe's paths
  are never traced, sampled or vectorised. The evidence file records icon
  metaphors as English sentences ("a shield", "a ruler") and icon optical size in
  points, and records nothing else about them.
- **Rule 3, public-language discipline.** This document and the evidence file are
  tracked and therefore public. Neither says "clone", "identical to Acrobat" or
  "drop-in replacement", and neither may be edited to.
- **Rule 4, reference images stay private.** Both instruments produce private
  output. Screen captures go under `parity/reference/`, which
  `parity/.gitignore` covers with a leading-slash `/reference/` entry, matching
  the directory at the root of the `parity/` tree, which is where captures go.
  Accessibility dumps are equally private in raw form and go under the same
  ignored tree; only the distilled tables reach the repository. Verified before
  this document was committed: `parity/.gitignore` ignores `/reference/`,
  `/onionskin/`, `/comparison/`, `/tmp/` and `/manifests/`, and the tooling this
  method adds lives at `parity/tools/`, which is deliberately outside those
  ignores because the tools contain no Adobe content.

One addition to the B7 protocol, not a change: **a raw accessibility dump is
treated exactly like a screenshot.** It is a complete extraction of another
product's interface, so it stays local, and the repository holds the table
distilled from it. Individual control names in a distilled table are the same
kind of fact `ACROBAT-PARITY.md` has published on 403 rows since the file
existed.

---

## 4. The confidence model

The model has three parts: what a source can establish, what kind of fact is
being claimed, and a matrix that refuses the combinations that do not hold. It
exists because the tempting failure is not fabrication, it is a true-sounding
number read off a source that could not have produced it.

### 4.1 Source classes

| Class | Source | What makes it strong | What makes it weak |
|---|---|---|---|
| **A** | Accessibility query against the running pinned build | The build reports its own controls as text. Version is the installed version, known exactly. No scaling, cropping or transcription step. | Only reaches surfaces the build renders. Says nothing about pixels, colour, spacing or artwork. Reports the tree, which is not always the visual order. |
| **B** | Private screen capture of the pinned build at known scale, uncropped, unannotated, on this machine | Real pixels of the real version at a known device pixel ratio. Geometry is measurable to the pixel. Nothing is composited or retouched. | Needs an unlocked session and an awake display. Only reaches states someone drove the product into. Text must be read by eye or by OCR, which can misread. Ink extents are not control boxes. |
| **C** | Adobe's published documentation prose, fetched and read | Adobe's own words for its own controls. Frequently gives dialog field lists and workflow step order explicitly. Independent of any image. | Version usually unstated. Adobe reshapes the interface continuously, and `PLAN.md`'s risk list says so. Pages routinely mix the current and classic interfaces. Prose orders sentences, which is not always the control order. |
| **D** | Adobe documentation reached only through a search engine's summary of the page | Carries real Adobe sentences the engine indexed. Often the only reachable form of C. | Which snapshot of which page produced the sentence is unknown, so version provenance is gone. The summary is a paraphrase unless the label appears verbatim. Ordering is the engine's, not Adobe's. |
| **E** | Third-party tutorials, forum posts, competitor comparison pages | Sometimes the only mention of an obscure control. | Version almost never stated, frequently a decade old, frequently wrong, frequently reproducing each other. |
| **X** | Documentation screenshots and marketing images | none for this purpose | Cropped, scaled, annotated, composited, and often an older release. Also Adobe's copyrighted artwork. **Never a source for anything.** |

Class X is listed so that it is explicitly excluded rather than silently
unused.

### 4.2 Fact classes

| Class | Fact | Example |
|---|---|---|
| **F1** | Presence or absence | `Redact a PDF` is an entry in the rail |
| **F2** | Naming | the entry reads exactly `Redact a PDF`, not `Redact PDF` |
| **F3** | Grouping and hierarchy | the eight Pro entries in the Edit menu sit in one block between two separators |
| **F4** | Ordering | `Microsoft Word`, `Microsoft PowerPoint`, `Microsoft Excel`, `Image format`, `Other format`, in that sequence |
| **F5** | Default state | `Microsoft Word` is the selected radio when the panel opens |
| **F6** | Dialog field list | the recipients dialog has exactly one text field, with that placeholder |
| **F7** | Workflow step order | `Create a PDF` opens a task view before any file chooser |
| **F8** | Iconography, as metaphor and optical size | a ruler, drawn at 15 to 16 pt on a 40 pt row |
| **F9** | Relative geometry | radio row pitch 44.0 pt; label left inset 59.5 pt |
| **F10** | Absolute geometry | the panel's origin on screen |
| **F11** | Colour and type | the exact fill of a selected row; the type size of a label |
| **F12** | Motion | how long the panel takes to open |

### 4.3 The permission matrix

Read as: may a source of this class establish a fact of this class, on its own?

| | F1 presence | F2 naming | F3 grouping | F4 order | F5 default | F6 fields | F7 steps | F8 icon | F9 rel. geom | F10 abs. geom | F11 colour/type | F12 motion |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **A** accessibility | yes | yes | yes | yes | yes | yes | no | no | no | no | no | no |
| **B** private capture | yes | yes | yes | yes | yes | yes | with a frame per step | yes | yes | no | yes | no |
| **C** doc prose | yes | yes | yes | if the prose asserts it | if the prose asserts it | yes | yes | no | no | no | no | no |
| **D** search summary | yes | only verbatim | no | no | no | partial | partial | no | no | no | no | no |
| **E** third party | as a lead only | no | no | no | no | no | as a lead only | no | no | no | no | no |
| **X** doc images | no | no | no | no | no | no | no | no | no | no | no | no |

Four entries in that matrix are the ones that do real work.

**F10 is refused to every source.** Absolute screen geometry is not establishable
from anything, including a private capture, because the window's origin depends
on which display it landed on. This is the same constraint
`docs/plans/parity-goal.md` section 4.3 already imposes on Onionskin's own side,
and it has to hold on the reference side too or the two ledgers are not
comparable. Everything in the geometry ledger is a size, an inset, a gap or a
pitch.

**F12 is refused to every source.** Nothing in this method captures timing.
`docs/plans/parity-goal.md` candor item 3 already says motion is the weakest
layer; this method does not improve it and should not be read as if it did.

**F9 and F11 are refused to documentation.** This is where the brief's intuition
holds and is worth keeping: pixel geometry is not establishable from
documentation, because documentation images are cropped, scaled, annotated and
often older, and documentation prose almost never states a number. Naming,
grouping, ordering, hierarchy, presence, default states, dialog field lists and
workflow step order **are** establishable from documentation prose, and often
better than from a capture, because a capture shows one state and the prose
enumerates the set.

**F3, F4 and F5 are refused to class D.** A search summary reorders and
paraphrases. It can prove a label exists, if the label appears verbatim in the
returned text; it cannot prove that label sits third in a list, because the
ordering in a summary is the engine's. This is the one place the model is
stricter than it first looks, and it is deliberate: class D is the only class
that reaches Adobe documentation from this machine at all (section 6.6), so the
temptation to lean on it is real.

Two amendments to the brief's stated intuition, both from the proof run:

1. **Iconography is not one axis.** Metaphor is establishable from a capture and
   from nothing else; a documentation page saying "the Measure tool" does not
   tell you it is drawn as a ruler. Optical size and grid are establishable from
   a capture and are numbers. Stroke weight is *not* established by any capture
   this method has produced, because a downscaled or antialiased 15 pt glyph does
   not yield a reliable stroke width. The evidence file therefore records
   metaphor and optical size and leaves weight open, rather than reporting a
   plausible number.
2. **Class A is stronger than the brief assumed and class B weaker.** The
   accessibility tree establishes naming, ordering, grouping, enabled state and
   shortcuts at higher confidence than any screenshot, because there is no
   transcription step, and it survives a sleeping display. But it needs an
   unlocked session for anything except the menu bar, and it establishes nothing
   about geometry. The two instruments are complements, not a primary and a
   backup.

### 4.4 The provenance record every fact carries

A fact with no provenance is not admitted, in the same way
`docs/plans/parity-goal.md` rule 4.4.2 refuses acceptance without a resolvable
evidence ID. Six fields:

| Field | Content | Failure mode it closes |
|---|---|---|
| Source class | one of A, B, C, D, E | stops a class D label being cited as if read off the running build |
| Source identity | the exact command, or the URL, or the private capture's SHA-256 | lets a stranger re-run or re-fetch it |
| Date accessed | ISO date and time | Adobe reshapes the interface; a fact without a date cannot be aged |
| Depicted version, and how determined | e.g. "25.001.20438, read from `CFBundleShortVersionString`", or "**could not determine**" | the single largest risk in documentation sourcing |
| What was read off it | the literal observation, before interpretation | separates the observation from the inference |
| Fact class and confidence | F-class, plus `established` or `inferred` | makes an inference visible as one |

**"Could not determine" is a legal value and must be written out.** Most Adobe
pages do not state a version. A provenance record that silently omits the field
reads as if the version were current.

### 4.5 Two checks that make a capture admissible

Both came out of the proof run, and both cost nothing.

1. **A capture is invalid if it is byte-identical to the capture before it.**
   Equality means the click did not land or the interface did not change; either
   way the file is not evidence of what its name says.
2. **A capture is invalid unless something inside the frame independently
   identifies the state.** A panel header, a highlighted rail row, a dialog
   title. A filename is a claim made by the capture script and is not evidence.

`parity/tools/capture-audit.py` implements the first and exits non-zero when a
capture set fails it. The second is a human read, once per frame, before any
measurement.

---

## 5. Extending the B7 ledger

B7 built the right structure and this method adds three things to it rather than
starting a second one.

- **A second instrument.** `docs/evidence/parity-reference.md` records
  screenshot pairs. `docs/evidence/pro-surface-reader.md` records accessibility
  queries alongside them, with the same discipline: what was run, when, against
  which build, what it returned.
- **A third column on every fact: the confidence class.** B7's ledger records
  provenance for the file. This method records provenance for the fact.
- **A separate file, not a rewrite.** B7's ledger tracks ten required M2 states
  and their pass or fail against Onionskin captures. Pro-surface work has no
  Onionskin counterpart to compare against yet, so its rows would be `pending`
  in every Onionskin column and would make the M2 ledger's incompleteness look
  worse than it is. The two files cross-reference and the acceptance rules in
  `docs/plans/parity-goal.md` section 4.4 apply unchanged to both.

Tooling added, all under `parity/tools/`, none of it containing Adobe content:

| Tool | Does |
|---|---|
| `ax-menus.js` | dumps a running app's whole menu tree: title, index, separators, submenu nesting, enabled state, mark character, command key and modifiers |
| `ax-render-menus.py` | renders that dump as an indented tree for reading |
| `ax-tree.js` | dumps a running app's window, panel, sheet and dialog trees with role, subrole, title, description, help, value, enabled, focused, selected, position and size |
| `capture-audit.py` | groups a capture directory by content hash and fails when filenames outnumber distinct frames |
| `measure-pane.py` | finds a pane's edge and its ink bands, and profiles the columns of one band, in logical points |
