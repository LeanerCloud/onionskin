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

---

## 4. Work packages

Format per package: goal, parity rows closed, files, depends-on, what exists to
build on, verification, review risk. Kernel packages close no rows directly and
say which rows they back. Section 6 is the complete 99-row ledger.

### P0. Split `ShellFrame`

**Goal.** Make the six M3 app packages able to run in parallel. This is not
tidying; it is the precondition for the schedule.

`crates/app/src/shell/chrome/tabs.rs` is 7278 lines with a single `impl
ShellFrame` block spanning lines 371 to 2765 (83 methods) and a 24-field struct.
M2's own audit named it the recurring conflict point. Every M3 app package adds
a field, a menu arm, a dialog call site and an accessibility child to that one
block. Six branches doing that concurrently is six rebases through a 2400-line
`impl`, and a rebase through an `impl` block resolves without judgement only
until two packages add a method with the same name.

The split is mechanical and behaviour-preserving:

- `ShellFrame`'s state moves into named sub-structs that already have module
  homes (`find`, `home`, `navigation`, `rail_state`, `quick_actions_state`,
  `side_panel_state` are already separate types; the loose fields are not).
- The `impl` block splits by concern into `impl ShellFrame` blocks in
  `chrome/{menu,dialogs,context,export,accessible}.rs`, which Rust permits
  across files within a crate via inherent impls in the same module tree.
- `run_main_menu_command`'s match and `canvas_context_entries`' match become
  the two extension points M3 packages append to, each in its own file.
- The export worker (about 4500 lines of free functions and tests after the
  `impl`) moves to `chrome/export.rs` unchanged.

**Rows closed.** None. This package changes no behaviour.

**Files.** `crates/app/src/shell/chrome/tabs.rs` (shrinks), new
`crates/app/src/shell/chrome/{menu,dialogs,context,export,frame_state}.rs`,
`crates/app/src/shell/chrome/mod.rs`.

**Depends on.** Nothing. Day-one root, and it must land before P17 through P22
start.

**What exists to build on.** `chrome/mod.rs` already re-exports `ShellFrame`
from `tabs`, so callers outside `chrome` see no change. `accessible.rs`,
`commands.rs` and the pane modules already demonstrate the target shape.

**Verification.**
- `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` green, with the **same test count** before and after. A split that changes a count either lost a test or added one, and neither belongs in a refactor.
- `cargo clippy -p onionskin-app --no-default-features --features shell,shell-test-support --all-targets -- -D warnings`.
- `git diff --stat` shows no line of moved code changed: verified by diffing the concatenation of the new files against the old file with whitespace and `impl` headers normalized, recorded in the PR.
- The accessibility probe (`--features a11y-probe --test a11y_probe`) still passes, so the tree assembly moved intact.

**Review risk.** Whether this is a refactor or a rewrite wearing a refactor's
name: the reviewer should reject any behaviour change, including "obvious"
improvements. Whether the split lines follow M3's package boundaries or the
author's taste, which is the difference between it buying parallelism and it
buying nothing. Whether `frame_state.rs` became a second god object.
**Mutation that must break its tests:** deleting any one moved method must fail
compilation in exactly the file that now owns its callers, not scatter errors
across five.

### P1. cos: the edit surface M3 needs

**Goal.** Four cos additions, landed together and reviewed once, that every
kernel package above depends on.

1. `Document::next_object_number(&self) -> u32`, so `core` can reserve numbers
   without calling `add_object` speculatively (T2). Trivial accessor, no new
   state.
2. `Document::sections(&self) -> Result<Vec<Section>>` where
   `Section { start: u64, end: u64, startxref: u64, prev: Option<u64> }`,
   walking the `/Prev` chain from `startxref` back to the first table. This is
   the skins panel's data source and there is nothing today: `original_len()`
   gives one boundary, not a chain. Must terminate on a cyclic `/Prev` (hostile
   documents do this) and must report a chain it cannot follow as a repaired
   document rather than silently truncating the list.
3. `Document::write_new(objects, trailer) -> Result<Vec<u8>>`: a complete
   document serialization, for the documents Onionskin authors (T8). Named
   consumers in M3: P12 (combine, split, extract), P14 (create from image,
   compress/flatten), P10 (the comment summary). This is the `flatten`
   primitive the crate's charter names; it lands now because it has three real
   callers, and M5's `redact` inherits it.
4. **A save-time reference validator.** `incremental_section` refuses to emit a
   section in which a written object references an object number the section or
   the file marks free. This is the standing "cos leaves dangling references"
   debt turned from a silent corruption into a typed error, and it makes P5's
   mistakes loud instead of latent. It walks only the objects the section
   writes, so it is proportional to the edit, not to the file.

**Rows closed.** None. Backs every row in P3, P5, P11, P12, P14 and P19.

**Files.** `crates/cos/src/document.rs`, `crates/cos/src/writer.rs`,
`crates/cos/src/lib.rs`, `crates/cos/src/error.rs`; new
`crates/cos/tests/sections.rs`, `crates/cos/tests/write_new.rs`; extend
`crates/cos/tests/delete.rs`.

**Depends on.** Nothing. Day-one root.

**What exists to build on.** `prev_startxref` is already a field
(`document.rs:103`). `writer::incremental_section` already builds classic xref
tables and already strips xref-stream-only trailer keys. The full-table path in
`incremental_section` (`document.rs:1055-1086`) already enumerates every live
object, which is most of `write_new`. `free_list_rows` already knows which
numbers a section frees.

**Verification.**
- `sections()` over every `external/` corpus file that has more than one `%%EOF`: the reported chain's byte ranges partition the file with no gap and no overlap, and the last section's `end` equals the file length. Name the fixtures; a file set chosen by "some corpus file" is not a proof.
- A hand-built fixture with a cyclic `/Prev` terminates and reports the cycle rather than looping.
- `write_new` output reopens through `Document::open` (not `open_repairing`), has the stated page count, and round-trips: `write_new` then `open` then `save_to_vec` with no edit is byte-identical.
- The validator: a test that deletes an object still referenced by a page dict and asserts `incremental_section` returns the typed error, plus a test that the *legal* case (deleting an object nothing references) still succeeds. Both are needed, or the validator could be a constant `Err`.
- `cargo test -p onionskin-cos`, `ONIONSKIN_CORPUS_REQUIRED=1 cargo test -p onionskin-cos`, and `cargo clippy --workspace --all-targets -- -D warnings`.

**Review risk.** Whether the validator's reference walk understands every place
an object number can appear (dict values, array elements, nested streams'
dictionaries) or only the shallow ones, which would make it pass on exactly the
cases P5 gets wrong. Whether `sections()` reports what it parsed or what is in
the file, given cos's laziness (the existing `recovered_boundaries` entry in
`known-issues.md` is the precedent, and this accessor must not repeat it).
Whether `write_new` invents a second serializer instead of reusing the writer.
**Mutation that must break its tests:** making the validator always return `Ok`
must fail the dangling-reference test; making `sections()` return only the last
section must fail the partition test.

### P1b. cos: empty-user-password decryption

**Conditional on section 9, decision A.** Planned in full so the decision can be
made on a real cost, not an estimate of an estimate. If the user rules against
it, only the measurement (first bullet) survives, as an unconditional item in P1.

**Goal.** Open the documents Acrobat opens without prompting.

1. **The measurement, unconditional.** Of the roughly 35 corpus files cos
   refuses with `Error::Encrypted` (5 pdf-association, 4 verapdf, 13
   hayro/custom, 12 hayro-corpus, 1 fuzzed, per `docs/spikes/m1-cos.md`), how
   many have an empty user password? m2-viewer assigned this as a
   `known-issues.md` ledger action and it was never written; `known-issues.md`
   has no encryption entry at all today.
2. `crates/crypto` gains the ISO 32000 standard security handler, read side:
   `/V` 1, 2, 4 and 5, `/R` 2 through 6; algorithm 2 and 2.A key derivation;
   algorithms 4 through 7 and 11 and 12 for password validation; per-object key
   derivation for RC4 and AES-128-CBC; the direct file key for AES-256;
   `/EncryptMetadata`; and the crypt filter dictionary (`/CF`, `/StmF`, `/StrF`,
   `/Identity`). Pure-Rust dependencies exist for all of it (`md-5`, `rc4`,
   `aes`, `cbc`, `sha2`).
3. `cos` decrypts strings and streams at parse time, keyed off the trailer
   `/Encrypt` and the file `/ID`. `refuse_encrypted` narrows to "encrypted and
   the user password is not empty", which stays a typed refusal naming M6.
4. **Editing is disabled at open on every encrypted document**, with a reason
   naming M6, because the write path stays M6. Disabling at open rather than at
   save is the whole reason this is coherent: the user never begins work they
   cannot keep.

**Rows closed.** Moves `Open an encrypted document` (currently M6) to `partial`
at M3, which is a scoreboard change outside M3's 99 and must be recorded as one.

**Files.** `crates/crypto/src/{lib,standard,algorithms,filters}.rs`,
`crates/crypto/Cargo.toml`, `crates/cos/src/{document,object,stream}.rs`,
`crates/cos/Cargo.toml`, new `crates/cos/tests/encrypted.rs`.

**Depends on.** Nothing. Runs parallel to everything, off the critical path.

**What exists to build on.** Nothing in-repo: `crates/crypto` is a four-line
doc comment. The spike's corpus tally gives the file set, and the refusal is a
single guarded predicate, so the wiring surface in cos is small.

**Verification.**
- Every corpus file with an empty user password opens, reports its known page count, and extracts text that matches `pdftotext` through the existing content oracle. Files with a non-empty user password still fail with the typed error.
- A password-protected fixture is **not** opened by an empty password, asserted, so the handler is not accidentally permissive.
- Round-trip: opening an encrypted file and saving with no edit is byte-identical (guarantee 1 must hold for this class too, and it is free, because a no-op save writes nothing).
- A save with a pending edit on an encrypted document is refused with a typed error naming M6, and the edit tools were already disabled at open, asserted in the app.
- Known-answer tests against the ISO 32000-2 algorithm vectors for each of `/R` 2, 3, 4 and 6.

**Review risk.** Whether AES-256 (`/R` 6) validation implements the full
hardened hash iteration or the simplified `/R` 5 form that some producers
accept, which would open some files and silently mis-key others. Whether
`/EncryptMetadata false` is honoured (the metadata stream stays plaintext).
Whether the string-decryption path covers strings inside object streams, which
are **not** separately encrypted because the container already was, and getting
that backwards corrupts every object-stream document. Whether the "editing
disabled" state is derived from the encryption state or from a flag someone can
forget to set. **Mutation that must break its tests:** returning the file key
unmodified as every object key must fail the RC4 and AES-128 fixtures while
still passing AES-256, which is exactly why the fixtures must cover all four
revisions.

### P2. `core::edit`: the edit graph, transactions and undo/redo

**Goal.** The spine of M3. One place that knows what has changed, one stack that
can take it back, and one typed vocabulary of edits that every plugin speaks.

Shapes, per T2:

```rust
pub enum ObjectState { Written(cos::Object), Deleted }
pub struct Change { number: u32, before: Option<ObjectState>, after: Option<ObjectState> }
pub struct Overlay { states: BTreeMap<u32, ObjectState>, next_number: u32 }
pub struct Entry { label: &'static str, changes: Vec<Change> }
pub struct History { entries: Vec<Entry>, cursor: usize, saved_mark: Option<usize> }
pub enum DocumentEdit { /* the typed vocabulary, grown by P5 and P6 */ }
```

`DocumentEdit` is a **closed enum in `core`**, and that is deliberate. Plugins
never write COS objects. Three reasons, each with a named beneficiary: it puts
the structure-tree maintenance hook (T6) at one choke point instead of in every
tool; it makes the registry-exhaustive "every edit is undoable and every edit
serializes" property test meaningful rather than tautological; and it is
exactly the structured verb set M4's MCP surface needs, which "what does not
transfer" item 7 already committed to. The cost is that a third-party plugin
cannot invent an edit, which is not a cost until `plugin-host-wasm` exists
post-1.0.

P2 lands the machinery plus two variants to prove the shape end to end
(`SetObjectDictEntry` and `DeleteObject` are not it: something real, and
`SetInfoField` plus `SetTrailerEntry` are the two cos already supports, which
`File > Properties` will use). P5 and P6 grow the enum.

Transactions: `EditSession::transact(label, |tx| ...)` collects changes into one
`Entry`. An aborted transaction leaves the overlay untouched, including any
object numbers it reserved.

**Rows closed.** None. Backs rows 2, 17 and every editing row in M3.

**Files.** New `crates/core/src/edit/{mod,overlay,history,verb}.rs`,
`crates/core/src/lib.rs`, `crates/core/src/session.rs` (the `Document` gains an
`edit: EditSession` and the accessors).

**Depends on.** P1.

**What exists to build on.** `cos::Object` is the object model and needs no
change. `cos::Document::get`/`resolve` read the base state a `Change`'s `before`
is computed against. `core::history::ViewHistory` is the shape to copy for the
cursor semantics and is explicitly not the thing to extend (its own doc comment
says the edit history "will own its own undo stack").

**Verification.**
- Headless property test over a generated sequence of edits: apply N edits then undo N leaves the overlay byte-identical to empty, for N up to a few hundred, including sequences that overwrite the same object repeatedly and sequences that delete an original object. **This is the case PLAN.md's "drop the overlay node" phrasing gets wrong**, so it is the case the test must cover explicitly and by name.
- Redo after undo restores exactly; a new edit after an undo truncates the redo tail, asserted.
- The saved mark: it survives undo and redo, and moves only when P3's save moves it.
- An aborted transaction leaves both the overlay and the reservation counter unchanged, so an abort cannot leak an object number.
- `cargo test -p onionskin-core` with no window and no `shell` feature; `cargo test -p onionskin-app --no-default-features` still green.

**Review risk.** Whether `Change::before` is captured from the overlay or from
the base document, which are different whenever an object has already been
edited, and getting it wrong makes exactly the second edit of an object
un-undoable. Whether the overlay collapses by value against the original (T3) or
by a dirty flag, which would make edit-then-undo-then-save append an empty
section. Whether `DocumentEdit` grew a `Raw(Change)` escape hatch, which would
delete the entire justification for the closed enum. Whether `History` is
unbounded (a 200-page ink session is a lot of `Entry`) and whether its bound, if
any, is a named constant derived from something. **Mutation that must break its
tests:** replacing undo with "drop the last overlay node" must fail the repeated-
overwrite sequence; making the overlay collapse a no-op must fail the
edit-then-undo-then-save test in P3.

### P3. `core::save`: one section per save, generations, and the preview buffer

**Goal.** Turn the overlay into bytes, and make what the user sees be what the
save writes.

Four pieces:

1. **Save.** Project the net overlay onto the `cos::Document` through
   `set_object` / `add_object` / `delete_object`, call `save_to_path`, reopen
   from the written bytes (T3), advance the saved mark, invalidate the caches
   for edited pages. `Save As` is the same with a different destination and no
   truncation relationship to the original. A save whose net overlay is empty
   writes nothing at all.
2. **The preview buffer** (T4). `Document::preview_bytes(&mut self, filter:
   AnnotationFilter) -> Result<Arc<Vec<u8>>>` returns `original ++
   incremental_section()`, cached per overlay generation. The render worker
   renders from it. The filter argument exists for T7's print path and for the
   Comments pane's hide-all view, and has exactly those two consumers.
3. **Generations.** `Document::generations() -> &[Generation]` over P1's
   `sections()`, plus `revert_to(generation)`, which truncates and reopens.
   Reverting is refused, loudly, if the document has unsaved edits, or if the
   target is not a trailing section.
4. **Autosave.** A periodic write of the overlay to a recovery file beside the
   config directory, not to the document. `cos::Document` is `!Send`
   (m2-viewer's candor item 7, now due), so the overlay is what crosses the
   thread boundary, not the document: autosave serializes the overlay's changes
   and the recovery path replays them into a freshly opened document. Named
   here because the alternative, an `Rc` to `Arc` swap in cos, is a bigger
   change than the feature justifies.

**Rows closed.** None directly; P18 surfaces them. Backs rows 2, 3, 7, 10, 11,
13, 17 and guarantees 1 and 2.

**Files.** New `crates/core/src/save.rs`, `crates/core/src/preview.rs`,
`crates/core/src/generations.rs`, `crates/core/src/recovery.rs`;
`crates/core/src/session.rs`, `crates/core/src/render.rs` (the worker takes
preview bytes), `crates/core/benches/save.rs`.

**Depends on.** P1, P2.

**What exists to build on.** `cos::incremental_section`, `save_to_path` with its
temp-file-and-rename and permission preservation, and `has_pending_changes` are
all real and tested. `ExportSnapshot::open` is the reopen pattern. The render
worker already builds its `render::Document` from an `Arc<Vec<u8>>` inside its
own spawned closure, so feeding it preview bytes is a new `Arc`, not a new
threading model.

**Verification.**
- **Guarantee 1, at the level the guarantee means it.** Open every corpus seed and every `external/` file through `core::Document`, save with no edit, assert byte-identical output and that `sections()` reports the same count as before. The `#[ignore]` on `a_save_with_no_edit_is_byte_identical_to_the_original` is deleted here, and the app-level test drives `core`, not `cos` (see section 8, item 2).
- **Guarantee 2, driven by a real edit.** Make an edit through `EditSession`, save, assert the output is `original bytes ++ exactly one section`, that truncating at `original_len()` yields the byte-exact original, and that the truncated file reopens with the pre-edit content. Then make ten edits and one save and assert it is still exactly one section, which is the clause PLAN.md leaves ambiguous.
- **Edit, undo, save writes nothing.** Byte-identical output, zero appended sections. This is the test that catches a dirty-flag overlay.
- Two saves produce two sections and the second's `/Prev` points at the first, asserted by parsing the trailers, not by scanning for the string `/Prev`.
- Preview: after a committed edit, `preview_bytes` parses as a valid PDF through `cos::Document::open` (not `open_repairing`), and its object graph equals what the subsequent save writes, compared object by object.
- Bench, in `crates/core/benches/save.rs`, with a stated budget in the P14-era harness shape: preview rebuild after one edit on a clean 1000-page document, and on a **repaired** document, where `needs_full_table()` forces a full-table section. The repaired number is the one that decides whether preview rebuilds need debouncing, and this plan does not guess it.
- `revert_to` on a document with unsaved edits is refused; on a non-trailing generation it is refused; on a trailing one it truncates and the reopened document matches the pre-save state.

**Review risk.** The highest-consequence package in M3. A reviewer will probe:
whether "one section" is asserted by parsing or by counting `%%EOF` occurrences
(the original file may legitimately contain one already); whether the reopen
after save leaves any cache holding a pointer into the old parse; whether the
preview cache is keyed on something that actually changes with every edit;
whether `revert_to` can be reached with the render worker still holding the
truncated bytes; whether autosave's recovery replay can double-apply an edit
that was also saved; whether a `Save As` leaves the generations list describing
the old file. **Mutation that must break its tests:** removing the empty-overlay
short circuit must fail the edit-undo-save test; emitting one section per edit
instead of per save must fail the ten-edits test; skipping the reopen after save
must fail the two-saves `/Prev` test.

### P4. `core::structure`: the tagged-PDF structure tree

**Goal.** Build what T6 says M3 owes: read the tree, keep it valid through every
M3 edit, and prove that with an invariant rather than with M5's checker.

- Readers for `/StructTreeRoot`, the `/ParentTree` number tree, per-page
  `/StructParents`, per-annotation `/StructParent`, `/StructParentsNext` on the
  root, `/MarkInfo`, and the `/K` element hierarchy. Depth-capped and
  cycle-guarded, like every other reader in `core`.
- A maintenance hook that every `DocumentEdit` passes through, with three
  operations M3 needs: remove a page's elements and their `/ParentTree` entries;
  reorder the `/K` sequence to match a new page order; attach an `/Annot`
  element with a fresh `/StructParent`.
- An honest "this document is not tagged" path, which is most documents, where
  every operation is a no-op and says so.
- The M3 invariant, callable from every editing package's tests: after an edit,
  the `/ParentTree` is a well-formed number tree, every `/StructParent` and
  `/StructParents` index resolves through it, and no `/K` entry references a
  removed page or a freed object number.
- `corpus/tagged/` gets populated, since it is a README today.
  `verapdf/PDF_UA-1` and `PDF_UA-2` are the raw material `corpus/README.md`
  already names.

Not in M3: the rule-based checker, reading-order repair, autotagging, `/RoleMap`
resolution beyond what the invariant needs.

**Rows closed.** None. Backs guarantee 8, which lands at M5, and every editing
row's correctness on a tagged document.

**Files.** New `crates/core/src/structure/{mod,read,maintain,invariant}.rs`,
`crates/core/src/edit/verb.rs` (the hook), `corpus/tagged/README.md`,
`corpus/fetch.sh` (the tagged set), `crates/core/tests/structure.rs`.

**Depends on.** P2.

**What exists to build on.** `outline.rs` is the model for a cycle-guarded,
depth-capped reader over a cos object graph reached through `catalog()` and
`resolve`, including its error reporting shape. Nothing else: this is greenfield
(section 1).

**Verification.**
- Per tagged fixture, named by filename: the reader recovers the element count and the page-to-element mapping that a reference implementation reports. veraPDF's own output is the oracle; where it is not available for a fixture, the fixture's expected values are recorded in the test and their provenance stated.
- The invariant **fails** on a deliberately broken fixture: one built by deleting a page's objects without the hook. That is the test that proves the invariant is not vacuous, and it is the test the M2 audit's guarantee-6 lesson demands.
- An untagged document: every maintenance operation is a no-op, the invariant passes trivially, and the code path is asserted to have been taken (not inferred from the result).
- A document with a `/ParentTree` whose `/Nums` are not sorted, and one with a cyclic `/K`, both terminate.
- `cargo test -p onionskin-core`; `ONIONSKIN_CORPUS_REQUIRED=1` for the tagged set, and the CI step that fetches it lands with this package, or the suite skips silently and measures nothing.

**Review risk.** Whether the invariant can pass on a tree the edit destroyed,
which is the whole guarantee-6 failure mode repeated: it must be run against a
known-broken input in the same suite. Whether `/ParentTree` renumbering
preserves the mapping or merely produces a well-formed tree that points at the
wrong elements (well-formed and wrong is the dangerous state). Whether the
reader holds the whole tree in memory on a 50k-element document. Whether the
"untagged" path is distinguishable from "tagged but unreadable", which are
different and must not both silently no-op. **Mutation that must break its
tests:** making the invariant return `Ok` unconditionally must fail the
broken-fixture test; making the reorder hook a no-op must fail the reading-order
test in P11.

### P5. `core::pages`: the page-tree transformation

**Goal.** The correctness landmine, given its own package: every page-set
operation as one transformation, per T5.

`fn rewrite_page_tree(&mut self, order: &[PageSource]) -> Result<DocumentEdit>`
where `PageSource` is either an existing page index or a page imported from
another document. It produces a single flat `/Pages` node reusing the original
root `/Pages` object number, with each surviving page dict rewritten so that:

- `/Resources`, `/MediaBox`, `/CropBox` and `/Rotate` are materialized from
  whatever ancestor supplied them by inheritance before flattening;
- `/Parent` points at the new node;
- `/Annots` survives for a surviving page and its annotation objects are deleted
  for a removed one;
- `/StructParents` is renumbered and P4's hook is called;
- everything else on the page dict (`/B`, `/Tabs`, `/Group`, `/UserUnit`,
  `/Contents`, private keys) is carried through untouched, which is the
  "unimplemented means untouched" rule applied at the page level.

Plus the document-level fixups a page-set change forces, each of which is a
separate, individually testable function on the same edit:

- `/PageLabels`, a number tree keyed on page index, rebuilt for the new order;
- named destinations (`/Dests`, `/Names /Dests`) and outline `/A` and `/D`
  entries that name a removed page: dropped, with the count reported so the UI
  can say what it dropped;
- `/OpenAction` and any `/Aa` page-level actions naming a removed page;
- the `/Count` on the new node, which is just the length.

**Rows closed.** None. Backs every row in P11 and P12.

**Files.** New `crates/core/src/pages/{mod,rewrite,inherit,labels,destinations}.rs`,
`crates/core/src/edit/verb.rs` (the `DocumentEdit` variants), new
`crates/core/tests/pages.rs`.

**Depends on.** P2, P4. Also needs P1's reference validator to be the thing that
catches its mistakes.

**What exists to build on.** `cos::Document::page(index)` already resolves the
four inheritable attributes against ancestors (M2's P1 built it), so the
materialization step is a read from an existing accessor rather than a new
walk. `content::Page` normalizes on top of it. `outline.rs` already resolves
bookmark destinations to page indices and already knows the O(pages) sweep it
does, which this package must not make worse.

**Verification.**
- A corpus sweep, run as part of this package and not afterwards, listing every `external/` file whose page tree is more than one level deep, and every file with `/PageLabels`. Pick three of each as named fixtures. If the plan cannot name the fixture files, the transformation is unproven.
- For each deep-tree fixture: delete a page, and assert through a fresh parse of the saved output that every surviving page's four inheritable attributes are **identical to what they resolved to before**. This is the bug the flat rewrite exists to prevent and it is invisible in a shallow-tree fixture.
- P1's reference validator finds nothing on the output of every operation over every fixture. A run that trips it is a failure of this package, not of the validator.
- `/PageLabels`: a fixture with roman-then-arabic labels keeps each surviving page's label after a reorder and after a delete, compared as resolved label strings, not as tree structure.
- A bookmark and a named destination pointing at a deleted page: both are dropped, the drop count is reported, and every surviving bookmark still resolves to the page it named before.
- P4's structure invariant passes after each operation on each tagged fixture, and the reading order matches the new page order for the reorder case.
- Rotation: `/Rotate 90` inherited from a `/Pages` node survives the flatten, and a page whose own `/Rotate` overrode its ancestor's keeps its own.
- Bench: rewriting the tree of the 1000-page bench file, with the section size reported. The number decides whether T5's fallback is needed and this plan does not guess it.

**Review risk.** The highest-correctness-risk package in M3, the way P3
(geometry) was in M2. A reviewer will probe: whether inheritance is materialized
before or after the parent pointer changes (after is wrong and passes on
shallow trees); whether `/Count` is recomputed or copied; whether a page that
appears twice in the new order (a legal duplicate, which "copy pages between
documents" produces) is handled or aliases one object; whether the annotation
deletion for a removed page also removes its appearance streams and its
`/Popup` partner, which is a two-object chain; whether the destination fixup
walks `/Names` trees or only the flat `/Dests` dictionary; whether the drop
count is real or an estimate. **Mutation that must break its tests:** skipping
inheritance materialization must fail the deep-tree fixture; copying `/Count`
instead of recomputing must fail the delete case; dropping the `/PageLabels`
rebuild must fail the label fixture.

### P6. `core::annots`: annotations as appended objects

**Goal.** One place that authors annotations correctly, so thirty-odd comment
tools do not each get it slightly wrong.

- `DocumentEdit::AddAnnotation`, `RemoveAnnotation`, `SetAnnotationProperties`,
  `SetAnnotationContents`, `AddAnnotationReply`, over a typed
  `Annotation { subtype, rect, quads, contents, author, subject, created,
  modified, color, opacity, flags, state, in_reply_to, .. }`.
- Each edit writes three things: the annotation dict, its `/AP /N` appearance
  stream as a Form XObject, and the page's rewritten `/Annots`. One transaction,
  one undo entry.
- **Appearance stream generation** per subtype, which is ordinary content-stream
  authoring (`/BBox`, `/Matrix`, `/Resources`, a graphics operator sequence).
  Onionskin writes `/AP` itself rather than relying on a reader to synthesize
  one, because Acrobat writes it and every reader renders it differently
  otherwise. This module owns the generator; the tool packages own only their
  geometry and their defaults.
- P4's hook: an `/Annot` structure element and a `/StructParent` on a tagged
  document.
- `AnnotationFilter` (T7): a set of subtypes and a rendering mode
  (`DocumentOnly`, `DocumentAndMarkups`, `DocumentAndStamps`, `FormFieldsOnly`),
  applied by setting `/F` bit 2 on the excluded annotations in a **preview**
  buffer that is never saved. `/OC` is not used and no M3 annotation carries
  one, because hayro ignores it.
- Reading existing annotations, because a comment on a file Acrobat produced has
  to appear in the Comments pane.

**Rows closed.** None. Backs every row in P8, P9, P10, P20 and the filter rows
in P15 and P17.

**Files.** New `crates/core/src/annots/{mod,model,author,appearance,read,filter}.rs`,
`crates/core/src/edit/verb.rs`, new `crates/core/tests/annots.rs`.

**Depends on.** P2, P4.

**What exists to build on.** `content::PageText` / `TextRun::quads_for` produce
the quad points a text markup annotation needs, already mapped through M2's P3
geometry. `cos::Object` and the stream writer serialize the dict and the
appearance stream. `render`'s `render_annotations` bool already exists on our
settings struct. `crates/render` already renders `/AP` (the hayro fork's
`6af63be9` appearance-state fix is pinned).

**Verification.**
- Author one annotation of each M3 subtype, save, reopen with `cos::Document::open`, and assert **structurally**: the page's `/Annots` gained exactly one reference, the annotation's `/Subtype`, `/Rect`, `/F` and `/AP /N` are what was asked for, and the appearance stream's `/BBox` contains the `/Rect` in the annotation's own coordinate space. No substring scanning of the file.
- Render the saved file and assert the annotation's device rect contains non-background pixels; render with the annotation's `/F` Hidden set and assert it does not. That pair is what proves the filter mechanism, and neither test alone does.
- The filter: `DocumentOnly` hides every markup, `DocumentAndStamps` hides everything except `/Stamp`, and each is asserted by rendering, not by inspecting the filter's own output.
- Round-trip through Acrobat's own reading of the file is not automatable; instead, assert that hayro and `pdftotext`/`pdfannots` agree on the annotation set, through the existing content-oracle pattern.
- P4's invariant passes on a tagged fixture after annotation authoring, and the annotation is reachable from the page's structure element.
- Undo of an annotation restores the page's `/Annots` to its exact original object, by value.

**Review risk.** Whether the appearance stream's coordinate space is right: the
`/AP` `/BBox` and `/Matrix` map into the annotation `/Rect`, and getting it
wrong produces an annotation that renders at the correct place at one zoom and
drifts at another, which a single-zoom test will not catch. Whether quad points
are in the order the spec requires (the order is famously counter-intuitive and
producers disagree; state which convention is written and why). Whether
`/CreationDate` and `/M` are written in PDF date format with a timezone.
Whether the author name comes from a preference or from the OS user name
without asking, which is a privacy question, not a correctness one. Whether the
filter mutates the saved document rather than only a preview buffer.
**Mutation that must break its tests:** removing the appearance stream must fail
the render test while leaving the structural test green, which is exactly why
both exist.

### P7. `plugin-api`: the edit contract

**Goal.** Let a tool express an edit, and make the registry prove every tool's
edits behave.

- `ToolCtx` gains `edits: &'a mut EditSession`; `CommandCtx` gains the same.
  This is the minimum change: `DocumentEdit` values go in, nothing comes out.
- `Requirement` moves from `crates/app/src/shell/context_menu.rs` into
  `plugin-api` and gains a `Command(&'static str)` variant, so the six
  hardcoded `Requirement::Milestone` arms that `context_menu.rs`'s own module
  doc calls "guesses" become registry queries. That module doc names this as the
  work: "When those plugins contribute commands, `requirement()` is where the
  guesses become queries."
- **The registry-exhaustive property tests** PLAN.md's testing strategy item 1
  specifies, run over every registered tool and command, so a new plugin
  inherits the contract by being registered:
  - has a non-empty id, name, icon and group, and a shortcut or a menu home;
  - survives a degenerate document (zero-object, one empty page, the 1000-page
    bench file);
  - **every edit is undoable**: apply then undo is identity on the overlay;
  - **every edit serializes**: the resulting section reopens through
    `cos::Document::open` and satisfies P1's reference validator;
  - deterministic given the same inputs.

**Rows closed.** None. Backs every tool and command row in M3.

**Files.** `crates/plugin-api/src/lib.rs`, `crates/plugin-api/src/registry.rs`,
new `crates/plugin-api/src/requirement.rs`, `crates/app/src/shell/context_menu.rs`
(deletes its private copy), `crates/plugin-api/tests/contract.rs`.

**Depends on.** P2. The `Requirement` move also depends on P0 having settled
where `context_menu.rs`'s callers live.

**What exists to build on.** `ToolCapability` and the `tool_with(registry,
capability)` query already work and are used by the quick action toolbar, the
canvas context menu and the global bar. `PluginRegistry::commands()` already
carries ids. The four-variant `Requirement` already exists and only needs a
fifth variant and a new home.

**Verification.**
- The property tests run against the real `build_registry()` and fail if a tool is added without a group, which is checked by adding a deliberately incomplete tool in a test and asserting the suite rejects it.
- The "every edit is undoable" test drives each tool's real gesture lifecycle (`on_pointer_down`/`move`/`up`/`on_commit`), not a synthetic `DocumentEdit`, or it proves nothing about the tools.
- `cargo test -p onionskin-app --no-default-features` and `--no-default-features --features tools-comment` still pass: guarantee 5 holds with the contract in place.
- Every `Requirement::Milestone` arm that a shipped M3 plugin now satisfies is gone, asserted by a test that no `Milestone` reason names M3.

**Review risk.** Whether `ToolCtx` grew more than `edits` (a tool that can reach
the save path or the generations list is a tool that can surprise a user).
Whether "every edit is undoable" is asserted on the overlay or on the saved
bytes, and whether it is run per tool or once. Whether the `Requirement` move
left the app with a second copy. Whether the degenerate-document test uses a
document degenerate enough to have caught anything. **Mutation that must break
its tests:** making `EditSession::undo` a no-op must fail the property test for
every tool, and if it fails for only some, the test is not exhaustive.

### P8. `tools-comment` A: text markup

**Goal.** The five annotations that are driven by a text selection, and the
`ToolCapability::Highlight` and `Comment` advertisements that make the shell's
existing gates open.

Tools: Highlight (`/Highlight`), Underline (`/Underline`), Strikethrough
(`/StrikeOut`), Insert text at cursor (`/Caret`), Replace text (`/StrikeOut`
plus a linked replacement note, which Acrobat models as a `/StrikeOut` with
`/RC` and an `/IRT` reply).

These land first among the tool packages because they are the ones whose quad
geometry comes from `content` and therefore exercise M2's coordinate mapping
under a write path for the first time. If P3's geometry has a residual error,
this is where it surfaces, and it is cheaper to find here than under thirteen
drawing tools.

**Rows closed.** 56 Highlight text, 57 Underline text, 58 Strikethrough,
59 Insert text at cursor (caret markup), 60 Replace text. **5 rows.**

**Files.** `plugins/tools-comment/src/lib.rs`,
`plugins/tools-comment/src/{markup,quads}.rs`,
`plugins/tools-comment/Cargo.toml` (adds `onionskin-core`).

**Depends on.** P6, P7.

**What exists to build on.** `tools-basic`'s `select_text` already produces a
`TextSelection` with quads in page space through M2's P3 transform, and
`commands-core`'s `edit.select-all` already builds one; both are the input.
`ToolPlugin`'s gesture lifecycle and `Overlay::Quads` are exactly what the
in-progress drag needs.

**Verification.**
- Headless gesture tests in the Schist shape, sentence-named, against a real corpus document: `a_highlight_drag_over_glyphs_creates_one_annotation_with_their_quads`, `a_tiny_drag_creates_nothing`, `shift_extends_the_markup_to_the_new_selection`, `a_drag_over_two_columns_produces_two_quads_not_one_bounding_box`.
- The last of those is the one that bites: a bounding-rect implementation passes the first three and fails only on a multi-quad selection, so it is not optional.
- Saved output: the annotation's `/QuadPoints` count is four times the quad count, in the spec's vertex order, and the rendered result covers the glyphs (assert non-background pixels inside each quad and background outside, over the full region, not one pixel).
- Undo restores the page's `/Annots` by value; redo re-adds the same object.
- `cargo test -p onionskin-app --no-default-features --features tools-comment` and the P7 property suite.

**Review risk.** Whether the quad order convention is written down and matches
what `pdfannots` and Acrobat read back. Whether a selection spanning a rotated
page maps correctly, which needs a `/Rotate 90` fixture and is the same trap M2's
P3 documented. Whether Replace text writes a real `/IRT` reply chain or two
unrelated annotations. Whether the tool holds a borrow of `PageText` across the
commit. **Mutation that must break its tests:** replacing the quad list with its
bounding rectangle must fail the two-column test and nothing else.

### P9. `tools-comment` B: notes, drawing and shapes

**Goal.** The thirteen free-form annotations, and `ToolCapability::Draw`.

Sticky note (`/Text`), Add text comment / typewriter (`/FreeText` with
`/IT /FreeTextTypewriter`), Text box (`/FreeText`), Callout (`/FreeText` with
`/CL` and `/IT /FreeTextCallout`), Draw freehand (`/Ink`, with stylus pressure
from the GPUI fork), Erase ink (removes or splits `/InkList` strokes), Line
(`/Line`), Arrow (`/Line` with `/LE` endings), Rectangle (`/Square`), Oval
(`/Circle`), Polygon (`/Polygon`), Connected lines (`/PolyLine`), Cloud
(`/Polygon` with `/BE` a cloudy border effect).

This is the largest tool package by row count and the appearance-stream
generator in P6 is what keeps it from being thirteen small renderers. Each tool
here contributes geometry and defaults; none of them writes a content stream.

**Rows closed.** 55 Sticky note, 61 Add text comment (typewriter), 62 Text box,
63 Callout, 64 Draw freehand (ink), 65 Erase ink, 66 Line, 67 Arrow,
68 Rectangle, 69 Oval, 70 Polygon, 71 Connected lines (polyline), 72 Cloud.
**13 rows.**

**Files.** `plugins/tools-comment/src/{note,freetext,ink,shapes}.rs`,
`plugins/tools-comment/src/lib.rs`.

**Depends on.** P6, P7. Lands after P8 so P6's appearance generator has one
consumer's worth of feedback before twelve more arrive.

**What exists to build on.** `PointerInput` already carries `pressure: f32` and
the pinned GPUI fork adds stylus pressure, which is the whole reason the fork is
pinned. `Overlay::{Polyline, Line, Circle, Rect}` cover every in-progress
gesture these tools need, so nothing new is required on the overlay side.

**Verification.**
- Per tool, a gesture test: the shape a drag produces, the shape a click produces (a sticky note is a click, a rectangle is not), and what a degenerate gesture produces (nothing, for every one of them).
- Ink: a stroke with varying pressure produces an `/Ink` annotation whose appearance stream has varying stroke width, asserted by rendering two strokes at different pressures and comparing covered pixel counts, not by reading the content stream text.
- Erase ink over the middle of a stroke splits it into two `/InkList` entries and leaves the annotation's `/Rect` correct for the remainder.
- Cloud: the `/BE` border effect renders as a scalloped edge, asserted by comparing against a plain `/Polygon` render (they must differ) rather than by asserting an exact pixel pattern.
- Every tool's edit passes P7's undoable-and-serializes property test, which is where the exhaustiveness lives; this package does not repeat it per tool.

**Review risk.** Thirteen tools in one package is the sprawl risk, and the
reviewer should check that the shared parts really are shared: one defaults
struct, one commit path, one appearance call. Whether Arrow is a `/Line` with
`/LE` or a separate subtype (it is the former, and shipping it as a `/Polygon`
would render in Acrobat as a line with no head). Whether the free-text tools
embed a font or reference a Base 14 name, and whether the Base 14 substitution
rule from Legal posture rule 6 is honoured. Whether pressure is used or accepted
and ignored. **Mutation that must break its tests:** ignoring `PointerInput::
pressure` must fail the ink test; dropping `/BE` must fail the cloud test.

### P10. `tools-comment` C: stamps, attachments and the comment summary

**Goal.** The stamp family, attach-as-comment, comment properties, and the
summary document.

Stamps are `/Stamp` annotations whose appearance stream is the stamp artwork.
Per Legal posture rule 2 and the parity row's own note, **every stamp is redrawn
in-house**; the generated-logo discipline (`tools/logo.py`, constants in, SVGs
out) applies. Dynamic stamps fill name, date and time natively from the system
clock and an identity preference, not through the `AF*` JavaScript helpers
Acrobat uses, because `scripting` is M5. The parity row already says so.

Attach a file as a comment is a `/FileAttachment` annotation with an embedded
file stream, which is the same embedded-file machinery P13 needs for the
Attachments pane, so the writer lives in `core` and both call it.

Summarize comments generates a new document (T8) through `cos::write_new`: a
page per source page with the comments listed, or the compact single-list form,
matching Acrobat's two layouts.

**Rows closed.** 73 Attach a file as a comment, 74 Comment properties,
76 Summarize comments, 79 Place a stamp, 80 Standard business stamps,
81 Sign Here stamp category, 82 Dynamic stamps, 83 Create a custom stamp,
84 Manage stamps, 85 Paste clipboard image as stamp. **10 rows.**

**Files.** `plugins/tools-comment/src/{stamp,attach,summary,properties}.rs`,
new `crates/core/src/embedded.rs` (the embedded-file writer, shared with P13),
new `tools/stamps.py` and the generated `assets/stamps/*.svg`.

**Depends on.** P6, P7, P12 (the summary needs `write_new` and P12's page
assembly), P14 (paste-as-stamp needs the image import path).

**What exists to build on.** `core::attachments` reads embedded files today, so
the stream structure and the `/Names /EmbeddedFiles` layout are already
understood in-repo; the writer is its inverse. `codecs-common` already encodes
PNG, which is what a clipboard image stamp becomes.

**Verification.**
- Every shipped stamp is generated by the script from constants, and a test asserts the committed SVGs match a fresh generation, so nobody hand-edits one into resembling Adobe's artwork.
- A dynamic stamp's rendered text contains the configured identity and a date matching a clock injected by the test, not the real clock, or the test is unstable and proves nothing.
- Attach-as-comment: the saved file's embedded stream round-trips byte-identically to the input, and the attachment appears in `core::attachments` after a reopen. Path traversal in the file name is rejected, matching the existing `attachments.rs` rule.
- Summary: the generated document opens through `cos::Document::open`, has the expected page count, and its extracted text contains every comment's contents and author.
- Custom stamp creation from a PDF page and from an image both produce a `/Stamp` whose appearance renders.

**Review risk.** Whether any stamp resembles Adobe's artwork, which is a legal
question the reviewer must actually look at rather than take on trust. Whether
the dynamic stamp reads the OS user name without a preference, which the
Commenting preferences row (P20) owns. Whether "manage stamps" can delete a
built-in category into an unrecoverable state. Whether the summary generator
duplicates P12's page assembly instead of calling it. **Mutation that must break
its tests:** freezing the injected clock's date must change the dynamic stamp
render, and if it does not, the stamp is static.

### P11. `tools-organize`

**Goal.** The Organize Pages toolset, every operation expressed through P5's one
transformation.

Rotate (left / right, on a page selection), reorder / move, insert (from file,
blank, clipboard), delete, extract (to a new document or in place), replace
pages, page labels / renumber, copy or move pages between open documents.

Every one of these is `rewrite_page_tree` with a different `order` argument,
plus, for insert and copy-between-documents, an **object import**: the source
document's page and everything it transitively references (content streams,
resources, fonts, images, annotations) copied in with renumbered references.
That importer is this package's one genuinely new piece of machinery and it is
where the review should concentrate.

**Rows closed.** 35 Create a blank page, 41 Rotate pages, 42 Reorder / move
pages, 43 Insert pages, 44 Delete pages, 45 Extract pages, 47 Replace pages,
48 Copy or move pages between open documents, 49 Renumber pages / page labels.
**9 rows.** Row 50 (the grid) is P21's, because it is app surface.

**Files.** `plugins/tools-organize/src/lib.rs`,
`plugins/tools-organize/src/{rotate,reorder,insert,delete,extract,replace,labels}.rs`,
new `crates/core/src/pages/import.rs`, `plugins/tools-organize/Cargo.toml`.

**Depends on.** P5, P7. Copy-between-documents also depends on P3, because the
source document is a live session.

**What exists to build on.** P5 does the tree work; this package is mostly
translating a user's selection into an `order`. `cos::Document::get` and
`resolve` are the importer's read side; `add_object` its write side.

**Verification.**
- The nine operations, each on a deep-tree fixture and a flat one, each asserted through a fresh parse: page count, page order (compared by extracted text, so a reorder that keeps the count but shuffles nothing is caught), and P1's validator clean.
- The importer: insert a page from a document with an embedded font, save, reopen, and assert the inserted page renders identically to its render in the source document, pixel-compared with the same tolerance the render tests already use. A shallow importer that copies the page dict and not its resources passes every structural test and fails this one.
- Import of a page that references an object number already used in the destination: renumbered, asserted by checking the destination's original object at that number is unchanged.
- Extract writes a new document through `write_new` whose pages match the source pages by extracted text.
- Rotate composes: rotating a page that already has `/Rotate 270` by 90 gives 0, not 360.
- P4's structure invariant clean after each operation on each tagged fixture, and reading order matches page order after a reorder.
- `cargo test -p onionskin-app --no-default-features --features tools-organize`; the thumbnails context menu's M3 entries become enabled and their disabled reasons are gone, asserted in P21.

**Review risk.** Whether the importer is transitive or one level deep, which is
the single most likely defect and is invisible without the render comparison.
Whether it terminates on a cyclic reference (resources referencing a form
XObject referencing the same resources is legal and common). Whether "move
pages between documents" leaves the source document's undo stack able to put
them back. Whether replace-pages is implemented as delete-then-insert with two
undo entries instead of one. Whether rotation is applied to `/Rotate` or to the
view. **Mutation that must break its tests:** making the importer copy only the
page dict must fail the embedded-font render comparison and nothing else.

### P12. `commands-core` A: combine and split

**Goal.** The commands that produce new documents, and the page-assembly
primitive three other packages need.

Combine files into a single PDF, with a list the user builds (add files, add
folders, reorder, preview, remove) and per-file page-granularity expansion.
Split by page count, by file size, or at top-level bookmarks. Create a PDF from
multiple files, which is Combine with a different entry point and says so.

Mechanically this is `cos::write_new` (P1) plus P11's importer plus a page
assembly function that takes an ordered list of `(document, page range)` and
produces one document. That function is the primitive P10's summary, P11's
extract and P14's create-from-images all call, which is why it lives in `core`
and not in this plugin.

**Rows closed.** 34 Create from multiple files, 37 Combine files into a single
PDF, 38 Add files / add folders, 39 Reorder, preview and remove entries,
40 Expand a file and combine at page granularity, 46 Split. **6 rows.**

**Files.** `plugins/commands-core/src/{lib,combine,split}.rs`, new
`crates/core/src/pages/assemble.rs`, `plugins/commands-core/Cargo.toml`.

**Depends on.** P1 (`write_new`), P3, P5, P11 (the importer).

**What exists to build on.** `core::outline` already resolves bookmark
destinations to page indices, which is exactly what split-at-bookmarks needs.
`codecs-common`'s existing export job model (background worker, progress,
cancellation, atomic publication) is the pattern for a long-running combine,
and reusing it rather than writing a second one is the point.

**Verification.**
- Combine three named corpus files: the output's page count is the sum, each page's extracted text matches its source page, and each page renders identically to its source render.
- Combine a file with itself: legal, and the duplicate pages are independent objects, not aliases.
- Split by page count on a 10-page file at 3: four files of 3, 3, 3, 1, with the last one asserted, because an off-by-one here produces three files and drops a page.
- Split at bookmarks: a fixture whose top-level bookmarks are at pages 1, 4 and 9 produces three files with those boundaries, and a bookmark pointing at a page that does not exist is reported, not skipped.
- Split by file size: the constraint is best-effort and the test asserts what it actually promises (no output exceeds the target unless a single page does), not an exact size.
- Every output opens through `cos::Document::open` and satisfies P1's validator.
- Outputs carry a structure tree if all inputs did, and P4's invariant is clean; if any input is untagged, the output is untagged and says so rather than producing a half-tagged document.

**Review risk.** Whether combine holds every input document in memory at once (a
100-file combine is the natural worst case) or streams. Whether the output's
`/Info` and metadata are the first input's, a merge, or fresh, and whether that
choice is stated. Whether split writes its outputs atomically, given B4.4 already
made export do so and this is the same failure mode. Whether the half-tagged
case is detected or produced. **Mutation that must break its tests:** an
off-by-one in the split boundary must fail the 10-at-3 test; making the importer
alias rather than copy must fail combine-with-itself.

### P13. `commands-core` B: properties, bookmarks, attachments and the File menu

**Goal.** The authoring surface for the panes M2 built read-only, plus the
document-level dialogs.

- **Document Properties**, the five tabs Acrobat's unified UI documents:
  Description (writes `/Info` and XMP), Security (read-only at M3, naming M6),
  Fonts (read-only, from `content`), Initial View (writes `/OpenAction` and
  `/PageLayout` / `/PageMode`), Custom (arbitrary `/Info` keys). The parity row
  says to confirm the tab list against the screenshot corpus before building;
  that confirmation is a task in this package, not an assumption.
- **Bookmark authoring**: create, rename, nest, set destination, delete, and the
  Bookmarks pane context menu. `New Bookmarks From Structure` is **not** M3: it
  needs the tagged tree and the parity row already puts it at M6.
- **Attachment authoring**: add and delete embedded files, and the Attachments
  pane context menu, over P10's embedded-file writer.
- The remaining File and Edit menu rows: Save as Other (only the sub-targets
  Onionskin supports; PDF/X and Reader-Extended stay out of scope), Attach to
  Email (hands the file to the OS mail client, no Adobe service), Copy File to
  Clipboard, and Edit Cut / Copy / Paste / Delete scoped to the active tool.

**Rows closed.** 12 File > Save as Other, 14 File > Properties, 16 File > Attach
to Email, 18 Edit > Cut / Copy / Paste / Delete, 19 Edit > Copy File to
Clipboard, 25 Bookmarks: create, rename, nest, set destination, delete,
26 Attachments: add and delete, 27 Bookmarks pane context menu, 28 Attachments
pane context menu, 32 Initial View settings. **10 rows.** This also enables the
Layers pane's `Properties` entry, whose disabled reason names "the properties
dialog".

**Files.** `plugins/commands-core/src/{properties,bookmarks,attachments,file_menu}.rs`,
new `crates/core/src/outline/write.rs`, `crates/core/src/embedded.rs` (shared
with P10), `crates/app/src/shell/panes/{bookmarks,attachments}.rs`, new
`crates/app/src/shell/properties_dialog.rs`, `crates/app/src/shell/dialog.rs`.

**Depends on.** P0, P3, P5 (destination fixup), P10 (embedded-file writer).

**What exists to build on.** `core::outline::read` and `core::attachments::read`
exist and are cycle-guarded; the writers are their inverses and can share the
traversal. `preferences_dialog.rs` is the model for a multi-category dialog body
with its own `accessible()` and `render_*()` pair, and the properties dialog
follows it exactly.

**Verification.**
- Bookmark authoring: create a nested bookmark, save, reopen, and assert the tree shape and each destination's resolved page through `core::outline::read`, which is the reader this is the inverse of. Renaming preserves the destination; deleting a parent with children either promotes or removes them, and which one is asserted, not left to chance.
- A bookmark whose destination page is later deleted by P11 is dropped and counted (this is P5's fixup, exercised from the surface that creates them).
- Attachments: add, save, reopen, `core::attachments::read` reports it with the right size and MIME; delete removes it from `/Names /EmbeddedFiles` and frees the stream.
- Document Properties: writing a Description field appears in `/Info` **and** in XMP, and reopening reports the new value from both; the two must agree or the reader that a given consumer uses decides what it sees.
- Initial View: setting "open at page 5, fit width" writes `/OpenAction` and reopening in Onionskin honours it, asserted through the session, not through the dialog's own state.
- Every new dialog control is in the AccessKit tree with a real label and state, and the dialog's controls leave the tree when it closes, asserted by the probe.
- `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` plus the matching clippy.

**Review risk.** Whether `/Info` and XMP are both written or only one, and
whether they can disagree. Whether bookmark destinations are written as explicit
destinations or as named ones, and whether the choice survives P5's fixup.
Whether "Attach to Email" can be made to run an arbitrary command through a
crafted file name. Whether the Properties dialog exposes a Security tab that
looks writable. Whether the five-tab list was confirmed against the screenshot
corpus or copied from the parity row's own note. **Mutation that must break its
tests:** writing `/Info` and skipping XMP must fail the properties round-trip.

### P14. `codecs-common` and `commands-core` C: image import, image export, compress

**Goal.** The Create-a-PDF and Export-a-PDF rows M3 owns, plus the one
deliberately destructive save path M3 ships.

- **Create from images**: a single image file, multiple files (which is P12's
  combine over image inputs), and the clipboard. Each image becomes a page whose
  content stream draws it as an XObject, sized to the image's own DPI. This is
  `CodecPlugin`'s import half, which its module doc says "waits for the edit
  graph ... which is M3".
- **Export pages to JPEG, JPEG 2000 and TIFF**, alongside M2's PNG and SVG.
  JPEG 2000 needs a patent check per Legal posture rule 7 (the essentials have
  expired, per PLAN.md's own note) and a pure-Rust encoder; if none exists at
  acceptable quality, the row ships `partial` with the reason, not with a C
  dependency.
- **Export all images in a document**: walk each page's `/XObject` resources,
  decode, write out.
- **Compress a PDF and Reduce File Size**: a **flattening rewrite** through
  `cos::write_new`, which the parity row already says and which the UI must say
  too. Scope: downsample and re-encode images above a DPI threshold, drop
  unreferenced objects, and write object streams and a cross-reference stream.
  The compatibility target selects the output `/Version`. This is the one M3
  path that discards history, and it is a Save As, never an in-place save.
- **Convert** (the global bar entry point) is one surface over the above, with a
  deliberately smaller target list than Acrobat's, as its row already states.

**Rows closed.** 1 Convert, 9 File > Create, 33 Create from a single image file,
36 Create from the clipboard, 51 Compress a PDF, 52 Reduce File Size, 53 Export
to JPEG / JPEG 2000 / TIFF, 54 Export all images. **8 rows.**

**Files.** `plugins/codecs-common/src/{lib,import,jpeg,tiff,images}.rs`,
`plugins/commands-core/src/compress.rs`, `crates/plugin-api/src/codec.rs` (the
import half of `CodecPlugin`), `plugins/codecs-common/Cargo.toml`.

**Depends on.** P1 (`write_new`), P3, P12 (page assembly).

**What exists to build on.** `codecs-common` already exports PNG, SVG and text
with a tested background-worker job model, progress, cancellation and atomic
publication (C1.1). `image` is already a dependency behind the `shell` feature
and the encoder belongs in `codecs-common`, per M2's P13 decision.

**Verification.**
- Create from a 300 DPI image: the page's `/MediaBox` is the image's physical size at its own DPI, not a fixed page size, and the rendered page's pixels match the source image within the render tolerance.
- Export to each format: dimensions correct, file decodable by an independent decoder, and for JPEG a quality setting that actually changes the output size.
- Export all images: on a fixture with a known image count, the count matches and each output decodes; an inline image (`BI`/`ID`/`EI`) is either included or explicitly out of scope and stated.
- Compress: output opens through `cos::Document::open`, page count and extracted text unchanged, every page renders within tolerance of the source render, and the file is smaller on a fixture chosen because it has recompressible images. **Also**: the output has no incremental sections, and the UI string for this command contains the word that tells the user history is discarded, asserted.
- `CodecPlugin`'s import half is shaped by its three real consumers and no more.

**Review risk.** Whether compress silently degrades a document that had nothing
to compress (it must report that it saved nothing rather than write a
same-size rewrite). Whether the flattening path can be reached from `File > Save`
by any route. Whether image downsampling honours the image's own colour space
or converts everything to RGB, which would break a CMYK document destined for
print. Whether JPEG 2000 shipped with a patent note. Whether create-from-image
embeds an ICC profile or drops it. **Mutation that must break its tests:**
removing the downsampling step must fail the size-reduction assertion while
leaving every correctness assertion green, which is why both exist.

### P15. `crates/print` A: imposition and the print-to-file backend

**Goal.** Printing as a testable pure function, before any platform API exists.

PLAN.md's M3 sentence, "`crates/print` lands with the macOS backend and the
Acrobat print dialog", names the last two of five things that have to exist.
The five are: a page-selection model, an imposition engine, a sheet renderer,
the backend trait with two implementations, and the dialog. Only the last is
app work, and only one of the two backends can be tested in CI.

The cut that makes printing verifiable:

```rust
pub struct Placement { source: PageIndex, transform: [f64; 6], clip: Option<Rect> }
pub struct Sheet { size: PaperSize, orientation: Orientation, placements: Vec<Placement> }
pub trait PrintBackend { fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<()>; }
```

**Imposition is pure math over page geometry and produces `Vec<Sheet>`.** Page
range and subset, sizing (Fit, Actual size, Shrink oversized pages, Custom
scale), N-up (with its ordering and border options), orientation and auto
orientation, and duplex sheet ordering are all decisions about `Placement`
transforms and sheet order. Every one of them is asserted structurally, by
inspecting the `Sheet` list, which is what "assert on structure, not on
substrings" means when the artefact is paper.

Two backends behind one trait: `FileBackend`, which writes a PDF whose pages are
the composed sheets (each `Placement` becoming a Form XObject reference at its
transform, or a raster XObject when Print as Image is on), and P16's macOS
backend. The file backend is the CI-testable one and it lands first, exactly as
PLAN.md's risk list says ("print-to-PDF-file backend first, testable in CI
against expected output, platform backends behind the same trait").

Comments and Forms (row 94) is T7's annotation filter: the sheet renderer asks
`core` for preview bytes with the requested `AnnotationFilter` and renders
those. Print as Image renders each sheet through `crates/render` and places one
raster.

**Rows closed.** 87 Page range and subset, 88 Page sizing and handling,
89 Multiple pages per sheet (N-up), 93 Orientation, 94 Comments & Forms,
97 Print as image, 98 Print to file / print to PDF. **7 rows.** Booklet (90) and
Poster / tile (91) are the subject of section 9, decision B.

**Files.** `crates/print/src/{lib,job,impose,sheet,render,backend}.rs`, new
`crates/print/src/backend/file.rs`, `crates/print/Cargo.toml` (which today has
no `[dependencies]` section at all: it gains `onionskin-core`,
`onionskin-render`, `onionskin-cos`), `crates/print/tests/impose.rs`,
`crates/print/tests/file_backend.rs`.

**Depends on.** P3 (preview bytes), P6 (the annotation filter).

**What exists to build on.** `core::PageGeometry` already carries the media box,
crop box, rotation and render size, which is every input imposition needs.
`crates/render`'s `render_page` produces the raster Print as Image places.
`cos::write_new` (P1) is the file backend's writer. Nothing else: the crate is a
doc comment.

**Verification.**
- Imposition is unit-tested with no I/O, sentence-named per rule: `four_up_on_a_landscape_sheet_places_pages_left_to_right_then_down`, `shrink_oversized_leaves_a_page_that_fits_at_actual_size`, `custom_scale_of_fifty_percent_halves_both_axes`, `an_odd_page_count_in_duplex_leaves_the_last_back_blank`, `an_even_and_odd_subset_of_a_five_page_document_selects_one_three_five`.
- The file backend's output is asserted **structurally**: sheet count, and per sheet the number of placed XObjects and each one's transform matrix, read back by parsing the produced PDF. Never by scanning its bytes for an operator.
- One end-to-end pixel check per mode, not per option: render the produced sheet and the source page and assert the source page's content appears at the expected sheet coordinates. This is what catches a transform that is structurally plausible and geometrically wrong.
- Print as Image on a page with a transparency group produces a sheet whose only content is one image XObject, asserted structurally, and whose render matches the direct render within tolerance.
- Comments and Forms: four modes, four renders, each asserted for the presence and absence of the right annotation subtypes.
- `cargo test -p onionskin-print` runs with no window, no display and no printer, and is in the default `cargo test --workspace` job, which is the whole reason this package exists before P16.

**Review risk.** Whether the transform is built from the crop box or the media
box, which differ on exactly the documents where it matters and is the same trap
M2's P3 documented. Whether rotation is applied before or after scaling.
Whether "Fit" fits to the printable area or to the paper, which differ by the
printer's hardware margins and are a support-ticket generator. Whether duplex
ordering is expressed here or left to the backend, and whether the file backend
can therefore prove it at all. Whether N-up borders are drawn by imposition or
by the renderer. **Mutation that must break its tests:** transposing the N-up
placement order must fail the four-up test; replacing the sizing transform with
identity must fail both the shrink and the custom-scale tests.

### P16. `crates/print` B: the macOS backend

**Goal.** The same `Vec<Sheet>` on paper, through NSPrintOperation.

An `NSView` subclass whose `drawRect:` renders the requested sheet through
`crates/render` and places it into the print context, driven by
`NSPrintOperation` with an `NSPrintInfo` populated from the `PrintJob` (paper
size, orientation, duplex, copies, collation, printer selection). GPUI-free:
this crate links AppKit directly through `objc2`, which the app already depends
on for the accessibility probe.

Duplex is the one parity row that is genuinely a platform capability rather than
imposition: the sheet ordering is P15's, the two-sided instruction to the
printer is `NSPrintInfo`'s.

**Rows closed.** 92 Print on both sides / duplex. **1 row.** Rows 86, 95 and 99
need this backend to be honest but are closed by P17, which owns their UI.

**Files.** `crates/print/src/backend/macos.rs`, `crates/print/Cargo.toml`
(`objc2` and the AppKit bindings, target-gated), `crates/print/tests/macos.rs`.

**Depends on.** P15.

**What exists to build on.** `crates/app`'s `a11y-probe` feature already links
`objc2` and drives AppKit objects from a test, which is the pattern for
exercising an AppKit API without a window. Nothing in `crates/print` yet.

**Verification.**
- **Stated honestly: this backend cannot be proven in CI.** No hosted runner has a printer, and `NSPrintOperation` without one is a dialog. What CI can and does check: the crate compiles on macOS, the `NSPrintInfo` population is a pure function from `PrintJob` and is unit-tested against expected key-value pairs, and the sheet renderer is P15's and already tested.
- **Manual acceptance, scripted like M2's VoiceOver session**: print a three-page seed to the macOS "Print to PDF" destination and to a real printer if one is available; two-up a ten-page document; print a document with comments in each of the four Comments and Forms modes. The script, the expected result per step and the observed result are recorded in this package. **M3 is not done until it has run**, and its result is recorded here including what it failed at.
- A test that the backend is not reachable from a non-macOS build, so a Linux CI job cannot silently compile a stub that claims to print.

**Review risk.** Whether the `NSPrintInfo` mapping is a pure function or reaches
into global state. Whether a print job that fails reports the failure or leaves
the user with a spinner. Whether the sheet renderer allocates a raster per sheet
at printer resolution without a bound (a 1200 DPI A3 sheet is about 200 MB of
RGBA and a poster job is many of them). Whether the acceptance run is honestly
reported. **Mutation that must break its tests:** swapping two `NSPrintInfo`
keys must fail the mapping test, and if the mapping is not a pure function there
is no such test, which is the point.

### P17. app: the print dialog and Page Setup

**Goal.** Acrobat's print dialog over P15's job model.

The dialog is a `ShellDialog` variant with its own body module, following
`preferences_dialog.rs` exactly: printer selection, copies, page range and
subset, page sizing and handling, multiple pages per sheet, orientation,
Comments and Forms, a live preview of the composed sheet, and Page Setup. The
preview is P15's imposition rendered through `crates/render`, which is the same
code the backend uses, so the preview cannot disagree with the output.

Advanced Print Setup ships carrying only its in-scope items, Print as Image and
Print to File, with the out-of-scope groups (Output, Marks and Bleeds,
PostScript, print colour management) absent and the row's Notes saying so. That
is the row's own stated subset, not a cut invented here.

Print comments (row 77) and Summarize comments in the print output (row 96) are
the two dialog options that reach back into P10's summary generator: "Document
and Markups" is P15's filter, "Comments summary only" and "append a summary" are
the generator's output printed as extra sheets.

**Rows closed.** 4 Print button (global bar), 15 File > Print, 77 Print comments,
86 Print dialog, 95 Page Setup dialog, 96 Summarize comments in print output,
99 Advanced Print Setup. **7 rows.**

**Files.** New `crates/app/src/shell/print_dialog.rs`,
`crates/app/src/shell/dialog.rs`, `crates/app/src/shell/chrome/dialogs.rs`
(P0's), `crates/app/src/shell/chrome/global_bar.rs` (the Print menu entry and
the button), `crates/app/src/shell/context_menu.rs` (`CanvasContextCommand::
Print`'s `Requirement` becomes a real query), `crates/app/src/shell/chrome/
commands.rs` (the `file.print` id and its `cmd-p` default), `crates/app/Cargo.toml`
(adds `onionskin-print` behind `shell`).

**Depends on.** P0, P15, P16.

**What exists to build on.** `ShellDialog` with its `title` / `accessible` /
`render_dialog` triple and its modal host. `preferences_dialog.rs` is the model
for a rich body. `CanvasContextCommand::Print` already exists, disabled with
the reason "Available in M3 with `crates/print`", and there is already a test
(`print_and_add_bookmark_are_disabled_with_their_milestone`) that will have to be
inverted, which is a good sign the gate was built to be opened.

**Verification.**
- `cmd-p` opens the print dialog, driven with `cx.simulate_keystrokes` on a real window, per T9. Not by calling the handler.
- Every dialog control appears in the AccessKit tree with a label and its current state; closing the dialog removes all of them, asserted by the probe, so nothing lingers as an invisible tab stop.
- The preview and the file backend produce the same `Vec<Sheet>` for the same settings, asserted by comparing the sheet lists, which is the assertion that makes "the preview cannot disagree" true rather than aspirational.
- A page range the user types as `2-4,7` parses to those pages, and `7-2` is rejected with a message rather than silently reversed.
- The existing `print_and_add_bookmark_are_disabled_with_their_milestone` test is updated so that Print is enabled and Add Bookmark's arm still asserts its own reason, rather than the whole test being deleted.
- `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy.

**Review risk.** Whether the dialog owns any imposition logic (it must not:
every setting is an argument to P15). Whether the preview renders on the UI
thread, which for a 1200 DPI sheet would freeze the shell exactly the way export
did before C1.1. Whether Page Setup and the print dialog can disagree about
paper size. Whether the Advanced Print Setup dialog exists at all or is a
disabled menu item claiming a subset it does not have. **Mutation that must
break its tests:** making the preview use different settings from the job must
fail the sheet-list comparison.

### P18. app: save, undo/redo, dirty state and crash recovery

**Goal.** The commands the user reaches for, and the state that tells them
whether they can close the window.

Undo and Redo on the Edit menu and as global bar icons, Save and Save As on both,
Revert, a dirty indicator on the tab, a close confirmation, and autosave with
crash recovery over P3's recovery file. The crash-recovery snapshot ranking is
pinned as a unit test, which PLAN.md's testing strategy item 4 already calls for
by name.

`global_bar.rs` already has `MenuCommand::{SaveAs, Undo, Redo}` disabled with the
reasons "Saving lands in M3" and "Document editing lands in M3", and
`commands.rs` deliberately gives them no keystroke, on the stated rule that "a
keystroke that reports 'lands in M3' is worse than no keystroke". This package
gives them `cmd-s`, `cmd-shift-s`, `cmd-z` and `cmd-shift-z`.

**Rows closed.** 2 Undo / Redo icons on the global bar, 3 Save / Save As in the
global bar, 7 Autosave and crash recovery, 10 File > Save, 11 File > Save As,
13 File > Revert, 17 Edit > Undo / Redo. **7 rows.**

**Files.** `crates/app/src/shell/chrome/{global_bar,commands,menu}.rs`,
`crates/app/src/shell/chrome/tabs.rs` (the dirty indicator and the close
confirmation), `crates/app/src/keymap.rs`, `crates/app/src/config.rs` (the
recovery directory), new `crates/app/src/shell/recovery.rs`.

**Depends on.** P0, P3.

**What exists to build on.** `MenuCommand::all()` / `id()` /
`default_keystroke()` is one table and the Keyboard Shortcuts dialog reads it
automatically. The close-tab path already defers correctly through `cx.defer`
(resolved at `7413186`), which is the shape every new listener must copy.
`recents.rs` and `config.rs` already own a 0700 config directory with the
known 0755-preexisting-directory residual.

**Verification.**
- Each of the four keystrokes proven with `cx.simulate_keystrokes` on a real window, per T9, in the shape of `the_find_keystroke_opens_the_find_bar`. This project shipped a dead Ctrl+F twice; four new global commands is four new chances.
- Undo after an edit restores the canvas: the rendered page after edit-then-undo is pixel-identical to the render before the edit. Asserting the overlay is empty is P2's job; this asserts the user sees it.
- The dirty indicator appears on the first edit, clears on save, and **reappears when the user undoes past the saved mark**, which is T1's behaviour and the one a naive dirty flag gets wrong.
- Closing a dirty tab prompts; the prompt retains the originating canvas identity, which is B4.2's rule for every async prompt in this shell and applies here unchanged.
- Crash recovery: a recovery file written for document A is offered when A is next opened and not when B is, ranked most-recent-first, asserted as a unit test on the ranking with no window.
- Every new control is in the AccessKit tree with a state that reflects enablement (Undo is disabled with a reason when the stack is empty, not absent).

**Review risk.** Whether the global action listeners defer (a listener body
calling `window_handle.update()` fails with "window not found" and drops the
command with an `eprintln`; this is a documented, previously-shipped bug in this
exact file). Whether the dirty state is derived from the history cursor against
the saved mark or from a boolean somebody sets. Whether autosave can run during
a save. Whether recovery can offer a file for a document that has since been
saved. Whether Undo's disabled reason is generated or hardcoded.
**Mutation that must break its tests:** removing `cx.defer` from any new listener
must fail its keystroke test; replacing the saved-mark comparison with a boolean
must fail the undo-past-the-mark test.

### P19. app: the skins panel

**Goal.** The interface the whole project is named after.

A panel listing the document's generations, newest at the top: the original
bytes as the bottom sheet and each incremental section above it, with its byte
range, its size, and where known its producer and date from that section's
trailer `/Info`. Sections other producers wrote (a file Acrobat has already
been appended to) appear too, and are labelled as not ours. Selecting a
generation previews the document as of that generation; rolling back truncates,
through P3's `revert_to`, with a confirmation that says what will be discarded.

The skins panel is **not** a `NavigationPane`: those are one-at-a-time viewers
of document content, and this is an app-level history of the file. It is a rail
entry with a side-panel surface, which is also where M3's tool-specific side
panel content lands (P20).

**Rows closed.** None. `ACROBAT-PARITY.md`'s counting convention explicitly
excludes Onionskin-only surface: "the skins/generations pane, the redaction
verifier, generation rollback, the MCP server, the CLI" are named as not
counted. This package closes zero rows and is still the identity of the release,
which is worth saying out loud so nobody schedules it by row count.

**Files.** New `crates/app/src/shell/skins.rs`,
`crates/app/src/shell/chrome/{rail,side_panel}.rs`,
`crates/app/src/shell/chrome/tabs.rs` (state).

**Depends on.** P0, P1 (`sections()`), P3 (`revert_to` and preview).

**What exists to build on.** `chrome/rail.rs` is registry-driven and
`chrome/side_panel.rs` is the contextual host M2 built with an empty state,
whose parity row says "tool-specific content starts at M3". P1's `sections()`
is the data.

**Verification.**
- A file with three generations lists three plus the original, in order, with byte ranges that partition the file, asserted against `sections()`.
- A file Onionskin has never written (a corpus file with a pre-existing incremental section) lists it and labels it as not ours.
- Rollback: select the middle generation, roll back, and assert the file on disk is byte-identical to a truncation at that generation's start, and that the reopened document matches.
- Rollback is refused with a visible reason when there are unsaved edits, and when the selected generation is not trailing.
- Preview of an older generation does not modify the file, asserted by hashing before and after.
- Every row and control is in the accessibility tree, and the panel's contents leave it when the panel closes.

**Review risk.** Whether "preview a generation" opens a second document or
mutates the current one. Whether rollback is reachable without a confirmation.
Whether the panel recomputes `sections()` per frame on a file with hundreds of
generations. Whether a truncation can race the render worker still reading the
truncated tail. Whether the "not ours" label is derived from something real or
guessed from `/Producer`. **Mutation that must break its tests:** truncating one
byte off the correct boundary must fail the byte-identity assertion, so the test
must compare bytes and not just reopen successfully.

### P20. app: the Comments pane and the comment surfaces

**Goal.** Everywhere a comment appears outside the page.

The Comments navigation pane (list, sort, filter, reply, status, checkmark,
read and unread), its context menu, comment properties including "make current
properties default", and Commenting preferences. Plus the two shell states that
flip: the quick action toolbar's Comment, Highlight and Draw entries become live
through their existing `ToolCapability` query, and the right-hand side panel
gains its first tool-specific content, which is the comment properties inspector.

The find bar's `Include Comments` checkbox, disabled with "Comments arrive with
the comment tools in M3", becomes live here. That also needs
`SearchResult::Unavailable::reason` widened from `&'static str`, which
`known-issues.md` records as "widen it after P9 lands".

**Rows closed.** 29 Comments pane, 30 Comments list context menu, 75 Comments
list sort/filter/reply/status/checkmark/read-unread, 78 Commenting preferences.
**4 rows.** It also flips two M2 `partial` rows (the quick action toolbar and the
right-hand side panel) to `implemented`, which is part of this package's
definition of done and not a follow-up.

**Files.** New `crates/app/src/shell/panes/comments.rs`,
`crates/app/src/shell/panes/mod.rs` (the `NavigationPane` variant, `PaneAction`,
`apply`, `read`, `render_body`, `accessible_body`),
`crates/app/src/shell/chrome/quick_actions.rs` (the `DeliveryStage::M3` gate),
`crates/app/src/shell/chrome/side_panel.rs`, `crates/app/src/shell/find_bar.rs`,
`crates/app/src/preferences.rs`, `crates/core/src/search.rs` (the reason type).

**Depends on.** P0, P8, P9, P10.

**What exists to build on.** The pane registration pattern is six mechanical
steps in `panes/mod.rs` and `results.rs` is the model for a live pane that
updates while work continues. `quick_actions.rs` resolves availability through
`tool_with(registry, capability)` already, so the M3 gate is one
`unavailable_stage` arm, not a rewrite.

**Verification.**
- The pane lists every annotation a corpus file already carries, not only ones Onionskin authored, which is the case a Comments pane built against the edit graph alone would miss.
- Sort by page, author and date; filter by type, author and status; each asserted by the resulting list order or membership, on a fixture with enough variety to distinguish them.
- Reply creates an `/IRT` annotation that reopens as a reply; set status writes an Acrobat-compatible `/State` and `/StateModel` annotation, which is how Acrobat models status and is not a property on the parent.
- "Make current properties default" writes a preference and the next annotation created uses it, asserted end to end through the tool, not through the preference store.
- The quick action toolbar's Comment, Highlight and Draw are enabled and carry no reason string, asserted, and the assertion reads the registry rather than a list.
- Find with Include Comments finds text that exists only in an annotation's `/Contents`.
- Pane rows and menu entries are in the accessibility tree; the pane's rows leave it when the pane closes.

**Review risk.** Whether the pane reads annotations through `core::annots` or
grows its own parser. Whether status is written as `/State` on a reply
annotation (correct) or as a key on the parent (what a reader would guess, and
wrong). Whether "read and unread" is persisted in the document (it must not be:
it is per-user state) or in preferences. Whether the pane extracts every page's
annotations on every frame, which is the same UI-thread extraction defect P12's
a11y residual already records for page text. Whether the widened `reason` type
leaks an allocation into a per-frame path. **Mutation that must break its
tests:** making the pane list only Onionskin-authored annotations must fail the
existing-comments fixture.

### P21. app: the Organize Pages grid

**Goal.** The page grid Acrobat's Organize Pages toolset shows, and the
activation of the thumbnails context menu M2 built disabled.

A grid of page thumbnails with zoom, multi-select (click, shift-click,
cmd-click, marquee), drag to reorder, and per-page and per-selection actions
that call P11's tools. The Page Thumbnails pane's context menu entries (Insert,
Extract, Replace, Delete, Rotate, Page Properties, Embed and Remove All Page
Thumbnails) lose their "Available in M3 tools-organize" disabled reasons.

**Rows closed.** 50 Page thumbnail zoom and multi-select in the Organize grid.
**1 row.** It also flips the M2 Page Thumbnails context-menu row from `partial`
to `implemented` for its M3 entries, leaving only Crop Pages disabled on its M5
reason.

**Files.** New `crates/app/src/shell/organize.rs`,
`crates/app/src/shell/panes/thumbnails.rs` (the disabled reasons),
`crates/app/src/shell/chrome/{rail,side_panel}.rs`.

**Depends on.** P0, P11.

**What exists to build on.** `panes/thumbnails.rs` is 917 lines of working
lazy thumbnail delivery with the B7 poll-rearm fix and the stale-size rejection
regression, and the grid is the same worker requests at a different layout.
Reusing it rather than writing a second thumbnail path is the point, and a
second path would reintroduce both fixed bugs.

**Verification.**
- Multi-select: shift-click extends a contiguous range, cmd-click toggles one, a marquee selects what it covers, each asserted on the selection set.
- Drag to reorder produces exactly one undo entry for the whole drag, not one per intermediate position.
- Every previously disabled thumbnail context-menu entry is enabled and runs its command, asserted through the menu, and Crop Pages is still disabled with its M5 reason (so the test proves the mechanism rather than that someone enabled everything).
- Thumbnails in the grid are requested lazily: a 1000-page document requests only the visible band, which is the existing pane's assertion applied to the new layout.
- After a reorder, the grid and the canvas agree on page order without a manual refresh.
- Grid rows are in the accessibility tree with page numbers as labels and selection as state.

**Review risk.** Whether the grid duplicates `ThumbnailsState` instead of
sharing it, which would give the two surfaces different eviction and reintroduce
APP-004 and APP-012. Whether a drag that is cancelled mid-flight leaves a
partial reorder. Whether the selection survives an undo that changes the page
count (it must be clamped, loudly). Whether the grid holds every page's
thumbnail on a 1000-page document. **Mutation that must break its tests:**
emitting one undo entry per intermediate drag position must fail the drag test.

### P22. app: the remaining shell rows

**Goal.** The nine M3 rows that belong to no other package, each small, grouped
because reviewing nine one-file changes together is cheaper than nine reviews.

- **Home view: Starred.** A local flag on disk in `recents.rs`; Acrobat keeps
  it in cloud storage and Onionskin does not, which the row already states.
- **Manage Tools / customize the tool rail.** Registry-driven show and hide over
  `PluginRegistry::tools()`; the rail is already registry-driven, so this is a
  persisted filter, not a rewrite.
- **Window menu** (New Window, Cascade, Tile, Minimize) and **View > New
  Window** (a second window on the same document). The second is the one with
  content: two windows over one `core::Document` means two viewports over one
  session and one edit stack, which is the correct model and needs stating.
- **View > Page Display > Automatically Scroll**: a timed scroll over the
  existing viewport, with speed control, pause on interaction.
- **View > Show/Hide > Line Weights.** Moved from M2 by its plan review, which
  also recorded the design so M3 does not repeat the wrong analysis: Acrobat's
  toggle draws **all strokes at a constant hairline width when off**, not a
  minimum-width floor on their true widths. The fork field to add is
  constant-hairline-width semantics, batched with whatever hayro fork bump M3
  takes anyway. If the fork commit does not land, the row stays disabled with a
  reason and moves to M4; that is a scoreboard change, decided in this package,
  and never both outcomes.
- **Copy with formatting / Export selected text.** See section 8, item 12: the
  clipboard half needs a GPUI capability that does not exist, so this ships as
  Export Selection As (RTF and plain text to a file) and the row is `partial`
  with the clipboard half named.
- **Advanced Search: include attachments** (two levels deep, as Acrobat does)
  and **document-property criteria** (author, dates, keywords, metadata), both
  over M2's existing document search.

**Rows closed.** 5 Home view: Starred, 6 Manage Tools, 8 Window menu,
20 Advanced Search include attachments, 21 Advanced Search document-property
criteria, 22 Automatically Scroll, 23 Line Weights, 24 View > New Window,
31 Copy with formatting / Export selected text. **9 rows.**

**Files.** `crates/app/src/recents.rs`, `crates/app/src/shell/home.rs`,
`crates/app/src/shell/chrome/{rail,global_bar,menu}.rs`,
`crates/app/src/shell/{canvas,find_bar}.rs`, `crates/core/src/{viewport,search}.rs`,
`crates/render/src/base.rs` and `crates/render/Cargo.toml` (the fork rev, if the
Line Weights commit lands), new `plugins/codecs-common/src/rtf.rs`.

**Depends on.** P0. Advanced Search's attachment half also depends on P13's
embedded-file work only for symmetry of code, not for function.

**What exists to build on.** `recents.rs` already persists a list with the same
shape Starred needs. `rail.rs` already derives its contents from the registry.
`core::search::DocumentSearch` already walks pages incrementally on a worker.
The Line Weights menu item already exists, disabled, with an asserted reason
naming M3.

**Verification.**
- Each row's own behaviour, unit-tested with no window where it has no window dependency (Starred persistence, rail filtering, search criteria matching, auto-scroll timing), and with `cx.simulate_keystrokes` where it has a keystroke.
- New Window: an edit made in one window appears in the other, and one undo in either takes it back once, not twice. That is the test that proves one session and not two.
- Line Weights: if the fork commit lands, a fixture with strokes of three different widths renders with three widths on and one width off; if it does not, the menu item is still disabled and its reason names M4, asserted. Exactly one of those tests exists.
- Advanced Search: a term present only in an attached PDF is found with the option on and not with it off; a document-property query matches on `/Info` and XMP.
- Export Selection As writes RTF whose text matches the selection and whose runs carry the selection's fonts.

**Review risk.** Whether nine small changes got nine tests or three. Whether New
Window shares the session or opens the file twice, which would give two edit
stacks over one file and is the defect this row invites. Whether auto-scroll
runs a timer when the window is not visible. Whether the Line Weights outcome
is carried as one branch or as a hedge. Whether Starred paths leak absolute
home directories, which is the residual `recents.rs` already carries.
**Mutation that must break its tests:** opening a second `core::Document` for
New Window must fail the shared-undo test.
