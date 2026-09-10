# Pro-only surface evidence, captured from the installed Reader

Companion to `docs/plans/pro-toolset-reference-method.md`, which defines the
method and the confidence model this file records results under. Extends the B7
protocol in `parity/README.md` and the ledger in
`docs/evidence/parity-reference.md`; it does not replace either.

Reference build: Adobe Acrobat Reader **25.001.20438**, the version already
pinned by `docs/evidence/parity-reference.md`, read from the installed bundle's
`CFBundleShortVersionString` on 2026-09-09.

Privacy rule, unchanged: image bytes stay under the ignored `parity/` tree. This
file records control names, ordering, grouping, enabled state, keyboard
shortcuts and geometry. Those are measurements, not artwork.

---

## E1. Session capability probe, 2026-09-09T23:12Z

The two instruments do not fail together, and that is worth recording once
rather than rediscovering.

| Instrument | Result | Evidence |
|---|---|---|
| `screencapture -x` of the whole display, session locked, displays asleep | All-black frame. `screencapture -R` fails outright with `could not create image from rect`. | 8,416,800 of 8,416,800 luminance samples below 10 |
| `screencapture -x` after `caffeinate -u`, session still locked | Renders the login window, not the desktop. No application window is composited. | 8,416,592 of 8,416,800 samples above 10, all of them lock screen |
| System Events accessibility, menu bar of `AdobeReader` | Full menu tree readable: titles, index, separators, submenu nesting, enabled state, mark characters, command key and modifiers. | `E2` below, 209 rendered lines from a live query |
| System Events accessibility, windows of `AdobeReader` | `count of windows` returns 0 while the session is locked, so no window, panel, dialog or sheet tree is readable. A window nonetheless **exists**: Reader's own Window menu lists `two-page.pdf` at index 14. So the zero is the lock hiding the window, not the absence of one. | repeated at 23:07Z, 23:12Z and 23:41Z, all 0 |

Consequence for the method: **menu-surface facts are capturable from an
unattended session; window-surface facts are not.** The B7 ledger already
recorded the pixel half of this ("WindowServer rejects the window rectangle
against the inactive displays"). The new fact is that the accessibility half
survives display sleep but not session lock, and that the menu bar survives
both.

---

## E2. Pro-gated entries in the Reader menu bar

Captured live at 2026-09-09T23:04Z by
`osascript -l JavaScript parity/tools/ax-menus.js AdobeReader`, rendered by
`parity/tools/ax-render-menus.py`. Raw dump kept locally, uncommitted.

Confidence class **A** for every row: the running pinned build reported it about
itself. Index is the item's position in its own menu including separators, so it
is a statement about ordering and grouping, not a coordinate.

### File menu, whole menu, in order

| # | Title | Enabled | Shortcut | Pro-gated |
|---|---|---|---|---|
| 0 | Open... | yes | cmd+O | no |
| 1 | Open Recent Files | yes | | no |
| 2 | Create PDF | yes | | yes |
| 3 | Combine Files | yes | | yes |
| 4 | separator | | | |
| 5 | Save As... | yes | cmd+shift+S | no |
| 6 | Convert to Word, Excel or PowerPoint | yes | | yes |
| 7 | Save as Text... | yes | | no |
| 8 | separator | | | |
| 9 | Compress File | yes | | yes |
| 10 | Password Protect | yes | | yes |
| 11 | Request e-signatures | yes | | yes |
| 12 | Share File | yes | | no (cloud) |
| 13 | separator | | | |
| 14 | Print... | yes | cmd+P | no |
| 15 | Find | yes | cmd+F | no |
| 16 | Advanced Search | yes | cmd+shift+F | no |
| 17 | separator | | | |
| 18 | Document properties... | yes | cmd+D | no |
| 19 | separator | | | |
| 20 | Close File | yes | cmd+W | no |

### Edit menu, Pro-gated block

The Edit menu carries a contiguous eight-item Pro block between two separators,
at indices 5 to 13. Every one reports `AXEnabled = true`, which is the mechanical
form of the claim in `docs/plans/parity-goal.md` candor item 1: the entry is
live, and the paywall is somewhere past the click.

Document state at query time, since an enabled flag means little without it: a
multi-page document is open at page 1. Reader's Window menu lists
`two-page.pdf`; `View > Page Navigation` reports `Previous page` and
`First page` disabled with `Next page` and `Last page` enabled; and
`Edit > Cut`, `Copy`, `Paste`, `Undo`, `Redo` and `Select all` all report
disabled, so nothing is selected.

| # | Title | Enabled | Toolset it opens |
|---|---|---|---|
| 5 | Edit a PDF | yes | Edit a PDF |
| 6 | Add Text | yes | Edit a PDF |
| 7 | Add Image | yes | Edit a PDF |
| 8 | separator | | |
| 9 | Delete Pages | yes | Organize pages |
| 10 | Rotate Pages | yes | Organize pages |
| 11 | Redact a PDF | yes | Redact |
| 12 | Scan and OCR | yes | Scan and OCR |
| 13 | Prepare Form | yes | Prepare a form |

Two further Edit-menu submenus are relevant and are **not** Pro-gated in the
same way, because their children report disabled rather than live:

- `Edit > Protection` (index 17): `Security properties` live; `Revoke document`,
  `View audit history`, `Synchronize for offline` and `Manage document security
  account` all report `AXEnabled = false`. These are account-tethered, which is
  a different gate from the purchase gate and produces a different observable.
- `Edit > Check Spelling` (index 15): three live children, no gate.

### View menu, for contrast

The View menu contains no Pro-gated entries. Its whole content is Reader
functionality, which is why M2 could be measured from this build without any of
this work.

---

## E3. Audit of the existing private capture set

`parity/reference/acrobat-reader-25.001.20438/pro-surface/` holds 26 PNG files
dated 2026-09-07, produced by an earlier click-and-capture run on this machine.
They were never measured and no committed artefact referenced them. Auditing
them before using them turned out to matter.

**26 files, 13 distinct frames.** Grouped by SHA-256 of the file bytes
(`parity/tools/capture-audit.py`):

| Frame | Files carrying it | What it actually shows |
|---|---|---|
| `650b2b0da68bc1ca` | 7 (`09-redact`, `12-fill-and-sign`, `13-add-comments`, `16-use-a-certificate`, `17-use-print-production`, `18-measure-objects`, `99-current-state`) | one E-Sign panel plus its recipients dialog |
| `0f3d9662c96d2c99` | 6 (`07-scan-and-ocr`, `08-protect-a-pdf`, `10-compress-a-pdf`, `11-prepare-a-form`, `14-convert-to-pdf`, `15-add-a-stamp`) | the same E-Sign state, one caret-blink earlier |
| `8d5d97144067677f` | 2 (`00-baseline`, `01-edit-a-pdf` at 15:50:05) | the All tools rail, no tool invoked |
| `ff9627ad8b5bc274` | 2 (`30-test-w1`, `30-test-w2`) | rail in its truncated 13-entry state |
| 9 more | 1 each | see E4 |

**Conclusion: 13 of the 26 filenames cannot be what they say.**
The earlier run clicked a rail entry, captured, and moved on. Once the E-Sign
recipients dialog opened it swallowed every later click, so twelve consecutive
captures recorded the same modal under twelve different toolset names, and the
first `01-edit-a-pdf` frame is byte-identical to the baseline because that click
never landed at all.

Two rules for the method follow directly, and both are cheap:

1. **A capture is invalid unless its frame differs from the frame before it.**
   Byte equality with the previous capture means the click did not land or the
   UI did not change; either way the file is not evidence of what its name says.
2. **A capture is invalid unless something in the frame independently identifies
   the state.** Here the panel header and the highlighted rail row do that. A
   filename is a claim by the capture script, never evidence.

Both are checks on the captured frames, not on the capture session, so they can
run long after the session ends, which is what made this audit possible at all.

---

## E4. What the surviving distinct frames establish

Thirteen frames are distinct. The states verified frame by frame are below;
three distinct frames were not opened and are listed as unverified in E4.7. Measured with `parity/tools/measure-pane.py`
against the source pixels. The captures are 2x; every number below is in logical
points and every one is **relative to the window or the pane**, per the
relative-measurement rule in `docs/plans/parity-goal.md` section 4.3. Window:
1800 x 1057 pt. Theme: dark. Document open: `corpus/seeds/two-page.pdf`.

### E4.1 The All tools rail, whole toolset panel, frame `8d5d971`

Pane occupies x = 0 to 287.0 pt of the window. Twenty entries, in this order:

| # | Label | # | Label |
|---|---|---|---|
| 1 | Export a PDF | 11 | Redact a PDF |
| 2 | Edit a PDF | 12 | Compress a PDF |
| 3 | Create a PDF | 13 | Prepare a form |
| 4 | Combine files | 14 | Fill & Sign |
| 5 | Organize pages | 15 | Add comments |
| 6 | AI Assistant | 16 | Convert to PDF |
| 7 | Generative summary | 17 | Add a stamp |
| 8 | Request e-signatures | 18 | Use a certificate |
| 9 | Scan & OCR | 19 | Use print production |
| 10 | Protect a PDF | 20 | Measure objects |

Geometry, measured as ink extents rather than control boxes, because a control
box is only observable under hover and no hover state was captured:

| What | Value | How |
|---|---|---|
| Pane width | 287.0 pt | first column whose colour leaves the pane background and stays away for 20 px |
| Pane header baseline band | top 100.5 pt, 13.0 pt tall | ink band |
| Header label left inset | 24.0 pt | ink |
| Header close control | left 256.0 pt, 8.0 pt wide | ink; right inset 23.0 pt |
| First entry ink top | 146.0 pt | ink band 4 |
| Last entry ink top | 906.5 pt | ink band 23 |
| Entry pitch | **40.03 pt** | (906.5 - 146.0) / 19 |
| Entry icon left inset | 35.0 to 36.0 pt | ink, two rows sampled |
| Entry icon optical width | 15.0 to 16.0 pt | ink, two rows sampled |
| Entry label left inset | **65.0 pt**, both rows sampled | ink |
| Icon-to-label gap | 13.0 to 14.0 pt | derived |
| Separator rule above the footer | 936 pt | ink |

Icon metaphors, in words, per Legal posture rule 2. Recorded because the
metaphor is a fact about the interface and the artwork is not: a sheet with an
outward arrow (Export), a sheet with a pencil (Edit), a sheet with a plus
(Create), two overlapping sheets (Combine), a sheet with page markers (Organize),
a speech bubble with a spark (AI Assistant), a sheet with lines and a spark
(Generative summary), a person with a pen (Request e-signatures), a sheet with a
scan frame (Scan and OCR), a shield (Protect), a sheet with a struck block
(Redact), a sheet with a downward arrow (Compress), a sheet with a form field
(Prepare a form), a pen nib (Fill and Sign), a speech bubble on a sheet (Add
comments), a sheet with an inward arrow (Convert to PDF), a stamp (Add a stamp),
a certificate rosette (Use a certificate), a printer sheet (Use print
production), a ruler (Measure objects). Optical size 15 to 16 pt on a pitch of
40 pt.

**The two elements below the rule are the upsell and are explicitly excluded
from what Onionskin reproduces**: a two-line caption and a filled pill button.
They are recorded here only so a later reader can tell that the space below the
rule is accounted for, and so nobody re-derives them as content.

### E4.2 The truncated rail, frame `ff9627a`

The same pane, same width, but only **13 entries** followed by a link-styled
`View more`. The cut falls after `Prepare a form`, which is entry 13, so the
seven entries hidden by default are `Fill & Sign`, `Add comments`,
`Convert to PDF`, `Add a stamp`, `Use a certificate`, `Use print production` and
`Measure objects`.

Frame `210fc67` settles which state is the default. It was captured immediately
after Reader was restarted, carries Reader's own `Reopen closed PDFs` recovery
toast, and shows the 13-entry state. So **the truncated rail is what a launched
Reader shows and the 20-entry list is the expanded state**, not the reverse. The
window height is identical across all three frames, so the cut is not a height
fit. Whether the expansion persists across launches is not established.

### E4.3 Export a PDF: a dense panel that renders in full, frame `07ffb1d`

Clicking `Export a PDF` switches the tab strip to `Convert` and replaces the
rail with the Convert panel. **No upsell modal appears.** The whole panel
renders, live, with its controls in their default states.

Structure, in order:

| # | Element | Kind | Detail |
|---|---|---|---|
| 1 | `Convert` | panel header | with a close control |
| 2 | `EXPORT PDF TO` | section label | carries a Pro badge dot |
| 3 | `Microsoft Word` | radio, **selected** | trailing format tag `DOCX`, trailing chevron |
| 4 | `Microsoft PowerPoint` | radio | trailing format tag `PPTX`, no chevron |
| 5 | `Microsoft Excel` | radio | trailing format tag `XLSX`, no chevron |
| 6 | `Image format` | radio | trailing format tag `JPEG`, trailing chevron |
| 7 | `Other format` | radio | trailing format tag `RTF`, no chevron |
| 8 | `Recognized text language` | field label | |
| 9 | `English US` | dropdown | trailing help control |
| 10 | `Convert` | primary button | right-aligned |
| 11 | `OTHER OPTIONS` | section label | |
| 12 | `Convert to PDF` | navigation entry | icon plus label |
| 13 | `Compress a PDF` | navigation entry | icon plus label |
| 14 | `Scan & OCR` | navigation entry | icon plus label |

Geometry:

| What | Value |
|---|---|
| Pane width | 287.0 pt, identical to the rail |
| Panel header ink top | 104.0 pt |
| `EXPORT PDF TO` ink top | 155.0 pt |
| Radio row pitch | **44.0 pt**, exactly, over five rows |
| Radio glyph left inset | 35.0 pt, 14.0 pt wide |
| Radio label left inset | 59.5 pt |
| Selected row box left / right edge | 26.0 pt / 254.5 pt, so 228.5 pt wide |
| `Recognized text language` ink top | 414.0 pt |
| Language dropdown ink top | 438.5 pt, 17.5 pt tall |
| `Convert` button | left 174.5 pt, width 79.5 pt, height 32.0 pt, right edge 254.0 pt |
| `OTHER OPTIONS` ink top | 544.0 pt |
| Other-option row pitch | 39.25 pt over three rows |
| Content column | left 26.0 pt, right 254.5 pt, so a 32.5 pt right gutter against a 26.0 pt left inset |

Two things worth naming. The content column is not centred in the pane: 26.0 pt
on the left against 32.5 pt on the right. The most likely reason is a scrollbar
gutter, and this frame cannot distinguish that from a deliberate asymmetry, so
it is recorded as measured and flagged. And the primary button and the upsell
button share a height of exactly 32.0 pt, which is evidence for one control-height
token rather than two.

**Where the wall is: on apply.** The panel is fully interactive up to the
`Convert` button. Nothing in this frame establishes what happens after that
click, because no frame captured it.

### E4.4 Create a PDF: a multi-step flow whose first step renders, frame `592642c`

Clicking `Create a PDF` opens a **new document tab** titled `New document` and
replaces the whole window content with a task view:

| Element | Detail |
|---|---|
| Task title | `Create a PDF`, top left of the task bar |
| `Close` | outline button, top right of the task bar |
| Empty-state line | `Create PDFs from images, Microsoft Office files, and more` |
| Primary button | `Select Files`, centred below the empty-state line |

Both the empty state and its call to action are centred horizontally in the
window and sit in the upper third. The tab strip keeps the original document tab
to the left of the new one, so the flow is tab-scoped rather than modal.

**Where the wall is: step two.** Step one is fully capturable. What the picker
returns to, and every step after it, is not in this capture set.

### E4.5 Request e-signatures: a panel and a dense dialog, frame `650b2b0`

The most complete Pro state in the whole set, and it arrived by accident.
Clicking `Request e-signatures` switches the tab strip to `E-Sign`, renders a
full panel, and opens a modal dialog over the document.

Panel, in order: header `E-Sign` with a close control; section label
`GET E-SIGNATURES FAST`; a bordered card `Request e-signatures` with the
secondary line `Send this document to anyone to e-sign online in 3 easy steps`;
section label `FILL AND SIGN YOURSELF`; a six-control icon strip (text field,
cross, check, filled dot, rounded rectangle, horizontal rule); a dashed-border
row `Add signature` with a trailing plus; a dashed-border row `Add initials`
with a trailing plus; the sentence `After signing, you can create a read-only
certified copy with an audit trail.`; an outline button `Save a certified copy`.

Dialog, in order: a full-width banner `Send up to 2 documents for signature for
free every 30 days on a rolling basis.` with an `Upgrade Now` link; a left
column headed `Get e-signatures faster than email` with three icon-and-text
rows and a `See how it works` external link; a right column headed `Add
recipients to e-sign this document` over a single text field with the
placeholder `Add People's names or email IDs`; a footer with `Cancel`
(secondary, **disabled**) and `Specify where to sign` (primary, enabled).

This is the only frame in the set where the gate is a **quota banner rather than
a block**: the dialog is live and the feature is free up to two documents per 30
days. That is a third gate shape, distinct from both the modal upsell and the
apply-time wall, and the method has to be able to record it.

### E4.6 Where the wall falls, for the toolsets the set does cover

| Toolset | Rail entry | Menu entry | Tooltip | Panel renders | Dialog renders | Wall falls |
|---|---|---|---|---|---|---|
| Export a PDF | yes | `File > Convert to Word, Excel or PowerPoint` | not captured | **yes, in full** | n/a | on apply |
| Edit a PDF | yes | `Edit > Edit a PDF` | `Modify or add text, images, pages, and more` | no | upsell modal | **at the click** |
| Create a PDF | yes | `File > Create PDF` | not captured | **yes, task view** | n/a | at step two |
| Combine files | yes | `File > Combine Files` | not captured | no | upsell modal | **at the click** |
| Organize pages | yes | `Edit > Delete Pages`, `Edit > Rotate Pages` | not captured | no | upsell modal | **at the click** |
| Request e-signatures | yes | `File > Request e-signatures` | not captured | **yes, in full** | **yes, in full** | quota, not a wall |
| Scan & OCR | yes | `Edit > Scan and OCR` | not captured | not established | not established | not established |
| Protect a PDF | yes | `File > Password Protect` | not captured | not established | not established | not established |
| Redact a PDF | yes | `Edit > Redact a PDF` | not captured | not established | not established | not established |
| Compress a PDF | yes | `File > Compress File` | not captured | not established | not established | not established |
| Prepare a form | yes | `Edit > Prepare Form` | not captured | not established | not established | not established |
| Convert to PDF | yes | none found | not captured | not established | not established | not established |
| Add a stamp | yes | none found | not captured | not established | not established | not established |
| Use a certificate | yes | none found | not captured | not established | not established | not established |
| Use print production | yes | none found | not captured | not established | not established | not established |
| Measure objects | yes | none found | not captured | not established | not established | not established |

`not established` is doing real work in that table. Eleven of the sixteen
toolsets have a rail entry and nothing past it, because the capture run that was
supposed to open them recorded the E-Sign dialog eleven times instead. The upsell
modal template, seen three times, is identical across toolsets except for its
eyebrow, headline, bullet list and illustration, so it carries capability prose
and no layout information Onionskin would use.

### E4.7 The remaining distinct frames

| Frame | What it shows | Why it is recorded |
|---|---|---|
| `7333d42` | The rail **closed**. Only the floating quick-tools strip (six controls, vertical, upper left of the canvas) and the right-hand pane switcher (four controls) remain, with the Pages pane still open. | Establishes that dismissing the tool pane is a distinct layout, not a width change: the canvas takes the pane's 287 pt and the quick-tools strip moves to the canvas edge. |
| `a8a380b` | The E-Sign panel and recipients dialog, one caret-blink apart from `0f3d966` and `650b2b0`. | Confirms all three E-Sign frames are one state, so the set contains one E-Sign capture, not three. |
| `210fc67` | The 13-entry rail plus Reader's `Reopen closed PDFs` recovery toast. | Dates the earlier run's force-quit and, as E4.2 says, settles which rail state is the default. |
| `7a5d8a0` | A 260 x 192 px thumbnail, the only capture in the set that is not a full window. | Not usable. Recorded so the count reconciles. |

### E4.8 Facts the capture set does not establish, listed so nobody assumes them

- No hover state was captured anywhere, so no control's hit box is known. Every
  geometry number in E4 is an ink extent.
- No focus ring was captured, so focus treatment is unknown.
- No light-theme frame exists. Every number is from the dark theme.
- No frame captures a tooltip except `Edit a PDF`'s.
- No frame captures a context menu.
- No frame captures a second step of any multi-step flow.
- No frame captures the state after a `Convert`, `Select Files` or
  `Specify where to sign` click, so "the wall falls on apply" for
  `Export a PDF` is an inference from the absence of an upsell before that
  point, not an observation of the wall itself.

---

## E5. Documentation sourcing, 2026-09-09

### E5.1 Class C is unavailable from this environment

| Attempt | Result |
|---|---|
| Fetch tool, `https://helpx.adobe.com/acrobat/using/grids-guides-measurements-pdfs.html` | HTTP 403 |
| `curl` with a desktop Chrome user agent, same URL | HTTP 403 |
| Fetch tool, eight further `helpx.adobe.com` URLs across four locales (`gr_en`, `my_en`, `ph_fil`, default) and three path shapes (`/using/`, `/desktop/`, `/current/`) | HTTP 403, all eight |
| Fetch tool, `https://web.archive.org/web/2024/<helpx url>` | refused by the fetch tool itself |

So Adobe's help server refuses this environment rather than gating particular
pages. **Every documentation fact in this file is therefore class D**, a search
engine's summary of a page nobody here fetched, and the permission matrix in
`docs/plans/pro-toolset-reference-method.md` section 4.3 forbids class D from
establishing grouping (F3) or ordering (F4). Restoring class C is a prerequisite
for any ordering claim about a walled dialog.

### E5.2 Per-toolset probe result

Sixteen toolsets, one or two search queries each against a list of roughly a
dozen expected control labels per toolset. A label counts as confirmed only when
it appeared in returned text.

| Toolset | Adobe URL surfaced | Path generation | Confirmed / listed | Order asserted |
|---|---|---|---|---|
| Edit a PDF | yes | current | 10 of 11 groups; `Background` not surfaced | no |
| Organize pages | yes | current | 8 of 9; `More` menu not surfaced | no |
| Redact a PDF | yes | current **and** `/acrobat/11/` | 6 of 7; `Apply` not surfaced verbatim | no |
| Protect a PDF | yes | current **and** `/acrobat/11/` | 5 of 8, plus four extra labels | no |
| Prepare a form | yes | current | 8 of 9 field types; tabs General, Appearance, Options, Actions, Format, Signed | partial: "General and Actions appear for all field types" |
| Scan & OCR | yes | current | 6 of 7, two as close variants | no |
| Use a certificate | yes | current | **2 of 6** | yes: certification precedes other signatures |
| Prepare for accessibility | yes | current | 6 of 8; one contradicted | no |
| Use guided actions | yes | current **and** `/acrobat/11/` | 5 of 5, two as close variants | yes: steps run in list order |
| Compare files | yes | current | 4 of 6 | no |
| Compress a PDF | yes | current | 8 of 9 | no |
| Combine files | yes | current **and** `/acrobat/11/` | 4 of 6 | no |
| Create a PDF | yes | current | 4 of 4, with menu paths | no |
| Export a PDF | yes | current | Word, Excel, PowerPoint, HTML, RTF, plain text, JPG, PNG, TIFF, export-images | no |
| Measure objects | yes | current | Distance, Perimeter, Area, Measurement Info panel, four snap types, Change Scale Ratio | no |
| Use print production | yes | **`/acrobat/11/` only**, plus a third-party page titled "Acrobat XI Pro" | 7 tool names including the one in-scope row | no |

Two provenance facts that the table is the point of recording:

- **No page in this probe stated its Acrobat version.** Version provenance came
  entirely from URL paths, which is why four toolsets are flagged as returning
  `/acrobat/11/` results (Acrobat XI, 2012) and Print Production is flagged as
  returning nothing else. For every other toolset the depicted version is
  **could not determine**.
- **Order was asserted for only two of sixteen toolsets**, and in both cases as
  a single sentence about a constraint rather than a list. Documentation prose
  gives field lists generously and orderings almost never, which is the
  empirical basis for the F4 row of the permission matrix.

### E5.3 Contradictions and close variants found

These matter because a parity row carrying the wrong label is a defect the
functional board would not catch.

| Row or expected label | What Adobe's prose says |
|---|---|
| `Set Alternate Text` | `Add alternate text` |
| `Correct recognized text` | `Correct Recognize Text` |
| `Editable Text and Images` | `Editable Text & Images` |
| `Reduce File Size` | `Reduce PDF file size` |
| `Tools to add` | `Choose Tools To Add` |
| `Action steps` | `Action Steps To Show` |
| `Add Open Files` | `Add open files` |
| `Select files` | `Select File` |
| `Touch Up Reading Order` | `Reading Order tool`; the old name survives only in the URL slug |

Class D may establish a label verbatim and may not establish that a variant is
the *current* one, because the snapshot behind a search summary is unknown. Each
row above is a question for a class C pass, not an answer.
