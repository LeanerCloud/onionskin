# M3 implementation plan: first edits, first print

Status: planning. No M3 code is written. This document is the authoritative
decomposition; it supersedes PLAN.md's M3 paragraph wherever the two disagree,
and section 8 lists every disagreement rather than resolving it silently.

M3 is the milestone where Onionskin stops being a viewer. It is the first time
the product writes a byte, and every architectural claim the project has made
about non-destructive editing becomes an executable fact or an embarrassment.
It is also the largest milestone by scoreboard weight: 99 `ACROBAT-PARITY.md`
rows against M2's 67.

Scope boundary, held throughout: `crates/mcp`, `crates/cli`, `crates/scripting`,
`crates/text-engine` and `plugins/{tools-edit,tools-form,tools-fill-sign,redact,
tools-protect,tools-accessibility,tools-measure}` are M4 and later and get no
code here. `cargo test -p onionskin-app --no-default-features` (guarantee 5)
must stay green at every commit, and no crate outside `crates/app` may import
GPUI. `crates/print` is GPUI-free; `app` supplies only the dialog.

---

## 1. Ground truth at plan authoring

Verified against `main` at `81f802f` on 2026-09-02. Four sibling agents are
closing M2 out on branches that are not merged; where their work changes a fact
below, the fact is marked.

| Component | State | The part that matters for M3 |
|---|---|---|
| `crates/cos` | 8152 lines, real | Already has the whole write side: `set_object`, `add_object`, `delete_object`, `set_trailer_entry`, `set_info_field`, `has_pending_changes`, `incremental_section`, `save_to_writer/path/vec`, `original_len`. Edits accumulate in one `Vec<Edit>` and **one call to `incremental_section` emits one section carrying all of them**. There is no way to withdraw a pending edit, no section-chain accessor, no `flatten` full-rewrite API, and no `next_object_number` accessor. `Document` is `!Send` (`Rc<ObjectStream>` at `document.rs:109`). |
| cos deletion | real, and the landmine | `delete_object` splices a chained free list at the head, refuses object 0 and refuses the trailer's `/Root`. It performs **no reference walk**: deleting a `/Pages` node, a page still in a `/Kids` array, a content stream, or an annotation's appearance stream leaves a dangling reference and cos will happily serialize it. The doc comment at `document.rs:868` names M3's `tools-organize` as the caller that must fix this up. |
| `crates/core` | 7257 lines, real session | `core::Document` (there is no `Session` type) holds `bytes: Arc<Vec<u8>>`, a private `cos::Document` it never mutates, page geometry and text caches, selection, search, the render worker handle, and the four read-only pane readers. **No edit graph, no history, no save.** `history.rs` is view history and says so in its own doc comment. `ExportSnapshot` is the only state-replay mechanism and it replays layer visibility only. |
| `crates/plugin-api` | 752 lines, real | `ToolPlugin` with its full gesture lifecycle, `CommandPlugin`, `CodecPlugin` (export only), `PluginRegistry`, `ToolCtx { doc, viewport }`, `ToolCapability` (7 variants), `Overlay` (6 variants). **A tool has no way to express a document edit.** Its own module doc says the import path "waits for the edit graph ... which is M3". `Requirement` is not here: it is a private four-variant enum in `crates/app/src/shell/context_menu.rs:47`. |
| `crates/render` | 2807 lines | Renders from `Arc<Vec<u8>>` through hayro; `render_annotations` is a settings bool; `TileStore` evicts. Annotation appearance streams render. hayro's annotation loop **never reads an annotation's `/OC`**, only the `/F` hidden flag. |
| `crates/content` | 13454 lines | `extract_page`, `PageText`/`TextRun`/`Glyph`/`Mapping`/`ByteProvenance`, `PageQuad`. This is where a highlight's quad points come from. Nothing writes. |
| `crates/print` | 6 lines, doc comment only | Workspace member, `onionskin-print` in `[workspace.dependencies]`, **no `[dependencies]` section at all**. Greenfield. |
| `crates/crypto` | 4 lines, doc comment only | No security handler, no RC4, no AES, no key derivation. `cos` raises `Error::Encrypted` from one `refuse_encrypted` check at four call sites. |
| `plugins/tools-comment` | 20 lines | Manifest with a no-op `register`. Already a default cargo feature and already installed by `build_registry`. |
| `plugins/tools-organize` | 19 lines | Same shape. |
| `plugins/commands-core` | 214 lines | Registers exactly two commands, `edit.select-all` and `edit.deselect-all`, both of which only touch in-memory `Selection`. |
| `crates/app` | 32896 lines under `src/`, of which `shell/chrome/tabs.rs` is **7278** and `shell/canvas.rs` is 5003 | `ShellFrame` in `tabs.rs` is the single top-level GPUI view: 24 fields, one `impl` block spanning lines 371 to 2765 with 83 methods, plus the export worker. Every new command, dialog, pane toggle and accessibility node lands in it. |
| corpus | seeds 3, external 3300+, malformed 15 (gitignored, generated), bench 1000-page (gitignored), **`tagged/` is a README and nothing else** | Guarantee 8's fixtures do not exist. `verapdf/PDF_UA-1` and `PDF_UA-2` are the named raw material. |
| guarantees | `crates/app/tests/guarantees.rs` 1, 2 and 6 are `#[ignore]`d and `unimplemented!()` on `main` | The capabilities they name are already proved one layer down in `crates/cos/tests/{roundtrip,incremental,repair}.rs`. See section 8, item 2, for what M3 actually owes them and why guarantee 6 was vacuous. |

Two structural facts that shape several packages:

- **Nothing in the workspace mentions `/StructTreeRoot`, `/ParentTree`,
  `/StructParents`, `/MarkInfo` or `MCID`.** A repository-wide grep returns one
  incidental comment in `crates/content/src/interpret.rs:33` about marked-content
  nesting, used for optional content and not for tags. Decision 12's document
  half is entirely unbuilt.
- **`crates/app/Cargo.toml` already lists `tools-comment` and `tools-organize`
  as default features and `build_registry` already installs both manifests.**
  Their tools appear the moment `register` stops being a no-op, and the quick
  action toolbar, canvas context menu and thumbnails context menu all resolve
  availability through `ToolCapability` queries rather than hardcoded lists. M3
  turns those rows live without touching the shell code that gates them, which
  was P7's and P10's whole point and is worth not squandering.

---

## 2. What M3 inherits

From `docs/audits/m2-forward-audit.md` and `known-issues.md`, the entries that
land on M3's desk rather than M2's:

| Inherited | Owner in this plan |
|---|---|
| cos refuses to delete `/Root` but leaves every other dangling reference; "M3's `tools-organize` has to fix up the page tree itself" | P1 (the refusal) and P5 (the transformation) |
| hayro's annotation loop ignores an annotation's `/OC`; `/VE` unsupported | P6 and P15: M3 authors no `/OC` on annotations and filters print output through `/F` instead |
| Only the copy loop of a save is bounded memory; a repaired or escalated document assembles a full table that materializes every compressed object | P3: the preview rebuild is per committed edit, so a repaired document pays that cost per edit. Benched, not assumed. |
| `cos::Document` is `!Send`; m2-viewer's candor item 7 predicted "M3's edit graph or M4's MCP sessions" would be the first to feel it | P3 and P18: autosave is the consumer |
| Line Weights moved from M2 to M3 with design guidance (constant hairline width, not a minimum-width floor) | P22 |
| Layers pane `Properties` disabled "Available in M3 with the properties dialog" | P13 |
| `Copy With Formatting` and `Export Selection As` disabled "Available in M3 with rich-text export" | P22, and see section 8 item 12 for why only half of it can ship |
| Find bar `Include Comments` disabled, "Comments arrive with the comment tools in M3" | P20 |
| The encryption class split (`permissions-only` versus password-protected) that m2-viewer section 6 assigned as a `known-issues.md` ledger action **was never written**. `known-issues.md` has no encryption entry at all. | Section 9, decision A: the measurement is unconditional |
| Textual CI tripwires in `guarantees.rs` are evadable by a softened harness (CR-005, REPO-011) | Section 3, T9: every guarantee M3 un-ignores states the mutation that must break it |
| `SearchResult::Unavailable::reason` is `&'static str`, to be widened "after P9 lands" | P20 (the comments filter needs a dynamic reason) |

Two M2 rows are `partial` **because** M3 has not shipped, and flip when it does:
the right-hand side panel ("tool-specific content starts at M3") and the quick
action toolbar (Comment, Highlight and Draw disabled). The page thumbnails
context menu is `implemented` with its page-mutating entries disabled on M3
reasons. Flipping all three is part of P20 and P21's definition of done, not a
follow-up.

---

## 3. The tensions, resolved

**T1. Undo cannot be truncation, and truncation is not undo.**

PLAN.md says two things that read as one and are not. The core invariant's point
2 says generations "roll back by truncation". "What does not transfer" item 1
and parity row 17 say undo is "dropping edit-graph overlay nodes". These are two
different operations at two different scopes, and conflating them produces a
design that is wrong the first time a user saves and then presses Ctrl+Z.

Truncation cannot be undo. It is wrong whenever the section being undone is not
the last thing in the file, wrong after a Save As (the truncation would apply to
a file the user did not edit), wrong when another producer appended after us,
and it destroys a generation the user may have deliberately kept. Appending a
reversing section cannot be undo either: ten undos would append ten sections,
which contradicts guarantee 2's "one incremental section", and it makes Ctrl+Z a
disk write.

**Resolution: two layers, sharply separated, with different names in the UI.**

1. **The edit stack** is `Ctrl+Z` / `Ctrl+Y`. It lives in memory in `core`,
   above the `cos::Document`, is session-scoped, and never touches disk.
   Acrobat's own undo stack is session-scoped too, so this is parity, not a cut.
2. **Generations** are the skins panel and `File > Revert`. They are the
   on-disk incremental sections. Rolling one back is a truncation, it is a
   named, explicit, user-visible operation, and it is not bound to a keystroke
   that means something else.

The edit stack carries a **saved mark**: the stack position at which the file
on disk was last written. Undoing past the saved mark does not truncate; it
makes the document dirty in the reverse direction, and the next save appends a
new section that expresses the reversal. That is one section per save, still,
and it keeps the on-disk history append-only, which is the entire point of the
invariant. The user who wants the bytes gone uses the skins panel.

**T2. What an edit is, and where the object numbers come from.**

`cos::Document` already accumulates `Edit::Set` and `Edit::Delete` and already
emits all of them as one section. What it cannot do is withdraw one. So cos is
**not** where the edit graph lives: `core` owns the authoritative overlay and
projects it onto cos only at save time.

```
core::edit::Overlay  =  BTreeMap<u32, ObjectState>       // net state, per object number
core::edit::ObjectState = Written(cos::Object) | Deleted
core::edit::Change   = { number, before: Option<ObjectState>, after: Option<ObjectState> }
core::edit::Entry    = { label: &'static str, changes: Vec<Change> }
core::edit::History  = { entries: Vec<Entry>, cursor: usize, saved_mark: Option<usize> }
```

Undo applies each `Change`'s `before`; redo applies each `after`. **This is
deliberately more than "dropping overlay nodes"**, and PLAN.md's phrasing is
wrong for two reachable cases: an edit that overwrites an object a previous edit
already overlaid (changing a highlight's colour twice) must restore the previous
overlay state rather than drop the node, and an edit that deletes an object that
existed in the original has no node to drop. Memory stays proportional to
changed objects, which is what PLAN.md actually cares about.

Object numbers: `core` allocates from its own reservation counter, seeded from a
new `cos::Document::next_object_number()`. An undo that drops an allocation
simply leaves a gap; PDF object numbers need not be dense. Nothing is handed to
cos until save, so cos's `add_object` is never called speculatively and its
unwithdrawable edit list is never polluted.

A **transaction** groups a gesture into one stack entry: an ink stroke is
hundreds of pointer events, one annotation, one `Ctrl+Z`. An annotation is
three object changes (the annotation dict, its appearance stream, the page dict
whose `/Annots` gained a reference) and one stack entry.

**T3. One section per save, and what a save of nothing writes.**

Guarantee 2 asks that "an edit appends one incremental section that truncates
away". The resolution is **one section per save, not per edit**, and cos already
behaves this way: the section is materialized from the net overlay at save time.
Ten edits then one save is one section carrying the net object writes of all ten.

Three consequences the plan states rather than discovers:

- **Edit, then undo, then save writes nothing at all.** The net overlay is
  empty, `has_pending_changes()` is false, and `save_to_writer` copies the
  original bytes with no appended section. That is guarantee 1's no-op rule
  applied to a document the user did edit, and it is a real test.
- **The net overlay collapses.** Adding an annotation and then deleting it in
  the same session leaves the page's `/Annots` back at its original value, so
  the page object is not in the overlay either, and the save writes nothing.
  Collapsing has to be by value comparison against the original object, not by
  a dirty flag, or the file grows a section that changes nothing.
- **After a save, `core` reopens the `cos::Document` from the written bytes.**
  This is cheaper than adding a `clear_edits` to cos, it makes the `/Prev`
  chain correct by construction for the second save, and it matches the
  existing `ExportSnapshot::open` reopen pattern. The caches keyed on edited
  pages are invalidated; the rest survive.

**T4. What the canvas shows before a save: preview bytes, not a second renderer.**

An unsaved annotation has to appear on the page. hayro renders from a byte
buffer, so there are two ways: composite the pending edits as `render::Overlay`
primitives, or hand hayro `original ++ pending section`.

**The preview buffer wins, and it is the strongest single decision in this
plan.** `incremental_section()` already builds exactly those bytes. Rendering
from them means hayro draws every M3 annotation through its own appearance
stream path, with no second renderer to keep in agreement, and it means what
the user sees is byte-for-byte what a save would produce. A whole class of
"the preview disagrees with the saved file" bugs cannot exist.

The split with overlays is clean: `Overlay` covers the **in-progress gesture**
(the rubber band, the ink stroke still under the stylus, the marquee), which is
what `ToolPlugin::overlays` already returns and what tools-basic already uses.
Committed edits go through the preview buffer. A tool never draws its own
committed result.

Cost, stated up front: a committed edit rebuilds the section, allocates a new
`Arc<Vec<u8>>`, and the render worker builds a new hayro `Pdf` from it. `Pdf::new`
was measured at about 1.4 microseconds per page in the M1 spike, so a
1000-page document is about 1.4 ms per commit. **The real risk is a repaired
document**, where `needs_full_table()` forces a section that walks every live
object and materializes every compressed one. That is per commit, not per save.
P3 benches it and, if it misses budget, debounces preview rebuilds on repaired
documents behind a named constant rather than pretending the cost is not there.

**T5. Page organisation: rewrite the tree, do not surgically edit it.**

`cos::delete_object` does no reference walk. Deleting a page leaves its entry in
a `/Kids` array; deleting a `/Pages` node orphans a subtree; `/Count` on every
ancestor goes stale; a page moved out from under an internal node loses whatever
`/Resources`, `/MediaBox`, `/CropBox` or `/Rotate` that node was supplying by
inheritance. Surgical fixes to that graph are where correctness goes to die.

**Resolution: every `tools-organize` operation is expressed as one
transformation, "rewrite the page tree as a single flat `/Pages` node listing
the surviving pages in their new order".** Each surviving page dict is rewritten
with its inheritable attributes materialized before flattening and its `/Parent`
pointing at the new node. The new node reuses the original root `/Pages` object
number, so `/Root /Pages` needs no change and the catalog is untouched.

Why this is right rather than lazy:

- It removes the dangling-reference class entirely instead of guarding against
  it. The old internal nodes become unreferenced, which under incremental update
  is exactly what they already are: bytes nobody points at, still on disk,
  recoverable by truncation. Garbage, never dangling.
- It is one function with one contract, testable against every corpus file with
  a nested page tree, rather than nine operations each with their own graph walk.
- Inheritance materialization is the same code every operation needs anyway.
- The cost is a rewritten page dict per page. On a 1000-page document that is a
  1000-object section. P5 measures it and states the number; if it is
  unacceptable the fallback is to keep the tree shape and rewrite only the
  ancestors on the changed paths, which is strictly more code and strictly more
  risk, so it is a fallback and not the plan.

What the transformation must also carry, and what an adversarial reviewer will
check it forgot: `/Annots` (an annotation belongs to a page, and a deleted page's
annotations must go), `/PageLabels` (a number tree keyed on page index, which
every reorder invalidates), named destinations and the outline's `/A` and `/D`
entries (a bookmark to a deleted page is a broken bookmark, not a parse error),
`/StructParents` and the structure tree (T6), and page-level `/B`, `/Tabs` and
`/Group`, which are per-page and survive untouched.

**T6. What M3 owes the tagged structure tree, even though guarantee 8 is M5.**

Decision 12 puts the tagged-PDF structure tree in `core` and says every edit
keeps it valid. PLAN.md assigns guarantee 8 to M5. PLAN.md's M3 paragraph says
nothing about tags at all. Left alone, that reads as "M3 may ignore it", and
that reading is expensive in a specific way: M5 would inherit a shipped M3 whose
page deletion and annotation authoring silently broke the tree, would have to
revisit every M3 tool, and would have to explain to users why documents edited
in earlier builds fail the accessibility checker Onionskin itself ships.

**M3 builds the minimum that makes M5 a checker rather than a repair job:**

1. `core::structure` reads `/StructTreeRoot`, its `/ParentTree` number tree,
   per-page `/StructParents` and per-annotation `/StructParent`, and reports
   honestly that a document has no structure tree (most do not).
2. Every `DocumentEdit` passes through one maintenance hook. Page deletion
   removes the deleted page's structure elements and their `/ParentTree`
   entries. Page reorder reorders the corresponding `/K` sequence, because a
   reading order that no longer matches visual order is precisely the
   accessibility failure this project claims to fix. Page insertion allocates
   fresh `/StructParents` indices.
3. Annotation authoring on a tagged document adds an `/Annot` structure element
   under the page's structure and a `/StructParent` on the annotation, which is
   what Acrobat does and what makes a comment reachable by a screen reader.

M3 does **not** build: the rule-based checker, reading-order repair, or
autotagging. Those stay M5 and M6.

M3's own executable check, available now and cheap: after every M3 edit on a
tagged fixture, the `/ParentTree` is a well-formed number tree, every
`/StructParent` and `/StructParents` index resolves through it, and no `/K`
entry references a removed page or a freed object. That is an invariant, not a
checker, and it bites without waiting for M5.

Prerequisite: `corpus/tagged/` is a README. P4 populates it from
`verapdf/PDF_UA-1` and `PDF_UA-2`, which `corpus/README.md` already names as the
raw material.

**T7. Annotations are appended objects, and print needs a filter hayro cannot give.**

Every comment tool writes an annotation dict, an appearance stream, and a
rewritten page `/Annots`. Onionskin generates the appearance stream itself
rather than relying on a viewer to synthesize one: Acrobat writes `/AP`, hayro
renders `/AP`, and an annotation without one renders differently in every
reader. `/AP /N` is a Form XObject, which is ordinary content-stream authoring.

The gap M3 inherits: hayro's annotation loop never reads an annotation's `/OC`.
For M2 that shaped the Layers pane. For M3 it shapes **printing**. Parity row
94 wants "Document / Document and Markups / Document and Stamps / Form Fields
Only", which is a per-subtype filter, and hayro offers one `render_annotations`
bool. Authoring `/OC` on annotations to express it would not work, because hayro
would ignore it.

**Resolution: the filter is a render-time overlay that sets `/F` bit 2 (Hidden)
on the excluded annotations, applied to a preview buffer built for that render
and never saved.** `/F` is honoured by hayro's annotation loop and by every
other reader. It composes with T4's preview mechanism at zero additional
machinery: the print path asks `core` for a preview buffer with a stated
annotation filter, and gets bytes. The same mechanism gives the Comments pane a
"hide all comments" view for free.

Consequence to state plainly: M3 authors **no** `/OC` on any annotation. If a
later milestone wants layer-controlled annotation visibility, it needs the
upstream hayro fix, and the `known-issues.md` entry stays.

**T8. Documents Onionskin authors have nothing to append to.**

Combine files, split, extract-to-file, create-from-image and the comment summary
all produce a **new** document. The core invariant has no clause for this: there
is no original underneath, and cos has no write-from-scratch API (its charter
names a `flatten` full rewrite that does not exist).

**Resolution, and a proposed amendment to the invariant's wording:** a document
Onionskin authors is written complete on its first save, and the invariant
applies from that point forward. The file Onionskin wrote is the base sheet.
This is not an exception to non-destructiveness, because nothing existed to
destroy; it is the only sane reading, and PLAN.md should say it.

Mechanically, this needs `cos` to be able to serialize a whole document, which
is `flatten` under a different name. P1 builds it as
`Document::write_new(objects, trailer) -> Result<Vec<u8>>` used by P12, P14 and
the comment summary, and it is the same primitive `redact` and compress will
use at M5. Naming a real M3 consumer is what lets it exist now (§ YAGNI).

**T9. Verification discipline, learned expensively in M2.**

Every package's verification section obeys these, and they are stated once here
rather than repeated fourteen times:

- **Feature configurations are named, never assumed.** `cargo test --workspace`
  does **not** compile `crates/app/src/shell`: it is behind the `shell` feature,
  off by default. Any package touching the shell states
  `cargo test -p onionskin-app --features shell` and, for anything driving a
  window, `cargo test -p onionskin-app --no-default-features --features
  shell,shell-test-support` plus the matching clippy invocation. Kernel packages
  state `cargo test -p onionskin-core` and
  `cargo test -p onionskin-app --no-default-features` (guarantee 5).
- **Keyboard routes are proven with `cx.simulate_keystrokes` on a real
  window.** Calling the handler proves nothing; this project shipped a dead
  Ctrl+F twice. A global action listener body that calls `window_handle.update()`
  fails with "window not found" and must be wrapped in `cx.defer`. Every new M3
  command with a keystroke gets a test in the shape of
  `the_find_keystroke_opens_the_find_bar` (`tabs.rs:4396`).
- **Every test must fail when the behaviour it names is removed.** Each package's
  review-risk list names the specific mutation that must break its tests, and
  the reviewer runs it. A test that passes against a no-op is a defect.
- **Assert on structure, not on substrings of files.** Parse the PDF, walk the
  object graph, compare objects. A recent package needed four review rounds
  because it hand-rolled scanners instead of parsing. This applies to
  `guarantees.rs`'s workflow assertions too, which CR-005 already flagged as
  evadable tripwires.
- **Accessibility is live and every new control joins the tree.** A control
  becomes a tab stop iff its role is focusable, it is enabled, and it carries an
  activation (`a11y/tree.rs:164`). Focus order is depth-first over the tree the
  surface builds in its own `accessible()`, so **child order is tab order** and
  there is no separate registration. Anything hidden must leave the tree rather
  than linger as an invisible tab stop, and the click path and the keyboard path
  must dispatch the same `Activation` value.
- **Every package merges through an adversarial review gate that mutation-tests
  its claims**, per the repository's standing rule. The review-risk list is the
  reviewer's starting point, not its limit.
