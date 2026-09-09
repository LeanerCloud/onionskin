# Goal: full Acrobat parity, in capability and in look and feel

Status: goal and measurement design. No source changes.
Base commit: `81f802f`. Counts re-derived from `ACROBAT-PARITY.md` at that
commit by the recount command in its Totals section.
Companions: `ACROBAT-PARITY.md` (the functional scoreboard), `PLAN.md`
("GUI: Acrobat parity", "Legal posture", "Milestones", decision 12),
`docs/plans/m2-viewer.md` section 6 (which deferred pixel parity and asked for
this document), `docs/evidence/parity-reference.md` (the private-evidence
ledger).

A note on language, before anything else. `PLAN.md` states the engineering
target in words it marks as internal. This document is tracked and public, so
it does not use them. What follows says "the workspace an Acrobat user already
knows" and means exactly what the plan means, under Legal posture rule 3.

---

## 1. The goal, in one paragraph

Onionskin reaches full parity when two things are true at once. First, every
one of the 323 Acrobat rows the project has agreed to build stands at its
declared terminal status on `ACROBAT-PARITY.md`, and the 80 rows it has
declined stay declined for a reason that is still true. Second, every visual
surface those rows appear on has been measured against a dated capture of the
reference product and carries a recorded, countable result: its colours,
spacing, type sizes, radii, motion timings and icon geometry come from a token
whose value was measured rather than guessed; its layout numbers sit inside a
stated tolerance of the reference numbers, proved by a test that fails when
they drift; and a person who did not build it has run a scripted session
through it and recorded what they saw. Neither half is an adjective. The first
is "N of 323 rows at terminal status", the second is "N of M surfaces
accepted", and both are numbers that can go down when Adobe moves the target.

---

## 2. Ground truth at authoring

Re-derived, not quoted. `ACROBAT-PARITY.md` at `81f802f`, its own recount
command extended to group by milestone as well as status:

| | M2 | M3 | M4 | M5 | M6 | post-1.0 | none | total |
|---|---|---|---|---|---|---|---|---|
| implemented | 49 | | | | | | | 49 |
| partial | 17 | | | | | 1 | | 18 |
| planned | 1 | 99 | 2 | 52 | 46 | 56 | | 256 |
| out-of-scope | | | | | | | 80 | 80 |
| **total** | **67** | **99** | **2** | **52** | **46** | **57** | **80** | **403** |

Two facts fall out of that table that the file's prose does not state.

**M2 owns 18 of the remaining rows, not zero.** The scoreboard's summary gives
the milestone split of all 323 in-scope rows (M2 67, M3 99, M4 2, M5 52, M6 46,
post-1.0 57) but never the split of the 274 that remain. A reader who adds
99 + 2 + 52 + 46 + 57 gets 256 and is left with an unexplained 18. Those 18 are
M2's own: 17 rows still `partial` and one, `View > Zoom > Dynamic Zoom`, still
`planned`. The milestone that is closing out is not the milestone that is done.

**"Full parity" as literally stated is unreachable, and not for the reason
people assume.** Five rows (export to `.docx`, `.xlsx`, `.pptx`, `.rtf` and
HTML) are `planned` today, but `PLAN.md` says they "land post-1.0 at best and
are marked partial forever", and the scoreboard's own convention says a
deliberately reduced target "stays `planned` until its supported subset ships",
after which it is `partial`. So five in-scope rows have `partial` as their
final resting status by design. A definition of full parity that reads "323
rows `implemented`" is false by construction. Section 3 restates it.

Source scale, for the estimates in section 8: 75,596 lines of Rust across
`crates/` and `plugins/`, of which 36,966 are `crates/app`. That is what one
milestone of 67 rows plus the whole foundation cost.

---

## 3. Half one: functional parity

### 3.1 The number, defined so it can be checked

> **Full functional parity: all 323 in-scope rows stand at their declared
> terminal status.** The terminal status is `implemented` for 318 rows and
> `partial` for the five deliberately lossy export rows named in section 2,
> each of which must additionally carry a Notes entry stating what its
> supported subset is and what it drops.
>
> **Today the number is 49 of 323.** A softer companion number, reported
> alongside and never instead: 67 of 323 rows have shipped something a user can
> touch (49 `implemented` plus 18 `partial`).

Three rules keep the number honest, all inherited from the scoreboard:

- A row at `partial` counts toward the softer number and never toward the
  terminal one, unless it is one of the five whose terminal status is `partial`.
- A row's Notes must name its gap. "Partial" with no named gap is a defect in
  the row, not a status.
- The denominator is Acrobat's surface, not Onionskin's. Onionskin-only
  surfaces (skins pane, generation rollback, the redaction verifier, MCP, CLI)
  are excluded, as they already are.

### 3.2 The 80 refusals, audited

Every out-of-scope row was read. They group as follows.

| Group | Rows | Verdict |
|---|---|---|
| Cloud-, account- or server-tethered | 33 | Genuinely out. Refusing them is the product's counter-pitch, not a shortfall. |
| Print production and prepress | 17 | Genuinely out. Plan-named as permanent; each needs a colour engine and a certification story. |
| Rich media, 3D, geospatial | 13 | Genuinely out. Plan-named; each needs a whole subsystem no crate claims. |
| Separate products in their own right | 8 | Genuinely out, but these are the scoreboard's judgment, not the plan's. |
| Reversible product decisions | 4 | Three genuinely out; one parked (below). |
| Adobe-cryptographic | 3 | Impossible, not declined. Reader extensions and AATL/EUTL are Adobe-signed. |
| Legal-posture refusals with an unshipped obligation | 2 | Refusal genuine; obligation untracked (below). |

Four of the 80 do not survive the audit as written:

1. **`Revert to the classic Acrobat interface` is parked, not refused.** The row
   says "the plan defers classic to a possible later theme, not a shipped
   toggle", and `PLAN.md` says "classic can become a theme later if demand
   shows". A row whose own note describes a possible future is `planned` at
   post-1.0, or the plan sentence goes. It cannot be both.
2. **`XFA / LiveCycle Designer forms` and `Document-level and interactive
   JavaScript beyond the forms API` each owe a user-visible notice that has not
   shipped.** Both rows say so in their own Notes. A notice is a thing a user
   can point at, which is the scoreboard's definition of a row. Two in-scope
   rows are missing from the board, and two obligations carry no milestone.
   The refusals themselves are correct and should stay.
3. **`Catalog` is out-of-scope on the scoreboard's own authority.** The file
   flags this itself in "What `PLAN.md` still does not name". It needs plan
   ratification, not re-argument.
4. **The Office asymmetry is worth one sentence.** Reading DOCX is out ("a
   product in itself"); writing it is a post-1.0 lossy target. That is
   defensible, because writing a lossy approximation is genuinely cheaper than
   reading one at fidelity, but the reason should be in the row rather than
   inferred.

The other 76 are sound. Note the split inside them: about 36 rows (the cloud
group plus the three Adobe-cryptographic ones) are unreachable no matter what
the project decides, because they need Adobe's services or Adobe's signature.
The remaining 44 are buildable and declined on cost. That distinction matters
for section 8 and is not currently visible on the board.

### 3.3 The milestone map, and whether it reaches parity

Every in-scope row carries a milestone, so the map is arithmetically
exhaustive: 49 done plus 18 in M2 plus 256 across M3 to post-1.0 is 323. Three
things are nevertheless true and should be said plainly.

**1.0 is not full parity, by construction.** M6 is the last pre-1.0 milestone.
57 in-scope rows sit after it. The ceiling at 1.0 is **266 of 323, or 82%**.
Nothing in the milestone map is wrong about this; it is simply never stated,
and a goal document that let a reader assume 1.0 meant parity would be the
defect.

**13% of the map is unratified.** 42 rows carry `(judgment)` because their
milestone does not follow from plan text. They are placed sensibly, but "the
milestone map reaches parity" is a claim about 281 ratified placements and 42
guesses. The scoreboard already lists them; a plan revision should confirm or
move them, and until it does the map's precision is overstated.

**M4 is not a parity milestone and should not be read as one.** Two rows. Its
deliverables (MCP, CUPS and Windows print backends) are mostly not Acrobat
surface. Reading the map as a slope from 49 to 323 makes M4 look like a stall;
it is not, it is a milestone the scoreboard is the wrong instrument for.

---

## 4. Half two: look and feel

### 4.1 Why the existing scoreboard cannot answer this

The scoreboard counts capabilities. It is possible, today, for a row to read
`implemented` while the surface it names looks nothing like the reference. That
is not hypothetical. `All tools pane (left tool rail)` is `implemented`, and
`crates/app/src/shell/chrome/rail.rs` draws its icons as five text glyphs
selected by a `match` on the tool id, one of which is the letter `T`, with `"?"`
for anything unmatched. `PLAN.md`'s Legal posture and GUI parity sections both
commit to redrawn in-house SVG icons on the Schist generated-artwork pattern.
There is no SVG or PNG asset anywhere in `crates/` or `plugins/`. The row is honestly `implemented` against its
own definition, the plan's commitment is unmet, and nothing in the repository
records the gap.

The project's only existing look-and-feel gate is `PLAN.md`'s testing strategy
item 6: "UI parity itself is checked visually against the `parity/reference/`
Acrobat screenshot corpus per release." Checked visually by whom, against what
threshold, recorded where. As written it is satisfiable by assertion, which is
the one thing the functional board was built to prevent.

`docs/plans/m2-viewer.md` saw this coming and deferred it honestly (section 6:
"Pixel parity against `parity/reference/`. Deferred... File a follow-up issue
at the end of M2"). No such issue exists in the repository. This document is
that follow-up.

### 4.2 What was considered, and what it is used for

Five mechanisms were on the table. None is discarded; four are demoted from
gate to instrument, for stated reasons.

**A parallel scoreboard of visual surfaces.** Kept, as the reporting layer. It
is the only mechanism that produces a number with the same shape as the
functional one, which is the point.

**Tolerance-based whole-window image comparison.** Demoted to a local
investigation tool, for three reasons and not for squeamishness. (a) Its output
cannot be committed: a diff image of an Acrobat window contains Acrobat window
pixels, and Legal posture rule 4 keeps those local. A gate whose evidence
nobody outside one machine can audit is not a gate. (b) It can never pass. The
icons are required by rule to differ, and a threshold loose enough to admit
redrawn icons is loose enough to admit a wrong toolbar. (c) Its number is not
diagnostic: "4.1% of pixels differ" does not say what is wrong, so it cannot be
acted on and cannot be argued with. It stays as the thing that *produces*
candidate numbers for the token and geometry ledgers, run locally, output kept
under `parity/comparison/`.

**Per-surface acceptance criteria with reference screenshots.** Kept, but the
screenshot is the private input and the criteria are the public artefact. This
is already the shape of `docs/evidence/parity-reference.md`; the change is that
the criteria become numbers and test names rather than prose verdicts. The
current B7 comparison notes read "Pass for the M2 shell/rendering baseline",
which is prose, and prose is what this whole design is trying to get away from.

**A checklist of design tokens with a stated source and a test.** Kept as the
foundation layer, because it is the only mechanism that prevents the problem
rather than detecting it, and because the colour axis proves it works: 21
colour tokens in `theme.rs`, light and dark, and zero hex literals anywhere
else in the shell.

**A "first five minutes" behavioural script.** Kept as the third layer, on the
`docs/spikes/m2-voiceover-acceptance.md` model, because motion, focus feel,
empty states and the small behaviours that make an interface recognisable are
not in any number the other two layers can produce.

The combination is deliberate: layer 1 stops the drift, layer 2 catches it, and
layer 3 covers what neither can see.

### 4.3 The measure

One public scoreboard, two evidence ledgers, one script. All four mirror
structures that already exist, so nothing here is a new kind of artefact.

```
LOOK-AND-FEEL.md                      public scoreboard, mirrors ACROBAT-PARITY.md
docs/evidence/lf-tokens.md            L1, public: token, value, source, test
docs/evidence/lf-surfaces.md          L2, public: surface, measurements, tolerances, tests
docs/spikes/lf-first-five-minutes.md  L3, public: the script and its dated results
parity/reference/, parity/onionskin/, parity/comparison/   private, unchanged
```

The legal mechanism that makes this work: **the repository holds measurements,
never images.** A coordinate, a colour value, a duration in milliseconds and a
corner radius are facts about a layout, not reproductions of artwork. They are
measured once off a private capture, recorded with the evidence ID of that
capture, and the capture stays where it is. This is the same move
`docs/evidence/parity-reference.md` already makes with SHA-256 values, extended
from "this file existed" to "this file measured N".

#### Layer 1: the token ledger

A fixed set of token categories. Each token has a name, a value, a **source**,
and a **test**.

| Category | Present today | Missing today |
|---|---|---|
| Colour | 21 tokens, light and dark, in `ThemeTokens` | source: every value is an unattributed literal |
| Spacing | nothing | the whole scale; 240 raw `px(...)` calls across the chrome and canvas stand in for it |
| Type | nothing | size scale, weights, line heights; call sites use gpui utility classes or `px(13.0)` |
| Radius | nothing | the whole scale; call sites use gpui's `rounded_*` utilities |
| Motion | nothing | durations, easing; the string `duration` does not appear in the shell |
| Elevation | nothing | shadow and border treatment for raised surfaces |
| Icon geometry | nothing | grid, stroke weight, optical sizes, corner treatment |

Rules:

- **`source` is an evidence ID plus what was measured.** For example
  `B7-REF-001 @ (412,18)-(413,19)`, or `B7-REF-001, tab strip height`. A token
  whose source reads `assumed` is counted as unsourced. It may ship; it may not
  be counted, and no surface that consumes it may be accepted.
- **`test` is the name of a test that fails if the token stops being used.**
  For spacing and sizing this is a lint-shaped test: the named file contains no
  raw `px(` literal outside the token module. For colour it is the test that
  already holds by construction and should be made explicit.
- **Icon tokens describe geometry, never artwork.** Grid size, stroke weight,
  terminal treatment, optical size steps, and per-icon: the metaphor in words
  ("a hand, open, palm forward"), the grid it is drawn on, and the SVG path
  produced in-house. The reference is measured for grid and weight only. Adobe's
  paths are never traced, sampled, or vectorised. This is the axis where the
  measure is deliberately weaker than it could be, and section 7 says so.
- The count reported is `sourced / total` and `enforced / total`, separately.
  A token that is sourced but not enforced is a fact nobody is holding to.

#### Layer 2: the surface geometry ledger

This is the layer that costs almost nothing to build, because the project
already built it for another reason.

`crates/app/tests/a11y_probe.rs` launches the real application binary, opens a
real window at a fixed 1100 x 860 (`WINDOW_WIDTH`/`WINDOW_HEIGHT` in
`crates/app/src/shell/mod.rs`), and prints a JSON tree of every named node with
its platform-reported rectangle. It has a `frame(&self, node) -> [f64; 4]`
helper and a test, `the_page_controls_report_the_rectangles_they_were_painted_at`,
that already asserts painted geometry. The macOS probe is a required CI gate.

So the shell already emits a machine-readable, deterministic geometry of every
named control, in a harness that already runs on every push. Decision 12
justified accessibility on Section 508 and European Accessibility Act grounds.
It turns out to also be the reason visual measurement is nearly free here, and
that is worth recording because it changes how the investment is justified.

One constraint on what the probe can be asked. The window is opened with
`Bounds::centered`, so its origin depends on the display it lands on, and the
existing test is careful to assert only relative facts: that each rectangle is
non-empty, that they run left to right, and that they share a row. **Every
measurement in this ledger is therefore relative**: a size, a gap, an inset from
a container edge, an ordering. Absolute screen positions are not measurable and
are not measured. That is not a limitation in practice, because every question
worth asking about a layout is relative anyway, but a ledger line that quietly
recorded an absolute origin would be green on one machine and red on the next.

A surface's ledger entry is:

| Field | Content |
|---|---|
| Surface | one of the enumerated surfaces (section 4.5) |
| Reference | evidence ID, Acrobat version, private filename, SHA-256 |
| Onionskin | commit, private filename, SHA-256 |
| Window | the pinned size the numbers were taken at |
| Measurements | one row each: what, reference value, Onionskin value, tolerance, verdict |
| Tests | the test names that assert each measurement |
| Verdict | `accepted`, `open`, or `stale` |

What gets measured, per surface: overall height or width of the container;
first and last child inset from the container edge; gap between adjacent
children; control height; baseline offset of any label; the count and order of
elements. Order matters as much as position: a surface with the right controls
in the wrong sequence is the failure a user notices first.

**Negative space is measured.** A surface that has an element the reference does
not have fails, exactly as one missing an element does. Otherwise the measure
rewards adding.

**Tolerances are stated per measurement class, in the ledger header, once:**

| Class | Tolerance | Why |
|---|---|---|
| Container dimension | plus or minus 2 px | rounding and border conventions differ |
| Inset, gap, control size | plus or minus 2 px | same |
| Element count and order | exact | there is no such thing as a nearly-right order |
| Text run width or baseline | plus or minus 8% | the typeface cannot match; see section 6 |
| Colour | exact against the token | the token is the contract, not the reference pixel |
| Motion duration | plus or minus 30 ms | below human discrimination for UI transitions |

A tolerance that is widened to make a surface pass is a change to the ledger
header, reviewed as such, and it re-opens every surface that used the old value.
Widening one surface's tolerance quietly is the most obvious way to cheat this
measure, so it is the one thing the format makes impossible to do locally.

#### Layer 3: the first five minutes

A scripted session, on the model of `docs/spikes/m2-voiceover-acceptance.md`,
which is the project's existing proof that a human acceptance pass can be
written so that a dishonest pass is harder than an honest one. Same properties:
numbered steps, each with an explicit "pass looks like" and "failure looks
like", a fill-in result block, and a stated rule that a session reported as a
pass when it was not is worse than no session.

The script covers what no number reaches. Sketch of its steps, to be written out
in full when the file lands:

1. **Cold open.** Launch with no document. Does the first screen offer what the
   reference's first screen offers, in the same places, without a tour.
2. **Open something.** Does the document appear where it appears in the
   reference, at the same default zoom and fit, with the same chrome visible.
3. **Reach for five things without looking them up**: page navigation, zoom,
   Find, the thumbnails pane, a tool from the rail. Each one either was where
   the hand went or it was not. Record which.
4. **Hover and focus.** Every control gives feedback on hover and shows a focus
   ring on keyboard focus, and the ring is the same treatment everywhere.
5. **Motion.** Panes, dialogs and menus open at a speed that reads as
   deliberate rather than instant or slow. This is the step with the weakest
   evidence in the whole design; section 7, item 3.
6. **Empty and error states.** Every pane with no content says something, and
   what it says is a sentence rather than a blank.
7. **The wrong thing.** Type a bad page number, open a file that is gone,
   cancel a save. Does the surface explain, in place, without a modal.
8. **Density.** Side by side with the reference at the same window size: does
   the same amount of document show, with the same amount of chrome around it.

Rules: **the person who built the surface does not run its script.** The result
block records the date, the Onionskin commit, the reference version, and every
failure in the words the person used. An unrun script is `open`, never
`assumed pass`.

### 4.4 The rules that stop someone claiming this is done

These are the load-bearing part. Everything above is machinery; this is what the
machinery is for.

1. **Three layers are ANDed, never averaged.** A surface is `accepted` only when
   its tokens are sourced and enforced, its geometry ledger is complete and its
   tests are green, and its script steps have been run and passed. Per-layer
   counts are also published separately, so a zero cannot hide behind two
   greens.
2. **No acceptance without a resolvable evidence ID.** The ID must resolve to a
   committed ledger line naming an Acrobat version, an Onionskin commit, two
   private filenames and two SHA-256 values. A `pending` reference blocks
   acceptance. It does not soften it, and there is no provisional pass. Eight of
   the ten required M2 reference states are incomplete today, which means the
   surfaces they cover cannot be accepted no matter how good they are.
3. **Every measurement names a test.** A ledger line with no test name is a
   claim, not a measurement, and is counted as unmeasured.
4. **Acceptance expires.** Each accepted surface names the Acrobat version its
   reference came from. When the project re-pins to a newer version, every
   surface referencing the old one drops to `stale` and the accepted count falls
   until each is re-measured. Parity is a number that can go down. The functional
   board has the same exposure and does not currently say so.
5. **Tolerances live in one header, not per surface.** See 4.3.
6. **Independent script runner.** See 4.3.
7. **Unsourced tokens poison their consumers.** A surface that consumes even one
   `assumed` token cannot be accepted. This is what stops the token ledger
   filling up with plausible round numbers.
8. **The public scoreboard states its own limits in its preamble**, including
   that no surface is ever pixel-identical because the icons and the typeface
   are required to differ. Someone reading only the number should not be able to
   misread it.

### 4.5 The scoreboard

`LOOK-AND-FEEL.md`, one row per visual surface, mirroring `ACROBAT-PARITY.md`'s
form and its counting discipline.

Denominator: every surface of the reference product on which at least one
in-scope parity row appears. That is set by Acrobat, exactly as 403 is, and the
first ledger pass must enumerate it. The 23 surfaces Onionskin has shipped today
are a lower bound, not the denominator:

- Chrome (10): main menu popup; document tab strip; tool rail; quick action
  toolbar; right side panel; page controls bar; global search field and its
  results; find bar; home view; modal dialog frame.
- Panes (7): pane switcher strip; thumbnails; bookmarks; attachments; layers;
  signatures; search results.
- Dialogs (3): preferences; about; keyboard shortcut reference.
- Canvas (3): page presentation and scroll/zoom modes; text selection
  presentation; canvas context menu.

Milestones add more: the comments pane, the organize grid, the print dialog and
Page Setup, the forms and redaction and protect surfaces, the measure surfaces,
the accessibility checker, the skins panel. The denominator grows as the
functional one does not, because Onionskin has not yet built surfaces that
Acrobat has.

Status values, one per row: `accepted`, `open`, `stale`, `blocked` (no reference
capture obtainable, with the reason), `n/a` (the surface exists only for
out-of-scope rows). Today every shipped surface is `open`, and the number is
**0 of 23 accepted**. That is the correct starting point and it should be
published as such.

### 4.6 Worked examples

#### Example A: the page controls bar

`crates/app/src/shell/chrome/page_controls.rs`, 833 lines, 22 raw `px(...)`
calls, no colour literals. The strongest case, because the a11y probe already
asserts its painted rectangles.

*L1.* Consumes `surface`, `text`, `disabled_text`, `error_surface`,
`error_text` and `hover` from `ThemeTokens`: six tokens, all enforced by
construction, none sourced. Needs a spacing token for the gap between control
groups, a control-height token, a radius token for the page number field, and a
type-size token for the field. Ledger state after a first pass: 10 tokens, 6
enforced, 0 sourced. Not acceptable until the source column is filled from a
reference capture, and not acceptable while four of its ten tokens are still
anonymous `px()` literals.

*L2.* Reference is `B7-REF-001` (`unified-shell-two-page-actual-size-...`),
which exists, is hashed and is ledgered. Measurements: bar height; left inset of
the first control; right inset of the last; gap between the navigation group and
the zoom group; page field width; page field height; the count and left-to-right
order of controls. Seven measurements. The per-control sizes, the gaps between
them and the order come straight out of the probe today:
`the_page_controls_report_the_rectangles_they_were_painted_at` already reads
`first-page`, `previous-page`, `next-page` and `last-page` by id, asserts each
has a non-empty rectangle, asserts they run left to right, and asserts they sit
on one row. What is not free is the container: the bar itself is not a named
node, so its height either gains one or is measured from the private capture by
hand and recorded as a hand measurement. That distinction, free versus
hand-measured, belongs in the ledger per line.

*L3.* Script steps 3 (reach for page navigation and zoom without looking), 4
(hover and focus), and 7 (type a bad page number) all land on this surface.
Step 7 has real content here: the file carries an explicit invalid-zoom and
invalid-page-number surface, and the question the script asks is whether the
error appears in place or in a dialog.

*Verdict today:* `open`. Blocking items: zero sourced tokens, seven unwritten
measurements. Nothing about it is blocked on something the project does not
have. This surface could be `accepted` inside a day of work.

#### Example B: the tool rail

`crates/app/src/shell/chrome/rail.rs`, 735 lines, 4 raw `px(...)` calls. The
example that shows why a capability board is not enough.

*L1.* The rail's icons are `icon_glyph()`, a match from tool id to a single text
glyph, with `"?"` for anything unmatched. Under the icon-geometry rules this
is not a low score, it is an absent axis. There is no grid, no stroke weight, no
optical size, and no SVG. The row `All tools pane (left tool rail)` reads
`implemented` on the functional board and is correct to, because the rail does
what the row says. The two boards disagree, which is the entire argument for
having two.

What parity means here, precisely, under Legal posture rule 2: the grid the
icons are drawn on, the stroke weight, the optical sizes, the metaphor each icon
uses, and the position of the icon within its cell. Not the paths. An icon of an
open hand for the hand tool is the metaphor the reference uses and is not its
artwork; the specific curve of its fingers is, and is never sampled.

*L2.* Reference: none of the ten required M2 states captures the rail expanded.
This surface is `blocked` on a capture that has not been taken, which is a
five-minute job on a machine with an awake display, not an engineering blocker.
Measurements once captured: collapsed width, expanded width, row height, icon
cell size, icon inset within the cell, label baseline offset, entry count and
order. `crates/app/src/shell/chrome/side_panel.rs` shows the shape the rail
should adopt: `CLOSED_WIDTH: f32 = 40.0` and `OPEN_WIDTH: f32 = 280.0` are
already named constants, which is one step from being sourced tokens and several
steps ahead of the 102 anonymous `px()` calls in `tabs.rs`.

*L3.* Script step 3 (reach for a tool from the rail) and step 8 (density). The
honest expected result today is a failure on step 3 for anyone who knows the
reference product's rail, because a handful of text glyphs do not read as a tool
set.

*Verdict today:* `blocked` on capture, and would be `open` with a large L1 gap
once unblocked. It is also the single largest look-and-feel line item in the
project: see section 8.

#### Example C: the home view

`crates/app/src/shell/home.rs`, 626 lines. The example that shows what happens
when a surface is genuinely finished and still cannot be accepted.

*L1.* Consumes ten colour tokens correctly, the widest colour surface of the
three examples: `canvas`, `error_text`, `hover`, `muted_text`, `raised`,
`secondary_text`, `selected`, `subtle_hover`, `surface` and `text`. Needs
spacing, a card size for the thumbnail grid, a radius, and two type sizes. The
grid/list toggle needs the
same `selected` treatment the rest of the shell uses, which is a token question
rather than a home-view question, and that is the point of having a token layer:
the answer is not allowed to be local.

*L2.* Reference `B7-REF-008` (Home recents list and thumbnail layouts) is
`pending` on every column of the ledger. Rule 2 in section 4.4 is absolute:
`pending` blocks. This surface could be perfect and its status would still be
`open`.

*L3.* Script step 1 lands entirely here, and it is the step with the most at
stake, because the home view is the first thing anyone sees. The functional
board flags this too, in "What `PLAN.md` still does not name": "The plan
describes the document window in detail and never mentions the
no-document-open state... Acrobat users meet it first."

*Verdict today:* `open`, blocked on a capture. Also worth noting what it shows
about the L3 script: home has a missing-file state with a caching policy that
does not retry, and no geometry measurement will ever notice whether a missing
file reads as an explanation or as a broken tile.

---

## 5. Sequencing

Look-and-feel work on a surface that is about to be rebuilt is wasted, so the
order is not "measure everything now".

**Tokens first, always, and independently of any surface.** Token work is never
wasted: every surface built after them consumes them, and building surfaces
before them is precisely what produced 240 loose `px()` calls and zero motion
constants. This is also the only look-and-feel work that should happen during
M3, because M3 adds surfaces and each one added without tokens is a future
migration.

**Then the surfaces whose structure is frozen.** A surface is measurable when
its layout is settled, even if some of its contents are still gated. The
distinction between structure and contents is the whole sequencing rule:

| Measure now | Wait, and until when |
|---|---|
| page controls bar | quick action toolbar *slots*: 5 of 6 enable across M3 and M5 |
| tool rail (structure) | right side panel *contents*: tool-specific content starts M3 |
| find bar | canvas context menu: many entries enable at M3 and M5 |
| main menu popup | thumbnails pane context menu: page commands land M3 and M5 |
| document tab strip | layers pane: Properties lands M3 |
| pane switcher strip | signatures pane: validation UI lands M6 |
| thumbnails, bookmarks, attachments panes | attachments pane Open: lands with its owner |
| search results pane | |
| home view | |
| preferences, about, shortcuts dialogs | |
| page presentation and text selection | |

**`ShellFrame`'s split is not a rebuild.** M3's P0 breaks up
`crates/app/src/shell/chrome/tabs.rs` (7,278 lines) and describes itself as
mechanical and behaviour-preserving. A geometry ledger measures behaviour, so
the split does not invalidate a single measurement. It does make the token
migration much easier, and the 102 `px()` calls in that file are the largest
single cluster in the codebase, so the token work for tab-strip surfaces is
better done alongside P0 than before it.

**Icons are their own track and start now.** They gate no other work, they block
nothing, and they are the longest-lead item because they need design time and
not just implementation time. Starting them at M6 would put the most visible
part of look and feel on the critical path to 1.0.

**Capture the references opportunistically.** Every reference capture requires a
physically present machine with an awake display, which is the exact condition
that blocked the incomplete M2 states. Captures should be batched into a single
session per Acrobat version rather than requested one at a time, because the
cost is the session, not the screenshot.

---

## 6. What full parity explicitly excludes

Stated so that the number cannot be read as more than it is.

- **The 80 refused rows.** Section 3.2. Roughly 36 of them would need Adobe's
  services or Adobe's signature and are not buildable by anyone else; the rest
  are declined on cost.
- **Adobe's icon artwork, wordmarks and trademarks.** Legal posture rule 2.
  Consequence, stated plainly: **no Onionskin surface will ever be
  pixel-identical to the reference, and the goal does not claim it will.** Icon
  parity means grid, stroke weight, metaphor and optical size.
- **Adobe's typeface.** Acrobat's interface type is Adobe's own and cannot be
  shipped. Type parity means the size scale, the weights and the line heights,
  never the glyphs. Every text-dependent measurement in the ledger carries a
  looser tolerance for this reason and says so.
- **Platform-drawn surfaces.** The macOS menu bar, window controls, file
  pickers, the system print panel, colour pickers. Onionskin draws its own main
  menu popup (measurable) and also populates the native menu bar (not
  measurable). The reference product makes different choices about which to
  draw itself. Excluded from the denominator, named so that nobody scores them.
- **Cloud-tethered behaviour and XFA.** Permanently, per `PLAN.md`.
- **Onionskin-only surfaces.** The skins pane, generation rollback, the
  redaction verifier: they have no reference and are excluded from both boards,
  as the functional board already does.

---

## 7. What the goal cannot promise

In the style of the M2 and M3 plans' candor lists. Each item is a thing a reader
would otherwise reasonably assume.

1. **No reference exists yet for most of the remaining work, but the material
   is reachable.** `parity/reference/` contains captures from Acrobat Reader
   25.001.20438 and nothing else, so every Pro-only toolset (Edit a PDF,
   Organize, Redact, Forms, Protect, Measure, Accessibility, Print Production)
   currently has no capture and no ledger row. This was originally written as a
   purchasing decision blocking measurement of M3 through M6. That framing was
   wrong. The free Reader exposes the Pro toolsets in its own interface as
   entries carrying a purchase link: the rail lists them, the menus carry them,
   and in many cases a panel or dialog renders before the upsell intervenes. So
   most of that surface is capturable from the build already installed, using
   the same private-capture method B7 built, and at higher confidence than any
   documentation screenshot because it is the pinned version at known DPI,
   uncropped and unannotated. Adobe's published documentation is the fallback
   for the states that genuinely sit behind the paywall, where prose often
   carries exact control labels and dialog field order independently of any
   image. What remains unowned is the work of doing it, not a licence.
   One rule for whoever does: Onionskin implements these features fully
   functional with no gate, so a locked panel is captured for its structure,
   naming, grouping and layout, and explicitly not for its lock affordance or
   purchase call to action.
2. **Capture cannot be automated into CI.** The assertion half of layer 2 runs
   in CI today. The capture half needs a physically present machine with an
   awake display, which is exactly what blocked the incomplete M2 states:
   capture permission was granted and WindowServer still rejected the window
   rectangle against sleeping displays. Look-and-feel acceptance inherits a
   manual step that no amount of design removes.
3. **Motion is the weakest layer, by a distance.** There are no motion constants
   in the shell, no animation code, and no way to read timing out of the
   accessibility tree. Layer 3 asks a person whether it felt right. That is
   genuinely weak evidence and it is the only evidence there is until someone
   builds a frame-capture harness, which nothing plans.
4. **Numbers do not add up to a feeling.** Two surfaces can match every
   measurement and still read differently, because of what neither layer
   captures: rhythm across surfaces, the cumulative effect of small
   inconsistencies, whether a thing looks finished. Layer 3 exists for this and
   layer 3 is the softest instrument in the project.
5. **The measure will make the number go down.** Re-pinning to a newer Acrobat
   version marks surfaces `stale`. Sourcing a token that was previously assumed
   can reveal that several accepted surfaces were measured against a guess. This
   is the design working, and it should be said before it happens rather than
   explained afterwards.
6. **1.0 is not full parity, and neither board says so today.** 266 of 323 is
   the pre-1.0 functional ceiling. The look-and-feel denominator at 1.0 is also
   smaller than the final one, because surfaces Acrobat has and Onionskin has
   not yet built are counted in the denominator from the start, so the
   percentage will look worse than the work suggests. That is correct and should
   not be smoothed.
7. **42 functional rows sit on guessed milestones.** The map is exhaustive and
   13% of it is unratified.
8. **The icon axis will be scored generously and should be read sceptically.**
   "Same grid, same weight, same metaphor" is a real standard, and it is a
   weaker standard than the rest of the measure, because two icons can satisfy
   all three and still not read as the same family. This is a cost of the legal
   posture, accepted deliberately.
9. **This document defines the measure; it does not build it.** Nothing in
   `LOOK-AND-FEEL.md`, `docs/evidence/lf-tokens.md`,
   `docs/evidence/lf-surfaces.md` or `docs/spikes/lf-first-five-minutes.md`
   exists yet. Until they do, the look-and-feel number is not zero, it is
   unmeasured, and those are different.

---

## 8. Is full parity achievable

Yes for capability, with a large and knowable amount of work. Yes for look and
feel in the sense the project has defined it, with a much smaller amount of
work, one purchasing decision and one design effort. No for a small, precisely
identifiable set of rows that nobody outside Adobe can build.

Orders of magnitude, in units the project has already measured. These are shapes,
not estimates anyone should schedule against.

**Functional, remaining: 274 rows, of order 10^5 more lines of Rust.** 75,596
lines bought M2's 67 rows plus the whole foundation. The foundation amortises,
but M3 alone adds an edit graph, a save engine, a structure tree, page-tree
transformation, annotations and a print pipeline against 99 rows, and M5's text
editing is the item `PLAN.md` calls the famous tar pit. The remaining work is
the same order as what exists, probably two to four times it. Not 10^4, and not
10^6. Distribution: 18 rows to finish M2, 99 in M3, 2 in M4, 52 in M5, 46 in M6,
57 after 1.0. The 57 post-1.0 rows contain the genuinely open-ended items (OCR,
bidi and vertical text shaping, reflowing text edit, portfolios, Compare Files),
and treating them as a tail rather than a milestone is correct.

**Look and feel: of order 10^4 lines plus a design effort.** Broken down:

| Item | Order | Note |
|---|---|---|
| Token scales and migrating 240 `px()` call sites | 10^3 lines | days, not months; best done with M3's P0 |
| Icon set: SVG pipeline plus roughly 150 to 250 icons | 10^3 to 10^4 lines | the largest item, and the only one needing design time rather than engineering time |
| Geometry ledger: 23 surfaces at 10 to 25 measurements each | 10^3 lines of test | rides on an existing harness |
| The script and the ledgers | 10^2 to 10^3 lines of prose | plus 30 to 60 minutes of human time per release |
| Reference captures | hours | one session per Acrobat version; the free build exposes the Pro toolsets, so no licence gates it |

That total is one to two orders of magnitude smaller than the remaining
functional work. The half nobody is tracking is the cheap half. It is also the
half that decides whether someone opening Onionskin for the first time
recognises where they are, which is the product goal.

**What is unreachable, and how much of it.** Of the 403 rows, roughly 36 (about
9%) cannot be built by anyone but Adobe: the 33 cloud- and account-tethered
rows, plus Reader-extended PDF twice and the AATL/EUTL trust programme, which
are cryptographically gated on Adobe's signature. A further 44 are buildable and
declined: prepress needs a colour engine and certification, rich media and 3D
need bundled playback and geometry engines, Distiller needs a PostScript
interpreter, web capture needs an HTML engine, Office import needs three format
readers. Those are cost refusals, and every one of them is a defensible product
decision rather than a limit.

On the look-and-feel side, exactly one thing is permanently unreachable and it
is unreachable by choice: **pixel identity, because the icons and the typeface
must differ.** This is why the measure is built as tokens plus geometry plus a
script and not as an image diff. An image diff would measure, with great
precision, a gap that the project has decided to keep.

The honest summary: full parity is achievable, the functional half is a
multi-year body of work with a clear map and a known 82% ceiling at 1.0, the
look-and-feel half is small and currently unmeasured, and about 9% of Acrobat's
surface is permanently closed to any implementation that is not Adobe's.

---

## 9. What this document found in `PLAN.md` and `ACROBAT-PARITY.md`

Recorded here rather than fixed, since both files are outside this document's
scope. Each is a proposed edit for the next plan revision.

| # | File | Finding |
|---|---|---|
| 1 | `PLAN.md` testing strategy item 6 | "UI parity itself is checked visually" is the project's only look-and-feel gate and is satisfiable by assertion. Replace with a reference to this measure. |
| 2 | Both | The plan commits to redrawn in-house SVG icons; the shipped icons are five text glyphs in a `match` with a `"?"` fallback, and no SVG asset or pipeline exists. Two parity rows read `implemented` over it. Nothing tracks the gap. |
| 3 | `ACROBAT-PARITY.md` Totals | The remaining-work split omits M2's own 18 unfinished rows, leaving a reader who sums the milestones an unexplained 18-row gap. Add "M2 18". |
| 4 | Both | "Full parity" is undefined, and 323 rows `implemented` is unreachable because five export rows are declared partial forever. Adopt a terminal-status definition (section 3.1). |
| 5 | `ACROBAT-PARITY.md` | `Revert to the classic Acrobat interface` is `out-of-scope` while `PLAN.md` says classic may become a theme. Parked, not refused: move the row or drop the plan sentence. |
| 6 | `ACROBAT-PARITY.md` | The XFA and document-JavaScript rows each owe an unshipped user-visible notice. By the file's own counting convention those notices are rows, and they are missing from the board with no milestone. |
| 7 | `ACROBAT-PARITY.md` | `Catalog` is refused on the scoreboard's own authority, which the file itself flags. Needs plan ratification. |
| 8 | `ACROBAT-PARITY.md` | Office import is refused while Office export is a post-1.0 target. Defensible, but the asymmetry should be stated in the row. |
| 9 | `PLAN.md` Milestones | Nowhere states that 1.0 tops out at 266 of 323 in-scope rows. A reader can reasonably assume 1.0 means parity. |
| 10 | `PLAN.md` GUI parity bullet | "Acrobat Pro screenshots/trials cover the Pro-only toolsets" reads as though a licence were required. The free build exposes those toolsets in its own interface, so the precondition is a capture session rather than a purchase. Still unowned and unscheduled, but not blocked. |
| 11 | `docs/plans/m2-viewer.md` section 6 | Deferred pixel parity with "file a follow-up issue at the end of M2". No issue exists. This document is that follow-up and the M2 plan should link it. |
| 12 | `PLAN.md` decision 12 | Accessibility is justified on Section 508 and EAA grounds. It is also the reason visual measurement is nearly free here, since the a11y tree already carries a rectangle for every named control in a CI-gated harness. Worth a line, because it changes how the investment reads. |
| 13 | `ACROBAT-PARITY.md` preamble | The board does not say that its number can go down when Adobe re-pins. Both boards have that exposure and only the risks section mentions it. |
