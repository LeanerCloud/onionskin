# M3 implementation plan: first edits, first print

Status: planning, with section 9's two open decisions ruled on 2026-09-02 and
folded in, and section 1's ground truth re-measured against `main` at `fa5a194`
on 2026-09-10. No M3 code is written except P0a, which is in flight. This
document is the authoritative decomposition; it supersedes PLAN.md's M3
paragraph wherever the two disagree, and section 8 lists every disagreement.
The PLAN.md corrections those disagreements called for **landed at `989d8a7`**;
section 8 marks each item as landed or still owed rather than describing all of
them as pending.

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

## 1. Ground truth

**Re-measured against `main` at `fa5a194` on 2026-09-10.** The first draft of this
section was taken at `81f802f`, 69 commits earlier, while four sibling agents were
closing M2 out on unmerged branches. Their work landed, and it moved numbers this
plan reasoned from: `crates/core` grew by half, `crates/app` by three thousand
lines, and the guarantee-honesty package (`0f1c295`, merged at `1a4b7cf`) un-ignored
three of the guarantee tests whose absence section 8 item 2 was built on. Every figure below was counted this session, by the method stated
in the footnote, and any figure not re-counted here is not a fact.

| Component | State | The part that matters for M3 |
|---|---|---|
| `crates/cos` | 8152 lines, real, **unchanged since `81f802f`** | Already has the whole write side: `set_object`, `add_object`, `delete_object`, `set_trailer_entry`, `set_info_field`, `has_pending_changes`, `incremental_section`, `save_to_writer/path/vec`, `original_len`. Edits accumulate in one `BTreeMap<u32, Edit>` (`Edit` is private) and **one call to `incremental_section` emits one section carrying all of them**. There is no way to withdraw a pending edit, no section-chain accessor, no `flatten` full-rewrite API, and no `next_object_number` accessor. `Document` is `!Send` (`Rc<ObjectStream>` at `document.rs:109`), is **not `Clone`**, and holds a `Reader` plus four `RefCell` caches (`document.rs:108-114`). |
| cos deletion | real, and the landmine | `delete_object` splices a chained free list at the head (`free_list_rows`, `document.rs:995`), refuses object 0 and refuses the trailer's `/Root`, and **bumps the generation** (`document.rs:906`). It performs **no reference walk**: deleting a `/Pages` node, a page still in a `/Kids` array, a content stream, or an annotation's appearance stream leaves a dangling reference and cos will happily serialize it. The doc comment at `document.rs:868` names M3's `tools-organize` as the caller that must fix this up. Symmetrically, `set_object` **refuses a number the file marks free**, returning `Error::FreedObject` (`document.rs:836-838`); T5's free-nothing rule is what keeps M3 clear of both. |
| `crates/core` | **11047** lines, real session | `core::Document` (there is no `Session` type) holds `bytes: Arc<Vec<u8>>` and a private `cos::Document` it never mutates (`session.rs:182-184`), page geometry and text caches, selection, search, the render worker handle, and the read-only pane readers (`outline`, `attachments`, `signatures`, `layers`). **No edit graph, no history, no save.** `history.rs` is view history and says so in its own doc comment. `ExportSnapshot` (`session.rs:163`) is the only state-replay mechanism and it replays layer visibility only. `&mut Document` already reaches `selection_mut`, `cancel_search`, `set_layer_visible`, `reset_layer_visibility`, `request_snapshot` and `select_match`, which is P7's real review risk. |
| `crates/plugin-api` | **755** lines, real | `ToolPlugin` with its full gesture lifecycle, `CommandPlugin`, `CodecPlugin` (export only), `PluginRegistry`, `ToolCtx { doc, viewport }` (`lib.rs:81`), `ToolCapability` (**8** variants, `lib.rs:60`), `Overlay` (6 variants, `lib.rs:43`: `AntsRect`, `Rect`, `Quads`, `Polyline`, `Line`, `Circle { center, radius }`). **A tool has no way to express a document edit.** Its own module doc says the import path "waits for the edit graph ... which is M3". `Requirement` is not here: it is a private four-variant enum in `crates/app/src/shell/context_menu.rs:47`. |
| `crates/render` | **3118** lines | Renders from `Arc<Vec<u8>>` through hayro; `render_annotations` is a settings bool (`base.rs:103`); `TileStore` evicts. Annotation appearance streams render. hayro's annotation loop **never reads an annotation's `/OC`**, only the `/F` hidden flag. |
| `crates/content` | 13454 lines, unchanged | `extract_page`, `PageText`/`TextRun`/`Glyph`/`Mapping`/`ByteProvenance`, `PageQuad`. This is where a highlight's quad points come from. Nothing writes. |
| `crates/print` | 6 lines, doc comment only | Workspace member, `onionskin-print` in `[workspace.dependencies]`, **no `[dependencies]` section at all**. Greenfield. |
| `crates/crypto` | 4 lines, doc comment only | No security handler, no RC4, no AES, no key derivation. `cos` raises `Error::Encrypted` from one `refuse_encrypted` predicate (`document.rs:1228`) at four call sites (171, 190, 198, 761). |
| `plugins/tools-comment` | 20 lines | Manifest with a no-op `register`. Already a default cargo feature and already installed by `build_registry`. |
| `plugins/tools-organize` | 19 lines | Same shape. |
| `plugins/commands-core` | **213** lines | Registers exactly two commands, `edit.select-all` and `edit.deselect-all`, both of which only touch in-memory `Selection`. |
| `crates/app` | **36112** lines under `src/`, of which `shell/chrome/tabs.rs` is **8521** and `shell/canvas.rs` is **6025** | `ShellFrame` in `tabs.rs` is the single top-level GPUI view: **24 fields** (`tabs.rs:234-264`), one `impl` block spanning lines **371 to 2924 with 115 methods**, plus the export worker. Every new command, dialog, pane toggle and accessibility node lands in it. |
| corpus | seeds 3 (tracked), external gitignored and fetched by `corpus/fetch.sh` (default sets `hayro`, `pdf-association`, `verapdf`; `fetch.sh:50`), malformed 15 (gitignored, generated), bench 1000-page (gitignored), **`tagged/` and `js-forms/` are a README and nothing else** | Guarantee 8's fixtures do not exist. `verapdf/PDF_UA-1` and `PDF_UA-2` are the named raw material. `crates/cos/tests/common/mod.rs:30-47` (`corpus_dir` and `missing`) returns `None` and prints `SKIPPED` when a corpus directory is absent, unless `ONIONSKIN_CORPUS_REQUIRED` is set. |
| guarantees | `crates/app/tests/guarantees.rs` is **4187 lines**; **guarantees 1, 2 and 6 run**; 3, 4, 7 and 8 are the `#[ignore]`d ones (lines 119, 127, 219, 228), each naming M5 | B6 closed the state this plan was drafted against. 1, 2 and 6 are now **executable tripwires**: each names its enforcing suite in `crates/cos/tests/{roundtrip,incremental,repair}.rs`, asserts the specific markers that suite must still contain, and asserts that CI still reaches it. Guarantee 6 additionally asserts both halves of the corpus rule as workflow steps. See section 8, item 2, for what M3 owes them now. |

Footnote on method: crate line counts are `find crates/<name> -name '*.rs' | xargs cat | wc -l`,
which is the convention the first draft used (it counts `tests/`, `benches/` and
`fuzz/` as well as `src/`); the `crates/app` figure is the one exception and counts
`src/` only, as its cell says. `impl` spans and method counts are brace-matched over
`tabs.rs`, counting `fn` at one level of indent inside the block.

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
| The encryption class split (`permissions-only` versus password-protected) that m2-viewer section 6 assigned as a `known-issues.md` ledger action **was never written**. `known-issues.md` has no encryption entry at all. | P1b produces the measurement (section 9, ruling A); the orchestrator lands the ledger entry |
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

**Undoing past the saved mark must never meet a freed object number, and T5's
free-nothing rule is what guarantees it.** This is the clause that makes the
paragraph above executable rather than aspirational, and it was missing.

The hazard is real and specific. If a save had freed an object number, T3's
reopen would leave the base document's xref marking that number `Free`, and
`cos::Document::set_object` **refuses a freed number outright**
(`Error::FreedObject`, `document.rs:836-838`) precisely because taking one back
means re-linking a free list written into a section that is already on disk.
Delete a page, save, undo past the saved mark, save: the second save would fail
and `Ctrl+Z` after a save would be a broken promise.

**M3 does not have that hazard, because M3 frees nothing** (T5). Every removal is
a rewrite of the referrer; the removed object stays in the file as garbage. So:

- Undo of a page deletion restores the root `/Pages` object and each rewritten
  surviving page dict from their captured `before` values. The removed page dict,
  its annotations and the old internal nodes were never touched, so there is
  nothing to bring back. The flat rewrite reusing the original root `/Pages`
  object number is what makes this a one-object restoration rather than a graph
  rebuild.
- Undo of an object creation across a save drops the overlay entry and restores
  the referrer, so the created annotation stops being reachable from the page.
  Its bytes stay in the file, unreferenced, which is what the core invariant
  promises about every byte this project writes.
- No overlay entry ever names a number the base marks free, so `section_for`
  never has to write an in-use row for one.

**The rule is enforced, not assumed.** P1's validator refuses to emit a section
that frees a number some object the section writes still references, and P5's
verification runs `audit_references` over the whole output file on every fixture,
which is the complete check. If a later package reaches for `delete_object`, the
first of those makes it loud and the second makes it visible.

**The roads not taken**, named so nobody re-derives them. Freeing and then
resurrecting is possible: a section builder controls its own free-list rows, so
it could write an in-use row and re-splice the chain, and the bumped generation
would not break existing references because `cos` matches on the object number
alone and treats the recorded generation as advisory (`document.rs:1235-1239`).
It is rejected because it is machinery in service of a state M3 never enters.
Re-creating a removed object at a *fresh* number is also possible and also
rejected: it produces a file isomorphic to the pre-edit file rather than equal to
it, and P3's `edit, save, undo, save` test compares the object graph by value.

**T2. What an edit is, and where the object numbers come from.**

`cos::Document` already accumulates `Edit::Set` and `Edit::Delete` and already
emits all of them as one section. What it cannot do is withdraw one. So cos is
**not** where the edit graph lives: `core` owns the authoritative overlay, and
**cos's own edit map is never populated by Onionskin at all.**

That last clause is the fix for a contradiction the first draft carried. It said
in one breath that nothing reaches cos until save, and in the next that preview
bytes come from `incremental_section()`, which is a method reading `self.edits`.
Both cannot be true. Getting preview bytes out of the old API would mean writing
the overlay into cos's edit map on every commit and having no way to take it
back; `cos::Document` is not `Clone` and owns a `Reader`, four `RefCell` caches
and `Rc<ObjectStream>` values, so there is no cheap scratch copy to write into;
and each `set_object` calls `forget_parsed_objects()`, so projecting N edits
drops the parsed-object cache and the object-stream cache N times.

**Resolution: the overlay is an argument, not state.** P1 gives cos a section
builder that takes the overlay instead of reading its own:

```rust
// cos, public
pub enum PendingEdit {
    Set { generation: u16, object: Object },
    Delete { generation: u16 },
}

impl Document {
    /// The bytes a save of `overlay` would append, or `None` when it would
    /// append nothing. Takes `&self`: no cache is dropped, and the document's
    /// own edit map is neither read nor written.
    pub fn section_for(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        trailer_edits: &Dict,
    ) -> Result<Option<Vec<u8>>>;

    pub fn save_overlay_to_path(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        trailer_edits: &Dict,
        path: &Path,
    ) -> Result<()>;
}
```

`incremental_section()` becomes `self.section_for(&self.edits, &self.trailer_edits)`
and keeps its behaviour, so nothing that exists changes. A method rather than a
free function because every input it needs is private to `Document` (`reader`,
`xref`, `provenance`, `prev_startxref`, `trailer`, `original_len`, `locate`,
`get`), and a free function would have to make all of them public to save one
`self`. With this, T2 and T4 stop contradicting: save and preview are the same
call with the same argument, so they cannot disagree by construction, and no
scratch document exists.

```
core::edit::Overlay  =  BTreeMap<u32, ObjectState>       // net state, per object number
core::edit::ObjectState = { generation: u16, object: cos::Object }
core::edit::Change   = { number, before: Option<ObjectState>, after: Option<ObjectState> }
core::edit::Entry    = { label: &'static str, changes: Vec<Change> }
core::edit::History  = { entries: Vec<Entry>, cursor: usize, saved_mark: Option<usize> }
```

There is **no `Deleted` variant**, because T5's rule is that M3 frees no object
number: a removal is a rewrite of the referrer, and the removed object simply
stops being reachable. So `ObjectState` is one shape and `Option<ObjectState>` is
two states, not three. `generation` is carried because a `cos::Object` written
back to an existing number has to be written at the generation that number
already has, and reading it back out of the base at write time would be a second
source for a fact the change already knows.

Projecting `Overlay` onto `BTreeMap<u32, cos::PendingEdit>` for `section_for` is
then a one-to-one map onto `PendingEdit::Set` alone; `PendingEdit::Delete` is
never produced.

Undo applies each `Change`'s `before`; redo applies each `after`. **This is
deliberately more than "dropping overlay nodes"**, and PLAN.md's phrasing is
wrong for a reachable case: an edit that overwrites an object a previous edit
already overlaid (changing a highlight's colour twice) must restore the previous
overlay state rather than drop the node. Memory stays proportional to changed
objects, which is what PLAN.md actually cares about.

**What `before: None` means, and the one capture rule that keeps it meaning that.**
Left undefined, `None` produces a silent no-op on the far side of a save: undo an
edit to an object that existed in the file, save, undo, and nothing is restored,
because the overlay was cleared at save and the base *is* the edited value, so
dropping a node that is not there reverts to the edit. That is data loss with a
green test suite, and P2's verification as first drafted covered every case
except it.

The rule, an invariant rather than a convention:

> **`before: None` means the object number was not in the overlay immediately
> before this change, and undoing the change removes it from the overlay.
> Nothing else is ever `None`.**

One capture rule makes that total:

> **At edit time, `before` for an object that exists in the base document is read
> out of the base through `cos::Document::get` and stored concretely** as
> `Some(ObjectState { generation, object })`. It is never left `None` on the
> grounds that "the overlay has no node for it yet". `None` is produced only by
> the reservation counter, for a number nothing has ever written.

With that rule, `None` needs no rebasing across a save and the meaning is the
same on both sides of one. Pre-save, dropping a `None` node means the object
never existed. Post-save, dropping it means the object's bytes stay in the file
but nothing reachable names them, because the referrer's own `Change` in the same
`Entry` carries a concrete `before` and is restored with it. The object graph the
user sees is identical in both cases, which is the property that matters; the
residual bytes are what the core invariant promises about everything this project
writes.

Object numbers: `core` allocates from its own reservation counter, seeded from a
new `cos::Document::next_object_number()`. An undo that drops an allocation
simply leaves a gap; PDF object numbers need not be dense. `add_object` is never
called at all, speculatively or otherwise, so cos's unwithdrawable edit list
stays empty for the whole session.

A **transaction** groups a gesture into one stack entry: an ink stroke is
hundreds of pointer events, one annotation, one `Ctrl+Z`. An annotation is
three object changes (the annotation dict, its appearance stream, the page dict
whose `/Annots` gained a reference) and one stack entry. **The referrer change is
not optional bookkeeping: it is the change that makes undo work at all**, since
under the free-nothing rule reachability is the only thing an undo can alter
about an object it created.

**T3. One section per save, and what a save of nothing writes.**

Guarantee 2 asks that "an edit appends one incremental section that truncates
away". The resolution is **one section per save, not per edit**, and cos already
behaves this way: the section is materialized from the net overlay at save time.
Ten edits then one save is one section carrying the net object writes of all ten.

Three consequences the plan states rather than discovers:

- **Edit, then undo, then save writes nothing at all** on a clean document. The
  net overlay is empty, `section_for` returns `None`, and the save copies the
  original bytes with no appended section. That is guarantee 1's no-op rule
  applied to a document the user did edit, and it is a real test.

  **The exception, which the first draft got wrong:** `has_pending_changes()` is
  `!edits.is_empty() || !trailer_edits.is_empty() || !provenance.is_clean()`
  (`document.rs:961-963`), so it is **true on every repaired document even with
  an empty overlay**, and `section_for` must keep that behaviour or a repair
  would stop being written. So "a save with an empty overlay writes nothing" is
  true for a clean document and false for a repaired one, where the save writes
  the repair and nothing else. Every test that asserts byte-identity on a no-op
  save has to carve repaired documents out and assert the positive case for them
  instead: a repaired document's no-op save appends exactly the repair section
  and the corrupt original bytes survive underneath. That is the same split
  `PLAN.md`'s "for every well-formed corpus file" already makes, and P3's
  guarantee-1 test as first drafted would have failed on every repaired file in
  `external/`.
- **The net overlay collapses.** Adding an annotation and then deleting it in
  the same session leaves the page's `/Annots` back at its original value, so
  the page object is not in the overlay either, and the save writes nothing.
  Collapsing has to be by value comparison against the original object, not by
  a dirty flag, or the file grows a section that changes nothing.
- **After a save, `core` reopens the `cos::Document` from the written bytes,
  clears the overlay, and rebases the history.** The reopen makes the `/Prev`
  chain correct by construction for the second save and matches the existing
  `ExportSnapshot::open` pattern. The overlay clears because the reopened
  document now *is* the state it described. The rebase is T2's
  `History::rebase_on_save`, and skipping it is the defect that makes undoing a
  creation across a save a silent no-op. The caches keyed on edited pages are
  invalidated; the rest survive. Nothing has to be cleared inside cos, because
  nothing was ever put there.

**T4. What the canvas shows before a save: preview bytes, not a second renderer.**

An unsaved annotation has to appear on the page. hayro renders from a byte
buffer, so there are two ways: composite the pending edits as `render::Overlay`
primitives, or hand hayro `original ++ pending section`.

**The preview buffer wins, and it is the strongest single decision in this
plan.** T2's `section_for(overlay, trailer_edits)` builds exactly those bytes,
and it is the same call the save makes with the same argument. Rendering from
them means hayro draws every M3 annotation through its own appearance stream
path, with no second renderer to keep in agreement, and it means what the user
sees is byte-for-byte what a save would produce. A whole class of "the preview
disagrees with the saved file" bugs cannot exist, and it cannot exist *by
construction* rather than by two code paths being kept in step.

The split with overlays is clean: `Overlay` covers the **in-progress gesture**
(the rubber band, the ink stroke still under the stylus, the marquee), which is
what `ToolPlugin::overlays` already returns and what tools-basic already uses.
Committed edits go through the preview buffer. A tool never draws its own
committed result.

Cost, stated in terms of `section_for` because that is what runs: a committed
edit calls `section_for(&overlay, &trailer_edits)`, which serializes the changed
objects, builds the table rows, allocates a new `Arc<Vec<u8>>` for
`original ++ section`, and the render worker builds a new hayro `Pdf` from it.
`Pdf::new` was measured at about 1.4 microseconds per page in the M1 spike, so a
1000-page document is about 1.4 ms per commit.

Two costs the naive design would have added and this one does not: `section_for`
takes `&self`, so **neither cos cache is dropped** (the old projection called
`forget_parsed_objects()` once per edit, clearing both the parsed-object cache
and the object-stream cache), and there is **no scratch `cos::Document`**, which
is fortunate because `Document` is not `Clone`.

**The real risk that remains is a repaired document**, where
`needs_full_table()` forces a section that walks every live object and
materializes every compressed one. That is per commit, not per save, and it is
unchanged by any of the above. P3 benches it and, if it misses budget, debounces
preview rebuilds on repaired documents behind a named constant rather than
pretending the cost is not there.

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

**The rule that makes the first bullet true, promoted here because the first
draft left it implicit and then broke it one section later: M3 frees no object
number.** Removal is always expressed by rewriting the referrer, never by
`cos::delete_object`. What a removal leaves behind is unreferenced garbage: bytes
still in the file, reachable only by truncation, pointing at each other in an
internally consistent subgraph that nothing live names.

The first draft violated this in one clause and produced exactly the class the
flat rewrite exists to remove. It said a removed page's annotation objects "must
go" while saying nothing about the removed page's own dict, which leaves that
dict in the file with `/Annots` naming freed numbers, and leaves the orphaned
internal `/Pages` node with `/Kids` naming that dict. Freeing the leaves and
leaving the parent is the one choice that is wrong: it is the only one that
manufactures a dangling reference, and P1's validator cannot see it, because the
objects doing the dangling are not objects the section writes.

So, per object class, for a removed page: the page dict is **not** rewritten and
**not** freed; its `/Annots` targets, its appearance streams, its `/Popup`
partners and its content streams are **not** freed; the internal `/Pages` nodes
it hung under are **not** freed. All of them become garbage together, and the
garbage subtree is internally consistent, so no reference in the file resolves to
a free entry. Nothing in M3 needs a free entry for anything.

What this rule costs, said plainly rather than discovered:

- **The file does not shrink when the user deletes a page**, and its object
  number space is not reclaimed. Both are properties of incremental update, not
  of this rule; the bytes were always going to stay. **Compress / Reduce File
  Size (P14) is the operation that reclaims**, and it does so through
  `write_new`'s full rewrite, which is why the parity row calls it destructive
  and the UI has to as well.
- **"Delete page" is not "remove the page's content from the file."** The page
  is fully recoverable from the bytes underneath. That is the core invariant
  working as designed and it is what redaction (M5) exists to do differently.
  M3's UI must not imply otherwise.
- **`cos::delete_object` gains no M3 caller.** Its doc comment
  (`document.rs:868`) and `known-issues.md` both name M3's `tools-organize` as
  the consumer that would fix up the page tree after using it. Under this rule
  the fix-up is not "after `delete_object`", it is "instead of it", and section
  8 item 7's ledger rewording has to say so rather than claiming P5 closes the
  entry by doing the walk.

What the transformation must also carry, and what an adversarial reviewer will
check it forgot:

- `/Annots` on surviving pages (carried through untouched; a removed page's stay
  attached to its own now-garbage dict, per the rule above).
- `/PageLabels`, a number tree keyed on page index, which every reorder
  invalidates.
- Named destinations (`/Dests`, `/Names /Dests`) and the outline's `/A` and `/D`
  entries: a bookmark to a removed page is a broken bookmark, not a parse error.
- `/StructParents` and the structure tree (T6).
- Page-level `/Tabs` and `/Group`, which are per-page and survive untouched.
- **`/AcroForm /Fields`**, whose entries are widget annotations that live on
  pages. M3 authors no form fields, but it deletes pages, and a form document is
  a completely ordinary thing to delete a page from. A widget on a removed page
  leaves `/Fields` naming an object hanging off a garbage page dict, so the
  field list has to be rewritten to drop it, along with any `/Parent` field node
  left with no children. Missing this produces a form whose field tree and page
  tree disagree, which Acrobat reports and this plan would not have.
- **Article beads**, which the first draft dismissed as "`/B` survives
  untouched". Page-level `/B` on a *surviving* page does survive untouched, and
  that is the trap: the beads it names form a doubly-linked ring through `/N`
  and `/V`, and the ring runs through the removed page's beads too. Removing a
  page leaves a circular chain with a garbage node in it and a thread `/T`
  outliving beads that no live page reaches. The ring has to be re-linked past
  the removed page's beads, and a thread all of whose beads are gone has to be
  dropped from `/Threads`.
- **Link annotations on surviving pages whose destination is a removed page.**
  This is a different object from the outline entries already listed: a
  `/Link` annotation with `/A` a `/GoTo` action, or `/Dest` naming a removed
  page directly or by name. The outline fix-up walks `/Outlines`; this one walks
  every surviving page's `/Annots`. Both are needed and neither finds the other's
  case.

Each of these is a distinct fix-up function on the same edit, individually
testable, and each has a fixture named in P5.

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
  `the_find_keystroke_opens_the_find_bar` (`tabs.rs:4579`).
- **Every test must fail when the behaviour it names is removed.** Each package's
  review-risk list names the specific mutation that must break its tests, and
  the reviewer runs it. A test that passes against a no-op is a defect.
- **Assert on structure, not on substrings of files.** Parse the PDF, walk the
  object graph, compare objects. A recent package needed four review rounds
  because it hand-rolled scanners instead of parsing. This applies to
  `guarantees.rs`'s workflow assertions too, which CR-005 already flagged as
  evadable tripwires.
- **A verification bullet that names an `external/` fixture also names the CI
  step that fetches it and the re-run that makes it mandatory.** `corpus/external`
  is gitignored and `crates/cos/tests/common/mod.rs:30-47` turns an absent
  directory into a printed `SKIPPED` and a pass, so a fixture CI never fetches is
  a fixture the assertion never saw. P1c lands the fetch-and-re-run pair for the
  three sets M3's fixtures come from; every package below that names one names
  P1c. Guarantee 6 spent a milestone green and unmeasured on exactly this, and it
  is the one M2 lesson this plan was in the middle of repeating.
- **Every package's verification names the `cargo` invocations that run it.**
  Not "the tests pass": the commands, with their feature flags, because the flags
  are the part this project gets wrong (`cargo test --workspace` compiles no
  shell code at all). A verification section with no command in it has not said
  how it is run and cannot be reproduced by a reviewer.
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

**The acceptance test, and why the obvious one is not enough.** A green suite
does not prove a 7278-line split preserved behaviour, because not every branch in
that file has a test: a dropped match arm or a method that lost its only call
site can leave all 500-odd shell tests green. The acceptance test is therefore
mechanical and does not depend on coverage.

1. **An item inventory, compared as a set.** Before the split, emit one line per
   top-level item and per `impl` method in `tabs.rs`: its name, its signature,
   and a hash of its body with whitespace and comments normalized. After the
   split, emit the same across the new files. **The two sets must be equal.** A
   moved body hashes the same; a changed body, a lost method or an invented one
   all show up as a set difference. This is stronger than `git diff --stat`,
   which says nothing about a body that moved and changed in the same commit.
2. **Exhaustive dispatch, enforced by the compiler.** Every dispatch match P0
   relocates (`run_main_menu_command`, the canvas and thumbnail context tables,
   the pane action apply) loses its wildcard arm if it has one, so a lost arm
   becomes a compile error rather than a silent fallthrough. Removing the
   wildcards is part of P0 and is the mechanism that makes the split safe, not a
   drive-by improvement.
3. The suite, at the **same test count** before and after, plus clippy, plus the
   accessibility probe (`--features a11y-probe --test a11y_probe`), which proves
   the tree assembly moved intact.

**Verification.**
- The item inventory before and after is identical as a set, with the two listings committed to the PR so a reviewer can diff them rather than trust a claim.
- Every relocated dispatch match compiles without a wildcard arm.
- `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` green at the same test count; `cargo test -p onionskin-app --features shell` green; `cargo clippy -p onionskin-app --no-default-features --features shell,shell-test-support --all-targets -- -D warnings`.
- `cargo test -p onionskin-app --no-default-features` still green: the split must not disturb the feature gating that guarantee 5 rests on.

**Review risk.** Whether this is a refactor or a rewrite wearing a refactor's
name: the reviewer should reject any behaviour change, including "obvious"
improvements, and the item inventory is what makes that reviewable rather than a
matter of trust. Whether the split lines follow M3's package boundaries or the
author's taste, which is the difference between it buying parallelism and it
buying nothing. Whether `frame_state.rs` became a second god object. Whether a
wildcard arm was preserved "for now", which would silently readmit the failure
mode the inventory exists to catch.

**Mutation that must break its tests.** Not a mutation of the product: a mutation
of the acceptance procedure itself, because that is the thing being trusted.
**Take a copy of the pre-split `tabs.rs`, delete one match arm and one whole
`impl` method from it, run the split procedure on that copy, and confirm the item
inventory reports exactly those two as missing and nothing else.** If it reports
nothing, the inventory is decorative and the split is unproven no matter how
green the suite is. Run it, and record the two deliberately deleted names in the
PR alongside the inventory's output.

### P1. cos: the edit surface M3 needs

**Goal.** Five cos additions, landed together and reviewed once, that every
kernel package above depends on.

1. `Document::next_object_number(&self) -> u32`, so `core` can reserve numbers
   without calling `add_object` speculatively (T2). Trivial accessor, no new
   state.
2. `Document::sections(&self) -> Result<Vec<Section>>` where
   `Section { start: u64, end: u64, startxref: u64 }`,
   walking the `/Prev` chain from `startxref` back to the first table. This is
   the skins panel's data source and there is nothing today: `original_len()`
   gives one boundary, not a chain. Must terminate on a cyclic `/Prev` (hostile
   documents do this) and must report a chain it cannot follow as a repaired
   document rather than silently truncating the list. There is deliberately **no
   `prev` field**: the returned `Vec` is the chain, in order, so a section's
   predecessor is the element before it and a stored `prev` is a second copy of
   that fact that can disagree with the first.
3. `Document::write_new(objects, trailer) -> Result<Vec<u8>>`: a complete
   document serialization, for the documents Onionskin authors (T8). Named
   consumers in M3: P12 (combine, split, extract), P14 (create from image,
   compress/flatten), P10 (the comment summary). This is the `flatten`
   primitive the crate's charter names; it lands now because it has three real
   callers, and M5's `redact` inherits it. Its serializer is `writer.rs`'s
   classic-table path; **it does not gain object streams or a cross-reference
   stream**, which are scoped in P14's entry and are not an implicit clause of
   any word in this plan.
4. **The overlay-taking section builder and save**, per T2: `pub enum
   PendingEdit`, `Document::section_for(&self, overlay, trailer_edits)` and
   `Document::save_overlay_to_path(&self, overlay, trailer_edits, path)`.
   `incremental_section()` and `save_to_path()` are re-expressed as calls to
   them with `self.edits`, so their behaviour and their tests are unchanged.
   This is the single piece that lets `core` own the overlay outright and lets
   save and preview share one path; without it T2 and T4 contradict each other.

   It grows no ability to resurrect a freed number, because T5's rule is that M3
   frees none. `set_object` keeps its refusal and its test
   (`crates/cos/tests/delete.rs:198`) untouched.
5. **Two reference checks, one cheap and one complete**, because one check cannot
   be both and pretending otherwise is what the first draft did.

   **`Document::audit_references(&self) -> Result<Vec<Dangling>>`** is the
   complete one: it walks every in-use object in the file and resolves every
   reference it contains against the xref, reporting each `(holder, target)` pair
   whose target is free or absent. It is O(file), it is a query rather than a
   gate, and it is what P5, P11, P12 and P14's verification run on every fixture
   output. Complete checking belongs in the test suite, where paying O(file) once
   per fixture is exactly right.

   **The gate inside `section_for`** is the cheap one, and its scope is stated as
   a contract: *every reference in every object this section writes must resolve,
   after this section, to an object that exists; and no object this section
   writes may reference a number this section frees.* Both halves are
   proportional to the edit. The gate refuses with a typed error naming the
   holder and the target.

   **What the gate cannot see, said out loud:** an object *already in the file*,
   not rewritten by this section, pointing at a number this section frees. No
   walk bounded by the edit can find it, and the first draft's "walks only the
   objects the section writes" quietly claimed to. Two things close it instead of
   a bigger walk. First, **T5's rule that M3 frees nothing** makes the class
   empty: a section with no free entries cannot create a dangling reference at
   all, which is checkable from the section in constant time and is the gate's
   first assertion in practice. Second, `audit_references` over the whole output
   is what every page-mutating package's verification asserts is empty, which
   catches the class if the rule is ever broken. Neither alone is enough and the
   plan says which does which.

**Rows closed.** None. Backs every row in P3, P5, P11, P12, P14 and P19.

**Files.** `crates/cos/src/document.rs`, `crates/cos/src/writer.rs`,
`crates/cos/src/lib.rs`, `crates/cos/src/error.rs`; new
`crates/cos/tests/sections.rs`, `crates/cos/tests/write_new.rs`; extend
`crates/cos/tests/delete.rs`.

**Depends on.** Nothing. Day-one root.

**What exists to build on.** `prev_startxref` is already a field
(`document.rs:106`). `writer::incremental_section` (`writer.rs:168`) already
builds classic xref tables and `trailer_for_new_section` (`writer.rs:157`)
already strips xref-stream-only trailer keys. The full-table path in
`incremental_section` (`document.rs:1053-1085`) already enumerates every live
object, which is most of `write_new`. `free_list_rows` (`document.rs:995`)
already knows which numbers a section frees, which is the gate's cheap input.
`incremental_section` is already `&self` and already reads `self.edits` in one
place, so item 4 is a parameter change, not a rewrite.

**Verification.**
- `sections()` over every `external/` corpus file that has more than one `%%EOF`: the reported chain's byte ranges partition the file with no gap and no overlap, and the last section's `end` equals the file length. Name the fixtures; a file set chosen by "some corpus file" is not a proof. This walk is one of the suites P1c's CI step makes mandatory, or it reports a pass over an empty file list.
- A hand-built fixture with a cyclic `/Prev` terminates and reports the cycle rather than looping.
- `write_new` output reopens through `Document::open` (not `open_repairing`), has the stated page count, and round-trips: `write_new` then `open` then `save_to_vec` with no edit is byte-identical.
- **`section_for` and `incremental_section` agree.** For every existing edit test, `section_for(&document.edits_for_test(), &trailer_edits)` returns the same bytes as `incremental_section()`. This is what makes item 4 a refactor rather than a second serializer, and it is asserted rather than argued.
- **The gate, three cases.** A section whose written object references a number that section frees is refused with the typed error naming both. A section whose written object references a number the *base file* marks free is refused the same way. The legal case, a section that frees an object nothing references, still succeeds, which is what `crates/cos/tests/delete.rs` already exercises and what stops the gate being a constant `Err`.
- **`audit_references`, three cases.** It is empty on every unmodified `external/` fixture, or it is reporting noise and no package can assert on it. It is non-empty on a hand-built file with one deleted target, naming that exact pair and no other. It finds a reference buried in a nested array inside a stream dictionary, which is the shallow-walk failure mode.
- **The two are not the same check**, asserted by construction: build a file whose *unrewritten* object points at a number a section frees, emit that section through `section_for`, and assert the gate accepts it while `audit_references` on the result reports it. That is the honest statement of the gate's limit, as a test rather than as a caveat, and it is what makes T5's free-nothing rule load-bearing rather than decorative.
- `cargo test -p onionskin-cos`, `ONIONSKIN_CORPUS_REQUIRED=1 cargo test -p onionskin-cos`, and `cargo clippy --workspace --all-targets -- -D warnings`.

**Review risk.** Whether either reference walk understands every place
an object number can appear (dict values, array elements, nested streams'
dictionaries) or only the shallow ones, which would make it pass on exactly the
cases P5 gets wrong. Whether the gate and `audit_references` share one walk or
grow two that can disagree; they must share, and the difference between them must
be the *set of objects walked* and nothing else. Whether `sections()` reports
what it parsed or what is in the file, given cos's laziness (the existing
`recovered_boundaries` entry in `known-issues.md` is the precedent, and this
accessor must not repeat it). Whether `write_new` invents a second serializer
instead of reusing the writer, and whether it quietly acquired object-stream or
cross-reference-stream output, which is P14's scoped deliverable and not this
one's. Whether `section_for` changed any byte `incremental_section` used to emit.
**Mutation that must break its tests:** making the gate always return `Ok`
must fail the dangling-reference test; making `audit_references` return an empty
vector must fail its hand-built fixture; making `sections()` return only the last
section must fail the partition test; making `section_for` ignore its `overlay`
argument and read `self.edits` must fail the agreement test on any document with
a non-empty overlay.

### P1b. cos: empty-user-password decryption

**Ruled in, 2026-09-02** (section 9, ruling A): read-only, with editing disabled
at open. This is a definite package, not a conditional one.

**Goal.** Open the documents Acrobat opens without prompting.

1. **The measurement**, which is also the input to the `known-issues.md` entry
   m2-viewer assigned at M2 and nobody wrote. Its output is a table, committed
   in this package as `docs/evidence/encryption-classes.md`, with one row per
   corpus file that `cos::Document::open` refuses with `Error::Encrypted`
   (roughly 35: 5 pdf-association, 4 verapdf, 13 hayro/custom, 12 hayro-corpus,
   1 fuzzed, per `docs/spikes/m1-cos.md`), carrying five columns: the file path,
   `/Encrypt /V` and `/R`, whether **algorithm 6 or 11 validates an empty user
   password** (the permissions-only class Acrobat opens without prompting),
   whether it validates an empty **owner** password, and the `/P` permission
   bits decoded into named flags. The two counts that matter, and that the
   ledger entry needs, are how many files fall in the permissions-only class and,
   within that class, how many have `/P` bit 4 (modify) **set**, because those
   are the ones ruling A's residual makes worse rather than better. The generator
   is a committed test that fails if the tally drifts, not a one-off script, so
   the numbers cannot go stale silently. **The orchestrator lands the
   `known-issues.md` entry from it**; this package does not touch that file.
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

**Rows closed.** None of M3's 99. It moves `Open an encrypted document` from M6
to `partial` at M3, a scoreboard change outside the 99 that must be recorded as
one, with the Notes naming the read-only scope and the M6 write path.

**The residual the user accepted, stated plainly.** Writing stays M6, so editing
is disabled at open on every encrypted document. For a file whose `/P` bits
forbid modification that matches Acrobat. **For a file whose `/P` bits allow
modification, Onionskin will open it, render it, print it and export it, and
refuse to let the user change it, which Acrobat would allow. That is a real
regression against Acrobat, it was weighed, and it was accepted** in exchange for
the class no longer being refused outright. It is not a defect to be filed later;
it is the shape of the ruling, and the open-time notice must say so in words a
user understands rather than naming a milestone alone.

**Files.** `crates/crypto/src/{lib,standard,algorithms,filters}.rs`,
`crates/crypto/Cargo.toml`, `crates/cos/src/{document,object,stream}.rs`,
`crates/cos/Cargo.toml`, new `crates/cos/tests/encrypted.rs`, new
`crates/cos/tests/encryption_classes.rs` (the measurement), new
`docs/evidence/encryption-classes.md` (its output),
`crates/core/src/session.rs` and `crates/app/src/shell/chrome/tabs.rs` (the
open-time notice and the editing gate).

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
- The measurement's tally is asserted, not printed: the test fails if the number of files in either class changes, so a corpus refresh that alters the picture is visible rather than silent.
- The open-time notice appears for an encrypted document and names the editing restriction in words, asserted on the notice text; the edit tools report disabled through the same `Requirement` query P7 owns, not through a second flag.
- **Runs.** `cargo test -p onionskin-crypto`; `cargo test -p onionskin-cos`; `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` for the open-time notice and the editing gate, plus the matching clippy.
- **Corpus.** The class tally and every open-the-encrypted-file assertion walk `external/`, so they run behind P1c's fetch step and are re-run under `ONIONSKIN_CORPUS_REQUIRED=1`. Without that, the tally this package exists to keep from going stale is computed over an empty set and passes.

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

### P1c. CI: make the corpus this plan's fixtures come from reach CI

**Goal.** Every assertion in this document over an `external/` fixture currently
runs in CI over an empty file list and reports a pass. This package fixes that,
once, for the whole milestone.

**The measurement, before the fix.** `.github/workflows/ci.yml`'s `test` job
fetches **nothing**. It generates `corpus/malformed`, runs `cargo test
--workspace`, and re-runs `cargo test -p onionskin-cos --test repair` under
`ONIONSKIN_CORPUS_REQUIRED: 1`. That is the whole two-step story and it covers
one set. `corpus/external` is fetched only by the `bench` job, only the
`hayro-corpus` set, only for `cargo bench -p onionskin-core`.
`crates/cos/tests/common/mod.rs:30-47` returns `None` and prints `SKIPPED` when a
directory is absent, so every walk over it passes having done nothing.

What that means today, item by item, because "the corpus is not in CI" is easy to
nod at and hard to feel:

- **Guarantee 1 in CI means "three tracked seeds round-trip."** `roundtrip.rs`
  has seven enforcing tests and six of them walk `external/`. All six skip.
- **P1's `sections()` partition test never runs**, because its subject is "every
  `external/` file with more than one `%%EOF`".
- **P1b's encryption-class tally never runs**, so the number it exists to stop
  going stale can go stale silently. That is the package's entire premise.
- **P5's deep-page-tree, `/PageLabels`, `/AcroForm`, `/Threads` and `/Link`
  fixtures never run**, and P5 is the highest-correctness-risk package in the
  milestone.
- **P11's importer render comparison, P12's combine fixtures and P14's compress
  fixture never run.**

The plan applies its own two-step rule to exactly one corpus, P4's tagged set,
and asserts it everywhere else.

**Deliverable: the same two steps guarantee 6 already has, for the sets M3 uses.**

1. A cached fetch in the `test` job: `./corpus/fetch.sh` with no arguments, which
   is `hayro`, `pdf-association` and `verapdf` (`corpus/fetch.sh:50`) and is where
   every fixture this plan names comes from. Cached on the same key shape the
   `bench` job already uses, over `corpus/fetch.sh`, its helper scripts and the
   checksum manifests, so the cache invalidates when a pinned revision moves.
   `hayro-corpus` stays opt-in and stays with the `bench` job: 159 MB for 41
   files, and no fixture here needs it.
2. A second step re-running the suites those fixtures live in under
   `ONIONSKIN_CORPUS_REQUIRED: 1`: `cargo test -p onionskin-cos`, `cargo test -p
   onionskin-core`, and, once P1b lands, its encryption suite. Generating or
   fetching a set is not proof it was measured; only this step turns a silent
   skip into a failure, which is what makes disabling step 1 visible.

Both pinned to `runner.os == 'Linux'`, like the two that exist, and for the
reason those give: corpus assertions are byte and structure work with no platform
dimension, and running the fetch on the three-way matrix is three caches and
three chances to flake for one claim.

**The tripwire, or this package is itself deletable.**
`crates/app/tests/guarantees.rs` asserts the pair the same way
`every_malformed_file_opens_and_repairs_into_a_new_section` asserts its own:
exactly one step per command, the allowed key set, `if` pinned to the value
rather than merely present so `if: false` is not an off switch, and
`ONIONSKIN_CORPUS_REQUIRED` asserted as `1` on the re-run. Reusing that test's
existing helper rather than writing a second one, since it is already the shape.

**Rows closed.** None. It is the reason the rest of the ledger's verification
means anything.

**Files.** `.github/workflows/ci.yml`, `crates/app/tests/guarantees.rs`,
`corpus/README.md` (which sets CI fetches and why).

**Depends on.** Nothing, and **it lands before P1**, because P1's own
verification names an `external/` sweep. It is the fourth day-one root.

**What exists to build on.** `corpus/fetch.sh` already skips a set already on
disk, so a cache hit makes step 1 a no-op. The `bench` job already demonstrates
the cache key. `guarantees.rs` already has the workflow-parsing helpers and the
one-step-per-command assertion.

**Verification.**
- The re-run step **fails** when the fetch step is removed, demonstrated on a scratch branch and the failure output recorded in the PR. Without that demonstration this package is two YAML steps nobody has proven do anything, which is precisely guarantee 6's history.
- `cargo test -p onionskin-app --test guarantees` asserts both steps and their pins; deleting either from `ci.yml` fails it.
- A cache hit makes the fetch a no-op, asserted by the step's own log rather than by wall time.
- **The `test` job's wall time before and after is recorded in the PR.** This is the step that makes CI slower, and not saying so is the same dishonesty pointing the other way.

**Review risk.** Whether the re-run names every suite whose fixtures come from
`external/` or only the ones somebody remembered, which would leave the same hole
in a narrower place. Whether the re-run is a second full `cargo test --workspace`
(wasteful, and it would re-run the shell suite for no reason) or the named
suites. Whether the cache key covers the revisions `fetch.sh` pins, or a moved
revision silently serves the old corpus forever. Whether putting the fetch on the
matrix instead of one runner was considered and rejected with a stated reason
rather than by default. **Mutation that must break its tests:** deleting the
fetch step must fail the re-run step; deleting the re-run step must fail the
`guarantees.rs` assertion; changing the re-run's `if` to `false` must fail it
too.

### P2. `core::edit`: the edit graph, transactions and undo/redo

**Goal.** The spine of M3. One place that knows what has changed, one stack that
can take it back, and one typed vocabulary of edits that every plugin speaks.

Shapes, per T2:

```rust
/// One overlaid object. There is no `Deleted` variant: M3 frees no object
/// number (T5), so a removal is a rewrite of the referrer.
pub struct ObjectState { generation: u16, object: cos::Object }
/// `before: None` means, and only ever means, "object `number` was not in the
/// overlay immediately before this change". See the capture rule below.
pub struct Change { number: u32, before: Option<ObjectState>, after: Option<ObjectState> }
pub struct Overlay { states: BTreeMap<u32, ObjectState>, next_number: u32 }
pub struct Entry { label: &'static str, changes: Vec<Change> }
pub struct History { entries: Vec<Entry>, cursor: usize, saved_mark: Option<usize> }
pub enum DocumentEdit { /* the typed vocabulary, grown by P5 and P6 */ }
```

**The base-capture rule, which is this package's sharpest correctness
obligation** (T2). Leaving `before: None` to mean "the overlay had no node for
this number" produces a silent no-op on the far side of a save: edit an object
that exists in the file, save, undo, and nothing is restored, because the save
cleared the overlay and made the base the edited value. That loses data with a
green suite, and P2's verification as first drafted covered every case except it.

> **`Change::before` for a number that exists in the base document is read
> through `cos::Document::get` at edit time and stored as a concrete
> `Some(ObjectState { generation, object })`.** `None` is produced only by the
> reservation counter, for a number nothing has ever written.

With that, `None` means one thing on both sides of a save and needs no rebasing.
Projecting `Overlay` onto `cos::PendingEdit::Set` for `section_for` is a
one-to-one map with nothing to decide, and **nothing is ever written into cos's
own edit map**.

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
- **The base-capture rule, by name:** editing an object that exists in the base document records `before` as `Some(ObjectState { .. })` holding the base object by value, asserted on the `Change` itself and not inferred from undo working. A test that only checks undo passes against an implementation that reads the base at undo time, which stops working the moment the base is reopened, which is the whole bug.
- **No `Change` ever carries a `before: None` for a number the base document has**, asserted as a property over the generated edit sequences. That is the invariant stated as a check, and it is what makes `None` safe to drop on either side of a save.
- Redo after undo restores exactly; a new edit after an undo truncates the redo tail, asserted.
- The saved mark: it survives undo and redo, and moves only when P3's save moves it.
- An aborted transaction leaves both the overlay and the reservation counter unchanged, so an abort cannot leak an object number.
- `cargo test -p onionskin-core` with no window and no `shell` feature; `cargo test -p onionskin-app --no-default-features` still green.

**Review risk.** Whether `Change::before` is captured from the overlay or from
the base document, which are different whenever an object has already been
edited, and getting it wrong makes exactly the second edit of an object
un-undoable. Whether `before: None` is ever produced for anything other than a
freshly reserved number, which is the invariant the whole undo-across-a-save
story rests on. Whether `rebase_on_save` is called on the `Save As` path as well
as `Save`, since both reopen. Whether the overlay collapses by value against the original (T3) or
by a dirty flag, which would make edit-then-undo-then-save append an empty
section. Whether `DocumentEdit` grew a `Raw(Change)` escape hatch, which would
delete the entire justification for the closed enum. Whether `History` is
unbounded (a 200-page ink session is a lot of `Entry`) and whether its bound, if
any, is a named constant derived from something. **Mutation that must break its
tests:** replacing undo with "drop the last overlay node" must fail the repeated-
overwrite sequence; making the overlay collapse a no-op must fail the
edit-then-undo-then-save test in P3; leaving `before` as `None` for an object the
base document has must fail both the capture test here and P3's
`edit, save, undo, save` object-graph comparison, and if it fails only the second
the unit test is not carrying its weight.

### P3. `core::save`: one section per save, generations, and the preview buffer

**Goal.** Turn the overlay into bytes, and make what the user sees be what the
save writes.

Four pieces:

1. **Save.** Hand the net overlay to `cos::Document::save_overlay_to_path`
   (P1 item 4), reopen from the written bytes, **clear the overlay**, advance the
   saved mark, invalidate the caches for edited pages. Nothing is ever written into
   cos's own edit map, so nothing has to be withdrawn from it. `Save As` is the
   same with a different destination and no truncation relationship to the
   original, and it takes the same reopen. A save whose net
   overlay is empty writes nothing at all **on a clean document**; on a repaired
   one it writes the repair, per T3.
2. **The preview buffer** (T4). `Document::preview_bytes(&mut self, filter:
   AnnotationFilter) -> Result<Arc<Vec<u8>>>` returns
   `original ++ section_for(&overlay_with_filter_applied, &trailer_edits)`,
   which is the same call the save makes, so preview and save cannot disagree.
   The render worker renders from it.

   **The cache key is `(overlay generation, filter)`, not the overlay generation
   alone.** The filter changes the bytes (T7 sets `/F` bit 2 on the excluded
   annotations), so keying on the generation alone returns one mode's bytes for
   another mode's request: the print dialog would show Document-and-Markups
   while the user has Document-Only selected, and the four-mode assertion in
   P15's verification would pass because it never asks twice at one generation.
   The cache holds at most one entry per live filter, which is bounded by the
   four `AnnotationFilter` modes plus the canvas's unfiltered one.
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

**What exists to build on.** P1's `section_for` and `save_overlay_to_path`,
`save_to_path`'s temp-file-and-rename with permission preservation, and
`has_pending_changes` are all real and tested. `ExportSnapshot::open`
(`session.rs:163`) is the reopen pattern. The render worker already builds its
`render::Document` from an `Arc<Vec<u8>>` inside its own spawned closure, so
feeding it preview bytes is a new `Arc`, not a new threading model.

**Verification.**
- **Guarantee 1, at the level the guarantee means it.** Open every corpus seed and every **well-formed** `external/` file through `core::Document`, save with no edit, assert byte-identical output and that `sections()` reports the same count as before. Well-formed is not a hedge: `has_pending_changes` is true on every repaired document (`document.rs:961-963`), so a repaired file's no-op save legitimately appends the repair and this assertion would fail on it. The partition is `Provenance`, read from the session, not a filename list. **The positive case is asserted too**, or the carve-out becomes a place to hide failures: for every repaired `external/` file, the no-op save appends exactly one section, the bytes beneath it are byte-identical to the original, and the result reopens through `Document::open`. PLAN.md's guarantee-1 sentence says "for every well-formed corpus file" and this is what that clause is for.
- **Guarantee 2, driven by a real edit.** Make an edit through `EditSession`, save, assert the output is `original bytes ++ exactly one section`, that truncating at `original_len()` yields the byte-exact original, and that the truncated file reopens with the pre-edit content. Then make ten edits and one save and assert it is still exactly one section, which is the clause PLAN.md leaves ambiguous. Guarantee 2 driven by a **tool** rather than by `EditSession` directly is P7's test, not this one: `crates/core` cannot depend on a plugin, so the DoD's clause cannot be satisfied here and this package does not claim it.
- **Edit, undo, save writes nothing.** Byte-identical output, zero appended sections, on a clean document. This is the test that catches a dirty-flag overlay.
- **Edit, save, undo, save**, which is the test the first draft had no bullet for and the one that catches the whole save-boundary class. Three assertions on the second output: it reopens through `Document::open`; **object N equals its pre-edit value**, compared as a parsed object; and **the object graph reachable from the catalog equals the pre-edit graph**, walked and compared node by node, which is what catches a reversal that restored the object and forgot the referrer. Run it in all three shapes M3 can produce: edit an object that exists in the file, create an object, and remove one (which under T5's rule means rewriting its referrer, so the assertion is that the removed object is unreachable rather than absent). The first is the silent no-op the base-capture rule exists to prevent. `audit_references` on the output must be empty in every shape.
- Two saves produce two sections and the second's `/Prev` points at the first, asserted by parsing the trailers, not by scanning for the string `/Prev`.
- Preview: after a committed edit, `preview_bytes` parses as a valid PDF through `cos::Document::open` (not `open_repairing`), and its object graph equals what the subsequent save writes, compared object by object. Since both come from one `section_for` call with one argument, this is a regression test on the wiring rather than a check on two implementations agreeing.
- **The preview cache key includes the filter**, asserted directly: two `preview_bytes` calls at one overlay generation with two different `AnnotationFilter` modes return different bytes, and the same mode twice returns the cached buffer. Without the first half the print dialog shows the wrong Comments-and-Forms mode and every downstream filter test still passes, because none of them asks twice at one generation.
- Bench, in `crates/core/benches/save.rs`, with a stated budget in the P14-era harness shape: preview rebuild after one edit on a clean 1000-page document, and on a **repaired** document, where `needs_full_table()` forces a full-table section. The repaired number is the one that decides whether preview rebuilds need debouncing, and this plan does not guess it.
- `revert_to` on a document with unsaved edits is refused; on a non-trailing generation it is refused; on a trailing one it truncates and the reopened document matches the pre-save state.
- **Runs.** `cargo test -p onionskin-core`; `cargo bench -p onionskin-core --bench save`; `cargo test -p onionskin-app --no-default-features` (guarantee 5 with a save path in the workspace); `cargo clippy --workspace --all-targets -- -D warnings`.
- **Corpus.** The guarantee-1 sweep and its repaired-document half walk `external/`, so both run behind P1c's fetch step and are re-run under `ONIONSKIN_CORPUS_REQUIRED=1`. Without it, guarantee 1 at the `core` level means three tracked seeds.

**Review risk.** The highest-consequence package in M3. A reviewer will probe:
whether "one section" is asserted by parsing or by counting `%%EOF` occurrences
(the original file may legitimately contain one already); whether the reopen
after save leaves any cache holding a pointer into the old parse; whether the
preview cache is keyed on something that actually changes with every edit;
whether `revert_to` can be reached with the render worker still holding the
truncated bytes; whether autosave's recovery replay can double-apply an edit
that was also saved; whether a `Save As` leaves the generations list describing
the old file; whether the guarantee-1 carve-out for repaired documents is
derived from `Provenance` or from a filename list somebody maintains; whether any
path here reaches `cos::delete_object`, which T5's rule forbids and which nothing
but a reviewer's grep will catch.
**Mutation that must break its tests:** removing the empty-overlay
short circuit must fail the edit-undo-save test; emitting one section per edit
instead of per save must fail the ten-edits test; skipping the reopen after save
must fail the two-saves `/Prev` test; dropping the filter from the preview cache
key must fail the two-filters-one-generation test.

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
  already names, and P1c already fetches the `verapdf` set, so this is a
  derivation step rather than a second download.

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
- **Runs.** `cargo test -p onionskin-core --test structure`; `cargo clippy --workspace --all-targets -- -D warnings`.
- **Corpus.** `corpus/tagged/` is derived from the `verapdf` set, which P1c already fetches into the `test` job and already re-runs under `ONIONSKIN_CORPUS_REQUIRED=1`. This package adds the derivation step and adds `cargo test -p onionskin-core --test structure` to P1c's re-run list; it does not add a second fetch. If P1c has not landed, this package cannot be verified and must not merge, which is the one dependency that is about CI rather than code.

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
- `/Annots` on a surviving page is carried through untouched;
- `/StructParents` is renumbered and P4's hook is called;
- everything else on the page dict (`/Tabs`, `/Group`, `/UserUnit`,
  `/Contents`, private keys) is carried through untouched, which is the
  "unimplemented means untouched" rule applied at the page level.

**A removed page is removed by not being listed, and nothing else happens to
it.** Its dict is not rewritten and not freed; neither are its annotations, their
appearance streams, their `/Popup` partners, its content streams, or the internal
`/Pages` nodes it hung under. They become one internally consistent garbage
subtree, per T5's free-nothing rule, and **no `cos::delete_object` call appears
anywhere in this package**. The first draft's "its annotation objects are deleted
for a removed one" is the clause that manufactured the dangling class the flat
rewrite exists to remove, and it is gone.

Plus the document-level fixups a page-set change forces. **Each is a separate
function on the same edit, with its own named fixture and its own test**, because
each walks a different part of the document and no one of them finds another's
case:

- `/PageLabels`, a number tree keyed on page index, rebuilt for the new order.
- Named destinations (`/Dests`, `/Names /Dests`) and outline `/A` and `/D`
  entries that name a removed page: dropped, with the count reported so the UI
  can say what it dropped.
- **Link annotations on surviving pages** whose `/A` `/GoTo` action or whose
  `/Dest` names a removed page, directly or by name. This walks every surviving
  page's `/Annots` and is a different object from the outline entries above.
- **`/AcroForm /Fields`**: entries naming a widget annotation on a removed page
  are dropped, together with any `/Parent` field node left with no children. M3
  authors no form fields; it deletes pages, and deleting a page out of a form
  document is ordinary. Without this the field tree and the page tree disagree.
- **Article threads**: the bead ring reached from a surviving page's `/B` is
  re-linked past the removed page's beads through `/N` and `/V`, and a thread in
  `/Threads` all of whose beads are gone is dropped. The first draft listed `/B`
  as "survives untouched", which is true of the page's own key and false of the
  ring it points into.
- `/OpenAction` and any `/AA` page-level actions naming a removed page.
- The `/Count` on the new node, which is just the length.

**Rows closed.** None. Backs every row in P11 and P12.

**Files.** New `crates/core/src/pages/{mod,rewrite,inherit,labels,destinations,fields,threads}.rs`,
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
- A corpus sweep, run as part of this package and not afterwards, listing every `external/` file whose page tree is more than one level deep, every file with `/PageLabels`, every file with `/AcroForm /Fields`, every file with `/Threads`, and every file with an intra-document `/Link`. Pick three of each as named fixtures. If the plan cannot name the fixture files, the transformation is unproven. **The sweep and every fixture below live behind P1c's CI corpus step**: `corpus/external` is gitignored and the `test` job fetches nothing, so without that step this entire verification section runs over an empty file list and reports a pass. This is the highest-correctness-risk package in M3 and it is the one whose fixtures CI currently never sees.
- For each deep-tree fixture: delete a page, and assert through a fresh parse of the saved output that every surviving page's four inheritable attributes are **identical to what they resolved to before**. This is the bug the flat rewrite exists to prevent and it is invisible in a shallow-tree fixture.
- **`audit_references` (P1) reports nothing on the whole output file** after every operation over every fixture, and `section_for`'s gate accepts every section. The first is the complete check and is the one that matters here; the second is free. A run that trips either is a failure of this package, not of the check.
- **No section this package emits carries a free entry**, asserted by parsing the output's last cross-reference table. That is T5's free-nothing rule made executable in the one package that would break it, and it is one assertion.
- `/PageLabels`: a fixture with roman-then-arabic labels keeps each surviving page's label after a reorder and after a delete, compared as resolved label strings, not as tree structure.
- A bookmark and a named destination pointing at a deleted page: both are dropped, the drop count is reported, and every surviving bookmark still resolves to the page it named before.
- **A link annotation on a surviving page pointing at a deleted page** is dropped, and one pointing at a surviving page still resolves to the page it named before. The fixture is a document with intra-document links, named in the sweep alongside the deep-tree ones.
- **`/AcroForm /Fields`** on a form fixture: deleting the page carrying a widget drops that field and leaves every other field resolving to its own widget, asserted by walking the field tree after a reopen. A `/Parent` node emptied by the drop is gone too.
- **Article threads** on a fixture with a thread spanning three pages: deleting the middle page leaves a two-bead ring whose `/N` and `/V` close, and deleting all three drops the thread from `/Threads`. A ring that still walks but visits a garbage bead passes a naive reachability check, so this is asserted by walking the ring and comparing the bead set.
- P4's structure invariant passes after each operation on each tagged fixture, and the reading order matches the new page order for the reorder case.
- Rotation: `/Rotate 90` inherited from a `/Pages` node survives the flatten, and a page whose own `/Rotate` overrode its ancestor's keeps its own.
- Bench: rewriting the tree of the 1000-page bench file, with the section size reported. The number decides whether T5's fallback is needed and this plan does not guess it.
- **Runs.** `cargo test -p onionskin-core --test pages`; `cargo bench -p onionskin-core`; `cargo clippy --workspace --all-targets -- -D warnings`.

**Review risk.** The highest-correctness-risk package in M3, the way P3
(geometry) was in M2. A reviewer will probe: whether inheritance is materialized
before or after the parent pointer changes (after is wrong and passes on
shallow trees); whether `/Count` is recomputed or copied; whether a page that
appears twice in the new order (a legal duplicate, which "copy pages between
documents" produces) is handled or aliases one object; whether anything here
frees an object number, which T5 forbids and which a grep for `delete_object`
settles in one command; whether the destination fixup
walks `/Names` trees or only the flat `/Dests` dictionary; whether the drop
count is real or an estimate; whether the six document-level fix-ups are six
functions with six fixtures or one function that handles the two easy cases.
**Mutation that must break its tests:** skipping
inheritance materialization must fail the deep-tree fixture; copying `/Count`
instead of recomputing must fail the delete case; dropping the `/PageLabels`
rebuild must fail the label fixture; dropping any one of the link, `/AcroForm`
and article fix-ups must fail exactly its own fixture and no other, which is
what proves they are six checks rather than one.

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
- **Runs.** `cargo test -p onionskin-core --test annots`; `cargo test -p onionskin-app --no-default-features`; `cargo clippy --workspace --all-targets -- -D warnings`.
- **Corpus.** The tagged fixture comes from `corpus/tagged/`, which P4 populates from the `verapdf` set P1c fetches, and the invariant runs under `ONIONSKIN_CORPUS_REQUIRED=1` with it.

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
- **Corpus.** The real corpus document, the `/Rotate 90` fixture and the two-column fixture are all `external/`, so they run behind P1c's fetch step and its mandatory re-run.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features tools-comment` and the P7 property suite; `cargo clippy -p onionskin-app --no-default-features --features tools-comment --all-targets -- -D warnings`.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features tools-comment`; `cargo test -p onionskin-core` for the embedded-file writer; `cargo clippy --workspace --all-targets -- -D warnings`.

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
- **Corpus.** The deep-tree fixtures, the flat ones and the embedded-font source document for the importer comparison are all `external/`, behind P1c's fetch step and its mandatory re-run. The importer render comparison is the assertion that catches a one-level-deep importer and it is the one CI currently never runs.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features commands-core`; `cargo test -p onionskin-core` for the assembly primitive; `cargo clippy --workspace --all-targets -- -D warnings`.
- **Corpus.** The three named combine inputs, the 10-page split fixture and the bookmark-boundary fixture are `external/`, behind P1c's fetch step and its mandatory re-run.

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
  pane context menu, over P10's embedded-file writer. Deleting one rewrites `/Names /EmbeddedFiles`
  without the entry and leaves the stream as garbage; it frees nothing, per T5.
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
- Attachments: add, save, reopen, `core::attachments::read` reports it with the right size and MIME; delete removes it from `/Names /EmbeddedFiles` and leaves the stream as unreferenced garbage, with `audit_references` clean and no free entry in the appended section.
- Document Properties: writing a Description field appears in `/Info` **and** in XMP, and reopening reports the new value from both; the two must agree or the reader that a given consumer uses decides what it sees.
- Initial View: setting "open at page 5, fit width" writes `/OpenAction` and reopening in Onionskin honours it, asserted through the session, not through the dialog's own state.
- Every new dialog control is in the AccessKit tree with a real label and state, and the dialog's controls leave the tree when it closes, asserted by the probe.
- `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` plus the matching clippy.
- **Corpus.** The properties and Initial View round-trips run on tracked seeds; the bookmark-destination fixtures are `external/` and run behind P1c's fetch step and its mandatory re-run.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features codecs-common,commands-core`; `cargo clippy --workspace --all-targets -- -D warnings`.
- **Corpus.** The recompressible-images fixture, the known-image-count fixture and the CMYK fixture are `external/`, behind P1c's fetch step and its mandatory re-run. Compress is the one destructive path M3 ships and its only correctness evidence is a fixture CI currently never fetches.

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
Poster / tile (91) were ruled out of M3 and move to M4 (section 9, ruling B):
both are pure imposition math over this package's `Sheet` model, so M4 adds them
without reopening anything P15 builds. P15 must therefore leave `Sheet` and
`Placement` able to express a sheet whose placements are not a uniform grid, and
a test asserting one hand-built such sheet composes correctly is this package's
only concession to them.

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
- **Runs.** `cargo test -p onionskin-print` on macOS, plus `cargo clippy -p onionskin-print --all-targets -- -D warnings` on every runner, which is what proves the target gating rather than the backend.

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
- **The user-visible half of T1's undo-across-a-save rule**, end to end through the shell rather than through `EditSession`: delete a page, `cmd-s`, `cmd-z`, `cmd-s`, and the page is back on the canvas, in the file on disk, and in the thumbnails pane. This is P3's `edit, save, undo, save` test driven by four keystrokes, and it exists separately because a green kernel test alongside a shell that never reaches it is exactly the failure §4 of the repository rules names.
- Closing a dirty tab prompts; the prompt retains the originating canvas identity, which is B4.2's rule for every async prompt in this shell and applies here unchanged.
- Crash recovery: a recovery file written for document A is offered when A is next opened and not when B is, ranked most-recent-first, asserted as a unit test on the ranking with no window.
- Every new control is in the AccessKit tree with a state that reflects enablement (Undo is disabled with a reason when the stack is empty, not absent).
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy; `cargo test -p onionskin-app --features shell`.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy.
- **Corpus.** The three-generation file is built by the test; the file Onionskin never wrote is an `external/` file with a pre-existing incremental section, so that case runs behind P1c's fetch step and its mandatory re-run.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy; `cargo test -p onionskin-core` for the widened reason type.
- **Corpus.** The fixture carrying comments Onionskin did not author is `external/`, behind P1c's fetch step and its mandatory re-run. It is the fixture that distinguishes a Comments pane from a view of the edit graph, so a skipped run leaves the package's central claim unmeasured.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy. The 1000-page lazy-request assertion uses the generated bench file, which CI's `bench` job builds and the `test` job does not, so it is generated by the test rather than assumed present.

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
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy; `cargo test -p onionskin-core` for the search criteria and the viewport timing.

**Review risk.** Whether nine small changes got nine tests or three. Whether New
Window shares the session or opens the file twice, which would give two edit
stacks over one file and is the defect this row invites. Whether auto-scroll
runs a timer when the window is not visible. Whether the Line Weights outcome
is carried as one branch or as a hedge. Whether Starred paths leak absolute
home directories, which is the residual `recents.rs` already carries.
**Mutation that must break its tests:** opening a second `core::Document` for
New Window must fail the shared-undo test.

---

## 5. Dependency graph, order and contention

Three roots, all startable on day one: **P0** (splitting `ShellFrame`, app only),
**P1** (cos, kernel only) and **P1b** (crypto, ruled in). They touch disjoint
crates, so day one has three branches and no conflict.

```
P0  split ShellFrame                    P1  cos edit surface         P1b crypto (ruled in)   
 │   (app only, no behaviour change)      └── P2  core::edit  (graph, undo/redo)
 │                                             ├── P3  core::save  (section, preview, generations)
 │                                             │    ├── P12 combine + split      (also P5, P11)
 │                                             │    ├── P13 properties/bookmarks (also P0, P5, P10)
 │                                             │    ├── P14 image import/export, compress (also P12)
 │                                             │    └── P15 print: imposition + file backend (also P6)
 │                                             │         └── P16 print: macOS backend
 │                                             ├── P4  core::structure
 │                                             │    ├── P5  core::pages  (the transformation)
 │                                             │    │    └── P11 tools-organize      (also P7)
 │                                             │    └── P6  core::annots
 │                                             │         ├── P8  tools-comment: text markup   (also P7)
 │                                             │         ├── P9  tools-comment: shapes and ink (after P8)
 │                                             │         └── P10 tools-comment: stamps, summary (also P12, P14)
 │                                             └── P7  plugin-api edit contract   (also P0)
 │
 ├── P17 print dialog        (also P15, P16)
 ├── P18 save/undo/dirty/recovery   (also P3)
 ├── P19 skins panel         (also P1, P3)
 ├── P20 comments pane       (also P8, P9, P10)
 ├── P21 organize grid       (also P11)
 └── P22 remaining shell rows
```

**Critical path.** `P1 → P2 → P4 → P5 → P11 → P21`. Six packages, and the two
longest of them (P5 and P11) are the two with the highest correctness risk, which
is the schedule's real hazard rather than its length. The print chain
(`P1 → P2 → P3 → P15 → P16 → P17`) is the same depth and mostly independent, so
it is the natural second track.

**Parallelism.**

- P0 runs alone in `crates/app` from day one and must finish before any of P17
  through P22 starts. Nothing else touches `crates/app`, so it blocks nobody.
- P1b touches only `crates/crypto`, a narrow seam in `crates/cos` and one notice
  in the app, so it runs the whole milestone alongside everything else. Its app
  seam lands after P0, like every other app change.
- Once P2 lands, P3, P4 and P7 are three independent branches in `crates/core`
  and `crates/plugin-api`.
- P5 and P6 are independent of each other; both need P4.
- P8, P9 and P10 are strictly ordered among themselves, on purpose: P8 gives P6's
  appearance generator one consumer's worth of feedback before twelve more
  arrive, and P9 gives the shape P10's stamps reuse.
- P12, P13 and P14 are three independent branches in `plugins/commands-core` and
  `plugins/codecs-common`, and they share only `Cargo.lock`.
- P15 is windowless and in a crate nobody else touches, so it runs from the
  moment P3 and P6 land.

**Contention, and the order it forces.** M2's audit named
`crates/app/src/shell/chrome/tabs.rs` the recurring conflict point, and M3 has
six packages in `crates/app`. After P0 splits it, each app package owns a
distinct new module, and the remaining shared files are the two match statements
(`chrome/menu.rs`'s command dispatch and `chrome/context.rs`'s availability
table), plus `panes/mod.rs`'s `NavigationPane` list, `commands.rs`'s
`MenuCommand` table, and `Cargo.lock`. All of those are append-only in these
packages, so a rebase resolves them without judgement. To keep it that way, the
app packages land in a fixed order:

1. **P18 first.** It introduces the dirty state, the saved mark's UI reading and
   four global commands that every other app package reads or extends. A save
   and undo model rebased under five branches is worse than five branches
   rebasing under it.
2. **P19 second.** It is the first consumer of the rail-plus-side-panel surface
   that P20 also uses, so it sets that shape.
3. **P20 third**, **P21 fourth**: both add a pane-shaped surface, and P20's is
   the one that also flips the quick action gates.
4. **P17 fifth.** It adds a dialog, which is the least entangled of the six, and
   it depends on the longest chain (P15 and P16), so it is naturally last of the
   substantial ones.
5. **P22 last.** Nine small changes across many files is exactly the shape that
   rebases cleanly under everything and painfully over anything.

Whoever rebases re-runs `cargo test -p onionskin-app --no-default-features
--features shell,shell-test-support` rather than trusting the merge, which is
the same rule M2's plan set and for the same reason.

**One split for ordering.** The corpus reaching CI is P1c's, and it is a day-one
root that lands **before P1**, because P1's own verification sweeps `external/`.
P4 then adds the `corpus/tagged/` derivation on top of the `verapdf` set P1c
already fetches, and adds its own suite to P1c's mandatory re-run list. Neither
is a "later package" problem: a suite that skips its corpus silently is exactly
how guarantee 6 stayed green and unmeasured for a milestone (section 8, item 2),
and this plan was in the middle of repeating it for eleven packages.

---

## 6. Parity row ledger: all 99 M3 rows

Every M3 row belongs to exactly one package or is deferred with a reason. Row
text is abbreviated; the source of truth is `ACROBAT-PARITY.md`, and the M3 rows
are recoverable with:

```sh
awk -F'|' '/^\|/ {gsub(/^ +| +$/,"",$4); if ($4=="M3") print $2}' ACROBAT-PARITY.md
```

| # | Parity section | Row | Package |
|---|---|---|---|
| 1 | Application shell | Convert (global bar entry point) | P14 |
| 2 | Application shell | Undo / Redo icons on the global bar | P18 |
| 3 | Application shell | Save / Save As in the global bar | P18 |
| 4 | Application shell | Print button | P17 |
| 5 | Application shell | Home view: Starred | P22 |
| 6 | Application shell | Manage Tools / customize the tool rail | P22 |
| 7 | Application shell | Autosave and crash recovery | P18 |
| 8 | Application shell | Window menu | P22 |
| 9 | Menus | File > Create | P14 |
| 10 | Menus | File > Save | P18 |
| 11 | Menus | File > Save As | P18 |
| 12 | Menus | File > Save as Other | P13 |
| 13 | Menus | File > Revert | P18 |
| 14 | Menus | File > Properties | P13 |
| 15 | Menus | File > Print | P17 |
| 16 | Menus | File > Attach to Email | P13 |
| 17 | Menus | Edit > Undo / Redo | P18 |
| 18 | Menus | Edit > Cut / Copy / Paste / Delete | P13 |
| 19 | Menus | Edit > Copy File to Clipboard | P13 |
| 20 | Menus | Advanced Search > include attachments | P22 |
| 21 | Menus | Advanced Search > document-property criteria | P22 |
| 22 | Menus | View > Automatically Scroll | P22 |
| 23 | Menus | View > Show/Hide > Line Weights | P22 |
| 24 | Menus | View > New Window | P22 |
| 25 | Navigation panes | Bookmarks: create, rename, nest, destination, delete | P13 |
| 26 | Navigation panes | Attachments: add and delete | P13 |
| 27 | Navigation panes | Bookmarks pane context menu | P13 |
| 28 | Navigation panes | Attachments pane context menu | P13 |
| 29 | Navigation panes | Comments pane | P20 |
| 30 | Navigation panes | Comments list context menu | P20 |
| 31 | Viewer and reading | Copy with formatting / Export selected text | P22 |
| 32 | Viewer and reading | Initial View settings | P13 |
| 33 | Create a PDF | Create from a single image file | P14 |
| 34 | Create a PDF | Create from multiple files | P12 |
| 35 | Create a PDF | Create a blank page | P11 |
| 36 | Create a PDF | Create from the clipboard | P14 |
| 37 | Combine files | Combine files into a single PDF | P12 |
| 38 | Combine files | Add files / add folders | P12 |
| 39 | Combine files | Reorder, preview and remove entries | P12 |
| 40 | Combine files | Expand a file and combine at page granularity | P12 |
| 41 | Organize pages | Rotate pages | P11 |
| 42 | Organize pages | Reorder / move pages | P11 |
| 43 | Organize pages | Insert pages | P11 |
| 44 | Organize pages | Delete pages | P11 |
| 45 | Organize pages | Extract pages | P11 |
| 46 | Organize pages | Split | P12 |
| 47 | Organize pages | Replace pages | P11 |
| 48 | Organize pages | Copy or move pages between open documents | P11 |
| 49 | Organize pages | Renumber pages / page labels | P11 |
| 50 | Organize pages | Thumbnail zoom and multi-select in the grid | P21 |
| 51 | Compress a PDF | Compress a PDF | P14 |
| 52 | Compress a PDF | Reduce File Size | P14 |
| 53 | Export a PDF | Export pages to JPEG / JPEG 2000 / TIFF | P14 |
| 54 | Export a PDF | Export all images in a document | P14 |
| 55 | Add comments | Sticky note | P9 |
| 56 | Add comments | Highlight text | P8 |
| 57 | Add comments | Underline text | P8 |
| 58 | Add comments | Strikethrough text | P8 |
| 59 | Add comments | Insert text at cursor (caret markup) | P8 |
| 60 | Add comments | Replace text | P8 |
| 61 | Add comments | Add text comment (typewriter) | P9 |
| 62 | Add comments | Text box | P9 |
| 63 | Add comments | Callout | P9 |
| 64 | Add comments | Draw freehand (ink) | P9 |
| 65 | Add comments | Erase ink | P9 |
| 66 | Add comments | Line | P9 |
| 67 | Add comments | Arrow | P9 |
| 68 | Add comments | Rectangle | P9 |
| 69 | Add comments | Oval | P9 |
| 70 | Add comments | Polygon | P9 |
| 71 | Add comments | Connected lines (polyline) | P9 |
| 72 | Add comments | Cloud | P9 |
| 73 | Add comments | Attach a file as a comment | P10 |
| 74 | Add comments | Comment properties | P10 |
| 75 | Add comments | Comments list: sort, filter, reply, status, read/unread | P20 |
| 76 | Add comments | Summarize comments | P10 |
| 77 | Add comments | Print comments | P17 |
| 78 | Add comments | Commenting preferences | P20 |
| 79 | Add stamps | Place a stamp | P10 |
| 80 | Add stamps | Standard business stamps | P10 |
| 81 | Add stamps | Sign Here stamp category | P10 |
| 82 | Add stamps | Dynamic stamps | P10 |
| 83 | Add stamps | Create a custom stamp | P10 |
| 84 | Add stamps | Manage stamps | P10 |
| 85 | Add stamps | Paste clipboard image as stamp | P10 |
| 86 | Printing | Print dialog | P17 |
| 87 | Printing | Page range and subset | P15 |
| 88 | Printing | Page sizing and handling | P15 |
| 89 | Printing | Multiple pages per sheet (N-up) | P15 |
| 90 | Printing | Booklet | **moved to M4** (section 9, ruling B) |
| 91 | Printing | Poster / tile | **moved to M4** (section 9, ruling B) |
| 92 | Printing | Print on both sides / duplex | P16 |
| 93 | Printing | Orientation | P15 |
| 94 | Printing | Comments & Forms | P15 |
| 95 | Printing | Page Setup dialog | P17 |
| 96 | Printing | Summarize comments in the print output | P17 |
| 97 | Printing | Print as image | P15 |
| 98 | Printing | Print to file / print to PDF | P15 |
| 99 | Printing | Advanced Print Setup dialog | P17 |

**Totals.** P8 5, P9 13, P10 10, P11 9, P12 6, P13 10, P14 8, P15 7, P16 1,
P17 7, P18 7, P20 4, P21 1, P22 9, moved to M4 2. Sum 99. P0, P1, P1b, P2, P3, P4,
P5, P6, P7 and P19 close zero rows and are named against them in section 4.

**Rows outside M3's 99 that M3 changes**, which the scoreboard update has to
carry and which nobody should discover at review time:

- Three M2 rows flip from `partial` to `implemented`: the right-hand side panel
  (P20 gives it tool content), the quick action toolbar (P20 opens the Comment,
  Highlight and Draw gates), and the Page Thumbnails pane context menu (P21
  enables its M3 entries; Crop Pages stays disabled on M5).
- The Layers pane context menu's `Properties` entry goes live with P13.
- `Open an encrypted document` moves from M6 to `partial` at M3, per ruling A,
  with Notes naming the read-only scope, the M6 write path and the accepted
  regression for documents whose `/P` bits allow modification.
- Rows 90 and 91 move from M3 to M4, per ruling B, with the reason recorded.
- The `implemented` count moves from 49 to 49 plus whatever M3 lands; the
  executable totals contract in `crates/app/tests/guarantees.rs` recounts it, so
  a mismatch fails the build rather than living in the preamble.

---

## 7. YAGNI ledger and deferrals

Every abstraction M3 introduces names the caller that exists in M3.

| Introduced | Consumer that exists in M3 |
|---|---|
| `cos::Document::next_object_number` | `core::edit`'s reservation counter |
| `cos::Document::sections` | P19's skins panel, P3's `revert_to` |
| `cos::Document::write_new` | P12 combine and split, P14 create-from-image and compress, P10's comment summary |
| `cos::{PendingEdit, Document::section_for, Document::save_overlay_to_path}` | P3's save and P3's `preview_bytes`, which are the same call with the same argument; `incremental_section` and `save_to_path` are re-expressed as callers so there is one serializer, not two |
| cos's save-time reference gate on `section_for` | every save in M3, as the cheap guard that T5's free-nothing rule was not broken |
| `cos::Document::audit_references` | the verification of P5, P11, P12 and P14, which is where the complete O(file) walk belongs |
| `core::edit::{Overlay, History, DocumentEdit}` | every tool and command in P8 through P14 |
| `core::preview_bytes(filter)` | the canvas (committed edits), P15's print filter, P20's hide-all-comments view |
| `core::generations` and `revert_to` | P19's skins panel, P18's `File > Revert` |
| `core::structure` | P5's page operations, P6's annotation authoring, and M5's guarantee 8 |
| `core::pages::rewrite_page_tree` | all nine P11 operations, P12's combine and split |
| `core::pages::import` | P11's insert and copy-between-documents, P12's combine |
| `core::pages::assemble` | P12, P10's summary, P11's extract, P14's create-from-images |
| `core::annots` and its appearance generator | P8's five tools, P9's thirteen, P10's stamps |
| `core::embedded` (the embedded-file writer) | P10's attach-as-comment, P13's Attachments pane |
| `ToolCtx.edits` / `CommandCtx.edits` | every M3 tool and command |
| `Requirement::Command` in `plugin-api` | the six `Requirement::Milestone` arms `context_menu.rs` calls "guesses" |
| `print::{Sheet, Placement, PrintBackend}` | the file backend, the macOS backend, and P17's preview |

Deliberately **not** built in M3, and why:

- **A `Render` trait, a GPU backend, or vello.** Unchanged from M2's reasoning:
  one implementor, no consumer.
- **Encrypted writing.** M6, per PLAN.md's risk list. Ruling A takes the read
  path only; appending to an encrypted file stays M6, and P1b's coherence
  depends on saying so at open time rather than at save time.
- **The accessibility checker, reading-order repair and autotagging.** M5 and
  M6. P4 builds the reader and the maintenance hook, which is what makes those
  a checker rather than a repair job (T6).
- **Text editing of any kind.** M5, and the famous tar pit. A `/FreeText`
  annotation is authored text drawn over the page, not an edit to the page's
  content stream, and P9 must not blur that.
- **Form fields, signatures, redaction, measurement.** M5 and M6.
- **XFDF and FDF comment interchange.** Post-1.0, riding with XFDF, as the
  parity row already says.
- **An open `DocumentEdit`, or a `Raw(Change)` escape hatch.** The closed enum
  is load-bearing for three named things (T2); an escape hatch deletes all
  three.
- **Any MCP or CLI affordance.** M4. `core` gains no API "for MCP later", though
  `DocumentEdit` happens to be the vocabulary M4 will want, which is a
  consequence of getting M3 right and not a reason for it.
- **`cos::Document: Send`.** Still not needed: P3's autosave crosses the thread
  boundary with the overlay's changes, not with a document. The `Rc` to `Arc`
  swap remains M4's if MCP sessions want it.

Deferred within M3, with reasons and ledger actions:

| Item | Decision | Ledger action |
|---|---|---|
| Booklet (row 90) and Poster / tile (row 91) | **Moved to M4**, per ruling B. Both are pure imposition math over P15's `Sheet` model and land alongside M4's CUPS and Windows backends. | Move both rows to M4 in `ACROBAT-PARITY.md` with the reason. PLAN.md decision 13 names booklet in the M3 print list and is corrected in this branch's PLAN.md commit. |
| Copy With Formatting to the clipboard (half of row 31) | Deferred. `gpui::ClipboardEntry` has only `String` and `Image`, so a rich-text flavour needs a fork addition (section 8, item 12). Export Selection As ships; the row is `partial`. | Add the cut to row 31's Notes; open a fork issue for a custom pasteboard flavour. |
| `New Bookmarks From Structure` | Not M3. Needs the tagged tree, and its parity row already puts it at M6. P4 makes it cheap when it arrives. | None; the row is already correct. |
| JPEG 2000 export, if no acceptable pure-Rust encoder exists | Row 53 ships `partial` naming JPEG and TIFF, with the reason. A C dependency is not an acceptable resolution (decision 4). | Split row 53's Notes if it happens; decided in P14, never carried as both outcomes. |
| Line Weights, if the hayro fork commit does not land | The menu item stays disabled with a reason and the row moves to M4. M2's review already recorded the correct semantics (constant hairline width, not a width floor) so M3 does not repeat the wrong analysis. | Only if it happens; decided in P22. |
| Inline images (`BI`/`ID`/`EI`) in Export all images (row 54) | Decided in P14 and stated either way. Including them is a content-stream walk, excluding them is a documented scope line. | Whichever P14 takes goes in row 54's Notes. |
| `corpus/tagged/` beyond what P4's invariant needs | M5 owns the guarantee-8 fixture set. P4 populates enough to exercise the invariant and says how many files that is. | `corpus/README.md`'s guarantee-8 row is updated from "not built yet" to what P4 built. |

---

## 8. Candor: where PLAN.md's M3 text does not survive contact with the code

Each item needs a plan edit or an explicit acceptance before implementation
starts. Evidence is cited.

1. **The M3 paragraph names seven deliverables; the scoreboard puts 99 rows in
   M3.** This is M2's candor item 12 repeating: the paragraph names
   `tools-comment`, `tools-organize`, Combine and split, incremental save,
   undo/redo, the skins panel and `crates/print`. It never mentions stamps
   (seven rows), document properties, bookmark and attachment authoring,
   compress and flatten export, image creation and export, autosave and crash
   recovery, the Window menu, Manage Tools, Line Weights, Automatic Scroll, the
   Advanced Search extensions, Copy with formatting, Home Starred, or Initial
   View. **Resolution: grow the paragraph**, as M2's resolution was, since the
   packages cover all 99 and moving rows out would shrink the milestone for no
   engineering reason. That is a PLAN.md edit for the orchestrator, not made
   here.

2. **Landed at B6, and this item now describes what M3 inherits rather than
   what is missing.** The draft of this item said guarantees 1, 2 and 6 were
   `#[ignore]`d and `unimplemented!()` in `crates/app/tests/guarantees.rs`.
   **That has not been true since `0f1c295`.** On `main` at `fa5a194` all three run;
   the four that stay ignored are 3, 4, 7 and 8, at `guarantees.rs:119`, `127`,
   `219` and `228`, each naming M5.

   What the three that run actually contain is not the property itself but an
   **executable tripwire over the suite that owns the property**, and M3 has to
   extend that shape rather than replace it:

   - `a_save_with_no_edit_is_byte_identical_to_the_original` names
     `roundtrip.rs` and its seven enforcing tests by function name, asserts the
     suite still invokes `document.incremental_section` and `.save_to_vec`,
     still asserts `non-empty-noop-save`, still holds a pass floor on each
     external-corpus walk, and calls `assert_ci_reaches_the_cos_suite()`.
   - `an_edit_appends_one_incremental_section_that_truncates_away` names
     `incremental.rs` and its two enforcing tests, and pins the three clauses of
     the guarantee sentence to three assertion markers in that file plus
     `document.original_len` as the truncation point.
   - `every_malformed_file_opens_and_repairs_into_a_new_section` names
     `repair.rs` and its three enforcing tests, pins three markers, **and
     asserts the two CI steps by hand**: exactly one step running
     `./corpus/make-malformed.sh` and exactly one running
     `cargo test -p onionskin-cos --test repair` with
     `ONIONSKIN_CORPUS_REQUIRED: 1`, both pinned to `runner.os == 'Linux'` so
     `if: false` cannot switch either off.

   So guarantee 6's vacuous pass is fixed, and **its fix is the pattern, not
   the exception**. What M3 owes these three files is therefore concrete:

   - **Guarantee 2 gains a second enforcing suite, one level up.** The clause
     PLAN.md and the DoD both promise, "driven by an edit a tool made through
     `core`", is satisfied by nothing that exists: `incremental.rs` drives
     `cos::set_info_field`. P7 owns the new test and the tripwire edit, and `crates/app/tests/guarantees.rs` is named in
     P7's file list because nobody currently owns editing it.
   - **Every guarantee whose corpus M3 makes load-bearing gains the same
     two-step CI pair**, generate-or-fetch plus a re-run under
     `ONIONSKIN_CORPUS_REQUIRED=1`, asserted from `guarantees.rs` the way
     guarantee 6 asserts its own. P4's tagged set is one instance; P1c
     (section 4) is the package that lands the pair for every other external
     set M3's fixtures come from, because today the `test` job fetches nothing
     and every external-corpus assertion in this plan silently skips in CI.
   - **The parity-totals contract lives in the same file.**
     `acrobat_parity_headline_matches_every_inventory_row`
     (`guarantees.rs:2959`) recounts `ACROBAT-PARITY.md`'s status totals, target
     total and per-milestone totals from the rows themselves, so ruling B's row
     moves fail the build unless the headline moves with them. P15 owns that
     edit, atomically with the two rows.

3. **PLAN.md's stated undo model is wrong for two reachable cases.** "What does
   not transfer" item 1 and parity row 17 both say undo is "dropping edit-graph
   overlay nodes". Dropping a node is correct only when the edit created the
   node. An edit that overwrites an object a previous edit already overlaid (a
   highlight recoloured twice) must restore the previous overlay state, and an
   edit that deletes an object present in the original has no node to drop.
   T2 resolves it with before-and-after states; the plan text should say so, and
   P2's property test covers exactly these two cases by name.

4. **The core invariant conflates generations with undo.** Point 2 of the
   invariant says generations "roll back by truncation", and the milestone list
   says M3 delivers "undo/redo". Read together they imply truncation is undo,
   which is wrong after any save that is not the last thing in the file, wrong
   after a Save As, and destructive of generations the user kept. T1 separates
   them into a session-scoped edit stack and a named, explicit generation
   rollback. PLAN.md should carry that distinction.

5. **The core invariant has no clause for documents Onionskin authors.** Combine,
   split, extract, create-from-image and the comment summary all produce a new
   file with nothing underneath to append to, and cos has no write-from-scratch
   API (its charter names a `flatten` that does not exist). T8 proposes the
   clause: a document Onionskin authors is written complete on its first save
   and the invariant applies from there. That is a PLAN.md edit.

6. **Decision 12 says `core` maintains the structure tree on every edit;
   nothing in the workspace mentions it.** A repository-wide grep for
   `StructTreeRoot`, `ParentTree`, `StructParents`, `MarkInfo` and `MCID`
   returns one incidental comment in `crates/content/src/interpret.rs:33`.
   PLAN.md assigns guarantee 8 to M5 and says nothing about M3's obligation,
   which reads as permission to ignore it. T6 states the obligation and P4 sizes
   it. The cost of not doing it now is not a delayed guarantee; it is M5
   inheriting shipped M3 builds that broke the tree, plus a revisit of every M3
   tool.

7. **The known dangling-reference debt is assigned to `tools-organize`, which is
   the wrong owner.** `known-issues.md` and `document.rs:868` both say "M3's
   `tools-organize` has to fix up the page tree itself". A plugin is the last
   place that knowledge should live: `commands-core`'s combine and split need
   exactly the same fixups, and so will `redact` at M5. This plan puts the
   transformation in `core::pages` (P5) and leaves `tools-organize` as the
   surface that calls it, and adds the guard in cos (P1) so the mistake is loud
   wherever it is made. The ledger entry's wording should move with it.

8. **`crates/print` "lands with the macOS backend and the Acrobat print dialog"
   understates it by three of five parts.** The five are a page-selection model,
   an imposition engine, a sheet renderer, the backend trait with two
   implementations, and the dialog. The crate today has six lines of doc comment
   and no `[dependencies]` section. Only the dialog is app work, and only the
   file backend can be verified in CI, which PLAN.md's own risk list already
   says and its milestone sentence does not.

9. **The Comments and Forms print row cannot be expressed through hayro's
   settings, and `/OC` will not rescue it.** Row 94 wants a per-subtype filter;
   `render_annotations` is a bool; and `known-issues.md` records that hayro's
   annotation loop never reads an annotation's `/OC`. T7's resolution is a
   render-time `/F` Hidden overlay on a preview buffer. The consequence that
   must be stated in the plan: M3 authors no `/OC` on any annotation, and
   layer-controlled annotation visibility stays blocked on the upstream fix.

10. **`cos::Document` is `!Send`, and M3 is the milestone m2-viewer predicted
    would feel it.** Candor item 7 of the M2 plan named "M3's edit graph or M4's
    MCP sessions". The consumer turns out to be autosave (row 7), not the edit
    graph, and P3 resolves it by moving the overlay rather than the document
    across the thread boundary. Worth knowing before P18 discovers it.

11. **The skins panel has no data source.** PLAN.md describes generations as
    sheets and rollback as truncation, but cos exposes `original_len()` (one
    boundary) and nothing that walks the `/Prev` chain. P1 adds `sections()`.
    The panel is also, per `ACROBAT-PARITY.md`'s own counting convention,
    explicitly not counted on the scoreboard, so a milestone scheduled by row
    count will schedule the release's namesake feature at zero.

12. **Copy With Formatting cannot fully ship, and M2's plan already found half
    of this.** M2's P10 recorded that `gpui::ClipboardEntry` has only `String`
    and `Image` variants and that `TextSelection` carries no font data, then
    pointed row 31 at M3. `ClipboardEntry` is unchanged. So M3 can ship Export
    Selection As (a file) and can add font data to `TextSelection`, but the
    clipboard half needs a GPUI fork addition for custom pasteboard flavours.
    The row ships `partial` with the clipboard half named, which is section 7's
    deferral, and PLAN.md's implicit promise that M3 closes it is wrong.

13. **`corpus/tagged/` is a README.** `corpus/README.md`'s guarantee-8 row says
    "not built yet". Any milestone that claims tag integrity, including M3's
    obligation under item 6, needs that set to exist. P4 builds it.

14. **The encryption ledger action M2 promised was never written.**
    m2-viewer section 6 assigned `known-issues.md` an entry distinguishing
    permissions-only encryption from password protection, with class counts.
    `known-issues.md` contains no encryption entry at all today. P1b produces
    the measurement (section 9, ruling A) as a committed table under
    `docs/evidence/` whose tally a test asserts; the orchestrator lands the
    ledger entry from it, and no package here edits that file.

---

## 9. Two decisions, ruled

Both were called out by PLAN.md or by M2's plan as due at M3 planning. Both were
put to the user on 2026-09-02 with the recommendation and the tradeoff below, and
both were ruled as recommended. The reasoning is kept because the tradeoffs are
what a future reader will want, not the verdict alone.

### Ruling A. Empty-user-password decryption comes into `cos` at M3, read-only.

**Ruled: take it, read-only, with editing disabled at open.** P1b is a definite
package. The accepted residual is stated in P1b and again at the end of this
subsection.

**The reassessment PLAN.md's M2 paragraph scheduled for now.** M2 ships
encrypted documents closed with a typed, fail-loud message naming the milestone.
The question was whether to keep that through M3.

**What it would cost.** One package, P1b, off the critical path and in a crate
nothing else touches. `crates/crypto` is four lines of doc comment today, so
this is the ISO 32000 standard security handler built from nothing: `/V` 1, 2, 4
and 5, `/R` 2 through 6, algorithm 2 and 2.A key derivation, algorithms 4
through 7 and 11 and 12 for validation, per-object keys for RC4 and AES-128-CBC,
the direct file key for AES-256, `/EncryptMetadata`, and the crypt filter
dictionary. Pure-Rust dependencies exist for all of it, so decision 4 is not at
risk. The seam in `cos` is narrow: strings and streams decrypt at parse, and
`refuse_encrypted` narrows from one predicate to a slightly longer one. The
review burden is the interesting part, not the line count: four revisions of key
derivation, each of which can be subtly wrong in a way that opens some files and
mis-keys others, which is why P1b's verification insists on known-answer vectors
per revision rather than on corpus files alone.

**What it would buy.** The roughly 35 corpus files cos refuses become
viewable, printable and exportable. More importantly it closes the
**permissions-only** class: an `/Encrypt` dict with an empty user password,
which Acrobat opens without ever prompting and which is what most encrypted
PDFs in the wild are. To a user those files are not "encrypted", they are
ordinary documents that Onionskin alone refuses, and that is a credibility hole
in exactly the institutional offices a drop-in claim targets. It also gives M6's
write path working key material to append with, instead of building key
derivation under signature-compatibility pressure.

**The tradeoff, stated honestly.** Writing stays M6. So a document that opens
must refuse to save, which is a worse shape than refusing at open **unless the
refusal is at open**. P1b therefore disables the edit tools on any encrypted
document at open time, with a reason naming M6, so the user never begins work
they cannot keep. For files whose permission bits forbid modification that is
what Acrobat does anyway. For files whose permission bits **allow**
modification, it is a real regression against Acrobat, and it is the residual
this decision buys: those documents become readable and printable in M3, and
stay uneditable until M6.

The argument the other way is genuine: M3 is already the largest milestone by
row count, cryptography is a domain where a subtle bug is a security defect
rather than a rendering artefact, and PLAN.md's risk list is unambiguous that
"M1 parses encryption; M6 writes it". Deferring costs nothing in M3 and leaves
the hole open through M3, M4 and M5, three milestones during which the parity
scoreboard is public.

**The recommendation, which was taken: read-only, scoped to the standard
security handler with an empty user password, with editing disabled at open.**
M3 is the first release where refusing a document costs the user work rather than
a view, and the read half sits inside the plan's own boundary (`crates/crypto`'s
charter calls decryption handlers "a kernel concern from the start"; only the
write path is M6). It is off the critical path, so it costs schedule only if it
is allowed to block something, which it is not.

**The accepted residual, restated because it is the part that will be
questioned later.** A document whose `/P` bits allow modification becomes
readable, printable and exportable in M3, and stays uneditable until M6, which
Acrobat would allow. That is a real regression against Acrobat. It was weighed
against leaving the whole class refused, and refusing the whole class was judged
worse. The open-time notice owes the user that sentence in plain words.

**The measurement, which was the unconditional half and is now P1b's first
deliverable.** Of the roughly 35 corpus files cos refuses, how many are
permissions-only, and how many of those have `/P` bit 4 set? The second number
sizes the residual above, and nobody has it. P1b emits it as a committed table
under `docs/evidence/`, generated by a test that fails if the tally drifts, and
**the orchestrator lands the `known-issues.md` entry from it**; no package in
this plan edits that file.

### Ruling B. M3 ships fourteen of the sixteen printing rows.

**Ruled: booklet and poster / tile move to M4.** M3 ships the `Sheet` imposition
model and the other fourteen rows.

`crates/print`'s doc comment names page ranges, scaling, N-up, booklet and
print-as-image. `ACROBAT-PARITY.md` puts sixteen printing rows in M3. Some of
that is deep.

**The defensible M3 subset**, which is what P15 through P17 as written deliver:
the imposition engine, page range and subset (all, current, custom, odd and
even), page sizing and handling (Fit, Actual size, Shrink oversized, Custom
scale), N-up, orientation, duplex, Comments and Forms, Page Setup, Print as
Image, Print to File, the print dialog itself with a live preview, and the
Advanced Print Setup dialog carrying only its two in-scope items. Fourteen of
the sixteen rows.

**What slips: booklet (row 90) and poster / tile (row 91).**

Booklet is not "N-up with a different order". It needs signature ordering,
creep and shingling compensation for paper thickness, a binding-edge model,
subset-booklet ranges, and duplex sheet pairing where a mistake is only
observable on folded paper. It is a package's worth of geometry whose
acceptance criterion is a physical artefact, which sits badly in a milestone
whose print backend already cannot be tested in CI.

Poster and tile needs tile overlap, tile marks, cut marks and page labels drawn
onto the output. Drawing marks onto printed output is the surface PLAN.md puts
permanently out of scope one paragraph earlier ("Output, Marks and Bleeds,
PostScript options and print colour management stay with the out-of-scope
print-production surface"), so it sits on a boundary the plan has already drawn
and would be the only in-scope feature on that side of it.

Neither is on the path to "M3 can print". Both are pure imposition math once
someone writes them, so both land cheaply at M4 on top of P15's `Sheet` model,
alongside the CUPS and Windows backends that M4 already carries.

**The tradeoff.** PLAN.md decision 13 and the `crates/print` doc comment both
name booklet in the M3 parity list, so this is a plan edit, not a silent cut.
Booklet is also the print feature Acrobat users name most often when comparing
products, and leaving it out of "the identity release" is a visible gap in a
way poster and tile is not.

**The recommendation, which was taken: move both to M4, and take the plan edit
rather than the silent cut.** PLAN.md decision 13 is corrected in this branch's
PLAN.md commit so the deferral is recorded where the promise was made. The road
not taken was a fourth print package delivering booklet imposition alone with
creep compensation excluded; it is named here only so nobody re-derives it as a
new idea.

**What ruling B obliges P15 to do anyway.** M4 must be able to add both rows
without reopening P15. So `Sheet` and `Placement` must already express a sheet
whose placements are neither a uniform grid nor in page order, and P15 carries
one test that composes a hand-built sheet of that shape. That is the whole cost
of keeping the door open, and it is cheaper than the alternative of discovering
at M4 that the model assumed a grid.

---

## 10. Definition of done for M3

- All 99 `ACROBAT-PARITY.md` M3 rows are `implemented`, `partial` with a stated
  cut in their Notes, or moved to a later milestone with a reason recorded in a
  review. The scoreboard's executable totals contract recounts and passes.
- The three M2 rows M3 unblocks are flipped (right-hand side panel, quick action
  toolbar, Page Thumbnails context menu), and the Layers pane's `Properties`
  entry is live.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` green on macOS, Linux and Windows.
- `cargo build -p onionskin-app --features shell`,
  `cargo test -p onionskin-app --features shell`, and
  `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support`
  green on the runners the shell job covers, with the matching clippy
  invocation. (`--features shell` is not a workspace-level flag; it must be
  `-p`.)
- `cargo test -p onionskin-app --no-default-features` green: guarantee 5 still
  holds with an edit graph, a print crate and three new plugins in the workspace.
- `cargo test -p onionskin-print` runs in the default workspace job, with no
  window, no display and no printer.
- No crate outside `crates/app` imports GPUI, asserted by the existing test.
  `crates/print` imports AppKit directly and no GPUI, asserted the same way.
- **Guarantees 1, 2 and 6 pass at the level the guarantee means**, not one layer
  down: guarantee 2 is driven by an edit a tool made through `core`, and every
  guarantee M3 un-ignores has a CI step that makes its corpus mandatory, so none
  of them can repeat guarantee 6's vacuous pass.
- P7's registry-exhaustive property tests pass over the real `build_registry()`:
  every registered tool and command is undoable, serializes to a section a fresh
  parse accepts, survives degenerate documents, and is deterministic.
- P4's structure invariant is clean after every M3 edit on every tagged fixture,
  and it **fails** on the deliberately broken fixture in the same suite.
- P1's reference validator finds nothing on the output of every P5, P11, P12 and
  P14 operation over every named fixture.
- P16's manual print acceptance script has run on macOS and its result is
  recorded in that package, including what it failed at. **M3 is not done until
  it has.**
- Every new control appears in the AccessKit tree with a real label and state,
  and every dismissible surface removes its controls from the tree rather than
  leaving invisible tab stops. The macOS accessibility probe stays a required CI
  gate.
- Every new keyboard route is proven with `cx.simulate_keystrokes` on a real
  window.
- `known-issues.md` has every M3-deadline entry removed or narrowed: the cos
  dangling-reference entry (reassigned to `core::pages` and closed by P1's
  validator and P5's transformation), the Line Weights entry, the
  `Include Comments` entry, the `SearchResult::Unavailable::reason` widening;
  plus the new entries M3 earns (the encryption class split and ruling A's
  accepted regression for documents whose `/P` bits allow modification, the
  `/OC` annotation-visibility consequence, and any deferral from section 7).
- P1b's measurement table is committed under `docs/evidence/` and its tally is
  asserted by a test, so the orchestrator can land the `known-issues.md`
  encryption entry from a number that cannot go stale silently.
- The dogfood claim carries its caveats. "M3 edits PDFs non-destructively" is
  stated with what it cannot do attached: no text editing, no form filling, no
  redaction, no signing, no booklet or poster printing, and, on an encrypted
  document, no editing at all even where its permission bits would allow it. A
  claim that omits them is a defect, not a simplification.
