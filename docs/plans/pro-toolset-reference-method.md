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
gate.** The reference product's paywall states are scaffolding around the
parts being replicated, not part of them.

From a locked panel we take its structure, its naming, its grouping, its
iconography in the grid-and-metaphor sense, and its layout. We explicitly do
not take the lock affordance or the purchase call to action. Every measurement
in this method's evidence stops at that line: where a captured frame contains
an upsell caption or a trial button, the evidence file records that the space
is accounted for and records nothing further about it, precisely so that a
later reader cannot mistake it for something to build.

This is worth stating first because the failure mode is subtle. A person
working from a faithful reference, in good faith, reproduces what the
reference shows. If the reference shows a rail with a pinned trial button
under a separator, a faithful implementation grows a pinned button under a
separator. The rule is that fidelity to the reference stops at the gate.

---

## 2. The method in one paragraph

Read the reference product's own interface with two instruments and a
fallback, then record only what the instrument used can actually establish.
The first instrument is the **accessibility tree** of the installed Reader,
queried through System Events: it yields exact control titles, their index
within their container, separators, submenu nesting, enabled state and
keyboard shortcuts, as text, from the pinned build, with no interpretation
step in between. The second is a **private screen capture** of a Reader window
driven to a named state, from which pane widths, insets, row pitches, control
sizes and element order are measured numerically and the numbers, not the
image, are committed. The fallback, for states the paywall genuinely closes,
is **public Adobe documentation prose**, which frequently names controls,
dialog fields and workflow step order in words. Every fact is written down
with the source that produced it, the date, the depicted version and how that
version was determined, what was read off it, and a confidence class; and the
confidence model in section 4 refuses to let a fact of one class be
established by a source of a weaker one. Captures are audited before they are
measured, because a click-and-capture run fails silently and produces a
directory of plausible filenames over one stuck frame.

---

## 3. Legal posture as it applies here

Nothing in this method changes `PLAN.md`'s Legal posture; it applies it.

- **Rule 1, spec-first clean room.** Everything here is black-box observation
  of a running product and reading of a published manual. No decompilation, no
  disassembly, no inspection of Acrobat's internals. The accessibility tree is
  the operating system's published interface to any application's controls; it
  is not Acrobat's internals.
- **Rule 2, trademarks and artwork.** Icons are redrawn in-house. Iconography
  in this method means grid, stroke weight, metaphor and optical size. Adobe's
  paths are never traced, sampled or vectorised. The evidence file records
  icon metaphors as English sentences ("a shield", "a ruler") and icon optical
  size in points, and records nothing else about them.
- **Rule 3, public-language discipline.** This document and the evidence file
  are tracked and therefore public. Neither says "clone", "identical to
  Acrobat" or "drop-in replacement", and neither may be edited to.
- **Rule 4, reference images stay private.** Both instruments produce private
  output. Screen captures go under `parity/reference/`, which
  `parity/.gitignore` covers with a leading-slash `/reference/` entry,
  matching the directory at the root of the `parity/` tree, which is where
  captures go. Accessibility dumps are equally private in raw form and go
  under the same ignored tree; only the distilled tables reach the repository.
  Verified before this document was committed: `parity/.gitignore` ignores
  `/reference/`, `/onionskin/`, `/comparison/`, `/tmp/` and `/manifests/`, and
  the tooling this method adds lives at `parity/tools/`, which is deliberately
  outside those ignores because the tools contain no Adobe content.

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
| **B** | Private screen capture of the pinned build at known scale, uncropped, unannotated, on this machine | Real pixels of the real version at a known device pixel ratio. Geometry is measurable to about a pixel, as ink extent. Nothing is composited or retouched. | Needs an unlocked session and an awake display. Only reaches states someone drove the product into. Text must be read by eye or by OCR, which can misread. Ink extents are not control boxes. |
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
| **F3** | Grouping and hierarchy | the Edit menu's Pro entries sit in two separator-bounded groups, three items then five |
| **F4** | Ordering | `Microsoft Word`, `Microsoft PowerPoint`, `Microsoft Excel`, `Image format`, `Other format`, in that sequence |
| **F5** | Default and enabled state | `Microsoft Word` is the selected radio when the panel opens; the recipients dialog's `Cancel` is rendered disabled |
| **F6** | Dialog field list | the recipients dialog has exactly one text field, with that placeholder |
| **F7** | Workflow step order | `Create a PDF` opens a task view before any file chooser |
| **F8** | Iconography, as metaphor and optical size | a ruler, drawn at 15 to 16 pt on a 40 pt row |
| **F9** | Relative geometry | radio row pitch 44.0 pt; label left inset 59.5 pt |
| **F10** | Absolute geometry | the panel's origin on screen |
| **F11** | Colour and type | the exact fill of a selected row; the type size of a label |
| **F12** | Motion | how long the panel takes to open |

### 4.3 The permission matrix

Read as: may a source of this class establish a fact of this class, on its
own?

| | F1 presence | F2 naming | F3 grouping | F4 order | F5 default/enabled | F6 fields | F7 steps | F8 icon | F9 rel. geom | F10 abs. geom | F11 colour/type | F12 motion |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **A** accessibility | yes | yes | yes | for menus | yes | yes | no | no | no | no | no | no |
| **B** private capture | yes | yes | yes | yes | yes | yes | with a frame per step | yes | yes | no | yes | no |
| **C** doc prose | yes | yes | yes | if the prose asserts it | if the prose asserts it | yes | yes | no | no | no | no | no |
| **D** search summary | yes | only verbatim | verbatim path only | no | no | partial | partial | no | no | no | no | no |
| **E** third party | as a lead only | no | no | no | no | no | as a lead only | no | no | no | no | no |
| **X** doc images | no | no | no | no | no | no | no | no | no | no | no | no |

Five entries in that matrix are the ones that do real work.

**F10 is refused to every source.** Absolute screen geometry is not
establishable from anything, including a private capture, because the window's
origin depends on which display it landed on. This is the same constraint
`docs/plans/parity-goal.md` section 4.3 already imposes on Onionskin's own
side, and it has to hold on the reference side too or the two ledgers are not
comparable. Everything in the geometry ledger is a size, an inset, a gap or a
pitch.

**F12 is refused to every source.** Nothing in this method captures timing.
`docs/plans/parity-goal.md` candor item 3 already says motion is the weakest
layer; this method does not improve it and should not be read as if it did.

**Class A's F4 is menus only.** A menu's accessibility index is its visual
order. A window tree's child order is the application's, and the two diverge
often enough that ordering a panel's controls from a tree dump is not
admissible.

**F9 and F11 are refused to documentation.** This is where the brief's
intuition holds and is worth keeping: pixel geometry is not establishable from
documentation, because documentation images are cropped, scaled, annotated and
often older, and documentation prose almost never states a number. Naming,
grouping, ordering, hierarchy, presence, default states, dialog field lists
and workflow step order **are** establishable from documentation prose, and
often better than from a capture, because a capture shows one state and the
prose enumerates the set.

**F3, F4 and F5 are refused to class D, with one narrow exception.** A search
summary reorders and paraphrases. It can prove a label exists, if the label
appears verbatim in the returned text; it cannot prove that label sits third
in a list, because the ordering in a summary is the engine's. This is the one
place the model is stricter than it first looks, and it is deliberate: class D
is the only class that reaches Adobe documentation from this machine at all
(section 6's conditions, and section 8 item 3), so the temptation to lean on
it is real.

The exception, added during review of the proof run because the run kept
producing facts the matrix as first written would have thrown away: **class D
may establish F3 when the grouping is contained inside a single verbatim
quoted string, and never when it is read off the order in which the summary
lists things.** `Organize pages > Page labels`, `File > Create > PDF from
clipboard` and "select `Scanner` from the left rail" are each one quoted
assertion of placement, which the engine transported rather than constructed.
A list of five labels in a summary paragraph is the engine's arrangement and
establishes nothing about grouping. The distinction is testable by a reader:
is the containment inside the quotation marks or between them.

Two amendments to the brief's stated intuition, both from the proof run:

1. **Iconography is not one axis.** Metaphor is establishable from a capture
   and from nothing else; a documentation page saying "the Measure tool" does
   not tell you it is drawn as a ruler. Optical size and grid are
   establishable from a capture and are numbers. Stroke weight is *not*
   established by any capture this method has produced, because a downscaled
   or antialiased 15 pt glyph does not yield a reliable stroke width. The
   evidence file therefore records metaphor and optical size and leaves weight
   open, rather than reporting a plausible number.
2. **Class A is stronger than the brief assumed and class B weaker.** The
   accessibility tree establishes naming, ordering, grouping, enabled state
   and shortcuts at higher confidence than any screenshot, because there is no
   transcription step, and it survives a sleeping display. But it needs an
   unlocked session for anything except the menu bar, and it establishes
   nothing about geometry. The two instruments are complements, not a primary
   and a backup.

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
pages do not state a version. A provenance record that silently omits the
field reads as if the version were current.

### 4.5 Two checks that make a capture admissible

Both came out of the proof run, and both cost nothing.

1. **A capture is invalid if it is byte-identical to any other capture in its
   set.** Equality means the click did not land or the interface did not
   change; either way at most one of the two filenames can be true. The check
   is set-wide rather than adjacent because a modal that swallows clicks
   produces a run of identical frames, not a pair.
2. **A capture is invalid unless something inside the frame independently
   identifies the state.** A panel header, a highlighted rail row, a dialog
   title. A filename is a claim made by the capture script and is not
   evidence.

`parity/tools/capture-audit.py` implements the first and exits non-zero when a
capture set fails it. The second is a human read, once per frame, before any
measurement.

---

## 5. Extending the B7 ledger

B7 built the right structure and this method adds three things to it rather
than starting a second one.

- **A second instrument.** `docs/evidence/parity-reference.md` records
  screenshot pairs. `docs/evidence/pro-surface-reader.md` records
  accessibility queries alongside them, with the same discipline: what was
  run, when, against which build, what it returned.
- **A third column on every fact: the confidence class.** B7's ledger records
  provenance for the file. This method records provenance for the fact.
- **A separate file, not a rewrite.** B7's ledger tracks ten required M2
  states and their pass or fail against Onionskin captures. Pro-surface work
  has no Onionskin counterpart to compare against yet, so its rows would be
  `pending` in every Onionskin column and would make the M2 ledger's
  incompleteness look worse than it is. The two files cross-reference and the
  acceptance rules in `docs/plans/parity-goal.md` section 4.4 apply unchanged
  to both.

Tooling added, all under `parity/tools/`, none of it containing Adobe content:

| Tool | Does |
|---|---|
| `ax-menus.js` | dumps a running app's whole menu tree: title, index, separators, submenu nesting, enabled state, mark character, command key and modifiers |
| `ax-render-menus.py` | renders that dump as an indented tree for reading |
| `ax-tree.js` | dumps a running app's window, panel, sheet and dialog trees with role, subrole, title, description, help, value, enabled, focused, selected, position and size. **Untested against a live window**: the only session available to this pass reported zero windows, so the tool returned an empty array correctly and its output shape is unverified |
| `capture-audit.py` | groups a capture directory by content hash and fails when filenames outnumber distinct frames |
| `measure-pane.py` | finds a pane's edge and its ink bands, and profiles the columns of one band, in logical points |
| `coverage-tally.py` | tallies the per-row coverage verdicts and checks the row count against the scoreboard |

---

## 6. Proof: ten Pro features run end to end

Executed 2026-09-09. Results in full in `docs/evidence/pro-surface-reader.md`;
this section is the verdict per sample and, where it failed, why.

Conditions, stated because they bound everything below: the macOS session was
**locked** with both displays asleep. That made class B unavailable live, so
the class B results come from the thirteen distinct frames already sitting in
`parity/reference/.../pro-surface/` from 2026-09-07, audited before use. Class
A was available for the menu bar and unavailable for windows. Class C was
unavailable: `helpx.adobe.com` returns HTTP 403 to this environment from both
the fetch tool and `curl` with a browser user agent, and the fetch tool
refuses `web.archive.org` outright, so every documentation result below is
class D.

### 6.1 Whole toolset panel: the All tools rail. **Full success, class B.**

Twenty entries with exact labels and order; pane width 287.0 pt; entry pitch
40.03 pt over nineteen gaps; icon left inset 35 to 36 pt; icon optical width
15 to 16 pt; label left inset 65.0 pt; header inset 24.0 pt; close control at
256.0 pt. Icon metaphors recorded in words for all twenty. Also established
that the rail's **default is 13 entries plus a `View more` link**, from the
frame captured immediately after a restart, and that the 20-entry list is the
expanded state.

Not obtained: stroke weight (a 15 pt antialiased glyph does not yield a
reliable width), hover and focus treatment, the light theme, absolute
position.

### 6.2 Whole toolset panel: Export a PDF, which renders in full. **Full success, class B.**

The strongest result in the run: clicking `Export a PDF` switches the tab
strip to `Convert` and renders the entire Pro panel, live, with no upsell
modal. Fourteen elements in order, five radio options with their format tags
and which two carry chevrons, the selected default, the language dropdown, the
primary button, two section labels and three navigation entries. Radio row
pitch **44.0 pt exactly** over five rows; `Convert` button 79.5 x 32.0 pt;
content column 26.0 to 254.5 pt inside a 287.0 pt pane.

The wall is **inferred** to fall on apply: no upsell appears anywhere up to
the `Convert` button, and no frame captured what the button does. That is an
absence-of-upsell argument, not an observation of the wall.

### 6.3 Dense dialog: the Request e-signatures recipients dialog. **Full success, class B.**

A banner, a two-column body, three icon-and-text rows, an external link, a
labelled single-field form with its placeholder, and a two-button footer in
which `Cancel` is rendered **disabled**. Recovered in full, including the
disabled state, which is an F5 fact no documentation page would have given.

This sample also produced the run's third gate shape. The gate here is a
**quota**, not a block: a banner states a free-tier allowance and the dialog
below it is live. The banner's wording is the purchase call to action, which
section 1 excludes, so the shape is recorded and the copy is not. A method
that assumed two states, gated and ungated, would have recorded this one
wrong.

### 6.4 Multi-step flow: Create a PDF. **Partial, and it produced the run's most important caveat.**

Step one renders: a new document tab, a task bar reading `Create a PDF` with a
`Close` button, the empty-state line `Create PDFs from images, Microsoft
Office files, and more`, and a centred `Select Files` button. Steps two and
after: not established, because no frame captured them.

The caveat. Adobe's documentation, reached at class D, quotes "select
`Scanner` from the left rail" and "`Blank page` on the left rail" for this
surface, and `File > Create > PDF from clipboard` for its clipboard path.
Those are verbatim placement strings and so are admissible for F3 under the
exception in section 4.3. Reader's rendering of the same surface offers
**only** `Select Files`. Two readings fit: Reader renders a reduced version of
the Pro surface, or the documentation describes a different release. This pass
cannot separate them, and either way the same rule follows:

> **A rendered Reader surface is evidence about Reader's rendering of a Pro
> surface, not about the Pro surface.** Where documentation names controls the
> rendered surface does not show, the difference is a finding, not a
> contradiction to be resolved in favour of the capture.

This matters most where a capture would otherwise look authoritative. The
Convert panel of 6.2 may equally be a reduced Convert panel; nothing in the
capture can tell. The difference is a finding to record, not a contradiction
to settle in favour of whichever source is at hand.

### 6.5 Context menu: Change Scale Ratio. **Failed at class A and B, recovered at class D, and the model refused half of it.**

The intended sample was a canvas context menu captured live. It failed: with
the session locked, `count of windows` on the Reader process returns 0, so no
window, panel or context-menu tree is readable, and screen capture returns the
login window. No frame in the 2026-09-07 set contains a context menu either.

Class D recovered this much: the control is named `Change Scale Ratio`, it is
reached by hovering the page and right-clicking (`Control`-click on macOS),
and it sets "the scaling ratio (such as 3:2) and unit of measurement".
Companion controls `Snap to Paths`, `Snap to Endpoints`, `Snap to Midpoints`
and `Snap to Intersections` are named, and the `Measurement Info panel` is
named with its contents "current measurement, delta values, and scale ratio".

What the model then refused: **the order of the context menu's items**, and
any grouping. Class D may not establish F3 or F4. So the sample yields
presence, naming and the invocation gesture, and the ordering stays open. The
gap is recorded as a gap rather than filled from a source that cannot fill it.

### 6.6 Dense dialog by documentation: the form field Properties dialog. **Partial, in a specific and instructive way.**

Recovered: tab names `General`, `Appearance`, `Options`, `Actions`, `Format`
and `Signed`, plus the `General` tab's field list with enumerated values,
which is the densest single result in the run: `Name`, `Tooltip`, `Form Field`
with the values `Visible`, `Hidden`, `Visible But Doesn't Print` and `Hidden
but printable`, `Orientation` at `0`, `90`, `180` or `270 Degrees`, and `Read
Only`. Plus the `Actions` tab's `Select Trigger` values `Mouse Up`, `Mouse
Down`, `Mouse Enter`, `Mouse Exit`, `On Focus` and `Blur`.

Not recovered: a `Validate` tab. `ACROBAT-PARITY.md` has a row naming `Format,
Validate, Calculate` as a group; `Format` and `Calculate` were confirmed and
`Validate` was not surfaced by any query. Also not recovered: the tab order.
The prose says "General and Actions appear for all field types", which orders
two of six and no more.

So class C and D genuinely deliver F6, the dialog field list, better than a
capture would, because the prose enumerates the whole option set where a
capture shows one state. And they deliver F4 barely at all.

### 6.7 The sample expected to source badly: Print Production. **Sourced badly, as expected, for a reason worth recording.**

Only one of Print Production's thirteen rows is in scope, and Adobe's own
pages for it that surfaced are `helpx.adobe.com/uk/acrobat/**11**/using/...`,
that is Acrobat XI, released 2012. The top non-Adobe result is titled
"Overview of the print production tools in Acrobat XI Pro". Tool names came
back (`Output Preview`, `Preflight`, `Convert Colors`, `Ink Manager`, `Add
Printer Marks`, `Trap Presets`, `Set Page Boxes`) and the one in-scope row,
`Set Page Boxes`, is confirmed with its dialog described as "Set Page Boxes
(Crop Tool)".

The provenance record for every one of those facts must read "depicted
version: Acrobat XI, 2012, determined from the URL path". Fourteen years and
at least two interface redesigns separate that from the pinned build. Under
the model those labels may establish F1 and F2 and nothing else, which for a
toolset that is 12/13 out of scope is enough and no more.

### 6.8 The second sample expected to source badly: Use a certificate. **Sourced badly, unexpectedly badly.**

Thirteen in-scope rows, of which **two** were sourced: `Certify (Visible
Signature)` and `Certify (Invisible Signature)`, plus `Digital ID` and
`Signatures Panel` as terms. `Digitally Sign`, `Time Stamp`, `Validate
Signature` and `Signature Properties` were all searched for and none appeared
as a label in returned text. The one thing the prose did give was an explicit
ordering constraint, "certification must be applied before other signatures",
which is an F7 workflow fact and admissible.

This is the worst-covered toolset on the board and the reason is not that
Adobe does not document signing. It is that this method's only route to
Adobe's prose is a search engine, and search engines return the pages people
link to. Digital signature reference material is not that.

### 6.9 Upsell at the click: Edit a PDF. **Class B refused it; class A and D partly recovered it.**

Clicking `Edit a PDF` opens the upsell modal immediately. No panel renders. So
the largest Pro toolset on the board, 18 in-scope rows, yields no geometry, no
grouping, no ordering and no iconography beyond its own rail icon.

What was recovered: the rail entry's exact label and position, its tooltip
`Modify or add text, images, pages, and more` (an F2 fact, captured), and
three class A menu entries, `Edit a PDF`, `Add Text` and `Add Image`, at Edit
menu indices 5, 6 and 7, all enabled, in one separator-bounded group. Two of
the eighteen rows, `Add text` and `Add image`, are those entries and so stand
at class A; class D named the controls of eight more.

The upsell modal itself is a fixed template that varies only in eyebrow,
headline, bullet list and illustration. Its bullet lists are Adobe's own
capability enumeration and are useful for cross-checking the parity board's
rows; they carry no layout information Onionskin would use, and its buttons
are the purchase call to action, which section 1 excludes.

### 6.10 Mixed: Organize pages. **Panel refused, two rows recovered at class A.**

`Organize pages` also opens the upsell modal at the click, so the Organize
grid is not established at all, which matters because two of its rows (`Page
thumbnail zoom and multi-select in the Organize grid`, `Copy or move pages
between open documents`) are about that grid specifically and are the two rows
in the toolset that class D also missed.

But `Delete Pages` and `Rotate Pages` are live entries in Reader's Edit menu
at indices 9 and 10, first and second in the five-item group that runs from
index 9 to index 13, and that is a class A fact about the commands themselves.
Two of eleven rows recovered at the strongest class available; seven more at
class D; two lost.

### 6.11 Sample scoreboard

| # | Sample | Kind | Best class reached | Verdict |
|---|---|---|---|---|
| 6.1 | All tools rail | whole toolset panel | B | full success |
| 6.2 | Export a PDF panel | whole toolset panel | B | full success |
| 6.3 | Request e-signatures dialog | dense dialog | B | full success |
| 6.4 | Create a PDF | multi-step flow | B | step one only; reduced surface or older documentation, not separable |
| 6.5 | Change Scale Ratio | context menu | D | naming yes, ordering refused |
| 6.6 | Form field Properties | dense dialog | D | field list yes, tab order no, one tab unconfirmed |
| 6.7 | Print Production | expected to source badly | D (legacy) | as expected; F1 and F2 only |
| 6.8 | Use a certificate | expected to source badly | D | worse than expected; 2 of 13 rows |
| 6.9 | Edit a PDF | upsell at the click | A and D | no panel; 10 of 18 rows named |
| 6.10 | Organize pages | upsell at the click | A and D | 2 rows at class A, 7 at D, 2 lost |

Three of ten reached class B in full, one reached it partly, two reached class
A only, and four never got past documentation. **No sample reached class C**,
because Adobe's help server refuses this environment, which was not a foreseen
failure and is the single largest constraint the run found.

---

## 7. Three-way coverage across the Pro-only rows

### 7.1 The denominator, defined so it can be re-derived

The Pro-only denominator is **the in-scope rows of the sixteen Pro-gated
toolset sections** of `ACROBAT-PARITY.md`: Edit a PDF, Create a PDF, Combine
files, Organize pages, Compress a PDF, Export a PDF, Prepare a form, Redact a
PDF, Protect a PDF, Use a certificate, Scan & OCR, Measure objects, Prepare
for accessibility, Use print production, Use guided actions and Compare files.

That is **145 rows**: 181 rows in those sections, of which 36 are
out-of-scope. Add comments, Add stamps and Fill & Sign are excluded, so their
38 in-scope rows are not part of this problem. That exclusion is **measured
for Fill & Sign** (the E-Sign panel's `FILL AND SIGN YOURSELF` section renders
live, with `Add signature`, `Add initials` and a six-control strip, and no
gate) and **asserted, not measured, for Add comments (25 rows) and Add stamps
(7 rows)**, from their presence in the rail and the product's free-tier
positioning. If both turn out to be gated the denominator rises to 177 and
every percentage below falls by about a fifth. Re-derive with
`parity/tools/coverage-tally.py`, which compares the two files' row counts.

### 7.2 The answer

Verdicts are one per row in `docs/evidence/pro-surface-coverage.tsv`, each
with the shortest evidence a reader can re-check. The reachable bucket splits
in two, because the split turns out to matter more than the bucket:

| Bucket | Rows | Share |
|---|---|---|
| **Rb** reachable and captured, source class B | **7** | **4.8%** |
| **Ra** reachable by accessibility query, source class A | **7** | **4.8%** |
| **D** documentation-sourceable only | **74** | **51.0%** |
| **N** neither, in this pass | **57** | **39.3%** |

Reachable in the installed Reader at all: **14 of 145, 9.7%**. Sourced at all:
**88 of 145, 60.7%**.

Per toolset:

| Toolset | Rb | Ra | D | N | rows |
|---|---|---|---|---|---|
| Export a PDF | 5 | 1 | 3 | 2 | 11 |
| Create a PDF | 2 | 0 | 3 | 0 | 5 |
| Edit a PDF | 0 | 2 | 8 | 8 | 18 |
| Organize pages | 0 | 2 | 7 | 2 | 11 |
| Prepare for accessibility | 0 | 2 | 6 | 4 | 12 |
| Compress a PDF | 0 | 0 | 4 | 0 | 4 |
| Measure objects | 0 | 0 | 6 | 1 | 7 |
| Combine files | 0 | 0 | 4 | 1 | 5 |
| Use print production | 0 | 0 | 1 | 0 | 1 |
| Compare files | 0 | 0 | 4 | 2 | 6 |
| Redact a PDF | 0 | 0 | 5 | 3 | 8 |
| Scan & OCR | 0 | 0 | 5 | 3 | 8 |
| Use guided actions | 0 | 0 | 3 | 4 | 7 |
| Prepare a form | 0 | 0 | 10 | 8 | 18 |
| Protect a PDF | 0 | 0 | 3 | 8 | 11 |
| Use a certificate | 0 | 0 | 2 | 11 | 13 |

Two toolsets of sixteen have any captured surface at all, and one of them,
Create a PDF, has only its first step.

### 7.3 What the 60.7% and the 4.8% buy, which is not the same thing

`docs/plans/parity-goal.md` builds look-and-feel acceptance out of three
layers: sourced design tokens, a relative geometry ledger, and a behavioural
script. Layers 1 and 2 need **F8, F9 and F11**: iconography, relative
geometry, colour and type. Section 4.3 refuses all three to documentation, and
refuses them to class A as well: an accessibility query returns no pixels.

So the number that governs the look-and-feel measure is neither 60.7% nor
9.7%. It is **Rb alone: 7 of 145 rows, 4.8%**. That is the whole of the Pro
surface from which a spacing token, a row pitch, an icon grid or a colour
could be taken today.

The other three buckets buy something real and different. The 7 Ra rows give a
command's exact label, its index, its group and its shortcut where one exists,
which is what the keyboard-parity and menu-parity claims rest on. Six of the
seven carry no shortcut, and that absence is itself a class A fact. The 74 D
rows give naming, field lists and workflow step order, which is what the
**functional** board counts and what stops Onionskin shipping a feature with
the wrong vocabulary.

Restated as the three numbers that matter:

| Question | Number |
|---|---|
| Pro rows whose vocabulary or workflow can be sourced today | 88 of 145, **60.7%** |
| Pro rows the installed Reader exposes at all | 14 of 145, **9.7%** |
| Pro rows whose geometry, iconography and colour can be measured today | 7 of 145, **4.8%** |

### 7.4 How much of the 57 is genuinely unreachable

N is a verdict about this pass, and saying otherwise would be the encouraging
answer rather than the true one. Two things follow.

First, **N cannot separate "Adobe does not document this" from "this pass did
not reach the page"**, because class C was unavailable throughout. A universal
negative is not available from a failed search. Exactly one of the 57 is an
absence this pass can assert without one: `Keep the structure tree valid
through every edit` names no Acrobat control because it is an obligation
Onionskin's incremental-save invariant creates, and 7.5 proposes it as a board
edit for that reason. Two more look like absences and are not established as
any: `Reflowing text edit` and `Export pages to SVG` both failed on the same
route as the rest of N, and 7.5 asks for the SVG one to be checked rather than
acted on. Everything else in the 57, including `Preserve signature validity
across edits`, is a row this pass failed to source, not a row with nothing
behind it.

Second, and despite that, the shape of the fix is knowable. Every one of the
57 is a row on the board, and the route that failed for all of them is the
same route: a search engine standing in for a documentation server that
refuses this environment. **The 39.3% is a measurement of this pass's access,
not of Adobe's documentation.** Restoring class C should move most of it, and
a reader comparing two passes should expect that and should not read it as
progress.

The 4.8% will not move the same way. It is bounded by what Reader renders, and
section 6 established that two of the sixteen toolsets in the denominator
render anything past the click.

### 7.5 Findings for the parity board, recorded not fixed

`ACROBAT-PARITY.md` is outside this document's scope. Four proposed row edits
fell out of the coverage pass.

| # | Row | Finding |
|---|---|---|
| 1 | `Export pages to SVG`, `implemented`, M2 | No Acrobat SVG export surfaced from any source in this pass. If Acrobat has none, the row sits in a section whose denominator is Acrobat's surface and inflates it with an Onionskin-only capability, which is the objection the file already applies to the skins pane. Needs one check by someone who can reach the documentation. |
| 2 | `Keep the structure tree valid through every edit`, `Prepare for accessibility` | An Onionskin obligation, not an Acrobat control. Same objection. |
| 3 | `Field properties: Format, Validate, Calculate` | `Format` and `Calculate` are confirmed Acrobat tab names; `Validate` did not surface in any query. Either the tab exists and this pass missed it, or the row groups a tab Acrobat does not have. Worth one check by someone who can reach the documentation. |
| 4 | `Set alternate text for figures` | Adobe prose gives `Add alternate text`. Whether that supersedes `Set Alternate Text` or is a different page's wording is not establishable at class D, which may confirm a label verbatim and may not confirm that it is the current one. An open question for a class C pass. |

Two further observations, not row edits:

- The board's `"View more" / expand full tool list` row, marked `implemented`,
  is matched by a real Reader behaviour this pass measured: the rail's default
  is 13 entries plus `View more`, expanding to 20. The row is correct and now
  has an evidence number behind it.
- `docs/plans/parity-goal.md` candor item 1 says the Pro surface is
  "substantially reachable" from the installed build and that "most of that
  surface is capturable". This pass measures 9.7% reachable and 4.8%
  measurable. Item 1 is right that no licence is needed and right that the
  rail and menus expose the toolsets; it is wrong about how far past the click
  the build goes. Section 8 states the correction.

---

## 8. What this method cannot do

In the style of the M2, M3 and parity-goal candor lists. Each item is
something a reader would otherwise reasonably assume.

1. **It does not reach most of the Pro surface, and the goal document
   currently overstates this.** `docs/plans/parity-goal.md` candor item 1 says
   most of the Pro surface is capturable from the installed Reader. Measured:
   **9.7% reachable, 4.8% measurable**. Of the sixteen toolsets in the
   denominator, two render anything past the click (Export a PDF in full,
   Create a PDF's first step) and three were observed to refuse at the click
   (Edit a PDF, Organize pages, Combine files); eleven are unestablished. A
   third toolset renders in full, Request e-signatures, and it is not in the
   denominator because its rows sit under Fill & Sign and the sharing
   sections. Item 1's underlying claim, that this needs no purchase, holds and
   is not weakened; its estimate of reach does not. Proposed correction, for
   whoever next edits that file: "the rail and every menu entry are
   capturable, and a minority of toolsets render a panel or dialog before the
   upsell; of the toolsets this pass reached, most refuse at the click, and
   most were not reached at all."
2. **What Reader renders may be a reduced version of what Pro renders.**
   Observed once, on `Create a PDF`, where the rendered task view offers less
   than Adobe's description of the same surface. The comparison does not
   settle it, because the documentation side is class D of unknown version and
   the difference could be a release difference instead. Either way every
   class B fact carries the uncertainty and no amount of capture removes it.
3. **Adobe's documentation server refuses this environment.**
   `helpx.adobe.com` returns HTTP 403 to the fetch tool and to `curl` with a
   browser user agent. The fetch tool refuses `web.archive.org` outright, and
   `curl` to the Wayback availability API returned HTTP 429, which is a rate
   limit rather than a refusal, so an archive route may still exist and was
   not established either way. So class C, the class the confidence model is
   built around for everything behind the wall, was unavailable for the whole
   run, and all 74 D verdicts rest on class D, a search engine's summary.
   Under the matrix in section 4.3 that means those 74 rows have naming and
   presence, have grouping only where a verbatim quoted path carries it, and
   **do not** have confirmed ordering. Restoring class C is the highest-value
   single fix available to this method and it is an access problem, not a
   research one.
4. **A locked session removes both live instruments except the menu bar.**
   Screen capture returns the login window; the Reader process reports zero
   windows. Menu enumeration survives. So the unattended fraction of this work
   is the menu bar and nothing else, and everything else inherits the manual
   step `docs/plans/parity-goal.md` candor item 2 already named.
5. **Stroke weight is not measurable from these captures.** Icon metaphor and
   optical size are. A 15 pt antialiased glyph does not yield a reliable
   stroke width, so the icon axis is weaker than `docs/plans/parity-goal.md`
   section 4.3's icon-token rules assume: they ask for grid and weight, and
   this method delivers metaphor and optical size. The gap should be closed by
   a deliberate capture at a larger optical size, not by reading a number off
   a small glyph.
6. **Nothing here measures motion, colour tokens or type.** No timing source
   exists. Colour and type are class B facts and no light-theme frame exists
   at all, so the entire colour axis of the Pro surface is unmeasured even for
   the toolsets that render.
7. **The 57 N rows will move, and the 4.8% will not.** Section 7.4. A reader
   comparing two passes should expect the D column to grow substantially and
   the Rb column to grow barely, and should not read the first as progress on
   look and feel.
8. **The per-row verdicts are a judgement over a control-level probe, not a
   row-level fetch.** Each toolset was probed with one or two queries against
   a list of about a dozen expected controls. A row was marked D when the
   probe confirmed its principal control. A row whose control was never
   queried is N. That is why 7.4 exists, and it is why the verdict file
   records the evidence string per row rather than only the letter.
9. **This method produces reference material, not acceptance.** Nothing here
   accepts a surface. `docs/plans/parity-goal.md` section 4.4 rules govern
   acceptance unchanged, including rule 2, which blocks acceptance on a
   `pending` reference. Every Pro surface remains `open` and most of them
   remain `blocked` on a capture that does not exist.
