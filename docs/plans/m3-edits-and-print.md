# M3 implementation plan: first edits, first print

Status: planning, with section 9's two open decisions ruled on 2026-09-02 and
folded in, and section 1's ground truth re-measured against `main` at `fa5a194`
on 2026-09-10. P0a merged at `28b311e` and P0b merged at `755842f`.
P0c's test relocation is committed locally at `fdf875c`; automated relocation
checks pass. Native smoke and Task A/B window-only captures passed at `d34b29f`
on 2026-09-11; final integration remains pending. Editing/printing packages
remain outstanding. This document is the
authoritative decomposition; it supersedes PLAN.md's M3
paragraph wherever the two disagree, and section 8 lists every disagreement.
The PLAN.md corrections those disagreements called for **landed at `989d8a7`**;
section 8 marks each item as landed or still owed rather than describing all of
them as pending.

M3 is the milestone where Onionskin stops being a viewer. It is the first time
the product writes a byte, and every architectural claim the project has made
about non-destructive editing becomes an executable fact or an embarrassment.
It is also the largest milestone by scoreboard weight: **99
`ACROBAT-PARITY.md` rows assigned, 97 shipped** once ruling B moves booklet and
poster/tile to M4, against M2's 67.

Scope boundary, held throughout: `crates/mcp`, `crates/cli`, `crates/scripting`,
`crates/text-engine` and `plugins/{tools-edit,tools-form,tools-fill-sign,redact,
tools-protect,tools-accessibility,tools-measure}` are M4 and later and get no
code here. `cargo test -p onionskin-app --no-default-features` (guarantee 5)
must stay green at every commit, and no crate outside `crates/app` may import
GPUI. `crates/print` is GPUI-free; `app` supplies only the dialog.

---

## 1. Ground truth

**Re-measured against `main` at `fa5a194` on 2026-09-10, which is the baseline
P0a started from and therefore PREDATES P0a's own merge at `28b311e`.** That is
deliberate: this section describes the tree the plan was written against, and
every package below is scoped against it. **One consequence to carry:
`crates/app/src/shell/chrome/tabs.rs` no longer exists** - P0a replaced it with
the nested `chrome/tabs/` tree, so every `tabs.rs:NNNN` citation in this document
is a citation into the pre-split file and resolves in `tabs/mod.rs` or one of the
six concern modules after it. The figures are still the right denominators for
P0a's inventory, which is what they are for. The first draft of this
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
| `crates/plugin-api` | **755** lines, real | `ToolPlugin` with its full gesture lifecycle, `CommandPlugin`, `CodecPlugin` (export only), `PluginRegistry`, `ToolCtx { doc, viewport }` (`lib.rs:81`), `ToolCapability` (**8** variants, `lib.rs:60`), `Overlay` (6 variants, `lib.rs:43`: `AntsRect`, `Rect`, `Quads`, `Polyline`, `Line`, `Circle { center, radius }`), of which **only `Quads` and `AntsRect` can be painted**: `canvas.rs`'s `OverlayPaint` (`canvas.rs:344`) has those two variants and `map_overlay` (`canvas.rs:1879-1895`) returns `Err` for the other four, surfacing `"the canvas cannot draw a {kind} overlay yet"`. `tools-basic` uses exactly the two that work. **A tool has no way to express a document edit.** Its own module doc says the import path "waits for the edit graph ... which is M3". `Requirement` is not here: it is a private four-variant enum in `crates/app/src/shell/context_menu.rs:47`. |
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
| Layers pane `Properties` disabled "Available in M3 with the properties dialog" | P13a |
| `Copy With Formatting` and `Export Selection As` disabled "Available in M3 with rich-text export" | P22, and see section 8 item 12 for why only half of it can ship |
| Find bar `Include Comments` disabled, "Comments arrive with the comment tools in M3" | P20 |
| The encryption class split (`permissions-only` versus password-protected) that m2-viewer section 6 assigned as a `known-issues.md` ledger action **was never written**. `known-issues.md` has no encryption entry at all. | P1b produces the measurement (section 9, ruling A); the orchestrator lands the ledger entry |
| Textual CI tripwires in `guarantees.rs` are evadable by a softened harness (CR-005, REPO-011) | Section 3, T9: every guarantee M3 extends states the mutation that must break it. M3 un-ignores none of them, since 1, 2 and 6 already run; it deepens 1, 2 and 6 and leaves 3, 4, 7 and 8 to M5 |
| `SearchResult::Unavailable::reason` is `&'static str`, to be widened "after P9 lands" | P20 (the comments filter needs a dynamic reason; the "after P9" in the ledger entry now means after P9a, P9b and P9c) |

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
a rewrite of the referrer; the removed object stays in the file as garbage.

**The save boundary is enumerated over the overlay's components, not argued case
by case.** Everything a save clears is something undo must be able to restore
afterwards, so the enumeration is the overlay's own field list, and **adding a
field to `Overlay` means adding a row here**. That rule exists because this
section was written before the overlay had a trailer, was not re-run when the
trailer arrived, and shipped a silent no-op in the component it had not
considered. One table, re-derived whenever the shape changes:

| Overlay component | What a save does to it | What undo across that save has to do | How |
|---|---|---|---|
| `states` (an object that exists in the base) | writes it; reopen makes it the base | put the base value back | the base-capture rule: `before` is a concrete `Some(ObjectState)` read at edit time, so undo does not consult a base that has since moved |
| `states` (an object this session created) | writes it; reopen has it | make it unreachable | drop the overlay entry, **and** restore the referrer, whose own `Change` in the same `Entry` carries a concrete `before`. The bytes stay, unreferenced, which is what the core invariant promises |
| `trailer` (a key the base already had) | writes the new value | put the old value back | same base-capture rule, one level up: `before` is the live base trailer's value, read at edit time |
| `trailer` (a key this session created) | writes it; reopen's base trailer **has** it | **clear the key** | **nothing above the trailer can un-name it, because the trailer is the root.** This is the row that has no referrer, and it needs an explicit cleared state; see below |
| `next_number` | nothing | keep handing out numbers above everything written | **reseed from the reopened document**, never reset; see T3 |

**The trailer row is the one that breaks without new machinery, and it breaks
silently.** Trace it on `corpus/seeds/minimal.pdf`, which has no `/Info`. Set a
Description: one `Change::Object { 100, before: None }` and one
`Change::TrailerKey { Info, before: None }`. Save: reopen, overlay cleared, and
the base trailer now **has** `/Info`. Undo: both `before`s are `None`, so both
changes remove their overlay keys and the overlay is empty. Save: nothing is
written. **The Description is still there**, and this section promises the
opposite in words.

For an object that case is safe because the referrer's `Change` restores
reachability. The trailer has no referrer. It is the root of the file and nothing
above it can stop naming a key.

Worse, `section_for` **cannot express removing a trailer key at all**:
`incremental_section` clones the base trailer and applies
`for (key, value) in self.trailer_edits { trailer.set(key, value) }`
(`document.rs:1097-1099`), and a `Dict` of edits is set-only. So pass 2's claim
that "`section_for`'s two arguments both come out of one undoable structure" was
true within one save and false across one.

**The fix, and it needs no new `cos` surface:**

- `Overlay.trailer` becomes `BTreeMap<Name, Option<Object>>`, where `Some(v)`
  sets the key and **`None` explicitly clears it**.
- `section_for` writes a cleared key as **`Object::Null`** into the emitted
  trailer. ISO 32000-1 section 7.3.7 makes a dictionary entry whose value is null
  equivalent to that entry being absent, so this needs nothing beyond
  `Dict::set`, which cos already has.
- `Change::TrailerKey`'s `before` is `Cleared` or `Set(v)`, captured against the
  overlay first and the **live base trailer** second, exactly as an object's
  `before` is. There is no "not in the overlay" state: a key's absence and a
  key's clearing are the same instruction to the section writer.

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
    /// append nothing, which is exactly when `overlay` and `trailer_edits` are
    /// both empty *and* `self.provenance` is `Clean` - a repaired document with
    /// an empty overlay still owes its repair (T3). Takes `&self`: no cache is
    /// dropped, and the document's own edit map is neither read nor written.
    pub fn section_for(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        trailer_edits: &BTreeMap<Name, Option<Object>>,   // None clears the key
    ) -> Result<Option<Vec<u8>>>;

    pub fn save_overlay_to_path(
        &self,
        overlay: &BTreeMap<u32, PendingEdit>,
        trailer_edits: &BTreeMap<Name, Option<Object>>,
        path: &Path,
    ) -> Result<()>;
}
```

`incremental_section()` becomes `self.section_for(&self.edits, &adapted)`, where
`adapted` maps cos's own `trailer_edits: Dict` into the argument form with every
entry as `Some`. cos's own edit path has no way to clear a key and never needed
one, so the adaptation is total and lossless in that direction, and
`incremental_section` keeps its behaviour exactly. The **clearing** direction
exists only for `core`, which is the caller that has an undo stack. A method rather than a
free function because every input it needs is private to `Document` (`reader`,
`xref`, `provenance`, `prev_startxref`, `trailer`, `original_len`, `locate`,
`get`), and a free function would have to make all of them public to save one
`self`. With this, T2 and T4 stop contradicting: save and preview are the same
call with the same argument, so they cannot disagree by construction, and no
scratch document exists.

```
core::edit::Overlay  =  { states:  BTreeMap<u32, ObjectState>,
                          trailer: BTreeMap<Name, Option<cos::Object>>,   // None = clear the key
                          next_number }
core::edit::ObjectState = { generation: u16, object: cos::Object }
core::edit::TrailerState = Cleared | Set(cos::Object)      // no NotInOverlay: nothing produces it
core::edit::Change   = Object    { number: u32, before: Option<ObjectState>, after: Option<ObjectState> }
                     | TrailerKey { key: Name,  before: TrailerState,       after: TrailerState }
core::edit::Entry    = { label: &'static str, changes: Vec<Change> }
core::edit::History  = { entries: Vec<Entry>, cursor: usize, saved_mark: Option<usize> }
```

**`TrailerState` has a `Cleared` variant where `ObjectState` has nothing like
it, and that asymmetry is the point** (T1). An object this session created is unmade by dropping it from
the overlay, because its referrer's own `Change` stops naming it. **The trailer
is the root and has no referrer**, so undoing the creation of a trailer key has
to say "clear it" rather than "forget it", and that state has to survive into the
emitted section. `section_for` writes a cleared key as `Object::Null`, which ISO
32000-1 section 7.3.7 makes equivalent to absent, so cos needs nothing beyond the
`Dict::set` it already has.

There is **no `Deleted` variant** on `ObjectState`, because T5's rule is that M3
frees no object number: a removal is a rewrite of the referrer, and the removed
object simply stops being reachable. So `ObjectState` is one shape and
`Option<ObjectState>` is two states, not three. `generation` is carried because a
`cos::Object` written back to an existing number has to be written at the
generation that number already has, and reading it back out of the base at write
time would be a second source for a fact the change already knows.

**The trailer is inside the overlay, and a trailer key is an undoable change.**
The first draft of this section left it outside both, and `section_for`'s second
argument (`trailer_edits: &Dict`) had no stated home, no capture rule and no way
into an `Entry`. That is not a theoretical gap: it is reachable on the **first
Document Properties edit of an ordinary file**. `cos::set_info_field` on a
document with no `/Info` does `add_object` **and** `set_trailer_entry("Info", ...)`
(`document.rs:936-955`), and `crates/cos/tests/incremental.rs:459` is named
`a_document_with_no_info_dictionary_gains_one` and runs against
`corpus/seeds/minimal.pdf`, a **tracked** seed, so the path is exercised on every
CI run rather than only when the external corpus is present.

Since T2 forbids `core` from calling `add_object` or `set_trailer_entry`, `core`
does both itself: reserve a number, write the new `/Info` dict into the overlay,
and set the trailer's `/Info` key. With the trailer outside `Change`, two things
break silently:

- **Undo leaves a dangling trailer.** It drops the `/Info` object from the
  overlay and leaves the trailer naming it, so the next save emits a trailer
  pointing at an object the section does not write and the base does not have.
  `section_for`'s gate as first written covers "every reference in every object
  this section writes", and **the trailer is not one of those objects**, so the
  gate does not see it either.
- **T3's headline claim is false on this path.** "Edit, then undo, then save
  writes nothing at all" rests on the net overlay being empty, but
  `has_pending_changes()` includes `!self.trailer_edits.is_empty()`
  (`document.rs:961-963`), and the collapse-by-value rule has no trailer key to
  compare against.

So `Overlay` gains a trailer map and `Change` gains a `TrailerKey` variant, in
the shape above. A trailer edit is then captured, undone and collapsed
by value exactly like an object edit, and **`section_for`'s two arguments both
come out of one undoable structure** rather than one of them coming from
nowhere - across a save as well as within one, which is the part pass 2 got
wrong.

The alternative, refusing `SetInfoField` on a document with no `/Info`, is a
visible cut that P13a would have to state in row 14's Notes, and it is worse: it
makes "set a Description" fail on the simplest possible file.

Projecting `Overlay` onto `section_for`'s two arguments is then a one-to-one map:
`states` onto `BTreeMap<u32, cos::PendingEdit>`, using `PendingEdit::Set` alone
since `PendingEdit::Delete` is never produced, and `trailer` straight through.

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
whose `/Annots` gained a reference) and one stack entry.

**Two producers can write into one `Entry`, so the order is a rule and not an
accident.** A single transaction can carry changes from the verb itself, from
P4's structure-tree maintenance hook, and now from a trailer key. Undo replays an
`Entry`'s `before`s and redo its `after`s, so if two producers both touch object
N the last write wins in each direction and the two directions must agree. The
rule:

> **Within one `Entry`, at most one `Change` exists per key** (per object number,
> and per trailer key). A producer that touches a number another producer in the
> same transaction already touched **updates that `Change`'s `after` and leaves
> its `before` alone**, because `before` is the state before the transaction, not
> before the producer.

That makes the order producers run in irrelevant to the result, which is the
property worth having: the hook runs after the verb today because the verb
decides what changed, but nothing downstream may depend on that. `transact`
enforces it by keying its in-flight changes rather than pushing onto a `Vec`,
and P2 asserts it with a transaction whose verb and hook both rewrite the same
page dict. **The referrer change is
not optional bookkeeping: it is the change that makes undo work at all**, since
under the free-nothing rule reachability is the only thing an undo can alter
about an object it created.

**T3. One section per save, and what a save of nothing writes.**

Guarantee 2 asks that "an edit appends one incremental section that truncates
away". The resolution is **one section per save, not per edit**, and cos already
behaves this way: the section is materialized from the net overlay at save time.
Ten edits then one save is one section carrying the net object writes of all ten.

Three consequences the plan states rather than discovers:

- **Edit, then undo, then save writes nothing at all** on a clean document.

  **`section_for`'s early-out predicate, stated explicitly, because this is where
  the repaired-document exception actually lives:**

  > `section_for` returns `Ok(None)` iff the overlay's `states` is empty **and**
  > its `trailer` is empty **and** the document's provenance is `Clean`.

  The third clause is not a hedge. `has_pending_changes()` is
  `!edits.is_empty() || !trailer_edits.is_empty() || !provenance.is_clean()`
  (`document.rs:961-963`), and dropping the provenance clause would stop a
  repaired document writing its repair. So "a save with an empty overlay writes
  nothing" is true for a clean document and false for a repaired one, where it
  writes the repair and nothing else. Every test asserting byte-identity on a
  no-op save partitions on `Provenance` (P3).

  The trailer clause is load-bearing rather than decorative: a trailer key left
  behind by an undo makes the whole claim false, which is exactly what happens on
  the first Document Properties edit of a file with no `/Info` (T1).

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
- **The net overlay collapses, by two rules and not one.** Adding an annotation
  and then deleting it in the same session must write nothing, and the rule as
  first stated does not achieve that.

  1. **Value comparison against the base**, for objects **and for trailer keys**.
     An overlay entry whose value equals the base document's object at that
     number drops out; an overlay **trailer** entry equal to the base trailer's
     state for that key drops out too, where **`Cleared` equals the base not
     having the key** and `Set(v)` equals the base having `v`. Not a dirty flag,
     or the file grows a section that changes nothing. This is what puts the
     page's `/Annots` back and takes the page dict out of the overlay, and it is
     what stops a pre-save undo of a new `/Info` writing `/Info null` onto a
     document that never had one.
  2. **Unreachable-creation collapse**, which rule 1 cannot do because **a
     session-created object has no base object to compare against**. An overlay
     entry for a number with **no base object**, referenced by **nothing in the
     overlay and nothing in the base**, drops out. That is what takes the
     annotation dict and its appearance stream out after rule 1 has restored the
     page.

  Symmetrically, `Change`'s `after` is `None` under the same meaning `before`
  carries: not in the overlay. So add-then-delete is two `Entry`s whose net
  effect the two rules erase, and `section_for` returns `None`. The test lives in
  P3's bullets, not only in this paragraph.
- **After a save, `core` reopens the `cos::Document` from the written bytes and
  clears the overlay.** The reopen makes the `/Prev` chain correct by
  construction for the second save and matches the existing
  `ExportSnapshot::open` pattern.

  **This is one function, `adopt`, and there are five call sites.** Written as a
  rule "after a save", which is how it first appeared, it covers one of them and
  four get nothing - and one of those four is the exact catastrophe the rule was
  written to prevent:

  > **`fn adopt(&mut self, bytes: Arc<Vec<u8>>)`**: reopen from `bytes`, clear
  > the overlay's `states` and `trailer`, reseed `next_number`, drop the cached
  > `structure()` document and the per-generation caches, and reseat the render
  > and search workers.

  | Call site | Why it needs `adopt` and not a save-shaped rule |
  |---|---|
  | **Save** | the case the rule was written for |
  | **Save As** | covered by implication only, which is not covered |
  | **`revert_to`** | truncates, reopens, **and clears `History` both ways** (below), so `next_number` must come **entirely** from the reopened document. `Overlay::default()` here puts the next annotation at object 0 or 1 and **overwrites the catalog** - and this is the one package whose review risk deferred the question to P3 |
  | **Crash-recovery replay** | replays an overlay naming numbers a freshly opened document has never seen, so seeding from that document alone **collides on the first edit after recovery** |
  | **Generation preview (P19)** | a fifth document over a truncated range, with no stated relationship to the overlay, to `next_number`, or to `structure()`'s readers until now |

  **The reseed itself**, which `adopt` performs and which differs by caller:

  > `next_number` = `max(reopened.next_object_number(), 1 + the highest object
  > number named anywhere in `History`, including the redo tail)`, never reset.
  > After `revert_to`, `History` is empty, so the second term vanishes and the
  > value comes wholly from the reopened document - which is correct, and is why
  > the two are one expression rather than two rules.

  `Overlay::default()` would give `next_number` zero, so the next annotation
  would be written at object 0 or 1 and **silently overwrite the catalog**, and
  nothing in the design catches it: `core` never calls `set_object`, so cos's
  object-0 and freed-number refusals never fire; `section_for` writes it at the
  colliding number; and `audit_references` finds every reference resolving,
  because they do. A green suite over a destroyed catalog.

  **The `History` term is not belt and braces; without it two objects alias onto
  one number.** `incremental_section` writes `Size = highest + 1` over the
  numbers it **wrote**, not over the session's counter, and this plan explicitly
  lets the counter run ahead of what was written ("an undo that drops an
  allocation simply leaves a gap"). So: add annotation A (objects 4 and 5), add
  annotation C (6 and 7), undo C, save - the save writes 4 and 5, `Size` is 6,
  and a reseed from the reopened document alone gives `next_number = 6`. Now redo
  C, which puts 6 and 7 back into the overlay, and add annotation D, which
  allocates **6**. D overwrites C, the page's `/Annots` names 6 twice, and every
  reference resolves. The redo tail is live state the file has never seen, so the
  file cannot be the only source for the high-water mark.

  With the `History` term the reseed is safe in both directions: it is at least
  the reopened document's value, which is at least
  `max(/Size, xref.max_number() + 1, 1)` (`document.rs:204-211`), and at least
  one above anything the session has named but not yet written.

  **The history needs no rebasing**, which is what T2's base-capture rule buys:
  `before: None` means the same thing on both sides of a save. The caches keyed
  on edited pages are invalidated; the rest survive. Nothing has to be cleared
  inside cos, because nothing was ever put there.

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
what `ToolPlugin::overlays` already returns. Committed edits go through the
preview buffer. A tool never draws its own committed result.

The seam is clean; it is not finished. `tools-basic` uses two of `Overlay`'s six
variants and those two are the only two the canvas can paint: `OverlayPaint`
(`canvas.rs:344`) has `Quads` and `AntsRect`, and `map_overlay`
(`canvas.rs:1879-1895`) returns `Err` for `Rect`, `Polyline`, `Line` and
`Circle`, which `overlay_paints` turns into the status
`"the canvas cannot draw a {kind} overlay yet"`. Every M3 comment tool except the
text markup ones needs one of those four, so **P9a lands the painters** before
P9b and P9c can preview anything.

Cost, stated in terms of `section_for` because that is what runs: a committed
edit calls `section_for(&overlay, &trailer_edits)`, which serializes the changed
objects, builds the table rows, allocates a new `Arc<Vec<u8>>` for
`original ++ section`, and the render worker builds a new hayro `Pdf` from it.
`Pdf::new` was measured at about 1.4 microseconds per page in the M1 spike, so a
1000-page document is about 1.4 ms per commit.

**And one cost the model omitted entirely: the `original ++ section` copy is
O(file size), on every committed edit.** A 67 MB scan re-copied per ink stroke is
not a rounding error next to 1.4 ms of `Pdf::new`. The strategy, stated rather
than discovered: **one preview buffer, reused** - truncate it to `original_len`
and extend with the new section, so only the **first** commit of a session pays
the copy and every later one pays the section length. The buffer is the
`Arc<Vec<u8>>` the worker holds, so reuse means allocating a fresh `Arc` only
when the old one is still referenced.

State also **which state survives a preview-generation bump, over
`core::Document`'s full field list rather than a sample of three.** The same
discipline T1's overlay table uses, and for the same reason: a list that samples
is a list that misses the field nobody thought about.

| `core::Document` field | Preview-generation bump | Save + reopen (`adopt`) | `revert_to` (`adopt`) |
|---|---|---|---|
| `bytes` / preview buffer | replaced; this **is** the bump | replaced with the saved bytes | replaced with the truncated bytes |
| `cos` | unchanged | reopened | reopened |
| `structure()` cache | dropped, rebuilt lazily | dropped | dropped |
| `provenance` | unchanged | **a repaired document becomes `Clean`**, so the repair notice must clear | re-derived; may become `Repaired` again |
| `page_count` | not a field; `content::page_count(structure())` | same | same |
| geometry cache | keyed `(generation, page)`; unedited pages hit | dropped | dropped |
| `pending_geometry` | drained and discarded, or responses carry the generation (F6) | drained | drained |
| text cache | keyed `(generation, page)` | dropped | dropped |
| tiles | unedited pages survive | dropped | dropped |
| `outline`, `attachments`, `signatures`, `layers` | dropped; re-read through `structure()` | dropped | dropped |
| `selection` | survives, **clamped** to the new page set | clamped | clamped |
| search matches and cursor | **page-keyed, so dropped**; a match on a removed page is not clampable | dropped | dropped |
| search worker | re-seeded | re-seeded | re-seeded |
| snapshot's pending page index | dropped | dropped | dropped |
| render worker | new `RenderSession` over the new bytes (F9) | same | same |
| `Overlay.states` / `.trailer` | unchanged; the bump is *caused* by them | cleared | cleared |
| `Overlay.next_number` | unchanged | reseeded (`adopt`) | reseeded from the reopened document alone |
| `History` | unchanged | unchanged; the saved mark advances | **cleared both ways**, `saved_mark = Some(0)` |

Getting a row wrong in one direction is a full re-render per keystroke, and in
the other is a pane showing a document that no longer exists. The table is over
the session's **real fields**, so adding a field means adding a row - the same
rule T1's overlay table and `structure()` both carry.

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
  Size (P14b) is the operation that reclaims**, and it does so through
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

**What free-nothing converts the hazard into, stated once because every fix-up
below is an instance of it.** Freeing nothing means no reference in the file ever
resolves to a free entry, which is the dangling class gone. It does **not** mean
the file is consistent: it converts *dangling* into **stale but resolvable**. A
live structure that still names a removed object now walks into a garbage node
whose own pointers resolve too, so `audit_references` reports nothing,
`section_for`'s gate reports nothing, and a naive reachability check passes.

Therefore **every fix-up whose job reads "drop X" actually means "unlink X from
every live structure that names it"**, and a fix-up that stops writing an object
without unlinking its referrers has done nothing at all. That is the general
form; the list below is its instances, and each one's verification walks the
structure rather than checking reachability.

What the transformation must also carry, and what an adversarial reviewer will
check it forgot:

- `/Annots` on surviving pages (carried through untouched; a removed page's stay
  attached to its own now-garbage dict, per the rule above).
- `/PageLabels`, a number tree keyed on page index, which every reorder
  invalidates.
- **Named destinations**, which are two different structures. `/Dests` in the
  catalog is a flat dictionary. `/Names /Dests` is a **name tree** and needs
  `/Kids`, `/Names` and **`/Limits`** rebuilt, exactly as `/PageLabels` needs a
  number tree rebuilt. "Walks the tree" is not "rebuilds the tree": a rebuild
  that drops entries without recomputing every ancestor's `/Limits` leaves a tree
  whose lookups miss surviving names. It shares one tree-rebuild helper with
  `/PageLabels` and with P4's `/ParentTree`, rather than growing a third.
- **The outline's sibling chain**, which is the same doubly-linked class as the
  article beads below and was getting the shallow treatment the beads no longer
  get. Outline items are a chain through `/Prev` and `/Next`, with `/First`,
  `/Last` and `/Count` on each parent. "Dropped" is a decision about the item and
  not a mechanism: if the item's object simply stops being written, its surviving
  siblings still name it, and under free-nothing that object is still in the file,
  so the live chain walks into a garbage node whose `/Dest` names a garbage page.
  Nothing catches it: `audit_references` resolves, the gate resolves, and P4's
  invariant does not look at `/Outlines`. The work is: re-link `/Prev` and
  `/Next` past the removed item, update the parent's `/First` or `/Last` when the
  removed item was one, recompute every ancestor's `/Count` **preserving its sign**
  (a negative `/Count` means a closed item), and unlink a node all of whose
  descendants are gone by the same rule. The item's `/A` and `/D` entries are the
  easy half and were the only half the first draft had.
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
- **Actions on surviving pages naming a removed page**, which is wider than the
  first draft's "link annotations" and is a different object from the outline
  entries already listed. The scope is **every annotation's `/A` and `/AA`, not
  only `/Link`'s**, following `/Next` action chains: a `/Widget` pushbutton with
  a `GoTo` is the most ordinary form control there is, and it sits on the same
  form fixture the `/AcroForm` bullet already mandates. Plus **the catalog's own
  `/AA`**. The outline fix-up walks `/Outlines`; this one walks every surviving
  page's `/Annots` and the catalog. Neither finds the other's case.
- **`/OpenAction` and page-level `/AA`** naming a removed page. Carried in the
  first draft's list, dropped when this one was rewritten, and restored here
  because P5 owns the work either way and **this list and P5's are meant to be
  the same list**: this one is what an adversarial reviewer checks the
  transformation forgot, so a fix-up present in P5 and absent here is a gap in
  the check rather than in the code.

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
annotation filter, and gets bytes.

**The filter argument has exactly one M3 consumer, and it is the print path.**
The first draft also claimed the Comments pane gets a "hide all comments" view
"for free" from the same mechanism. That view is **not one of M3's rows**: the
Comments pane's rows are 29, 30, 74, 75 and 78, and none of them is a
visibility toggle. Naming it as a second consumer would have been a parameter
justified by a feature nobody scheduled. One real consumer is enough for a
parameter that is the entire point of this resolution; a second invented one is
not an improvement.

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
destroy; it is the only sane reading. **PLAN.md now says it**, at `989d8a7`:
the core invariant carries "A document we author has nothing beneath it".

Mechanically, this needs `cos` to be able to serialize a whole document, which
is `flatten` under a different name. P1 builds it as
`Document::write_new(objects, trailer) -> Result<Vec<u8>>` used by P11, P12, P14a, P14b, P15 and
the comment summary, and it is the same primitive `redact` and compress will
use at M5. Naming a real M3 consumer is what lets it exist now (§ YAGNI).

**T9. Verification discipline, learned expensively in M2.**

Every package's verification section obeys these, and they are stated once here
rather than repeated thirty-two times:

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
  `the_find_keystroke_opens_the_find_bar` (pre-split `tabs.rs:4579`; after P0a it
  is in `tabs/mod.rs`'s test module, and after P0c in the module for its
  concern).
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
say which rows they back. Section 6 is the complete ledger of all 99 rows M3
was assigned, including the two it hands to M4.

Four numbers (P0, P9, P13, P14) are **umbrella sections**: a shared preamble
saying why the work splits, then `####` sub-packages that are the real units.
A sub-package carries every field above except where the umbrella already
carries it: P0 states Rows closed, Depends on, What exists to build on and
Review risk once for all three of P0a, P0b and P0c; P9 and P13 state the shared
goal once, and P9a, P9c and P13c have no "What exists to build on" because there
is nothing in-repo for them to build on. Thirty-two packages in total.

### P0. Split `ShellFrame`, in three packages

**Goal.** Make the sixteen M3 packages that touch `crates/app/src/shell` able to
run in parallel. This is not tidying; it is the precondition for the schedule.

**P0a alone is necessary and not sufficient**, and the plan says which packages
complete it. After P0a, `tabs/mod.rs` still carries a **4669-line test module**
that every package adding a shell test must edit, which is the same conflict
magnet in a new place. **P0c** moves those tests out. **P0b** does the field
restructuring. "P0" as a precondition means all three.

`crates/app/src/shell/chrome/tabs.rs` is **8521 lines** with a single `impl
ShellFrame` block spanning lines **371 to 2924** and a **24-field** struct
(`tabs.rs:234-264`). The inherent block holds **115** methods; `impl Drop` and
`impl Render` add one each, for 117 across all three.

**Settled: 115 methods in the inherent block, 117 across all three impls, 24
fields.** The earlier report of 118 and 25 counted the two trait-impl methods
(`Drop::drop` and `Render::render`) toward the inherent block's total and counted
one doc-commented item as a field. Recorded because the count is the denominator
of P0a's inventory, and an inventory whose total is described as unknown is the
failure mode this package exists to prevent. M2's own audit named it the recurring conflict point. Every M3 app
package adds a field, a menu arm, a dialog call site and an accessibility child
to that one block. Sixteen branches doing that concurrently is sixteen rebases
through a 2500-line `impl`, and a rebase through an `impl` block resolves without
judgement only until two packages add a method with the same name.

**2026-09-10 status: P0a merged to `main` at `28b311e`, and P0b at `755842f`.**
Every Files line below that names a `chrome/tabs/*.rs` path names a path that
exists. P0c remains outstanding; the production split does not complete the
test-module split or any editing/printing feature. The pre-split counts above
remain the historical acceptance baseline, not current source measurements.

**It is three packages. Two of them because the acceptance test only works for
one, and the third because P0a leaves the tests where they were.**
The inventory below compares method bodies by normalized hash, on the principle
that a moved body hashes the same. That holds for pure relocation and fails
completely for field restructuring: moving a loose field into a sub-struct
rewrites `self.foo` to `self.state.foo` in every method that touches it, so
**every body hash changes and the inventory degenerates into noise for exactly
the transformation it most needs to prove**. Running both under one acceptance
test would mean running neither. **P0c** is separate for the opposite reason:
the inventory certifies it for free, so it needs no acceptance work of its own
and should not wait behind P0b's.

**Shared by all three packages**, stated once rather than three times:

**Rows closed.** None, in any of the three. None of them changes behaviour.

**Depends on.** Nothing. P0a is the first of section 5's three day-one roots;
P0c and P0b both follow it and are independent of each other, since one moves
tests and the other moves fields.

Every app package below that writes "P0b" in its own Depends-on line means the
production split being complete. P1c and P8 name neither, because their only app
files are test targets none of the three rewrites. **P0c is not in anyone's
Depends-on line and still gates the schedule in practice**: until it lands, every
package adding a shell test edits one 4669-line module, so it is a contention
fact rather than a compile-order fact (section 5).

**What exists to build on.** `chrome/mod.rs` already re-exports `ShellFrame`
from `tabs`, so callers outside `chrome` see no change. `accessible.rs`,
`commands.rs` and the pane modules already demonstrate the target shape.
`ShellDialog` (`shell/dialog.rs:27`) is already a separate host with its own
`accessible`/`render` pair, so dialogs are not part of this split.

**Review risk**, also shared: whether this is a refactor or a rewrite wearing a
refactor's name. The reviewer should reject any behaviour change, including
"obvious" improvements, and the item inventory is what makes that reviewable
rather than a matter of trust. Whether the split lines follow M3's package
boundaries or the author's taste, which is the difference between it buying
parallelism and it buying nothing. Whether `frame_state.rs` became a second god
object. Whether a wildcard arm was preserved "for now", which would silently
readmit the failure mode the inventory exists to catch. Whether P0b's
normalization rule was written to make the inventory pass rather than to make it
meaningful, which is the one place in either package where the test can be tuned
to the result.

#### P0a. Relocation only

- The `impl` block splits by concern into `impl ShellFrame` blocks in a **nested
  `chrome/tabs/` module tree**, which Rust permits via inherent impls in the same
  module tree. **Nested, not sibling files.** The first draft named
  `chrome/{menu,dialogs,context,export,accessible}.rs`, and that layout was never
  implementable, for three reasons P0a's implementation review established:

  1. **It collides on day one.** `crates/app/src/shell/chrome/accessible.rs`
     **already exists**. Naming it as a new sibling retires the layout on its
     own, before any argument about visibility.
  2. **Nesting is what makes the split scope-preserving, which is the entire
     claim of this package.** `pub(super)` in a child of `chrome::tabs` reaches
     exactly the set a private item in `chrome::tabs` reached. Under the sibling
     layout the new modules are peers of `chrome::tabs`, so 120 items would have
     needed `pub(in crate::shell::chrome)`, which also hands `side_panel`,
     `rail`, `theme`, `tool_search`, `commands`, `accessible`, `quick_actions`,
     `page_controls` and `global_bar` direct access to every one of
     `ShellFrame`'s fields. That is a 120-item encapsulation leak wearing a
     relocation's name, and it is the opposite of "behaviour-preserving". P0a's
     reach analysis over the nested tree found **0 of 444 items widened and 50
     narrowed**.
  3. **Nesting leaves `chrome/mod.rs` untouched**, so zero Rust files outside
     `tabs/` changed. Siblings would have needed six new `mod` declarations there
     plus re-plumbing the `pub(in crate::shell) use tabs::ShellFrame` re-export,
     which is edits to a shared file in a package whose whole point is not
     touching shared files.

  This plan's own words, impl blocks "in the same module tree", are satisfied by
  nesting and violated in spirit by siblings.
- `run_main_menu_command`'s match and `canvas_context_entries`' match become
  the two extension points M3 packages append to, each in its own file.
- The export worker moves to `chrome/tabs/export.rs` unchanged.
- **No field moves, no signature changes, no body changes.** Every byte of every
  body is the byte that was there.

**Files.** `crates/app/src/shell/chrome/tabs.rs` becomes
`crates/app/src/shell/chrome/tabs/`, and **nothing outside it changes**:

| Module | Production lines |
|---|---|
| `tabs/mod.rs` | 1407, plus a 4669-line test module (P0c's) |
| `tabs/export.rs` | 791 |
| `tabs/accessible.rs` | 618 |
| `tabs/menu.rs` | 422 |
| `tabs/context.rs` | 381 |
| `tabs/frame_state.rs` | 317 |
| `tabs/dialogs.rs` | 162 |

`chrome/mod.rs` is untouched, `chrome/accessible.rs` is a different, pre-existing
file and stays where it is, and no file outside `tabs/` is edited.

(Those seven sizes are as of `28b311e`. Section 1's footnote rule applies: they
were not re-counted in this document's own pass, so re-count before relying on
any one of them.)

**The acceptance test, and why the obvious one is not enough.** A green suite
does not prove an 8521-line split preserved behaviour, because not every branch
in that file has a test: a dropped match arm or a method that lost its only call
site can leave all 500-odd shell tests green. The acceptance test is therefore
mechanical and does not depend on coverage.

1. **An item inventory, compared as a set, keyed on
   `(impl target, trait path, name, signature, normalized body)`.** Before the
   split, emit one line per top-level item and per `impl` method in `tabs.rs`.
   After the split, emit the same across the new files. **The two sets must be
   equal.** The key includes the impl target and the trait path because a method
   moved from `impl ShellFrame` to `impl Render for ShellFrame`, or between two
   inherent blocks with different `#[cfg]`s, hashes identically under a
   name-and-body key while changing what dispatches to it. A key that cannot see
   that is a key that passes on a real breakage.
2. **Exhaustive dispatch, by the mechanism each dispatcher actually has.** These
   are two different things and the first draft ran them together:
   - **Matches** (`run_main_menu_command`, `canvas_context_entries`, the pane
     action apply) lose their wildcard arm if they have one, so a lost arm is a
     compile error. This is real compiler enforcement and it is why removing the
     wildcards is part of P0a rather than a drive-by improvement.
   - **Tables have no wildcard arm to lose, and dropping a row from one is not a
     compile error.** `MenuCommand::all()` (`chrome/commands.rs:29`) returns a
     `Vec` built by pushing; deleting an entry compiles and silently removes a
     menu item. It is therefore **length-asserted against a `const` count, with
     every enum variant proven present by an exhaustive-match helper** whose
     arms the compiler checks. The fixed-size tables need nothing: the count is
     already in the type (`CanvasContextCommand::ALL: [Self; 13]`
     (`context_menu.rs:60`), `ThumbnailContextCommand::ALL: [Self; 11]`,
     `NavigationPane::ALL: [Self; 6]`, `QuickAction::ALL: [Self; 6]`,
     `LayerAction::ALL: [Self; 6]`), and P0a must not turn any of them into a
     `Vec` on the way past.
3. The suite, at the **same test count** before and after, plus clippy, plus the
   accessibility probe (`--features a11y-probe --test a11y_probe`), which proves
   the tree assembly moved intact.

**Verification.**
- The item inventory before and after is identical as a set, with the two listings committed to the PR so a reviewer can diff them rather than trust a claim.
- Every relocated dispatch match compiles without a wildcard arm; every `Vec`-shaped table is length-asserted and variant-exhaustive; no fixed-size table became a `Vec`.
- **Runs: every app configuration CI has, named, because a subset is how this package cut itself.** P0a shipped one self-inflicted defect, an import whose only user was feature-gated, which **compiled under all four configurations the first draft listed and not under CI's**. The list is therefore CI's own, from `.github/workflows/ci.yml`, **including the two that compile `onionskin-app` at default features and are easy to forget because they name the workspace rather than the crate**: `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build -p onionskin-app --features shell`; `cargo test -p onionskin-app --features shell`; `cargo test -p onionskin-app --features shell,shell-test-support`; `cargo test -p onionskin-app --no-default-features`; `cargo test -p onionskin-app --no-default-features --features tools-basic,commands-core`; `cargo clippy -p onionskin-app --features shell,shell-test-support --all-targets -- -D warnings`; and `cargo test -p onionskin-app --features a11y-probe --test a11y_probe` on macOS.
- **Green means no new diagnostics, not exit code zero.** The reviewer found a second instance of the same class shipping as ten warnings, which every one of the commands above reported and none of them failed on. The check is a diff of the diagnostic set before and after, and a new warning is a finding.

**What goes into CI afterwards, and what does not.** The inventory is a
**one-shot acceptance artifact**, and the two obvious ways to keep it are both
wrong for the same reason: regenerating a baseline and maintaining an allow-list
each try to turn it into a standing invariant, and any such gate fails on the
first legitimate behaviour change and is disabled within a week. What belongs in
CI is **`procedure_mutation.py`**: it mutates copies, needs no baseline, runs in
seconds, and asserts the only thing that needs asserting standing, which is that
the inventory still bites. It breaks when somebody deletes the mutated method or
renames a snippet, and **that is the feature**, in exactly the way
`ONIONSKIN_CORPUS_REQUIRED` turning a silent skip into a failure is the feature
(P1c). The two listings get a `README` saying what they are: a dated acceptance
record, not regenerated, expected to differ from the current tree, and a future
relocation emits its own pair at its own base.

**Mutation that must break its tests.** Not a mutation of the product: a mutation
of the acceptance procedure itself, because that is the thing being trusted.
**Take a copy of the pre-split `tabs.rs`, delete one match arm, delete one whole
`impl` method, and move one method from `impl ShellFrame` into `impl Render for
ShellFrame`. Run the split procedure on that copy and confirm the inventory
reports exactly those three and nothing else.** If it reports nothing, the
inventory is decorative and the split is unproven no matter how green the suite
is. Run it, and record the three deliberate mutations in the PR alongside the
inventory's output. Separately, delete one row from `MenuCommand::all()` and
confirm the length assertion fails, since that is the case the compiler does not
cover.

#### P0b. Field restructuring

**Status: merged at `755842f` on 2026-09-10.** The requirements below retain the
package's planned shape; this status correction checks commit ancestry and does
not claim a fresh verification run.

`ShellFrame`'s loose fields move into named sub-structs. Six state types already
have module homes (`find`, `home`, `navigation`, `rail_state`,
`quick_actions_state`, `side_panel_state`); the loose ones do not.

**Files.** `crates/app/src/shell/chrome/tabs/mod.rs` and
`crates/app/src/shell/chrome/tabs/frame_state.rs`. Neither is new: P0a already
shipped `tabs/frame_state.rs` at 317 lines holding the state types that were
already grouped. P0b moves the **loose** fields into it, which is why its
acceptance test is the compiler and a normalization rule rather than the
inventory.

**How it is verified instead**, since the inventory cannot be:

- **The compiler carries the weight, and it genuinely can here.** A field that
  moves is a type error at every use site, so a field dropped in the move does
  not compile. That is a stronger guarantee than P0a has for its bodies, which
  is why the two are separable at all rather than one being weaker.
- **The inventory still runs, under a normalization rule**: `self.<field>` and
  `self.<sub>.<field>` canonicalize to the same field-path-insensitive token
  before hashing. So a body that changed only because a field moved hashes the
  same, and a body that changed for **any other reason** still shows as a set
  difference. The normalization is the deliverable to review, not the count.
- **The mutation that must break it**: change one statement in one relocated
  method in a way that has nothing to do with a field path, and confirm the
  normalized inventory still reports it. If the normalization swallows that, it
  is normalizing too much and P0b has no acceptance test at all.

**Verification.** P0a's full CI configuration set, at the same test count, with
the same rule that green means no new diagnostics rather than exit code zero.

#### P0c. Move the tests out of `tabs/mod.rs`

**Local source checkpoint, 2026-09-10: `fdf875c`, based on `65d29c6`.**
All 118 inline test functions are accounted for: 24 remain in `mod.rs`, with
38 in `export.rs`, 28 in `accessible.rs`, 11 in `menu.rs`, 7 in `context.rs`,
5 in `frame_state.rs` and 5 in `dialogs.rs`. Shared helpers remain in the root
test module; the three external C1.2 test files are byte-identical. Production
prefixes are unchanged. The existing inventory compares 516 items with zero
non-visibility differences; 12 test-only visibility changes have sibling callers.
The independent raw-body check covers 357 functions, and all 741 comment lines
are preserved as a multiset.

Eleven before/after Cargo configurations pass with unchanged diagnostics and
3,060 test-result entries, including ignored cases, preserved by configuration,
target, leaf name and status. Scratch mutations detect a missing test in three
feature configurations and a missing import only when shell test support is
enabled. A fresh unmodified control passes 624 tests with 7 ignored. Records:
`crates/app/tools/item-inventory/p0c-test-relocation-*.txt`. Earlier P0a/P0b
records are unchanged. Native verification at `d34b29f` on 2026-09-11 now covers
opening PDFs, Cmd+F, tab switching and C1.2 export input/validation, including
page-2 PNG output at 144 DPI. Task A/B window-only captures and exact binary/output
hashes are in `docs/evidence/milestone-screenshots.md`. Final integration remains
pending. No Acrobat feature row closes in P0c.

The exact committed shell build and headless boot also pass, reporting
12 plugins, 6 tools, 2 commands and 3 codecs. This verifies startup, not native
window interaction.

The configuration census does not certify external-corpus coverage. That
coverage was not measured or equalized between worktrees; successful test-result
entries can conceal internal fixture skips. P1c owns the explicit corpus run.

The description below retains its original planning baseline.

`tabs/mod.rs` carries **118 test functions in one 4669-line `#[cfg(test)]`
module**, which is more than three times its production half and is what every
package adding a shell test has to edit. P0a relocated the production code and
left this untouched, deliberately, so that its inventory had one thing to prove.

P0c moves those tests into per-file `#[cfg(test)] mod tests` blocks beside the
code they exercise: `export.rs`, `accessible.rs`, `menu.rs`, `context.rs`,
`frame_state.rs`, `dialogs.rs` and what remains in `mod.rs`.

**P0a's inventory certifies this for free**, which is why it is a separate
package rather than a risk. The inventory's container key excludes the file, so a
pure test relocation compares as **identical sets with zero differences**. No new
acceptance machinery is needed; the existing procedure is the acceptance.

**The real cost is imports, not tests.** Seven test modules each need their own
`use` list, each with its own `cfg` pass, and that is where a relocation of this
shape actually goes wrong: an import whose only user is feature-gated compiles in
one configuration and not another, which is precisely the defect P0a shipped.
So P0c runs the same full CI configuration set, with the same
new-diagnostics-not-exit-code rule.

**Files.** `crates/app/src/shell/chrome/tabs/*.rs`.

**Verification.** The inventory reports zero differences. The test count is
identical before and after, per configuration and not only in total, since a test
that stopped compiling under one feature set is exactly what this risks. The full
CI configuration set from P0a, with no new diagnostics.

**Review risk.** Whether a test moved to a file whose code it does not exercise,
which the inventory cannot see because it excludes the container. Whether any
test was quietly deleted rather than moved, which the identical-count check
catches only if it is per configuration. Whether a shared test helper was
duplicated into seven modules instead of living in one.
**Mutation that must break its tests:** deleting one test function must make the
per-configuration count differ; moving one test without its `use` must fail one
configuration and not the others, which is the whole point of running all of them.

### P1. cos: the edit surface M3 needs

**Goal.** Five cos additions, landed together and reviewed once, that every
kernel package above depends on.

1. `Document::next_object_number(&self) -> u32`, so `core` can reserve numbers
   without calling `add_object` speculatively (T2). Trivial accessor, no new
   state.
2. `Document::sections(&self) -> Result<Vec<Section>>` where
   `Section { start: u64, end: u64 }`,
   walking the `/Prev` chain from `startxref` back to the first table. This is
   the skins panel's data source and there is nothing today: `original_len()`
   gives one boundary, not a chain. Must terminate on a cyclic `/Prev` (hostile
   documents do this) and must report a chain it cannot follow as a repaired
   document rather than silently truncating the list. There is deliberately **no
   `prev` field**: the returned `Vec` is the chain, in order, so a section's
   predecessor is the element before it and a stored `prev` is a second copy of
   that fact that can disagree with the first. **And no `startxref` field**, for
   the same reason and with the same test: no M3 consumer names it. P19 shows
   byte ranges and sizes and P3's `revert_to` truncates at a `start`; neither
   needs the offset of a table inside the section. The walk still reads
   `startxref` to follow the chain; it just does not hand the result out.
3. `Document::write_new(objects, trailer) -> Result<Vec<u8>>`: a complete
   document serialization, for the documents Onionskin authors (T8). Named
   consumers in M3, five of them: P12 (combine and split), P11 (extract), P14a
   (create from image), P14b (compress) and P10 (the comment summary), plus
   P15's `FileBackend`, which composes sheets rather than copying a graph and is
   the sixth. This is the `flatten` primitive the crate's charter names; it lands
   now because it has six real callers, and M5's `redact` inherits it. Its serializer is `writer.rs`'s
   classic-table path; **it does not gain object streams or a cross-reference
   stream**, which are scoped out in P14b's entry and are not an implicit clause of
   any word in this plan.
4. **The overlay-taking section builder and save**, per T2: `pub enum
   PendingEdit`, `Document::section_for(&self, overlay, trailer_edits)` and
   `Document::save_overlay_to_path(&self, overlay, trailer_edits, path)`.
   `incremental_section()` and `save_to_path()` are re-expressed as calls to
   them with `self.edits`, so their behaviour and their tests are unchanged.

   `trailer_edits` is `&BTreeMap<Name, Option<Object>>` rather than `&Dict`, so
   the argument can say **clear this key** as well as set it. A cleared key is
   emitted as `Object::Null`, which ISO 32000-1 section 7.3.7 makes equivalent to
   absent. That is the whole of the new surface: `incremental_section`'s existing
   loop is set-only (`document.rs:1097-1099`) and cannot remove a key, which is
   what made undoing a trailer-key creation across a save impossible (T1).
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
   gate, and it is what the verification of P3, P5, P7, P11, P12, P13b, P14a and
   P14b runs on every fixture output. Complete checking belongs in the test suite,
   where paying O(file) once per fixture is exactly right.

   **The gate inside `section_for`** is the cheap one, and its scope is stated as
   a contract: *every reference in **the trailer this section emits** and in
   every object this section writes must resolve, after this section, to an
   object that exists; and neither the trailer nor any written object may
   reference a number this section frees.* All of it is proportional to the edit.
   The gate refuses with a typed error naming the holder and the target.

   **The trailer is named explicitly because it is the one thing the section
   emits that is not one of its objects**, and it can dangle on its own. A
   trailer whose `/Info` names an object the section does not write and the base
   does not have is exactly what an undone "set a Description on a file with no
   `/Info`" produces (T2), and a gate scoped to "objects this section writes"
   passes it.

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

**Rows closed.** None. Backs every row in P11, P12, P14a and P14b, and the
kernel work of P3, P5 and P19, which close no rows themselves.

**Files.** `crates/cos/src/document.rs`, `crates/cos/src/writer.rs`,
`crates/cos/src/lib.rs`, `crates/cos/src/error.rs`; new
`crates/cos/tests/sections.rs`, `crates/cos/tests/write_new.rs`; extend
`crates/cos/tests/delete.rs`.

**Depends on.** P1c. Nothing in code: P1 compiles and runs against an empty
`corpus/external`. But its `sections()` sweep and its `audit_references` sweep
are both stated over `external/` fixtures, and until P1c's fetch step exists
those sweeps report a pass over an empty file list, so this package cannot be
verified before P1c lands.

**What exists to build on.** `prev_startxref` is already a field
(`document.rs:106`). `writer::incremental_section` (`writer.rs:168`) already
builds classic xref tables and `trailer_for_new_section` (`writer.rs:157`)
already strips xref-stream-only trailer keys. The full-table path in
`incremental_section` (`document.rs:1053-1085`) already enumerates every live
object, which is most of `write_new`. `free_list_rows` (`document.rs:995`)
already knows which numbers a section frees, which is the gate's cheap input.
`incremental_section` is already `&self`, so item 4 is a parameter change
rather than a rewrite - **but it reads `self.edits` in FOUR places, not one, and
missing the fourth silently drops an edit**:

| Site | What it does |
|---|---|
| `document.rs:962` | `has_pending_changes`, which gates the early-out |
| `document.rs:995-1002` | `free_list_rows`, which collects the deletions |
| `document.rs:1042-1044` | the objects collection |
| **`document.rs:1057`** | the full-table skip, `if number == 0 \|\| self.edits.contains_key(&number) { continue; }` |

**All four take the argument.** Leave the fourth reading `self.edits` - which is
permanently empty for `core` - and on a **repaired** document an overlaid object
whose base entry is `InObjectStream` is no longer skipped, so the compressed
branch calls `self.get(number)` and pushes the **base** copy into `objects`
alongside the overlay's. `sort_by_key` is stable so the overlay copy sorts first,
`writer`'s `rows.insert` is last-write-wins, and **the xref row points at the base
copy**. The edit is written into the file and then indexed away. Nothing catches
it: `audit_references` resolves, the gate resolves, and preview and save agree
because they are one call. The user sees the annotation not appear, on repaired
documents only, with a fully green suite.

**Verification.**
- `sections()` over every `external/` corpus file that has more than one `%%EOF`: the reported chain's byte ranges partition the file with no gap and no overlap, and the last section's `end` equals the file length. Name the fixtures; a file set chosen by "some corpus file" is not a proof. This walk is one of the suites P1c's CI step makes mandatory, or it reports a pass over an empty file list.
- A hand-built fixture with a cyclic `/Prev` terminates and reports the cycle rather than looping.
- `write_new` output reopens through `Document::open` (not `open_repairing`), has the stated page count, and round-trips: `write_new` then `open` then `save_to_vec` with no edit is byte-identical.
- **A cleared trailer key round-trips, in cos's own suite.** `section_for` with a trailer argument of `{Info: None}` emits `/Info null` into the section, and the reopened document's trailer reports `/Info` **absent**, per ISO 32000-1 section 7.3.7. This bullet exists because the mutation that covers it otherwise ("emitting nothing instead of `Object::Null`") is only caught by P2's test, in a crate this package's own `Runs` line never compiles and in a package that lands **after** this one. Without it the round's whole new cos surface ships untested at its own merge.
- **`write_new`'s cross-reference table is well formed by ISO 32000-1 section 7.5.4**, asserted by **parsing the output's first subsection header** rather than by reopening it through our own `Document::open`: it emits the object-0 free-list head at generation 65535, and its subsections cover every object number it wrote with no gap. Reopening through our own parser proves our parser accepts what our writer emits, which is the one thing it will always do; five M3 commands ship documents to other readers this way.
- **`section_for` is a refactor, not a second serializer.** Two halves, because the obvious test is true by construction and proves nothing: `incremental_section()` delegates to `section_for`, so comparing the two always agrees. The half that bites is that **every existing test in `crates/cos/tests/{incremental,delete,roundtrip,repair}.rs` passes byte-for-byte unchanged**, which pins the delegation. The half that exercises the new path is a section built from an overlay the document's own edit map does **not** contain: it reopens through `Document::open` and resolves every written number to the overlay's object, proving the argument is read rather than ignored.
- **The gate, three cases.** A section whose written object references a number that section frees is refused with the typed error naming both. A section whose written object references a number the *base file* marks free is refused the same way. The legal case, a section that frees an object nothing references, still succeeds, which is what `crates/cos/tests/delete.rs` already exercises and what stops the gate being a constant `Err`.
- **`audit_references`, three cases.** It is empty on a **named sample** of `external/` fixtures with a stated pass floor, in the shape `roundtrip.rs` already uses for its corpus walks, or it is reporting noise and no package can assert on it. It is non-empty on a hand-built file with one deleted target, naming that exact pair and no other. It finds a reference buried in a nested array inside a stream dictionary, which is the shallow-walk failure mode.
- **The sweep is bounded on purpose.** `audit_references` is O(file), P1c makes `external/` mandatory, and the set is 3300-plus PDFs, so "empty on every unmodified fixture" would put a new unbounded walk over the whole corpus on every CI run. Every other use in this plan is already bounded at three fixtures per package; only this one was not. The sample is named in the test and its size is chosen against the wall-time P1c records, not guessed. A floor rather than an exact count, so a corpus refresh that adds files does not fail the build for adding them.
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
cross-reference-stream output, which P14b scopes out and which is not this
one's. Whether `section_for` changed any byte `incremental_section` used to emit.
**Mutation that must break its tests:** **making `TrailerKey`'s cleared form emit
nothing instead of `Object::Null` must fail P2's set-save-undo-save test**, and
nothing else, since it is the only path that removes a trailer key; making the
gate always return `Ok` must fail the dangling-reference test; making `audit_references` return an empty
vector must fail its hand-built fixture; making `sections()` return only the last
section must fail the partition test; making `section_for` ignore its `overlay`
argument and read `self.edits` must fail the test that hands it an overlay the
edit map does not contain, which is the half of the refactor check that is not
true by construction.

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

**The rule for authoring a new document from an encrypted source, stated here
because this package owns the encryption posture and every other package
references it.** Disabling *editing* does not cover this. Compress, Reduce File
Size, extract-to-file, split, combine and the comment summary take **no pending
edit**; they read an encrypted document and produce a new one through
`cos::write_new(objects, trailer)`, and ruling A puts the permissions-only class
in reach of exactly that by making it exportable. Nothing said what happens to
`/Encrypt`, and **both available outcomes are wrong**:

- **Pass the source trailer through** and the output is plaintext objects under
  an `/Encrypt` trailer. No reader can open it, from a command whose whole
  promise is a smaller working file.
- **Strip `/Encrypt`** and the output is a silently decrypted copy with the `/P`
  bits gone, produced by a milestone that has just told the user it will not let
  them edit that document because its permissions matter.

> **No M3 operation may read an encrypted source's object graph into any
> document other than that source.** Refused with the same typed reason and the
> same M6 milestone the edit gate uses.

**And no section may be written to an encrypted document at all**, which is a
second rule and not a restatement, because it is reachable with an **empty
overlay**. `section_for` early-outs on an empty overlay only when provenance is
`Clean`. On a document that is both **encrypted and repaired**, provenance is
`Repaired`, `needs_full_table()` is true, and the full-table branch re-serializes
every compressed object by calling `self.get`, which after this package returns
the **decrypted** in-memory `Object` with nothing re-encrypting it. The result of
a **no-op save** is plaintext objects under a trailer that still names
`/Encrypt`: unopenable by any conforming reader, **and** a plaintext disclosure
of a protected document, from a save the user believes changed nothing.

So: **a document whose trailer carries `/Encrypt` gets a typed refusal from
`section_for` rather than a section**, naming M6. Writing to an encrypted
document is M6's whole job and M3 has no business emitting one byte into one.

**The predicates are ordered, and the order is the whole of it.** "Refuses on an
empty overlay as much as a full one", which is how this was first written, makes
**every encrypted document unrenderable**: the canvas draws from `preview_bytes`,
which is `original ++ section_for(...)`, so an unconditional refusal means
`preview_bytes(Unfiltered)` returns `Err` on any encrypted document and there is
nothing to draw. Ruling A's entire deliverable - open it, render it, print it,
export it - would have been unreachable, including this package's own
"print-to-file with Print as Image on succeeds" bullet, since the sheet renderer
asks `core` for preview bytes. It would have been found at P15, four packages
later.

> `section_for` returns `Ok(None)` when `states` and `trailer` are both empty
> **and** provenance is `Clean`. It returns `Err(EncryptedWrite)` **only when it
> would otherwise emit bytes.**

**The residual that ordering leaves, named here rather than found at P15.** A
document that is **both encrypted and repaired** has a non-empty section on an
empty overlay, because the repair is the section. So it can be **neither
previewed nor rendered nor printed**: it opens, and the canvas is empty. That
class needs a **count in this package's measurement table**, alongside the
permissions-only and `/P` bit-4 tallies, and a sentence in ruling A, because it
is a hole in "renders it" that the ruling currently claims without qualification.

Second half of the same composition: the annotation filter (T7) works by writing
`/F` bit 2 into a section, so **on an encrypted document row 94's four
Comments-and-Forms modes cannot be applied at all**, not even inside the
Print-as-Image path the ruling blesses. P15 states that limitation on row 94
rather than discovering it.

**The encrypted-source rule**, which every other package cites by that name.
**Its scope is "every operation reachable in an M3 build", and the sweep that
enforces it walks the registry rather than a list somebody maintains.** That
is the third attempt at this rule and the first structural one. A hand-kept list
missed **print-to-file** two passes ago, on the argument that it authors its
trailer fresh, and the same list misses **SVG export** now: SVG is a vector
transcription of the source's content streams, paths, text and images into
another document, and this plan's own argument for closing print-to-file applies
to it verbatim. **Attachment extraction** is a twelfth, writing an embedded file
byte-for-byte decrypted to disk. Both are newly reachable *because* of ruling A,
and neither was on any list. There is no reason to believe eleven is closed
either.

So the assertion is: **walk every registered command and every registered codec,
and require each to be either refused on an encrypted document or provably
raster-only.** A new command or codec that is neither fails the test by existing,
which is the only form of this check that survives the next feature.

**And the rule is over inputs, not over authorship**, which was the second
attempt at it. Stated as "would author a new document", it missed **four** commands whose
input is a file rather than the session:

- **Combine's file list is chosen after the command is invoked**, so a
  session-scoped `Requirement` query cannot see it.
- **Insert-pages-from-file** authors no new document at all. It reads an
  encrypted source into a **different, unencrypted, editable destination**, whose
  own query correctly reports available. The result is a permanent editable
  unprotected copy of the protected pages inside another file, produced by the
  milestone that refuses Compress on the same source *because* a silently
  decrypted copy is wrong.
- **Replace-pages-from-file** is the same shape and needs naming separately,
  because Acrobat's Replace Pages takes its replacements from another file and
  P11's own review risk warns it may be built as delete-then-insert: a rule that
  covered insert and missed replace would be bypassed by the obvious
  implementation rather than by a clever one.
- **Copy-or-move-pages-between-open-documents** is the same shape again, with the
  source already open rather than picked from disk.

So there are two check points and **one predicate**: a single function in `core`
taking a `&cos::Document` and answering whether its graph may be read out.
`Requirement::Command` calls it for commands whose input is the active session;
an explicit **per-input check at execution** calls it for combine's file list,
insert-from-file, replace-pages-from-file and copy-between-open-documents,
reporting the same typed reason naming M6. The command list is **ten**, not six.

**Print-to-file is in the class too, and pass 2's reason for exempting it was
false.** It said `FileBackend` "authors its trailer fresh: not a copy of the
source's graph". P15 places each `Placement` as a **Form XObject reference at its
transform**, which copies the source's content streams, resources, fonts and
images into the output. The fresh trailer answers the `/Encrypt` question and
nothing else; it says nothing about the graph. Combined with the `/P` asymmetry
below, the reachable composition was: take a document whose `/P` forbids
printing, extraction and modification; Print to File; range All; Actual size; and
receive a fully decrypted, unrestricted, editable PDF of every page. This
package's own verification blessed it, by asserting only that no `write_new`
output carries an `/Encrypt` key.

**Ruled: on an encrypted source, print-to-file is available only with Print as
Image on.** A raster genuinely is not a copy of the object graph, and it is what
"print" means, so this keeps ruling A's "printable" and closes the bypass. P15
asserts the condition.

**The asymmetry that remains, named rather than discovered.** M3 enforces **no**
`/P` bit: it disables editing on every encrypted document, which is stricter than
any permission bit, and it gates nothing else. So a document whose `/P` forbids
printing can still be printed, as a raster. That is a real gap against Acrobat in
the opposite direction from the residual above, it is out of scope for M3 because
permission enforcement belongs with the write path at M6, and it goes in the
`known-issues.md` entry the orchestrator lands from this package's measurement.

**Files.** `crates/crypto/src/{lib,standard,algorithms,filters}.rs`,
`crates/crypto/Cargo.toml`, `crates/cos/src/{document,object,stream}.rs`,
`crates/cos/Cargo.toml`, new `crates/cos/tests/encrypted.rs`, new
`crates/cos/tests/encryption_classes.rs` (the measurement), new
`docs/evidence/encryption-classes.md` (its output),
`crates/core/src/session.rs` and `crates/app/src/shell/chrome/tabs/mod.rs` (the
open-time notice and the editing gate).

**Depends on.** P0b and P7.

Its `crates/crypto` and `crates/cos` work depends on nothing and runs the whole
milestone alongside everything else, off the critical path. **Its app seam
depends on P0b**, because the open-time notice and the editing gate land in the
file the split rewrites, so that seam is a separate commit after the split rather
than a concurrent edit to it. **And on P7**, because both gates report through
the `Requirement` query P7 moves into `plugin-api` and extends with
`Requirement::Command`; claiming that query while not depending on the package
that builds it was the alternative, and it would have made "one predicate, not a
second flag" unbuildable at this package's own landing. The cost is depth five
instead of three, which is still off the critical path.

**What exists to build on.** Nothing in-repo: `crates/crypto` is a four-line
doc comment. The spike's corpus tally gives the file set, and the refusal is a
single guarded predicate, so the wiring surface in cos is small.

**Verification.**
- Every corpus file with an empty user password opens, reports its known page count, and extracts text that matches `pdftotext` through the existing content oracle. Files with a non-empty user password still fail with the typed error.
- A password-protected fixture is **not** opened by an empty password, asserted, so the handler is not accidentally permissive.
- Round-trip: opening an encrypted file and saving with no edit is byte-identical (guarantee 1 must hold for this class too, and it is free, because a no-op save writes nothing).
- A save with a pending edit on an encrypted document is refused with a typed error naming M6, and the edit tools were already disabled at open, asserted in the app.
- **The `core` predicate itself**, over a `&cos::Document`: it answers "may this graph be read out" for an encrypted document and for a plain one, and the two check points call it rather than duplicating it. That is what this package can prove at its own depth.
- **The two check points**, asserted by driving both: a session-scoped refusal through `Requirement::Command`, and an execution-time per-input refusal, both returning the same typed reason naming M6 from that one predicate.
- **The sweep is not a list and is not asserted here.** It is **exhaustive over
  the registry**, and this package's only app invocation
  (`--no-default-features --features shell,shell-test-support`) registers **no
  plugin**, so nothing is in the registry to sweep. P20 owns it; each package
  also asserts its own refusal, which is where the per-command coverage lives.
- **Print-to-file with Print as Image on succeeds** on the same document and its output carries no `/Encrypt`; **with Print as Image off it is refused**. That pair is what makes the ruling a decision with a test rather than a carve-out with a story, and neither half alone proves it.
- **No output of any of those operations ever contains an object imported from the encrypted source**, asserted on the produced file rather than on the absence of an `/Encrypt` key. The `/Encrypt`-key assertion was the one pass 2 had, and it is exactly the assertion that blessed the print bypass.
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
forget to set. Whether the authoring refusal is derived from the same predicate
as the edit gate or from a second one that can drift, which is the whole point of
reusing it. Whether anything reached `write_new` from an encrypted source by a
route the predicate does not cover. **Mutation that must break its tests:**
returning the file key unmodified as every object key must fail the RC4 and
AES-128 fixtures while still passing AES-256, which is exactly why the fixtures
must cover all four revisions; making the encrypted-source predicate always
return available must fail both check-point tests here and, in the late package
that owns it, all nine non-print command tests while leaving the
print-with-Print-as-Image one green.

### P1c. CI: make the corpus this plan's fixtures come from reach CI

**2026-09-10 implementation checkpoint:** the default-only corpus reproduced
the documented lazy I/O failure at `65d29c6`. The parser-window fix reduces
returned bytes from 1,857,768 to 1,335,528 on that fixture, with six red/green
regressions and 16 of 17 broader checks passing. The remaining lazy-budget
failure is preserved, tracked in [read-window evidence](../evidence/m3-p1c-read-windows.md).
The existing 25% total-byte budget remains unchanged. Corpus fetch/rerun CI gates,
the shared helper and P1c acceptance remain outstanding; this is not a feature
row promotion.

**2026-09-11 validation-reuse checkpoint:** committed COS revision
`e7778898724fe732b6fb79cff63d4ccbf59c21a7` was verified against the same Isartor
input with clean provenance and successful first-page access. See [read-window
evidence](../evidence/m3-p1c-read-windows.md). Through-first-page reads fell
from 1,335,528 to 1,072,232 bytes, a further 19.7% reduction and 42.3% below
the original 1,857,768-byte baseline. The unchanged budget assertion therefore
still fails at 26.73% of the file; opening alone remained 1,070,056 bytes
(26.68%), also over budget. The fresh 17-command runtime retained the
lazy-budget failure (16 other commands exited 0). Corpus CI, required-input
enforcement, the shared helper, hosted timing/mutation evidence, P1c
integration and the remaining 25% performance work remain open. The named M2
carry-forwards remain open as well; no feature row, headline count or milestone
state changes here.

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
- **P11's importer render comparison, P12's combine fixtures and P14b's compress
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
2. **A shared corpus helper the app and plugin test targets can reach**, because
   for eight of this plan's packages the mandatory half is currently
   *unimplementable*, not merely unlisted. `ONIONSKIN_CORPUS_REQUIRED` is
   honoured by `crates/cos/tests/common`, `crates/content/tests/common`,
   `crates/core/tests` and `plugins/codecs-common/tests`. **`crates/app/tests/`
   has no corpus helper at all**: `guarantees.rs`'s only mention of the variable
   is inside a string it asserts against `ci.yml`. The `external/` assertions of
   P8, P11, P12, P13b, P14a, P14b, P19 and P20 live in `crates/app/tests/` and
   plugin test targets and have nothing to skip-or-fail through.

   So: **one test-support crate**, depended on by `dev-dependencies` wherever it
   is needed, rather than a fifth near-duplicate of the same file. The four
   existing copies are not this package's to consolidate, but nothing new copies
   them.
3. A third step re-running under `ONIONSKIN_CORPUS_REQUIRED: 1`, **enumerated
   per SUITE with `--test`, never per crate**. Per-crate is not a shortcut, it is
   three separate red builds:

   - `cargo test -p onionskin-core` **panics**: `crates/core/tests/search.rs`
     requires `external/hayro-corpus`, which this package deliberately does not
     fetch (159 MB for 41 files, and it stays with the bench job).
   - `cargo test -p onionskin-cos` **panics**: `crates/cos/tests/pages.rs`
     requires `corpus/bench/pages-1000.pdf`, generated only by
     `corpus/make-bench.py`, which the `test` job never runs. Today this is
     invisible because CI's re-run is scoped to `--test repair`; widening it to
     the crate is what exposes it.

   So: `cargo test -p onionskin-cos --test roundtrip --test incremental --test
   delete`, `cargo test -p onionskin-core --test <the ones that do not need the
   bench file>`, and so on, each suite named because each suite's corpus needs
   are different.

4. **A deliverable for the suites the fetch newly switches on**, which is a
   separate hazard from the re-run and does not involve the env var at all.
   `crates/cos/tests/lazy.rs` returns early today because `external/` is absent;
   with the `verapdf` set present it **runs and fails on real data** -
   `isartor-6-1-12-t01-fail-a.pdf` reads 46% of the file against a 25% budget -
   so **plain `cargo test --workspace` goes red the moment step 1 lands**, with
   no env var in the picture. Fetching a corpus turns on every suite that was
   silently skipping, and this package owns triaging that set before it lands:
   `lazy.rs`'s budget in particular is either widened with a stated reason or
   the file is excluded with one. A reproduction corpus root is at
   `/tmp/claude/ci_corpus`.

   This is the same class as guarantee 6 pointing the other way: a skip that
   flipped to a pass hid a gap, and a skip that flips to a failure hides a real
   defect nobody has looked at. Generating or fetching a set is not proof it was measured; only this
   step turns a silent skip into a failure, which is what makes disabling the
   fetch visible.

   **Every package that names an `external/` fixture adds its own command to that
   list as part of its definition of done**, the way P4 already does. Twelve carry
   an explicit `Corpus.` bullet: **P1b, P3, P4 and P6** reach `external/` from
   `crates/cos` and `crates/core`, where helpers already exist, and **P8, P11,
   P12, P13b, P14a, P14b, P15, P19 and P20 are the nine that need the new one**,
   because their assertions live in `crates/app/tests/`, `crates/print/tests/` or
   plugin targets. P15 joined that list when its encrypted-fixture Corpus bullet
   landed. **P1
   and P5 also sweep `external/`**, inside other verification bullets rather than
   a `Corpus.` one, and both are in crates that already have a helper. P4 is
   currently the only package naming its command at all, and it should be the
   pattern rather than the exception.

The corpus steps are pinned to `runner.os == 'Linux'`, like the two that
exist, and for the reason those give: corpus assertions are byte and structure
work with no platform dimension, and running the fetch on the three-way matrix is
three caches and three chances to flake for one claim.

**The app suite is the exception and goes in the `shell` job on macOS**, not in
the `test` job. The `test` job installs none of GPUI's Linux dependencies and has
never built the shell on Linux at all; putting `cargo test -p onionskin-app
--features shell,shell-test-support` there fails at link time before it reaches a
fixture. The `shell` job already runs the windowed tests on macOS, so the app
half of this package's re-run lands beside them, with its own fetch step, and the
cache key is shared.

**The tripwire, or this package is itself deletable.** It asserts the re-run
step's **command set matches the enumerated list**, so a suite added to the plan
and not to CI is a failing build rather than a silent gap; that is the assertion
that stops the list drifting as those packages land.
`crates/app/tests/guarantees.rs` asserts the steps the same way
`every_malformed_file_opens_and_repairs_into_a_new_section` asserts its own:
exactly one step per command, the allowed key set, `if` pinned to the value
rather than merely present so `if: false` is not an off switch, and
`ONIONSKIN_CORPUS_REQUIRED` asserted as `1` on the re-run. Reusing that test's
existing helper rather than writing a second one, since it is already the shape.

**Rows closed.** None. It is the reason the rest of the ledger's verification
means anything.

**Files.** `.github/workflows/ci.yml`, `crates/app/tests/guarantees.rs`,
new `crates/corpus-testing/` (the shared helper crate), the workspace
`Cargo.toml` (which must list the new member and carry it in
`[workspace.dependencies]`), the `dev-dependencies` entries that reach it, and
`corpus/README.md` (which sets CI fetches and why).

**Depends on.** Nothing.

It **lands before P1**, because P1's own verification sweeps `external/`, and it
is the second of section 5's three day-one roots. It is also the one package with
a `crates/app` file that depends on neither P0a nor P0b:
`crates/app/tests/guarantees.rs` is a test target, not shell code, and neither
sub-package touches it. All of that is ordering prose and sits below the blank
line on purpose, because section 5 derives edges from the paragraph above it.

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
in a narrower place, and whether the tripwire actually compares the set rather
than checking the step exists. Whether the shared helper is one crate or a fifth
copy of `common/mod.rs`. Whether the re-run is a second full `cargo test --workspace`
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
/// One overlaid trailer key. `Cleared` exists because the trailer is the root
/// and has no referrer that can stop naming a key, so undoing a key's creation
/// has to say "clear it" rather than forget it (T1). There is deliberately no
/// `NotInOverlay`: nothing produces it, and its only reachable use was the
/// wrong capture the rule below forbids, so cutting it makes that unexpressible.
pub enum TrailerState { Cleared, Set(cos::Object) }
/// For an object, `before: None` means, and only ever means, "this number was
/// not in the overlay immediately before this change". Trailer keys do not use
/// `Option`: a key's absence and a key's clearing are the same instruction to
/// the section writer, so `Cleared` covers both. See the capture rule.
pub enum Change {
    Object     { number: u32, before: Option<ObjectState>, after: Option<ObjectState> },
    TrailerKey { key: Name,   before: TrailerState,        after: TrailerState },
}
/// The trailer lives here, not beside here: `section_for`'s two arguments are
/// `states` and `trailer`, and both have to be undoable (T2). `None` in the
/// trailer map means "clear this key", which is what undoing the creation of a
/// key across a save needs and what a `Dict` of edits cannot say.
pub struct Overlay {
    states: BTreeMap<u32, ObjectState>,
    trailer: BTreeMap<Name, Option<cos::Object>>,
    next_number: u32,
}
pub struct Entry { label: &'static str, changes: Vec<Change> }
pub struct History { entries: Vec<Entry>, cursor: usize, saved_mark: Option<usize> }
pub enum DocumentEdit { /* the typed vocabulary, grown by P5 and P6 */ }
```

**`Change::TrailerKey` is not speculative.** `SetInfoField` on a document with no
`/Info` is one object write **and** one trailer write, and it is the first thing
`File > Properties` does on `corpus/seeds/minimal.pdf`. Without the variant, undo
drops the object and leaves the trailer naming it. T2 has the full reasoning.

**The base-capture rule, which is this package's sharpest correctness
obligation** (T2). Leaving `before: None` to mean "the overlay had no node for
this number" produces a silent no-op on the far side of a save: edit an object
that exists in the file, save, undo, and nothing is restored, because the save
cleared the overlay and made the base the edited value. That loses data with a
green suite, and P2's verification as first drafted covered every case except it.

> **`Change::Object`'s `before` is captured at edit time, by this precedence:**
> the **overlay's current `ObjectState`** for that number if the overlay has one;
> **else** the base's value through `cos::Document::get` if the base has the
> number; **else `None`**, which the reservation counter alone produces.

**The overlay clause first, and it is not a refinement.** Written as "read
through `get`" alone - which is how this rule and T2's both first stated it - the
**second edit of an object is un-undoable**, because `core` never writes into
cos's edit map and `get` therefore cannot see the overlay. Base object 7 is A;
edit it to B, capturing `before = A`; edit it to C, capturing `before` = the base
again = **A, not B**. One `Ctrl+Z` and the overlay holds A: two edits gone, and B
unreachable in either direction. This package's own review-risk line names that
hazard, so the plan was flagging as a risk the behaviour its normative rule
required.
>
> **`Change::TrailerKey`'s `before` follows the same precedence, one level up**:
> the overlay's current state for that key if the overlay has one; else `Set(v)`
> if the **live base trailer** has the key; else **`Cleared`**. There is no third
> outcome, which is why `TrailerState` has two variants.

That is the whole fix, and getting it wrong moves the failure rather than
closing it. An earlier draft of this rule carried a third `NotInOverlay` state
and captured it for a key the base lacks, which means undo **removes the key from
the overlay map** instead of clearing it - so across a save the base still has
`/Info` and the Description survives. That is the same silent no-op T1 built
`Cleared` to prevent, one level down from where it was fixed. Cutting the variant
is what makes the wrong capture unexpressible rather than merely forbidden.

**And the collapse rule has to cover the trailer, or the pre-save case breaks
instead.** `section_for`'s early-out requires the trailer map empty, so an undo
that correctly leaves `{Info: Cleared}` in a *pre-save* session - where the base
never had `/Info` - would write a section saying `/Info null` on a document that
never had one. T3's collapse gains the matching clause: an overlay trailer entry
equal to the base's state for that key drops out, where **`Cleared` equals
absent** and `Set(v)` equals a base value of `v`.

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

P2 lands the machinery plus two variants to prove the shape end to end, and each
one names the M3 consumer that reaches it: **`SetInfoField`**, which P13a's
Description tab writes, and **`SetCatalogEntry`**, which P13a's Initial View tab
writes (`/OpenAction`, `/PageLayout`, `/PageMode`). `SetObjectDictEntry` and
`DeleteObject` are deliberately not the two: they are shapes rather than verbs.

`SetTrailerEntry` **is not one of them**, which the first draft had it be.
`File > Properties` writes `/Info`, which is `SetInfoField`, and Initial View
writes catalog entries, which the trailer is not; no M3 surface writes a trailer
key. It would have been a verb with no caller in the package whose whole argument
for a closed enum is that every verb has one. P5 and P6 grow the enum.

Transactions: `EditSession::transact(label, |tx| ...)` collects changes into one
`Entry`. An aborted transaction leaves the overlay untouched, including any
object numbers it reserved.

**The stack is bounded, and the bound is measured rather than guessed.** An
`ObjectState` holds a fully materialized `cos::Object`, which for a `Stream` is
its bytes, and a `Change` holds two of them. The worst case in M3 is not an ink
session: it is **one page reorder on the 1000-page bench file**, where T5's flat
rewrite produces about a thousand rewritten page dicts as `after` and a thousand
captured base values as `before`, so roughly **two thousand objects in a single
`Entry`**. Page dicts are small, but nothing in the design says they have to be,
and a reorder of a document whose pages carry large inline resources is the same
shape with a different constant.

So P2 owns three things the first draft left as a review-risk question:

- A bench in `crates/core/benches/` that reports the resident size of one
  `Entry` for the 1000-page reorder and for a hundred-annotation session, so the
  bound is derived from a number rather than chosen.
- A bound expressed as a named constant over **total resident bytes**, not entry
  count, because one reorder and one thousand highlights are the same entry count
  and three orders of magnitude apart.
- An eviction rule that drops the **oldest** entries past the bound and makes the
  truncation **visible**: the Edit menu's Undo says how far back it can go, and a
  stack that has forgotten something says so rather than silently having a
  shorter history than the user expects.

**Rows closed.** None. Backs rows 2, 17 and every editing row in M3.

**Files.** New `crates/core/src/edit/{mod,overlay,history,verb}.rs`,
`crates/core/src/lib.rs`, `crates/core/src/session.rs` (the `Document` gains an
`edit: EditSession` and the accessors), new `crates/core/tests/edit.rs` (this
package names about ten tests and every sibling `core` package names the file
they live in).

**Depends on.** P1.

**What exists to build on.** `cos::Object` is the object model and needs no
change. `cos::Document::get`/`resolve` read the base state a `Change`'s `before`
is computed against. `core::history::ViewHistory` is the shape to copy for the
cursor semantics and is explicitly not the thing to extend (its own doc comment
says the edit history "will own its own undo stack").

**Verification.**
- Headless property test over a generated sequence of edits: apply N edits then undo N leaves the overlay byte-identical to empty, for N up to a few hundred, including sequences that overwrite the same object repeatedly and sequences that delete an original object. **This is the case PLAN.md's "drop the overlay node" phrasing gets wrong**, so it is the case the test must cover explicitly and by name.
- **The base-capture rule, by name:** editing an object that exists in the base document records `before` as `Some(ObjectState { .. })` holding the base object by value, asserted on the `Change` itself and not inferred from undo working. A test that only checks undo passes against an implementation that reads the base at undo time, which stops working the moment the base is reopened, which is the whole bug.
- **The precedence, asserted on the INTERMEDIATE state**, which is the one thing that distinguishes the right implementation from the wrong one: **edit an object twice, undo once, and assert the overlay holds the FIRST edit's value** - not the base's. Every other test in this package passes against the wrong implementation. The property test asserts the *end* state (undo N leaves the overlay empty), which holds either way because undoing edit 1 writes the base value back and collapse rule 1 then drops it; and the capture test asserts `before` holds "the base object by value", which is exactly what the wrong implementation produces. Without this bullet the suite is green and `Ctrl+Z` loses two edits.
- **The trailer is undone with everything else**, by name: **setting a Description field on a document with no `/Info`, then undoing, leaves the trailer as it was, asserted on the trailer and not on the overlay.** `corpus/seeds/minimal.pdf` is the fixture and it is tracked, so this runs everywhere. Then the consequence: the following save writes nothing at all, which is T3's headline claim on the one path that falsifies it.
- **The same thing across a save**, which is the case `TrailerState::Cleared` exists for: **set a Description on a document with no `/Info`, save, undo, save**, asserted on the **reopened trailer** and on the **reopened `/Info` object**. Both halves are needed: after the first save the base trailer has `/Info`, so undo has to emit a cleared key rather than forget an overlay entry, and an assertion on the overlay alone passes while the Description is still in the file. This is T1's one overlay component with no referrer above it.
- **Two producers, one `Entry`, one `Change` per key**: a transaction whose verb and whose P4 structure hook both rewrite the same page dict produces exactly one `Change` for that number, whose `before` is the state before the transaction and whose `after` is the hook's. Asserted on the entry, and asserted to hold with the two producers run in either order, which is what makes the ordering rule a rule rather than a description of today's code.
- **No `Change` ever carries a `before: None` for a number the base document has**, asserted as a property over the generated edit sequences. That is the invariant stated as a check, and it is what makes `None` safe to drop on either side of a save.
- **The stack's memory bound.** A bench reporting one `Entry`'s resident size for the 1000-page reorder and for a hundred-annotation session, and a test that a session past the bound has dropped its oldest entries, reports a shorter reach in the Edit menu, and has not dropped anything below the saved mark without saying so.
- Redo after undo restores exactly; a new edit after an undo truncates the redo tail, asserted.
- The saved mark: it survives undo and redo, and moves only when P3's save moves it.
- An aborted transaction leaves both the overlay and the reservation counter unchanged, so an abort cannot leak an object number.
- `cargo test -p onionskin-core` with no window and no `shell` feature; `cargo test -p onionskin-app --no-default-features` still green.

**Review risk.** Whether `Change::before` is captured from the overlay or from
the base document, which are different whenever an object has already been
edited, and getting it wrong makes exactly the second edit of an object
un-undoable. Whether `before: None` is ever produced for anything other than a
freshly reserved number or an absent trailer key, which is the invariant the
whole undo-across-a-save story rests on. Whether anything writes the overlay's
trailer without recording a `Change::TrailerKey`, which is the shape of the
original defect and would not show up in any object-level test. Whether the overlay is cleared on the `Save As` path as well as
`Save`, since both reopen. Whether the overlay collapses by value against the original (T3) or
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
the unit test is not carrying its weight; **making `Change::TrailerKey`'s undo a
no-op must fail the no-`/Info` test and nothing else**, which is what proves that
test is the one carrying the trailer.

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

   **The cache holds ONE entry, not one per mode**, and that is a correction to
   the draft rather than a detail. Five live entries means five live `Arc`s, so
   **every buffer is still referenced and T4's buffer-reuse strategy never
   fires** - the two paragraphs contradicted each other. Cycling the print
   dialog's four modes on T4's own 67 MB worst case would cost four more full
   copies. Filtered previews are **transient**, which is what T7 already calls
   them: built for a render, used, dropped.

   **And `AnnotationFilter` is the five-mode enum alone**, with `subtypes()` a
   function over it. The draft gave it "a set of subtypes **and** a rendering
   mode", and no caller can supply a subtype set that disagrees with its mode -
   the mode determines the set, so carrying both invites the two to differ and
   gives no caller anything.

   **One cost this owes and the draft passed over:** building a filtered preview
   has to **synthesize overlay entries for annotations that live in the base**,
   since `/F` bit 2 has to be written onto objects the overlay does not hold.
   That is an O(document) walk to find them plus an O(annotations) section per
   filter change, on top of the section build. It is bounded and it is per filter
   change rather than per frame, but it is real and P3's bench measures it.
3. **Every read path becomes overlay-aware, which is the piece nobody
   specified.** `preview_bytes` was `core`'s only overlay-aware output, and
   everything else reads the document as it was opened:

   - `page_count` is a **field**, set once at open (`session.rs:186`, `221`).
   - `page_geometry`, `page_text`, `outline`, `attachments`, `signatures` and
     `layers` all read `&self.cos` (`session.rs:280`, `330`, `401`, `409`, `440`,
     `452`).
   - `check_page` and `start_search` bound on `self.page_count`
     (`session.rs:389`, `544`), and the search worker spawns on the **original**
     bytes.

   So after deleting page 3 of ten: `page_count` still reports 10,
   `check_page(9)` still succeeds, `page_geometry(3)` returns the **old** page
   3's metrics beside the **new** page 3's raster, `outline()` returns the
   pre-fixup bookmarks, and search still hits the deleted page. **P18's
   definition of done ("the page is back on the canvas, in the file on disk, and
   in the thumbnails pane") and P21's ("the grid and the canvas agree on page
   order without a manual refresh") both fail against the design as specified**,
   and T4 rules out a scratch `cos::Document` to read through, so there was no
   mechanism anywhere. Two packages asserted the outcome; none owned the how.

   **One mechanism, not six, and cache invalidation is not it.** The first
   attempt at this bullet prescribed four changes - three cache keys and one
   page-set special case - and **not one of them changes where the data comes
   from**. `outline::read`, `attachments::read`, `signatures::read`,
   `layers::read`, `content::page` and `content::extract_page` all take
   `&self.cos`, the document **as opened**, so invalidating a cache and re-reading
   returns the identical stale answer. The draft admitted it in its own words
   ("this is invalidation, not new machinery") and did not notice that was the
   defect. P6's annotation reader was not even in the list, because P6 lands
   after this package, so P20's central claim - the Comments pane lists what the
   session just authored - would have failed too.

   > **`fn structure(&mut self) -> &cos::Document`**: a second `cos::Document`,
   > opened with `open_repairing(BytesSource::from_shared(preview_bytes))`, built
   > **lazily, once per preview generation**, cached beside the buffer it was
   > built from. **Every structural read routes through it instead of
   > `&self.cos`.**

   That is one referent instead of six, and it is strictly better than the four
   bullets it replaces:

   - `page_count` falls out of `content::page_count(structure())`. It is still a
     method, but there is no overlay-derived page set to special-case, and the
     special case only ever worked because P5 always writes a flat `/Pages` node -
     an assumption about another package that this one had no way to enforce.
   - Geometry, text, outline, attachments, layers, signatures and P6's
     annotations are all correct **by construction**, because they read a
     document that has the edits in it.
   - The caches still key on `(generation, page)`, but now as an optimisation
     rather than as the mechanism.
   - The search worker still re-seeds, because it holds bytes rather than a
     document.

   **Cost: one lazy xref parse per generation**, paid only when a structural read
   actually happens, and not at all for a generation that is only rendered.

   **T4 does not forbid this.** It rules out a scratch `cos::Document` to *build
   the section through*, because that one would have to be mutated and cos cannot
   withdraw an edit. This one is opened read-only from bytes that already exist
   and is thrown away on the next bump. They are different documents solving
   different problems, and conflating them is what left this hole in the first
   place.

   **The counterpart to T1's rule, so this does not recur: adding a reader to
   `core` means routing it through `structure()`.** T1's table says adding a
   field to `Overlay` means adding a row; this says the same thing on the read
   side. Both exist because the previous version of each was a list that sampled.

4. **Generations.** `Document::generations() -> &[Generation]` over P1's
   `sections()`, plus `revert_to(generation)`, which truncates and reopens.
   Reverting is refused, loudly, if the document has unsaved edits, or if the
   target is not a trailing section.

   **`revert_to` clears `History` in both directions and sets `saved_mark` to
   `Some(0)`.** After it, undo and redo are both unavailable and the tab reports
   clean. Left unstated, the stack and the saved mark go on describing bytes that
   **no longer exist**: every captured `before` was read against a base the
   truncation just removed, so an undo after a revert restores objects into a
   document that never had them. Refusing on unsaved edits does not cover this,
   because the entries below the saved mark are exactly the ones that survive it.

   `revert_to` also truncates a **file**, and `core::Document` has **no `path`
   field**. It takes the path, or the session grows one; either way it is named
   here rather than discovered.

   **`revert_to` owns the render worker's lifetime across the truncation, and
   this package owns that**, which the first draft left as a review-risk question
   in two packages and a deliverable in neither: both P3 and P19 asked "whether
   `revert_to` can be reached with the render worker still holding the truncated
   bytes", and asking twice is not owning once. The worker renders from an
   `Arc<Vec<u8>>` it was handed, so it does not read the file and cannot observe
   a truncation directly; what it can do is finish a render against the **old**
   bytes and deliver tiles for a document that no longer has those pages. The
   rule: `revert_to` **bumps the document's byte generation before it truncates**,
   swaps in the reverted `Arc`, and the worker's existing stale-response
   discipline drops anything in flight against the old one. That is the same
   mechanism M2 already uses for stale thumbnail and snapshot completions, so
   this is reusing a solved problem rather than inventing a second one. P19 calls
   `revert_to` and asserts the user-visible half; it does not re-decide the
   lifetime.
5. **Autosave.** A periodic write of the overlay to a recovery file, not to the
   document. `cos::Document` is `!Send` (m2-viewer's candor item 7, now due), so
   the overlay is what crosses the thread boundary, not the document: autosave
   serializes the overlay's changes and the recovery path replays them into a
   freshly opened document. Named here because the alternative, an `Rc` to `Arc`
   swap in cos, is a bigger change than the feature justifies.

   **The recovery file is the user's document content, and it is treated that
   way.** This is the correction that matters, because the first draft put it
   "beside the config directory" as if it were a preference. The overlay's
   `before` states are objects read out of the user's PDF and its `after` states
   are what they are about to write: a recovery file for a contract under review
   contains that contract. `config.rs` creates new directories `0o700` and files
   `0o600`, but `known-issues.md` records that **a pre-existing config directory
   keeps its mode, and only newly created ones get `0o700`**, so a user whose
   config directory predates that rule at `0755` would have their document
   content written into a world-readable directory. That is a privacy defect, not
   a permissions nit, and it is not acceptable to inherit it.

   So: recovery files live in their **own** directory, not the config one; the
   directory's mode is **verified after creation** and the write refused with a
   visible error if it is not `0o700`, rather than assumed from the create call;
   each file is `0o600`, verified the same way; and a recovery file is **deleted
   as soon as its document is saved or its tab closed cleanly**, so the window in
   which document bytes exist outside the document is as short as the feature
   allows. All four are asserted, on Unix, by reading the mode back.

**Rows closed.** None directly; P18 surfaces them. Backs rows 2, 3, 7, 10, 11,
13, 17 and guarantees 1 and 2.

**Files.** New `crates/core/src/save.rs`, `crates/core/src/preview.rs`,
`crates/core/src/generations.rs`, `crates/core/src/recovery.rs`;
`crates/core/src/session.rs`, `crates/core/src/render.rs` (the worker takes
preview bytes), new `crates/core/tests/save.rs` (about fourteen tests, including
this round's create-save-create-save and the four-shape edit-save-undo-save),
`crates/core/benches/save.rs`.

**Depends on.** P1, P2, **P6**.

P6 is not optional and not merely for the filter's behaviour: `preview_bytes`'s
signature **names `AnnotationFilter`**, which P6 defines, so without it this
package's central API does not compile. The first draft had this package landing
a depth **before** P6, which would have been discovered by `cargo build` in
minute one. The alternative, defining `AnnotationFilter` here and having P6 take
it over, splits one type across two packages for no gain.

**What exists to build on.** P1's `section_for` and `save_overlay_to_path`,
`save_to_path`'s temp-file-and-rename with permission preservation, and
`has_pending_changes` are all real and tested. `ExportSnapshot::open`
(`session.rs:163`) is the reopen pattern.

**But the render worker cannot simply be handed new bytes, and the draft said it
could.** `worker_loop` takes `renderer: &mut RenderSession<'_>` borrowing
`document: &render::Document` (`render.rs:501-503`), so the session cannot
outlive the document it borrows and the loop cannot swap in a new one. Feeding it
preview bytes is a **restructure of `crates/core/src/render.rs`**, not a new
`Arc`. Budget it as real work in this package rather than as a line.

Two consequences for the bench: hayro's `RenderCache` is discarded for **every
page on every generation bump**, not only for edited pages, so the first frame
after each commit re-renders everything visible; and P3's budget is set knowing
that rather than against a steady-state number.

**Verification.**
- **Guarantee 1, at the level the guarantee means it.** Open every corpus seed and every **well-formed** `external/` file through `core::Document`, save with no edit, assert byte-identical output and that `sections()` reports the same count as before. Well-formed is not a hedge: `has_pending_changes` is true on every repaired document (`document.rs:961-963`), so a repaired file's no-op save legitimately appends the repair and this assertion would fail on it. The partition is `Provenance`, read from the session, not a filename list. **The positive case is asserted too**, or the carve-out becomes a place to hide failures: for every repaired `external/` file, the no-op save appends exactly one section, the bytes beneath it are byte-identical to the original, and the result reopens through `Document::open`. PLAN.md's guarantee-1 sentence says "for every well-formed corpus file" and this is what that clause is for.
- **Guarantee 2, driven by a real edit.** Make an edit through `EditSession`, save, assert the output is `original bytes ++ exactly one section`, that truncating at `original_len()` yields the byte-exact original, and that the truncated file reopens with the pre-edit content. Then make ten edits and one save and assert it is still exactly one section, which is the clause PLAN.md leaves ambiguous. Guarantee 2 driven by a **tool** rather than by `EditSession` directly is P7's test, not this one: `crates/core` cannot depend on a plugin, so the DoD's clause cannot be satisfied here and this package does not claim it.
- **Edit, undo, save writes nothing.** Byte-identical output, zero appended sections, on a clean document. This is the test that catches a dirty-flag overlay.
- **Add an annotation, delete it, save writes nothing**, in the same session and with no undo involved. This is T3's second collapse rule and it needs its own bullet, because rule 1 cannot reach it: the annotation dict and its appearance stream have **no base object to compare against**, so value comparison leaves them in the overlay after the page's `/Annots` has already collapsed out. Zero appended sections, byte-identical output.
- **Edit, save, undo, save**, which is the test the first draft had no bullet for and the one that catches the whole save-boundary class. Three assertions on the second output: it reopens through `Document::open`; **object N equals its pre-edit value**, compared as a parsed object; and **the object graph reachable from the catalog equals the pre-edit graph**, walked and compared node by node, which is what catches a reversal that restored the object and forgot the referrer. Run it in all **four** shapes M3 can produce: edit an object that exists in the file, create an object, remove one (which under T5's rule means rewriting its referrer, so the assertion is that the removed object is unreachable rather than absent), and **set a trailer key the base document does not have**. The first is the silent no-op the base-capture rule exists to prevent; the fourth is the one with no referrer above it, where undo has to emit an explicit cleared key and where an assertion on the overlay rather than on the reopened trailer passes while the change is still in the file (T1). `audit_references` on the output must be empty in every shape.
- Two saves produce two sections and the second's `/Prev` points at the first, asserted by parsing the trailers, not by scanning for the string `/Prev`.
- **An edit against a REPAIRED fixture**, which none of this package's other tests does: the `Provenance` partition is applied only to the no-op save. Edit an object the base stores **in an object stream**, on a repaired fixture, and assert it appears **exactly once** in the reopened document, with the overlay's value. That is the assertion that fails if `section_for`'s full-table skip still reads `self.edits` (P1), where the base copy is written alongside the overlay's and the xref indexes the base one.
- **`structure()` answers from the edits, not from the open document**, per package-visible read: delete page 3 of ten and assert `page_count()` is 9, `page_geometry(3)` returns the **new** page 3's metrics, `outline()` reflects P5's fix-ups, and search does not hit the deleted page. Each of those reads a different `core` module, and every one of them returns the stale answer if any is left on `&self.cos`.
- **Async geometry responses carry the generation they were computed for.** `Request::GeometryAsync { page }` and `GeometryResponse = (PageIndex, Result<..>)` (`render.rs:132`, `164`) carry **no** staleness marker, while `RenderRequest` carries a `generation` and `ThumbnailRequest` an `epoch`. So a response computed before a bump is filed under the new generation and the canvas lays the page out at the **old size**. Either the request and response carry the generation and stale ones are dropped, or the channel is drained and discarded on a bump; assert whichever, here rather than in P18, because this package owns the generation.
- **Create an annotation, save, create a second annotation, save**: two distinct objects at two distinct numbers, both reachable from their pages, `audit_references` empty. This is the only bullet that allocates an object number **after** a save, which is where `next_number` is either reseeded or catastrophically reset (T3): `Overlay::default()` puts the second annotation at object 0 or 1 and silently overwrites the catalog, with every reference still resolving and every other test green.
- Preview: after a committed edit, `preview_bytes` parses as a valid PDF through `cos::Document::open` (not `open_repairing`), and its object graph equals what the subsequent save writes, compared object by object. Since both come from one `section_for` call with one argument, this is a regression test on the wiring rather than a check on two implementations agreeing.
- **The preview cache key includes the filter**, asserted directly: two `preview_bytes` calls at one overlay generation with two different `AnnotationFilter` modes return different bytes, and the same mode twice returns the cached buffer. Without the first half the print dialog shows the wrong Comments-and-Forms mode and every downstream filter test still passes, because none of them asks twice at one generation.
- Bench, in `crates/core/benches/save.rs`, with a stated budget in the existing `crates/core/benches/` harness shape: preview rebuild after one edit on a clean 1000-page document, on a **repaired** document where `needs_full_table()` forces a full-table section, and on the **largest file in `external/`** rather than only the synthetic one, since the cost this measures is O(file size) and the synthetic bench file is not the worst case a user has. Report first-commit and second-commit separately, which is where the buffer-reuse strategy either works or does not.
- `revert_to` on a document with unsaved edits is refused; on a non-trailing generation it is refused; on a trailing one it truncates and the reopened document matches the pre-save state.
- **A save whose write succeeds and whose reopen fails** leaves the session usable and says what happened: the bytes are on disk and correct, so the file is not lost, but the session's `cos::Document` is now the pre-save one while the overlay describes a state already written. The rule: the overlay is **not** cleared, the saved mark is **not** advanced, and the user is told the file was written but the session could not reload it, so a second save would append a second section carrying the same changes. Refusing to clear is the safe direction; clearing on a failed reopen loses the edits from a session whose file is fine.
- **The recovery file's permissions, read back rather than assumed** (Unix): its directory is `0o700` and its file is `0o600`, asserted by reading the mode after the write; a pre-existing directory at `0o755` makes the write **fail visibly** rather than proceed, which is the case `known-issues.md`'s P11 residual describes and the case that would otherwise put document bytes in a world-readable place; and the file is gone after a save and after a clean tab close.
- **Runs.** `cargo test -p onionskin-core`; `cargo bench -p onionskin-core --bench save`; `cargo test -p onionskin-app --no-default-features` (guarantee 5 with a save path in the workspace); `cargo clippy --workspace --all-targets -- -D warnings`.
- **Corpus.** The guarantee-1 sweep and its repaired-document half walk `external/`, so both run behind P1c's fetch step and are re-run under `ONIONSKIN_CORPUS_REQUIRED=1`. Without it, guarantee 1 at the `core` level means three tracked seeds.

**Review risk.** The highest-consequence package in M3. A reviewer will probe:
whether "one section" is asserted by parsing or by counting `%%EOF` occurrences
(the original file may legitimately contain one already); whether the reopen
after save leaves any cache holding a pointer into the old parse; whether the
preview cache is keyed on something that actually changes with every edit;
whether `revert_to`'s generation bump really precedes the truncation, since the
reverse order leaves exactly the window it exists to close; whether autosave's
recovery replay can double-apply an edit
that was also saved; whether a `Save As` leaves the generations list describing
the old file; whether the guarantee-1 carve-out for repaired documents is
derived from `Provenance` or from a filename list somebody maintains; whether any
path here reaches `cos::delete_object`, which T5's rule forbids and which nothing
but a reviewer's grep will catch.
**Mutation that must break its tests:** removing the empty-overlay
short circuit must fail the edit-undo-save test; emitting one section per edit
instead of per save must fail the ten-edits test; skipping the reopen after save
must fail the two-saves `/Prev` test; **replacing the `next_number` reseed with
`Overlay::default()` must fail the create-save-create-save test**, and nothing
else in the suite notices, which is why that bullet exists; dropping the filter
from the preview cache key must fail the two-filters-one-generation test.

### P4. `core::structure`: the tagged-PDF structure tree

**Goal.** Build what T6 says M3 owes: read the tree, keep it valid through every
M3 edit, and prove that with an invariant rather than with M5's checker.

- Readers for `/StructTreeRoot`, the `/ParentTree` number tree, **`/IDTree`**,
  per-page `/StructParents`, per-annotation `/StructParent`,
  `/StructParentsNext` on the root, `/MarkInfo`, the `/K` element hierarchy, and
  **each `/StructElem`'s own `/Pg`**. The last two are the ones a page removal
  reaches that `/K` alone does not: a surviving element's `/Pg` can name a
  removed page without any `/K` entry pointing at it, so the invariant below
  misses it, and `/IDTree` can name an element that is gone. Depth-capped and
  cycle-guarded, like every other reader in `core`.
- A maintenance hook that every `DocumentEdit` passes through, with three
  operations M3 needs: remove a page's elements and their `/ParentTree` entries;
  reorder the `/K` sequence to match a new page order; attach an `/Annot`
  element with a fresh `/StructParent`.
- An honest "this document is not tagged" path, which is most documents, where
  every operation is a no-op and says so.
- The M3 invariant, callable from every editing package's tests: after an edit,
  the `/ParentTree` is a well-formed number tree, every `/StructParent` and
  `/StructParents` index resolves through it, **every surviving element's `/Pg`
  names a surviving page**, **every `/IDTree` entry resolves to an element that
  is still there**, and no `/K` entry references a removed page or a freed object
  number. The `/Pg` clause is the one a `/K`-only invariant passes while the tree
  points at a page that is gone.
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
- Named destinations: `/Dests` (a flat dictionary) and `/Names /Dests` (a **name
  tree**, rebuilt including `/Limits` on every ancestor, through the same
  tree-rebuild helper `/PageLabels` and P4's `/ParentTree` use). Entries naming a
  removed page are dropped, with the count reported so the UI can say what it
  dropped.
- **The outline chain**, its own function: re-link `/Prev` and `/Next` past a
  removed item, fix the parent's `/First` and `/Last` when the removed item was
  one, recompute every ancestor's `/Count` preserving its sign, and unlink a node
  whose descendants are all gone. The item's own `/A` and `/D` entries are
  handled with the destinations above; **the chain is separate work and it is
  the half that fails silently**, because under free-nothing a sibling still
  naming the removed item resolves into garbage (T5).
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
- The `/Count` on the new `/Pages` node, which is just the length. Mechanical, and
  not one of the seven fix-ups.

**Rows closed.** None. Backs every row in P11 and P12.

**Files.** New `crates/core/src/pages/{mod,rewrite,inherit,labels,destinations,outline,links,fields,threads,actions}.rs`
(one module per fix-up, so "seven functions" is visible in the file list rather
than asserted in prose: `labels`, `destinations`, `outline`, `links`, `fields`,
`threads`, `actions`),
new `crates/core/src/pages/tree.rs` (the number-tree and name-tree rebuild helper
`labels`, `destinations` and P4's `/ParentTree` all call),
`crates/core/src/edit/verb.rs` (the `DocumentEdit` variants), new
`crates/core/tests/pages.rs`.

**Depends on.** P2, P4. Also needs P1's reference validator to be the thing that
catches its mistakes.

**What exists to build on.** `cos::Document::page(index)` already walks the tree
and resolves the four inheritable attributes against ancestors (M2's P1 built
it), **but its resolved view is lossy for exactly the two attributes
materialization exists to preserve**, so the materialization reads
`PageNode.dict` and the ancestors' raw dicts rather than `PageNode`'s convenience
fields:

- **`/Resources` comes back as a `Dict` value, not the `Object::Ref` the file
  had** (`document.rs:627`, which stores `resolved(&dict, b"Resources")`).
  Materializing from it **inlines a full copy of a shared `/Resources` into every
  rewritten page dict**. A document whose 1000 pages inherit one resource
  dictionary gets 1000 copies of it, and that is precisely the section-size
  number P5's bench is meant to decide T5's fallback on, so materializing this
  way would make the bench measure the bug.
- **`rectangle()` policy-filters** (`document.rs:702-722`): a degenerate or
  non-finite box yields `None`. A page whose own `/MediaBox` is degenerate
  therefore has `media_box: None`, and materializing from that emits a page with
  **no `/MediaBox` at all** under a flat `/Pages` node that has none either -
  turning a page with a bad box into a page with no box.

Both are invisible to a before-and-after comparison that reads through the same
accessor, which is what the first draft's verification did. P1 adds a raw
inheritable-entry accessor, or this package descends `PageNode.dict` itself;
either way the rule is **preserve `Object::Ref` and preserve the entry as
written**. `content::Page` normalizes on top of it. `outline.rs` already resolves
bookmark destinations to page indices and already knows the O(pages) sweep it
does, which this package must not make worse.

**Verification.**
- A corpus sweep, run as part of this package and not afterwards, listing every `external/` file whose page tree is more than one level deep, every file with `/PageLabels`, every file with `/AcroForm /Fields`, every file with `/Threads`, and every file with an intra-document `/Link`. Pick three of each as named fixtures. If the plan cannot name the fixture files, the transformation is unproven. **The sweep and every fixture below live behind P1c's CI corpus step**: `corpus/external` is gitignored and the `test` job fetches nothing, so without that step this entire verification section runs over an empty file list and reports a pass. This is the highest-correctness-risk package in M3 and it is the one whose fixtures CI currently never sees.
- For each deep-tree fixture: delete a page, and assert through a fresh parse of the saved output that every surviving page's four inheritable attributes are **identical to what they resolved to before**. This is the bug the flat rewrite exists to prevent and it is invisible in a shallow-tree fixture.
- **A shared `/Resources` stays shared**, asserted on the saved bytes: on a fixture whose pages inherit one resource dictionary, every rewritten page's `/Resources` is the **same `Object::Ref`**, and the section does not contain N copies of it. Comparing resolved values passes on the inlined form, which is why this assertion reads the reference.
- **A degenerate `/MediaBox` survives as it was written**, not as `rectangle()` reports it: on a fixture with a zero-area or non-finite box, the rewritten page still carries that `/MediaBox` entry byte-for-byte, rather than losing it because the accessor filtered it to `None`. "Unimplemented means untouched" applies to values the parser dislikes.
- **`audit_references` (P1) reports nothing on the whole output file** after every operation over every fixture, and `section_for`'s gate accepts every section. The first is the complete check and is the one that matters here; the second is free. A run that trips either is a failure of this package, not of the check.
- **No section this package emits carries a free entry other than the object-0 list head, and none naming a number any object references**, asserted by parsing the output's last cross-reference table. The exception is not a hedge: `free_list_rows` returns the object-0 head unconditionally for a full-table section (`document.rs:1004-1015`), which is every repaired document, so "no free entry at all" fails on any repaired fixture regardless of what this package does. Fixtures are partitioned by `Provenance`, the same way P3's guarantee-1 sweep partitions.
- `/PageLabels`: a fixture with roman-then-arabic labels keeps each surviving page's label after a reorder and after a delete, compared as resolved label strings, not as tree structure.
- A bookmark and a named destination pointing at a deleted page: both are dropped, the drop count is reported, and every surviving bookmark still resolves to the page it named before.
- **A link annotation on a surviving page pointing at a deleted page** is dropped, and one pointing at a surviving page still resolves to the page it named before. The fixture is a document with intra-document links, named in the sweep alongside the deep-tree ones.
- **The outline chain, walked rather than reached.** On a fixture with a three-item outline whose middle item names the deleted page: the surviving chain **walks from `/First` to `/Last` visiting exactly two items**, neither of them naming the dropped object, and the parent's `/Count` is 2. A naive reachability check passes on the broken form, which is exactly why the assertion walks the chain. A closed parent keeps its negative `/Count`, asserted separately, since sign is the part a recount loses.
- **`/Names /Dests` is rebuilt, not just filtered**: after removing a page, every surviving name still resolves through the tree, asserted by lookup rather than by inspecting `/Kids`, and every ancestor's `/Limits` bracket its subtree. A tree whose entries were dropped without recomputing `/Limits` fails the lookup for names outside the stale brackets and passes every structural check.
- **`/AcroForm /Fields`** on a form fixture: deleting the page carrying a widget drops that field and leaves every other field resolving to its own widget, asserted by walking the field tree after a reopen. A `/Parent` node emptied by the drop is gone too.
- **Article threads** on a fixture with a thread spanning three pages: deleting the middle page leaves a two-bead ring whose `/N` and `/V` close, and deleting all three drops the thread from `/Threads`. A ring that still walks but visits a garbage bead passes a naive reachability check, so this is asserted by walking the ring and comparing the bead set.
- **`/OpenAction` and page-level `/AA`** on a fixture whose `/OpenAction` names the deleted page and whose surviving page carries an `/AA /O`: the catalog's `/OpenAction` is dropped or retargeted (whichever this package decides, asserted rather than left open), the surviving page's `/AA` is untouched, and reopening does not land on a page that is gone. This is the seventh fix-up and it had no fixture bullet at all, which is how a list of seven and a set of six coexisted.
- P4's structure invariant passes after each operation on each tagged fixture, and the reading order matches the new page order for the reorder case.
- Rotation: `/Rotate 90` inherited from a `/Pages` node survives the flatten, and a page whose own `/Rotate` overrode its ancestor's keeps its own.
- Bench: rewriting the tree of the 1000-page bench file, with the section size reported. The number decides whether T5's fallback is needed and this plan does not guess it.
- **Runs.** `cargo test -p onionskin-core --test pages`; `cargo bench -p onionskin-core`; `cargo clippy --workspace --all-targets -- -D warnings`.

**Review risk.** The highest-correctness-risk package in M3, the way P3
(geometry) was in M2. A reviewer will probe: whether inheritance is materialized
before or after the parent pointer changes (after is wrong and passes on
shallow trees); whether `/Count` is recomputed or copied; whether a page index that
appears twice in the new order is handled: under P11 copy-between-documents goes
through the **importer** with renumbered references, so it never produces one,
and `rewrite_page_tree` therefore **refuses a repeated existing-page index with a
typed error** rather than silently aliasing one object into two `/Kids` slots.
Asserted, since a duplicate that aliases passes every structural check and
produces two page dicts that are the same object; whether anything here
frees an object number, which T5 forbids and which a grep for `delete_object`
settles in one command; whether the destination fixup
walks `/Names` trees or only the flat `/Dests` dictionary; whether the drop
count is real or an estimate; whether the **seven** document-level fix-ups are
seven functions with seven fixtures or one function that handles the easy cases.
Whether the `/Names /Dests` rebuild recomputes `/Limits` or only filters entries,
which passes every structural check and breaks lookup. Whether the outline,
thread and name-tree fix-ups each share the one tree-rebuild helper or grew
their own.
**Mutation that must break its tests:** skipping
inheritance materialization must fail the deep-tree fixture; copying `/Count`
instead of recomputing must fail the delete case; dropping the `/PageLabels`
rebuild must fail the label fixture; **dropping any one of the seven fix-ups
must fail exactly its own fixture and no other** - `/PageLabels`, destinations
(including the `/Names /Dests` tree), the outline chain, link annotations,
`/AcroForm /Fields`, article threads, and `/OpenAction` with page-level `/AA` -
which is what proves they are seven checks rather than one; re-linking the
outline chain but not recomputing `/Count` must fail only the `/Count` half of
the outline assertion.

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
  to appear in the Comments pane. **This reader goes through P3's `structure()`,
  not `&self.cos`**, or it cannot see a comment the session just authored and
  P20's central claim fails. It is named here because this package lands after
  P3, so P3's own conversion list could not include it.

**Rows closed.** None. Backs every row in P8, P9a, P9b, P9c, P10, P20 and the filter rows
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

**Goal.** Let a tool express an edit, make the registry prove every tool's edits
behave, and **own guarantee 2 at the level the DoD promises it**.

- **`ToolCtx` gains a way to reach the edit session, and it is not a field.**
  The first draft said `ToolCtx { doc: &'a mut Document, edits: &'a mut
  EditSession }`, and **that does not compile** given P2, which puts
  `EditSession` inside `core::Document` as a field. Handing out `&mut Document`
  and `&mut document.edit` at the same time is two mutable borrows of one value.

  The shape is `ToolCtx { doc, viewport }` unchanged, plus
  `impl Document { pub fn edits(&mut self) -> &mut EditSession }`. A tool
  reaches the session through the document it already has. `CommandCtx` is the
  same. **The `edits` field is deleted from this package's spec**, and no
  `&mut EditSession` is stored anywhere alongside a `&mut Document`.
- `Requirement` moves from `crates/app/src/shell/context_menu.rs:47` into
  `plugin-api` and gains a `Command(&'static str)` variant, so the
  `Requirement::Milestone` arms that `context_menu.rs`'s own module
  doc calls "guesses" become registry queries. There are **seven** such arms and
  **four name M3** (rich-text export, Add Bookmark, Print, Page Commands); the
  other three name M5 and stay. That module doc names this as the
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
    `cos::Document::open` and satisfies P1's section gate, with
    `audit_references` clean on the whole output;
  - deterministic given the same inputs.
- **The registry test is not guarantee 2, and this package says which package
  is.** The DoD and PLAN.md both promise guarantee 2 "driven by an edit a tool
  made through `core`", and **no package specified it**. P3 cannot: it says
  "make an edit through `EditSession`", which is `core`'s own API, and
  `crates/core` depends on `content`, `cos` and `render` only, so a test living
  there can never reach a plugin. The property test above drives real gestures
  but asserts only that the result reopens and satisfies the gate, which is a
  weaker sentence.

  **P8 owns it**, because P8 is the package that ships the highlight the test
  drives and it already depends on this one. Putting it here instead would make
  P7 depend on P8 while P8 depends on P7, which is a cycle rather than a
  schedule. What this package owns is the contract the test satisfies:
  `ToolCtx`'s reach into the edit session, and the property suite that says every
  tool's edit is undoable and serializes.

**Rows closed.** None. Backs every tool and command row in M3.

**Files.** `crates/plugin-api/src/lib.rs`, `crates/plugin-api/src/registry.rs`,
new `crates/plugin-api/src/requirement.rs`, `crates/app/src/shell/context_menu.rs`
(deletes its private copy), `crates/plugin-api/tests/contract.rs`,
`crates/core/src/session.rs` (the `edits()` accessor).

**Depends on.** P0b (`context_menu.rs`'s callers), P2 (`EditSession`).

**What exists to build on.** `ToolCapability` (8 variants) and the
`tool_with(registry, capability)` query already work and are used by the quick
action toolbar, the canvas context menu and the global bar.
`PluginRegistry::commands()` already carries ids. The four-variant `Requirement`
already exists and only needs a fifth variant and a new home.

**Verification.**
- The property tests run against the real `build_registry()` and fail if a tool is added without a group, which is checked by adding a deliberately incomplete tool in a test and asserting the suite rejects it.
- The "every edit is undoable" test drives each tool's real gesture lifecycle (`on_pointer_down`/`move`/`up`/`on_commit`), not a synthetic `DocumentEdit`, or it proves nothing about the tools.
- `cargo test -p onionskin-app --no-default-features` and `--no-default-features --features tools-comment` still pass: guarantee 5 holds with the contract in place.
- Every `Requirement::Milestone` arm that a shipped M3 plugin now satisfies is gone, asserted by a test that no `Milestone` reason names M3.
- **Runs.** `cargo test -p onionskin-plugin-api`; `cargo test -p onionskin-app --no-default-features --features tools-comment`; **`cargo test -p onionskin-app --no-default-features --features shell,shell-test-support`** for the `context_menu.rs` change, which is shell code and compiles nowhere without it; `cargo clippy --workspace --all-targets -- -D warnings`.

**Review risk.** **What `&mut Document` already lets a tool reach**, which is the
sharper version of "whether `ToolCtx` grew more than `edits`". Today
`&mut core::Document` exposes `selection_mut`, `cancel_search`,
`set_layer_visible`, `reset_layer_visibility`, `request_snapshot` and
`select_match` (`session.rs`), and after P3 it will also expose the save path,
the generations list and `revert_to`. A tool that can call `revert_to` is a tool
that can truncate the user's file mid-gesture. Adding `edits()` to that surface
is small; the surface it joins is not, and the reviewer's question is whether P3
put its save and generation methods behind something narrower than `&mut
Document` or simply added them to what every tool already holds. That question
was invisible while the review risk was phrased as "did `ToolCtx` grow a second
field".

Also: whether "every edit is undoable" is asserted on the overlay or on the
saved bytes, and whether it is run per tool or once. Whether the `Requirement`
move left the app with a second copy. Whether the degenerate-document test uses
a document degenerate enough to have caught anything.
**Mutation that must break its tests:** making `EditSession::undo` a no-op must
fail the property test for every tool, and if it fails for only some, the test is
not exhaustive.

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
under a write path for the first time. If M2's P3 geometry has a residual error,
this is where it surfaces, and it is cheaper to find here than under thirteen
drawing tools.

**This package also owns guarantee 2 at the level the DoD promises it.** PLAN.md
and the definition of done both say guarantee 2 is "driven by an edit a tool made
through `core`", and no package specified it. P3 cannot: it drives `EditSession`,
which is `core`'s own API, and `crates/core` depends on `content`, `cos` and
`render` only, so nothing there can reach a plugin. P7's property suite drives
real gestures but asserts a weaker sentence. It lands **here** because this is the
package that ships the highlight the test drives, and because putting it in P7
would make P7 depend on P8 while P8 depends on P7.

**Rows closed.** 56 Highlight text, 57 Underline text, 58 Strikethrough,
59 Insert text at cursor (caret markup), 60 Replace text. **5 rows.** Plus
guarantee 2, which closes no row and is named against it here.

**Files.** `plugins/tools-comment/src/lib.rs`,
`plugins/tools-comment/src/{markup,quads}.rs`,
`plugins/tools-comment/Cargo.toml` (adds `onionskin-core`), new
`crates/app/tests/tool_edit_guarantee.rs`, and
**`crates/app/tests/guarantees.rs`**.

That last file needs naming because **nobody currently owns editing it**.
Guarantee 2's tripwire
(`an_edit_appends_one_incremental_section_that_truncates_away`) currently names
`incremental.rs` and its two enforcing tests; this package adds the new app-level
test to that tripwire's enforcing list along with the assertion markers it must
still contain, so deleting the tool-driven test fails the guarantee rather than
passing quietly. The same file carries
`acrobat_parity_headline_matches_every_inventory_row`, which is **P15's** to
change; the two edits are in one file and neither should be made assuming the
other has been.

**Depends on.** P1c, P3, P6, P7.

P3 is for the guarantee-2 test's save, and P1c for the `external/` fixtures that
test drives. **Not P0b**: this package's two `crates/app` files are test targets,
not shell source, and the split rewrites neither. It is the second of the two
exceptions to "everything with a `crates/app` file waits on P0b", the first being
P1c. That is ordering prose and sits below the blank line, because section 5
derives edges from the paragraph above it.

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
- **Guarantee 2, driven by a tool**: drive the highlight through a real gesture, save through `core`, and assert all three clauses of the guarantee sentence: the original bytes survive as a prefix, exactly one section is appended (counted by parsing, not by scanning for `%%EOF`, since the original may legitimately contain one), and truncating at `original_len()` yields the byte-exact original. Ten highlights and one save is still one section.
- `guarantees.rs`'s guarantee-2 tripwire names that test and its markers, so deleting the test fails the guarantee. Proven by deleting it on a scratch branch and recording the failure in the PR, which is the same demonstration P1c owes its CI pair.
- `cargo test -p onionskin-app --no-default-features --features tools-comment`, `cargo test -p onionskin-app --test guarantees`, and the P7 property suite.
- **Corpus.** The real corpus document, the `/Rotate 90` fixture and the two-column fixture are all `external/`, so they run behind P1c's fetch step and its mandatory re-run.

**Review risk.** Whether the quad order convention is written down and matches
what `pdfannots` and Acrobat read back. Whether a selection spanning a rotated
page maps correctly, which needs a `/Rotate 90` fixture and is the same trap M2's
P3 documented. Whether Replace text writes a real `/IRT` reply chain or two
unrelated annotations. Whether the tool holds a borrow of `PageText` across the
commit. **Mutation that must break its tests:** replacing the quad list with its
bounding rectangle must fail the two-column test; making the save emit two
sections must fail the guarantee-2 test. Neither says "and nothing else": both
tests share fixtures with the rest of this package's suite, so a mutation that
breaks one plausibly disturbs a neighbour, and the claim to check is that the
named test **does** fail, not that it fails alone.

### P9. `tools-comment` B: notes, drawing and shapes

Three packages. Thirteen tools in one review was the sprawl risk the first draft
named in its own review-risk list and then did not act on, and the three groups
have genuinely different hard parts: free text needs font handling, ink needs
pressure and stroke splitting, shapes need only geometry and the appearance
generator.

All three share P6's appearance-stream generator, one defaults struct and one
commit path, and none of them writes a content stream. All three depend on P6 and
P7, and all three land after P8, so P6's generator has one consumer's worth of
feedback before thirteen more arrive.

**The in-progress overlay seam does not exist yet, and the first draft said it
did.** It claimed "`Overlay::{Polyline, Line, Circle, Rect}` cover every
in-progress gesture these tools need, so nothing new is required on the overlay
side". Measured against `main` at `fa5a194`, that is wrong twice, and the second
way is the expensive one.

**Four of the six `Overlay` variants cannot be drawn.** `plugin_api::Overlay`
(`lib.rs:43`) has six variants, but `canvas.rs`'s `OverlayPaint`
(`canvas.rs:344`) has **two**: `Quads` and `AntsRect`. `map_overlay`
(`canvas.rs:1879-1895`) returns `Err` for `Rect`, `Polyline`, `Line` and
`Circle`, and `overlay_paints` turns that into a user-visible status,
`"the canvas cannot draw a {kind} overlay yet"`. Only the two that
`tools-basic` uses have painters: `select_text` emits `Quads` and `marquee`
emits `AntsRect`. So every tool in this group would ship with its in-progress
preview replaced by an error string. That is not a shape problem; there is no
painter.

**And `Circle` is the wrong shape anyway**, in two ways:

- **Oval is an ellipse inscribed in a dragged rectangle, and `Circle { center,
  radius }` cannot express one.** A preview drawn as a circle is a preview that
  does not match what the commit produces, which is the exact class T4's preview
  buffer exists to abolish. `Circle` also has **no consumer at all** today: the
  search-hit marker its doc comment names is an example, not a caller, and
  search hits are painted through `HighlightPaint`.
- **Polygon and Cloud are closed and `Polyline` is open**, so the in-progress
  preview of a polygon is missing its closing edge.

**P9a owns the whole seam**, because it lands first of the three, needs `Rect`
itself for the text box and the callout box, and one package owning the mapping
means one review of it rather than three:

- `plugin_api::Overlay`: `Circle { center, radius }` becomes
  `Ellipse { bounds: PageRect }`, and `Polyline` gains `closed: bool`. Named
  consumers: P9c's Oval and P9c's Polygon and Cloud.
- `canvas.rs`: `OverlayPaint` gains `Rect`, `Polyline`, `Line` and `Ellipse`,
  with painters and viewport mapping, and `map_overlay` loses every `Err` arm so
  a future unpainted variant is a compile error rather than a status message.
  Named consumers: P9a's text box and callout (`Rect`), P9b's ink (`Polyline`),
  P9c's line, arrow, rectangle, oval, polygon, polyline and cloud (all four).

Naming this here rather than discovering it mid-gesture is the point, and it is
the difference between three tool packages and three tool packages plus a shell
change nobody scheduled.

#### P9a. Notes and free text

Sticky note (`/Text`), Add text comment / typewriter (`/FreeText` with
`/IT /FreeTextTypewriter`), Text box (`/FreeText`), Callout (`/FreeText` with
`/CL` and `/IT /FreeTextCallout`).

**Rows closed.** 55 Sticky note, 61 Add text comment (typewriter), 62 Text box,
63 Callout. **4 rows.**

**Files.** `plugins/tools-comment/src/{note,freetext}.rs`,
`plugins/tools-comment/src/lib.rs`, `crates/plugin-api/src/lib.rs` (the two
`Overlay` corrections above), `crates/app/src/shell/canvas.rs` (the four new
`OverlayPaint` variants, their painters, and the removal of `map_overlay`'s
`Err` arms).

**Depends on.** P0b (the canvas painters), P6, P7, P8.

**Verification.**
- Per tool, a gesture test: the shape a drag produces, the shape a click produces (a sticky note is a click, a text box is not), and what a degenerate gesture produces (nothing, for every one of them).
- **Every `Overlay` variant paints**, asserted by handing the canvas one of each and comparing the resulting `PaintList` against expected view-space geometry. `map_overlay` has no `Err` arm left, so this is exhaustive by the compiler rather than by the test remembering all six.
- **No overlay produces the `"the canvas cannot draw a {kind} overlay yet"` status**, asserted on the recorded errors after each gesture test. That string is the current behaviour for four of six variants and it is what a user would have seen.
- A callout's `/CL` line points from the leader's tail to the annotation's `/Rect`, asserted on the array rather than on the render, plus one render that shows the leader.
- Every tool's edit passes P7's undoable-and-serializes property test.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features tools-comment` and the P7 property suite; `cargo clippy -p onionskin-app --no-default-features --features tools-comment --all-targets -- -D warnings`.

**Review risk.** Whether the free-text tools embed a font or reference a Base 14
name, and whether the Base 14 substitution rule from Legal posture rule 6 is
honoured. Whether a `/FreeText` annotation's `/DA` and its appearance stream can
disagree about the font, which is what makes a text box render differently in
Acrobat. **Mutation that must break its tests:** dropping `/CL` must fail the
callout test.

#### P9b. Ink

Draw freehand (`/Ink`, with stylus pressure from the GPUI fork), Erase ink
(removes or splits `/InkList` strokes), and `ToolCapability::Draw`.

**Rows closed.** 64 Draw freehand (ink), 65 Erase ink. **2 rows.**

**Files.** `plugins/tools-comment/src/ink.rs`,
`plugins/tools-comment/src/lib.rs`.

**Depends on.** P6, P7, P8, P9a (the `Polyline` painter).

**What exists to build on.** `PointerInput` already carries `pressure: f32` and
the pinned GPUI fork adds stylus pressure, which is the whole reason the fork is
pinned. `Overlay::Polyline` is the in-progress stroke and P9a gives it a
painter.

**Verification.**
- A stroke with varying pressure produces an `/Ink` annotation whose appearance stream has varying stroke width, asserted by rendering two strokes at different pressures and comparing covered pixel counts, not by reading the content stream text.
- Erase over the middle of a stroke splits it into two `/InkList` entries and leaves the annotation's `/Rect` correct for the remainder; erase over a whole stroke removes it; erase over nothing produces no edit and no undo entry.
- An ink gesture of several hundred pointer events is **one** undo entry, asserted on the history, which is T2's transaction rule at its worst case.
- **Runs.** As P9a.

**Review risk.** Whether pressure is used or accepted and ignored. Whether the
stroke splitter can produce a zero-point `/InkList` entry. Whether the ink tool
holds the overlay across the commit. **Mutation that must break its tests:**
ignoring `PointerInput::pressure` must fail the varying-width test; emitting one
undo entry per pointer event must fail the transaction test.

#### P9c. Shapes

Line (`/Line`), Arrow (`/Line` with `/LE` endings), Rectangle (`/Square`), Oval
(`/Circle`), Polygon (`/Polygon`), Connected lines (`/PolyLine`), Cloud
(`/Polygon` with `/BE` a cloudy border effect).

**Rows closed.** 66 Line, 67 Arrow, 68 Rectangle, 69 Oval, 70 Polygon,
71 Connected lines (polyline), 72 Cloud. **7 rows.**

**Files.** `plugins/tools-comment/src/shapes.rs`,
`plugins/tools-comment/src/lib.rs`.

**Depends on.** P6, P7, P9a (which owns every `Overlay` and `OverlayPaint`
change these seven tools need, so this package adds no shell file).

**Verification.**
- Per tool, a gesture test: the shape a drag produces, and what a degenerate gesture produces (nothing, for every one of them).
- **The in-progress overlay matches the committed result**, asserted for Oval and Polygon specifically by rendering the overlay and the committed annotation and comparing: an ellipse preview against an ellipse annotation, a closed polygon preview against a closed polygon. This is the assertion the `Overlay` changes exist for, and without it a circle-preview-for-an-oval ships and looks almost right.
- Cloud: the `/BE` border effect renders as a scalloped edge, asserted by comparing against a plain `/Polygon` render (they must differ) rather than by asserting an exact pixel pattern.
- `Overlay::Ellipse` with equal axes draws the circle `Overlay::Circle` described, asserted once, so the replacement loses nothing. There is no existing consumer to regress: `Circle` had none, and search hits are painted through `HighlightPaint`.
- Every tool's edit passes P7's undoable-and-serializes property test.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features tools-comment`; `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` for the canvas arms; the P7 property suite; the matching clippy.

**Review risk.** Whether Arrow is a `/Line` with `/LE` or a separate subtype (it
is the former, and shipping it as a `/Polygon` would render in Acrobat as a line
with no head). Whether the seven tools share one defaults struct, one commit path
and one appearance call, or are seven small renderers. Whether this package
reached back into `canvas.rs` after all, which would mean P9a's seam was
incomplete and is worth knowing rather than patching. **Mutation that must break its tests:** dropping `/BE` must fail the
cloud test; drawing the Oval preview as a circle must fail the preview-matches-
commit test.

### P10. `tools-comment` C: stamps, attachments and the comment summary

**Goal.** The stamp family, attach-as-comment, and the summary document. Comment
properties is **row 74 and belongs to P20**, which builds the inspector; what
lives here is `properties.rs`, the plugin-side defaults a stamp or an attachment
is created with, over P6's `SetAnnotationProperties` verb.

Stamps are `/Stamp` annotations whose appearance stream is the stamp artwork.
Per Legal posture rule 2 and the parity row's own note, **every stamp is redrawn
in-house**; the generated-logo discipline (`tools/logo.py`, constants in, SVGs
out) applies. Dynamic stamps fill name, date and time natively from the system
clock and an identity preference, not through the `AF*` JavaScript helpers
Acrobat uses, because `scripting` is M5. The parity row already says so.

Attach a file as a comment is a `/FileAttachment` annotation with an embedded
file stream, which is the same embedded-file machinery P13b needs for the
Attachments pane, so the writer lives in `core` and both call it.

Summarize comments generates a new document (T8) through `cos::write_new`: a
page per source page with the comments listed, or the compact single-list form,
matching Acrobat's two layouts.

**Rows closed.** 73 Attach a file as a comment, 76 Summarize comments,
79 Place a stamp, 80 Standard business stamps, 81 Sign Here stamp category,
82 Dynamic stamps, 83 Create a custom stamp, 84 Manage stamps, 85 Paste
clipboard image as stamp. **9 rows.** Row 74 (Comment properties) **moves to
P20**, which builds the properties inspector in the side panel and already
carried the row's surface in its goal text while P10 carried the row. P6 owns
the `SetAnnotationProperties` verb either way.

**Files.** `plugins/tools-comment/src/{stamp,attach,summary,properties}.rs`,
new `crates/core/src/embedded.rs` (the embedded-file writer, shared with P13b),
new `tools/stamps.py` and the generated `assets/stamps/*.svg`; **and the app
surface its rows are scored on**: new `crates/app/src/shell/stamps_dialog.rs`
(rows 83 and 84, Create a custom stamp and Manage stamps),
`crates/app/src/shell/dialog.rs` (the `ShellDialog` variant),
`crates/app/src/shell/chrome/tabs/menu.rs` (their entries), and **P14a's
clipboard image helper** for row 85, called rather than reimplemented: P14a lands first and
owns it, and this package adds no pasteboard code of its own.

**Depends on.** P0b (the app surface above), P1b (the encrypted-source
predicate), P6, P7, P9c (a custom stamp's
appearance is drawn artwork, and P9c's shapes are where that geometry-to-
appearance path gets its first seven consumers), P12 (the summary needs
`write_new` and P12's page assembly), P14a (paste-as-stamp needs the image import
path).

**Why the app files are here rather than absent.** The first draft closed rows
83, 84 and 85 from a package whose Files line named no `crates/app` path and
whose Depends on line named no P0b, so three rows would have been marked
`implemented` against a dialog and a clipboard read that nobody had scheduled.
`ACROBAT-PARITY.md` exists to stop exactly that, and its design is worth nothing
if the ledger scores a row against a capability rather than against the surface a
user reaches.

**What exists to build on.** `core::attachments` reads embedded files today, so
the stream structure and the `/Names /EmbeddedFiles` layout are already
understood in-repo; the writer is its inverse. `codecs-common` already encodes
PNG, which is what a clipboard image stamp becomes.

**Verification.**
- Every shipped stamp is generated by the script from constants, and a test asserts the committed SVGs match a fresh generation, so nobody hand-edits one into resembling Adobe's artwork.
- A dynamic stamp's rendered text contains the configured identity and a date matching a clock injected by the test, not the real clock, or the test is unstable and proves nothing.
- Attach-as-comment: the saved file's embedded stream round-trips byte-identically to the input, and the attachment appears in `core::attachments` after a reopen. Path traversal in the file name is rejected, matching the existing `attachments.rs` rule.
- **Encrypted sources are refused**, per P1b's encrypted-source rule: Summarize comments reads the open document, so it is a **session-scoped** refusal through `Requirement::Command`, carrying P1b's typed reason naming M6. Asserted here as well as in P1b, because a package that can reach `write_new` and does not check is the way that rule gets a hole.
- Summary: the generated document opens through `cos::Document::open`, has the expected page count, and its extracted text contains every comment's contents and author.
- Custom stamp creation from a PDF page and from an image both produce a `/Stamp` whose appearance renders.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support,tools-comment` for the stamps dialog and the clipboard call site, which are shell code; `cargo test -p onionskin-core` for the embedded-file writer; the matching clippy.

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

**Depends on.** P1b (the encrypted-source predicate, in both its shapes), P5,
P7. Copy-between-documents also depends on P3, because the source document is a
live session.

**What exists to build on.** P5 does the tree work; this package is mostly
translating a user's selection into an `order`. `cos::Document::get` and
`resolve` are the importer's **read** side.

**Its write side is the overlay, not `cos::add_object`**, which T2 forbids
absolutely and which the first draft of this line named. Calling it would have
put the imported objects in `self.edits`, which `section_for` and
`save_overlay_to_path` do **not** read, so the imported page dict (in the
overlay) would be written while its content streams and resources (in cos's edit
map) were not: either the gate refuses the save or the file ships with a page
pointing at objects that were never written. They cannot be withdrawn, so undo of
Insert Pages could not take them back; `has_pending_changes()` would be
permanently true, falsifying T3 for any session that inserted a page; and each
call runs `forget_parsed_objects()`, so a 200-object import would drop both
caches 200 times, which is the cost T2 rejected the old projection design over.

The importer allocates from `Overlay.next_number` and writes `ObjectState`s into
`Overlay.states`, inside one `transact`, exactly like every other verb.

**Verification.**
- The nine operations, each on a deep-tree fixture and a flat one, each asserted through a fresh parse: page count, page order (compared by extracted text, so a reorder that keeps the count but shuffles nothing is caught), and P1's validator clean.
- The importer: insert a page from a document with an embedded font, save, reopen, and assert the inserted page renders identically to its render in the source document, pixel-compared with the same tolerance the render tests already use. A shallow importer that copies the page dict and not its resources passes every structural test and fails this one.
- Import of a page that references an object number already used in the destination: renumbered, asserted by checking the destination's original object at that number is unchanged.
- **No cos write API is called from `crates/core` or any plugin**, asserted by a source grep in the same shape T5's `delete_object` grep uses, over `add_object`, `set_object`, `delete_object`, `set_trailer_entry` and `set_info_field`. This package is the one that would have broken it, and a grep is the only check that catches a call which otherwise compiles and half-works.
- **Inserting a page from an encrypted file into an unencrypted document is refused** (P1b's encrypted-source rule), asserted on the destination's saved output containing no object imported from the source, not merely on the command reporting unavailable.
- **Replace-pages from an encrypted file is refused the same way**, asserted separately. It needs its own assertion rather than riding on insert's, precisely because this package's own review risk warns replace may be built as delete-then-insert: an implementation that routes through insert passes with one test, and one that does not passes with none.
- **Encrypted sources are refused**, per P1b's encrypted-source rule, in **both** of its shapes, because this package has both: extract-to-file is a **session-scoped** refusal through `Requirement::Command`, while insert-from-file, replace-pages-from-file and copy-or-move-between-open-documents are **per-input checks at execution** against a source the session query cannot see. Both call P1b's one `core` predicate over a `&cos::Document` and report the same typed reason naming M6.
- Extract writes a new document through `write_new` whose pages match the source pages by extracted text.
- Rotate composes: rotating a page that already has `/Rotate 270` by 90 gives 0, not 360.
- P4's structure invariant clean after each operation on each tagged fixture, and reading order matches page order after a reorder.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features tools-organize`, plus `--features shell,shell-test-support,tools-organize` for the disabled-reason assertions, which read shell state; the thumbnails context menu's M3 entries become enabled and their reasons gone, asserted in P21.
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
extract and P14a's create-from-images all call, which is why it lives in `core`
and not in this plugin.

**Rows closed.** 34 Create from multiple files, 37 Combine files into a single
PDF, 38 Add files / add folders, 39 Reorder, preview and remove entries,
40 Expand a file and combine at page granularity, 46 Split. **6 rows.**

**Files.** `plugins/commands-core/src/{lib,combine,split}.rs`, new
`crates/core/src/pages/assemble.rs`, `plugins/commands-core/Cargo.toml`; **and
the app surface rows 38, 39, 40 and 46 are scored on**: new
`crates/app/src/shell/combine_dialog.rs` (the list the user builds, with add
files, add folders, reorder, preview, remove and per-file page expansion),
`crates/app/src/shell/dialog.rs` (two `ShellDialog` variants, combine and split),
and `crates/app/src/shell/chrome/tabs/menu.rs` (their File menu entries).

**Depends on.** P0b (the app surface above), P1 (`write_new`), P1b (the
encrypted-source predicate, in both its shapes), P3, P5, P11 (the importer).

**Why the app files are here rather than absent.** Rows 38 and 39 are *"Add files
/ add folders"* and *"Reorder, preview and remove entries"*: they are a dialog and
nothing else. Row 40 is page-granularity expansion inside that dialog, and row 46
needs a dialog to choose between by-count, by-size and at-bookmarks. The first
draft closed all four from a package with no `crates/app` path and no P0b
dependency, which is four rows scored against a surface nobody had scheduled.

**What exists to build on.** `core::outline` already resolves bookmark
destinations to page indices, which is exactly what split-at-bookmarks needs.
`codecs-common`'s existing export job model (background worker, progress,
cancellation, atomic publication) is the pattern for a long-running combine,
and reusing it rather than writing a second one is the point.

**Verification.**
- Combine three named corpus files: the output's page count is the sum, each page's extracted text matches its source page, and each page renders identically to its source render.
- **The per-input encryption check runs per input**, asserted with the encrypted file in **position 2 of 3**: the combine is refused, the refusal names that file, and no output is written. A check written at the wrong loop level passes with the encrypted file first and fails only here.
- Combine a file with itself: legal, and the duplicate pages are independent objects, not aliases.
- Split by page count on a 10-page file at 3: four files of 3, 3, 3, 1, with the last one asserted, because an off-by-one here produces three files and drops a page.
- Split at bookmarks: a fixture whose top-level bookmarks are at pages 1, 4 and 9 produces three files with those boundaries, and a bookmark pointing at a page that does not exist is reported, not skipped.
- Split by file size: the constraint is best-effort and the test asserts what it actually promises (no output exceeds the target unless a single page does), not an exact size.
- **Encrypted sources are refused**, per P1b's encrypted-source rule, in **both** of its shapes: split reads the open document and is a **session-scoped** refusal through `Requirement::Command`, while combine's file list is chosen after invocation and needs a **per-input check at execution**. Both call P1b's one `core` predicate and report the same typed reason naming M6.
- Every output opens through `cos::Document::open` and satisfies P1's validator. For combine, the refusal covers **any** encrypted input, not only the first, which is the case a per-document check written at the wrong loop level would miss.
- Outputs carry a structure tree if all inputs did, and P4's invariant is clean; if any input is untagged, the output is untagged and says so rather than producing a half-tagged document.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support,commands-core` for the combine and split dialogs, which are the surface rows 38, 39, 40 and 46 are scored on; `cargo test -p onionskin-core` for the assembly primitive; the matching clippy.
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

Three packages. Ten rows spanning a five-tab dialog, XMP writing (for which
there is no reader in the repository), bookmark authoring, attachment authoring,
two pane rewrites and four menu rows is not one review, and the three groups
share nothing but a plugin crate.

All three depend on **P0b** and are the deepest app packages in the graph
(section 5), so they land near the end of the app order.

#### P13a. Document Properties

**Goal.** The document-level dialog, and the metadata writing behind it.

The five tabs Acrobat's unified UI documents: Description (writes `/Info` and
XMP), Security (read-only at M3, naming M6), Fonts (read-only, from `content`),
Initial View (writes `/OpenAction` and `/PageLayout` / `/PageMode`), Custom
(arbitrary `/Info` keys). The parity row says to confirm the tab list against the
screenshot corpus before building; **that confirmation is the package's first
task, not an assumption**, and its result is recorded here.

**XMP has no reader in this repository.** Nothing reads `/Metadata` today, so
"writes `/Info` and XMP, and the two must agree" needs a reader to assert it
with. This package builds the minimum: an XMP packet reader over the Dublin Core
and PDF schemas that the round-trip test compares against. Writing a format
nothing in the repo can read back is how a "both are written" claim becomes
unfalsifiable.

**Rows closed.** 12 File > Save as Other, 14 File > Properties, 32 Initial View
settings. **3 rows.** This also enables the Layers pane's `Properties` entry,
whose disabled reason names "the properties dialog".

**Files.** `plugins/commands-core/src/properties.rs`, new
`crates/core/src/metadata.rs` (the XMP packet reader and writer), new
`crates/app/src/shell/properties_dialog.rs`,
`crates/app/src/shell/dialog.rs`, `crates/app/src/shell/chrome/tabs/menu.rs`,
`crates/app/src/shell/panes/layers.rs` (the enabled `Properties` entry).

**Depends on.** P0b, P3.

**What exists to build on.** `preferences_dialog.rs` (465 lines) is the model for
a multi-category dialog body with its own `accessible()` and `render_*()` pair,
and the properties dialog follows it exactly. `ShellDialog` (`dialog.rs:27`)
already hosts modals, so this is one more variant.

**Verification.**
- Writing a Description field appears in `/Info` **and** in XMP, and reopening reports the new value from **both readers**, which is why the reader is in scope. The two must agree, or the reader a given consumer uses decides what it sees.
- **On a document with no `/Info` at all**, which is `corpus/seeds/minimal.pdf` and is the simplest file this command can meet: the edit creates the dictionary, sets the trailer's `/Info` key through `Change::TrailerKey` (T2), and **undo puts the trailer back**, asserted on the trailer. This package is the first caller of the trailer half of the edit model and it is the one that would have shipped the defect.
- Initial View: setting "open at page 5, fit width" writes `/OpenAction` and reopening in Onionskin honours it, asserted through the session, not through the dialog's own state.
- Save as Other offers exactly the sub-targets Onionskin supports, asserted against the registry rather than a hardcoded list, and PDF/X and Reader-Extended are absent rather than present-and-disabled.
- The Security tab is read-only and **looks** read-only, asserted on the controls' accessibility state rather than on a visual claim.
- The five-tab list matches the screenshot corpus, with the comparison recorded in the package.
- Every new dialog control is in the AccessKit tree with a real label and state, and the dialog's controls leave the tree when it closes, asserted by the probe.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support`; `cargo test -p onionskin-core` for the metadata reader; the matching clippy.

**Review risk.** Whether `/Info` and XMP are both written or only one, and
whether they can disagree. Whether the XMP writer emits a packet a third-party
reader accepts, or one only its own reader round-trips. Whether the Properties
dialog exposes a Security tab that looks writable. Whether the five-tab list was
confirmed against the screenshot corpus or copied from the parity row's own note.
**Mutation that must break its tests:** writing `/Info` and skipping XMP must
fail the round-trip; making the XMP reader return the `/Info` value must fail it
too, which is the check that stops the reader being a mirror.

#### P13b. Bookmark and attachment authoring

**Goal.** The authoring half of the two panes M2 built read-only.

- **Bookmark authoring**: create, rename, nest, set destination, delete, and the
  Bookmarks pane context menu. `New Bookmarks From Structure` is **not** M3: it
  needs the tagged tree and the parity row already puts it at M6.
- **Attachment authoring**: add and delete embedded files, and the Attachments
  pane context menu, over P10's embedded-file writer. Deleting one rewrites
  `/Names /EmbeddedFiles` without the entry and leaves the stream as garbage; it
  frees nothing, per T5.

**Rows closed.** 25 Bookmarks: create, rename, nest, set destination, delete,
26 Attachments: add and delete, 27 Bookmarks pane context menu, 28 Attachments
pane context menu. **4 rows.**

**Files.** `plugins/commands-core/src/{bookmarks,attachments}.rs`, new
`crates/core/src/outline/write.rs`, `crates/core/src/embedded.rs` (shared with
P10), `crates/app/src/shell/panes/{bookmarks,attachments}.rs`.

**Depends on.** P0b, P3, P5 (destination fixup), P10 (embedded-file writer).

**What exists to build on.** `core::outline::read` and `core::attachments::read`
exist and are cycle-guarded; the writers are their inverses and can share the
traversal.

**Verification.**
- Bookmark authoring: create a nested bookmark, save, reopen, and assert the tree shape and each destination's resolved page through `core::outline::read`, which is the reader this is the inverse of. Renaming preserves the destination; deleting a parent with children either promotes or removes them, and **which one is asserted**, not left to chance.
- A bookmark whose destination page is later deleted by P11 is dropped and counted (this is P5's fixup, exercised from the surface that creates them).
- Attachments: add, save, reopen, `core::attachments::read` reports it with the right size and MIME; delete removes it from `/Names /EmbeddedFiles` and leaves the stream as unreferenced garbage, with `audit_references` clean and no free entry in the appended section. Path traversal in the file name is rejected, matching the existing `attachments.rs` rule.
- Both context menus' entries are in the accessibility tree and leave it when the menu closes.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support`; `cargo test -p onionskin-core`; the matching clippy.
- **Corpus.** The bookmark-destination fixtures are `external/` and run behind P1c's fetch step and its mandatory re-run.

**Review risk.** Whether bookmark destinations are written as explicit
destinations or as named ones, and whether the choice survives P5's fixup.
Whether the outline writer and reader share their traversal or grow two that can
disagree about cycles. Whether the delete-a-parent behaviour is decided or
emergent. **Mutation that must break its tests:** making the outline writer emit
a flat list must fail the nesting assertion.

#### P13c. The remaining File and Edit menu rows

**Goal.** Three small menu rows, grouped because reviewing three one-file changes
together is cheaper than three reviews.

Attach to Email (hands the file to the OS mail client, no Adobe service), Copy
File to Clipboard, and Edit Cut / Copy / Paste / Delete scoped to the active
tool.

**Rows closed.** 16 File > Attach to Email, 18 Edit > Cut / Copy / Paste /
Delete, 19 Edit > Copy File to Clipboard. **3 rows.**

**Files.** `plugins/commands-core/src/file_menu.rs`,
`crates/app/src/shell/chrome/tabs/menu.rs`, `crates/app/src/shell/chrome/commands.rs`.

**Depends on.** P0b, P3.

**Verification.**
- Attach to Email hands the OS a file path and **cannot be made to run an arbitrary command through a crafted file name**, asserted against a fixture whose name contains shell metacharacters, quotes and a newline. This is the one security-shaped row in the package and it gets a named test rather than a review-risk mention.
- Copy File to Clipboard puts a file reference on the pasteboard that a second application resolves, driven through the GPUI test platform.
- Cut / Copy / Paste / Delete dispatch to the active tool and are disabled with a reason when no tool claims them, asserted through the registry rather than a hardcoded list.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy.

**Review risk.** Whether "Attach to Email" builds a command line at all, which it
should not. Whether the Edit verbs are routed through the tool or special-cased
per tool in the shell. **Mutation that must break its tests:** passing the file
name through a shell must fail the metacharacter fixture.

### P14. `codecs-common` and `commands-core` C: image import, image export, compress

Two packages. The first draft was eight rows over three unrelated concerns, one
of which is the only destructive save path M3 ships. Compress deserves its own
review gate for that reason alone, and it turned out to be carrying two
serializer subsystems nobody had scheduled.

#### P14a. Image import and export

**Goal.** The Create-a-PDF and Export-a-PDF rows M3 owns.

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
- **Convert** (the global bar entry point) is one surface over the above, with a
  deliberately smaller target list than Acrobat's, as its row already states.

**Rows closed.** 1 Convert, 9 File > Create, 33 Create from a single image file,
36 Create from the clipboard, 53 Export to JPEG / JPEG 2000 / TIFF, 54 Export all
images. **6 rows.**

**Files.** `plugins/codecs-common/src/{lib,import,jpeg,tiff,images}.rs`,
`crates/plugin-api/src/codec.rs` (the import half of `CodecPlugin`),
`plugins/codecs-common/Cargo.toml`; **and the app surface its rows are scored
on**: `crates/app/src/shell/chrome/global_bar.rs` (row 1, the Convert entry
point), `crates/app/src/shell/chrome/tabs/menu.rs` (row 9, `File > Create`), and **the
clipboard read**, in `crates/app/src/shell/chrome/tabs/mod.rs`, for row 36, since GPUI owns the pasteboard and no plugin can
reach it. **This package owns that helper and P10 calls it**: both need an image
off the pasteboard (row 36 here, row 85 there), the landing order already puts
P14a at depth nine and P10 at depth ten, and two packages writing a pasteboard
read into one file is how the second one silently wins.

**Depends on.** P0b (the app surface above), P1 (`write_new`), P3, P12 (page
assembly).

**Why the app files are here rather than absent.** Row 1 is *"Convert (global bar
entry point)"* and row 9 is *"File > Create"*: both are named after the shell
surface they are, and row 36 is the clipboard. The first draft closed them from a
package whose Files line contained no `crates/app` path at all.

**What exists to build on.** `codecs-common` already exports PNG, SVG and text
with a tested background-worker job model, progress, cancellation and atomic
publication (C1.1). `image` is already a dependency behind the `shell` feature
and the encoder belongs in `codecs-common`, per M2's own P13 decision (M2's
package numbering, not this plan's).

**Verification.**
- Create from a 300 DPI image: the page's `/MediaBox` is the image's physical size at its own DPI, not a fixed page size, and the rendered page's pixels match the source image within the render tolerance.
- Export to each format: dimensions correct, file decodable by an independent decoder, and for JPEG a quality setting that actually changes the output size.
- Export all images: on a fixture with a known image count, the count matches and each output decodes; an inline image (`BI`/`ID`/`EI`) is either included or explicitly out of scope and stated.
- `CodecPlugin`'s import half is shaped by its three real consumers and no more.
- **`crates/app/tests/kernel_emptiness.rs` is updated in this commit.** It asserts `registry.codecs().count() == 3` under `codecs-common` (`kernel_emptiness.rs:70-80`), and this package adds JPEG, JPEG 2000 and TIFF. No package named that file, so the count would have gone red on merge. P22's `rtf.rs` changes it again and names it too.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support,codecs-common` for the Convert entry, the File > Create menu and the clipboard read, which are the surfaces rows 1, 9 and 36 are scored on; the matching clippy.
- **Corpus.** The known-image-count fixture and the CMYK fixture are `external/`, behind P1c's fetch step and its mandatory re-run.

**Review risk.** Whether image downsampling honours the image's own colour space
or converts everything to RGB, which would break a CMYK document destined for
print. Whether JPEG 2000 shipped with a patent note. Whether create-from-image
embeds an ICC profile or drops it. Whether the clipboard read is in `crates/app`
or somebody reached for a second pasteboard crate. **Mutation that must break its
tests:** fixing the created page's `/MediaBox` to A4 must fail the 300 DPI test.

#### P14b. Compress a PDF and Reduce File Size

**Goal.** The one deliberately destructive save path M3 ships, with its own gate.

**Encrypted sources are out of scope, per P1b's encrypted-source rule.** Compress on an
encrypted document would have to either pass `/Encrypt` through, producing a file
no reader opens, or strip it, producing a silently decrypted copy with the `/P`
bits gone. It is refused at the command instead, with the same reason and
milestone the edit gate uses. **Rows 51 and 52 carry that in their Notes**, which
is a parity edit the totals contract recounts.

**Scope, cut to what `cos` can actually emit, which is the correction that makes
this package buildable.** The first draft's scope said compress would "write
object streams and a cross-reference stream". **Neither writer exists and neither
was anybody's deliverable.** `writer::incremental_section` (`writer.rs:168`)
emits a classic `xref` table and a `trailer` unconditionally; there is no
`/Type /XRef` writer and no `/ObjStm` writer anywhere in the crate; and
`trailer_for_new_section` (`writer.rs:157-163`) actively strips `Type`, `W`,
`Index`, `Filter`, `DecodeParms`, `Length`, `Prev` and `XRefStm`, so an
xref-stream source gets a classic-table update appended. That behaviour is
deliberate and pinned by `crates/cos/tests/incremental.rs:152`
(`editing_an_xref_stream_file_writes_a_clean_classic_trailer`). Carrying object
streams as an implicit clause of the word "compress" would have discovered two
serializer subsystems at implementation time.

So M3's compress is: **downsample and re-encode images above a DPI threshold,
drop unreferenced objects, and write a classic cross-reference table**, through
`cos::write_new`. It is a Save As, never an in-place save, and it is the one M3
path that discards history, which the UI must say in words. **Row 51's Notes
record the cut**: object-stream and cross-reference-stream output are not in M3,
so the size reduction available is whatever image re-encoding and garbage
dropping deliver, and a document whose bulk is already object-streamed will
shrink by little. Saying that in the row is the difference between a scoped
feature and an over-promise the user discovers.

**If a later milestone wants the rest**, `/ObjStm` and `/Type /XRef` writing is
its own deliverable with its own verification, in `cos`, not a clause here.

**Rows closed.** 51 Compress a PDF, 52 Reduce File Size. **2 rows.**

**Files.** `plugins/commands-core/src/compress.rs`,
`plugins/commands-core/Cargo.toml`; new
`crates/app/src/shell/compress_dialog.rs` and
`crates/app/src/shell/dialog.rs` (the compatibility target, the destination, and
the words that say history is discarded).

**Depends on.** P0b (the dialog), P1 (`write_new`), P1b (the encrypted-source
predicate), P3, P14a (the image re-encoders).

**What exists to build on.** `cos::write_new` (P1) is the whole serializer.
`codecs-common`'s decoders and encoders are P14a's. The export job model
(background worker, progress, cancellation, atomic publication) already exists
and a long compress is the same shape.

**Verification.**
- Output opens through `cos::Document::open`, page count and extracted text unchanged, every page renders within tolerance of the source render.
- The file is smaller on a fixture chosen because it has recompressible images, and **the output carries no incremental section**, asserted by parsing rather than by counting `%%EOF`.
- **The output's cross-reference is a classic table**, asserted by parsing, which pins the scope cut above rather than leaving it as prose. A future object-stream writer will have to change this assertion deliberately.
- A document with nothing to compress **reports that it saved nothing** rather than writing a same-size rewrite, asserted on the reported result and not on the byte count.
- **Encrypted sources are refused**, per P1b's encrypted-source rule: compress reads the open document, so it is a **session-scoped** refusal through `Requirement::Command`, carrying P1b's typed reason naming M6. Asserted here as well as in P1b, because a package that can reach `write_new` and does not check is the way that rule gets a hole.
- The flattening path is unreachable from `File > Save` by any route, asserted by driving `File > Save` on a document and checking the output has an appended section rather than a rewrite.
- The UI string contains the word that tells the user history is discarded, asserted on the string.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support,commands-core,codecs-common`, one invocation rather than two, since the compress dialog and the command have to be compiled together for the dialog's assertions to reach it; plus the matching clippy.
- **Corpus.** The recompressible-images fixture is `external/`, behind P1c's fetch step and its mandatory re-run. Compress is the one destructive path M3 ships and its only correctness evidence is a fixture CI does not currently fetch.

**Review risk.** Whether the scope cut above survived contact with a reviewer who
wanted smaller files, or whether an `/ObjStm` writer appeared in this package
under another name. Whether compress silently degrades a document that had
nothing to compress. Whether the flattening path can be reached from `File >
Save` by any route. Whether the destructive wording is in the dialog or only in
the row's Notes. Whether `write_new`'s output loses anything the source had that
the parser did not understand, which is the "unimplemented means untouched" rule
and is the one place in M3 where a full rewrite can break it.
**Mutation that must break its tests:** removing the downsampling step must fail
the size-reduction assertion while leaving every correctness assertion green,
which is why both exist; emitting an incremental section instead of a rewrite
must fail the no-sections assertion.

### P15. `crates/print` A: imposition and the print-to-file backend

**Goal.** Printing as a testable pure function, before any platform API exists.

PLAN.md's M3 sentence, "`crates/print` lands with the macOS backend and the
Acrobat print dialog", names the last two of five things that have to exist.
The five are: a page-selection model, an imposition engine, a sheet renderer,
the backend trait with two implementations, and the dialog. Only the last is
app work, and only one of the two backends can be tested in CI.

The cut that makes printing verifiable:

```rust
pub struct Placement { source: PageIndex, transform: [f64; 6] }
pub struct Sheet { size: PaperSize, orientation: Orientation, placements: Vec<Placement> }
pub trait PrintBackend { fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<()>; }
```

**`Placement` has no `clip` field**, which the first draft gave it. Ruling B moved
poster and tile to M4, and poster and tile is the only thing that clips a
placement: Fit and Shrink scale, Custom scale scales, N-up scales, and an
oversized page at Actual size is bounded by the sheet, which is a property of the
sheet rather than of one placement on it. So `clip` had no M3 caller, which is
what section 7 says every abstraction must have. M4 adds it with the feature that
needs it, on the same `Placement`, which is a field addition and not a reopening.

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
97 Print as image, 98 Print to file / print to PDF. **7 rows.**

**The executable parity-totals contract has no owner, and this package is not
enough of one.** `acrobat_parity_headline_matches_every_inventory_row`
(`guarantees.rs:2959`) asserts a literal line of counters that **every row flip
changes**; nineteen packages in this plan close rows; and **no package's Files
line names `ACROBAT-PARITY.md`** - including this one, whose body says it lands
that edit. The line is also **not append-only**, so two packages flipping rows on
two branches resolve their rebase only by recounting 403 rows by hand.

So, as a rule rather than a note: **every row-closing package names
`ACROBAT-PARITY.md` in its Files line and flips its own rows in the same commit
that closes them, together with the headline recount.** The recount is one `awk`
invocation, given in section 6, and running it is cheaper than resolving a
conflict on the counter line afterwards.

**And the arithmetic in the definition of done is wrong.** P1b moves `Open an
encrypted document` (`ACROBAT-PARITY.md:460`) from `planned`/`M6` to
`partial`/`M3`, which the DoD records as a scoreboard change and then does not
count. With ruling B's two rows out of M3 and that one in, the recount is
**M3 98, M4 4, M6 45**, not the stated M3 97 / M4 4.

**This package lands ruling B's scoreboard move atomically.**
`ACROBAT-PARITY.md` still says `M3` for Booklet and Poster / tile
(`ACROBAT-PARITY.md:587-588`), and its preamble still says
`By milestone: M2 67, M3 99, M4 2, M5 52, M6 46, post-1.0 57.`
(`ACROBAT-PARITY.md:63`). Both must change in **one commit**, because
`acrobat_parity_headline_matches_every_inventory_row`
(`crates/app/tests/guarantees.rs:2959`) recounts the per-milestone totals from
the rows themselves and asserts the preamble line matches. Moving the two rows
without the headline fails the build; moving the headline without the rows fails
it too. After the move: **M3 97, M4 4**, every other milestone unchanged. The
row Notes carry ruling B's reason. PLAN.md's M3 paragraph reads "99 rows
assigned, 97 shipped once ruling B moves booklet and poster/tile to M4", so it is
already correct either side of this commit and needs no change here.

That edit is P15's rather than the orchestrator's because P15 is the package
whose scope the ruling defines, and it is called out here because
`crates/app/tests/guarantees.rs` is edited by **three** packages for unrelated
reasons and none should assume the others have been there: **P8** owns the
guarantee-2 tripwire (not P7, which the first draft of this note said), **P1c**
owns the corpus re-run assertions, and this package owns the parity totals.

Booklet (90) and Poster / tile (91) move to M4 (section 9, ruling B):
both are pure imposition math over this package's `Sheet` model, so M4 adds them
without reopening anything P15 builds. P15 must therefore leave `Sheet` and
`Placement` able to express a sheet whose placements are not a uniform grid, and
a test asserting one hand-built such sheet composes correctly is this package's
only concession to them.

**Files.** `ACROBAT-PARITY.md` (rows 90 and 91, and the `By milestone` headline,
in one commit), `crates/app/tests/guarantees.rs` (the totals contract it
recounts), `crates/print/src/{lib,job,impose,sheet,render,backend}.rs`, new
`crates/print/src/backend/file.rs`, `crates/print/Cargo.toml` (which today has
no `[dependencies]` section at all: it gains `onionskin-core`,
`onionskin-render`, `onionskin-cos`), `crates/print/tests/impose.rs`,
`crates/print/tests/file_backend.rs`.

**Depends on.** P1b (the encrypted-source predicate and its typed M6 reason,
which the Print-as-Image condition below calls), P3 (preview bytes), P6 (the
annotation filter).

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
- **On an encrypted source, print-to-file requires Print as Image**, per P1b's encrypted-source rule. Both halves asserted: with Print as Image **on** the job succeeds and the output contains **no object imported from the source**, only rasters; with it **off** the job is refused with the same typed reason naming M6. `FileBackend` otherwise places each `Placement` as a Form XObject reference, which copies the source's content streams, resources, fonts and images, so the unrastered path is a full graph copy and this condition is what stops it being a decryption bypass. Neither half alone proves the rule.
- **Corpus.** The encrypted fixture the Print-as-Image condition is asserted on is an `external/` file, so it runs behind P1c's fetch step and is named in P1c's mandatory re-run as `cargo test -p onionskin-print`. Without that this package's one security-shaped assertion runs over an absent file and reports a pass, which is the rule T9 states and which this package was violating.
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
`crates/app/src/shell/dialog.rs`, `crates/app/src/shell/chrome/tabs/dialogs.rs`
(P0a's; note the nesting, `chrome/dialogs.rs` does not exist), `crates/app/src/shell/chrome/global_bar.rs` (the Print menu entry and
the button), `crates/app/src/shell/context_menu.rs` (`CanvasContextCommand::
Print`'s `Requirement` becomes a real query), `crates/app/src/shell/chrome/
commands.rs` (the `file.print` id and its `cmd-p` default), `crates/app/Cargo.toml`
(adds `onionskin-print` behind `shell`).

**Depends on.** P0b, P15, P16.

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

**Files.** `crates/app/src/shell/chrome/{global_bar,commands}.rs` (both
pre-existing siblings, untouched by the split),
`crates/app/src/shell/chrome/tabs/menu.rs` (P0a's, nested),
`crates/app/src/shell/chrome/tabs/mod.rs` (the dirty indicator field and its
`render` call site) and `crates/app/src/shell/chrome/tabs/frame_state.rs` (the
close confirmation's state, following the merged P0b layout), `crates/app/src/keymap.rs`, `crates/app/src/config.rs` (the
recovery directory), new `crates/app/src/shell/recovery.rs`.

**Depends on.** P0b, P3.

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
- **Replaying a recovery file goes through P3's `adopt`**, and the object numbers it names do not collide with the next edit's: replay an overlay naming numbers a freshly opened document has never seen, then make one more edit, and assert it lands above all of them. Seeding from the reopened document alone collides on the first edit after recovery (T3).
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
generation previews the document as of that generation **through P3's `adopt`**,
which is the same reopen-clear-reseed the save path takes and is why previewing a
generation does not leave the session's readers, `next_number` or workers
describing a different document (T3). Rolling back truncates, through P3's
`revert_to`, with a confirmation that says what will be discarded; `revert_to`
also clears the undo stack in both directions, so the panel's own affordances are
the only way back after it.

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
`crates/app/src/shell/chrome/{rail,side_panel}.rs` (pre-existing siblings),
`crates/app/src/shell/chrome/tabs/frame_state.rs` (state). P19 depends on P0b,
so `frame_state.rs` is where its state goes; there is no before-P0b case.

**Depends on.** P0b, P1 (`sections()`), P3 (`revert_to` and preview).

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
generations. The render worker's lifetime across a truncation is **P3's**, not
this package's: it is decided in `revert_to` (generation bump before truncation)
and this package calls it rather than re-deciding it. Whether the "not ours" label is derived from something real or
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
`known-issues.md` records as "widen it after P9 lands", which now means after
P9a, P9b and P9c.

**Rows closed.** 29 Comments pane, 30 Comments list context menu, 74 Comment
properties, 75 Comments list sort/filter/reply/status/checkmark/read-unread,
78 Commenting preferences. **5 rows.** Row 74 moved here from P10: this
package's goal already said it builds "comment properties including 'make
current properties default'" and gives the right-hand side panel "its first
tool-specific content, which is the comment properties inspector", so P10 owned
the row while P20 owned its surface. It also flips two M2 `partial` rows (the quick action toolbar and the
right-hand side panel) to `implemented`, which is part of this package's
definition of done and not a follow-up.

**Files.** New `crates/app/src/shell/panes/comments.rs`,
`crates/app/src/shell/panes/mod.rs` (the `NavigationPane` variant, `PaneAction`,
`apply`, `read`, `render_body`, `accessible_body`),
`crates/app/src/shell/chrome/quick_actions.rs` (the `DeliveryStage::M3` gate),
`crates/app/src/shell/chrome/side_panel.rs`, `crates/app/src/shell/find_bar.rs`,
`crates/app/src/preferences.rs`, `crates/core/src/search.rs` (the reason type).

**Depends on.** P0b, P1b (the encrypted-source predicate whose registry-wide
sweep this package owns), P8, P9a, P9b, P9c, P10.

**What exists to build on.** The pane registration pattern is six mechanical
steps in `panes/mod.rs` and `results.rs` is the model for a live pane that
updates while work continues. `quick_actions.rs` resolves availability through
`tool_with(registry, capability)` already, so the M3 gate is one
`unavailable_stage` arm, not a rewrite.

**Verification.**
- The pane lists every annotation a corpus file already carries, not only ones Onionskin authored, which is the case a Comments pane built against the edit graph alone would miss.
- Sort by page, author and date; filter by type, author and status; each asserted by the resulting list order or membership, on a fixture with enough variety to distinguish them.
- Reply creates an `/IRT` annotation that reopens as a reply; set status writes an Acrobat-compatible `/State` and `/StateModel` annotation, which is how Acrobat models status and is not a property on the parent.
- **A reply whose `/IRT` parent is on a page P11 removed is listed as orphaned**, with its own contents and author, under a heading that says its parent is gone. Not dropped, because the reply is a comment the user wrote and this pane is where comments live; not re-parented, because there is nothing truthful to re-parent it to. This is not a dangling reference under T5's free-nothing rule (the parent survives in the file as garbage), so no validator catches it and no page-tree test sees it: it is a **reading** defect that only a pane which walks the whole document meets. Acrobat keeps a reply on its parent's page, so this arrives chiefly from foreign files, which is exactly the fixture class this package's first bullet already insists on.
- "Make current properties default" writes a preference and the next annotation created uses it, asserted end to end through the tool, not through the preference store.
- The quick action toolbar's Comment, Highlight and Draw are enabled and carry no reason string, asserted, and the assertion reads the registry rather than a list.
- Find with Include Comments finds text that exists only in an annotation's `/Contents`.
- **The encrypted-source refusal sweep**, which P1b defines and cannot assert (its own app invocation registers no plugin). This package is the last of the deep app packages, so it is the first point at which `--features commands-core,tools-organize,codecs-common` has everything registered at once.

  **Three buckets, not two, and the registry is not the whole surface.** A binary "refused or raster-only" fails today on a shipped command: `commands-core`'s `SELECT_ALL` (`plugins/commands-core/src/lib.rs:18`, reading `page_text` at line 67) puts text into a `TextSelection` and is neither refused nor raster. So each entry is **refused**, **provably raster-only**, or **reads into session state or the clipboard and writes no file**, with every entry in the third bucket naming why it is safe.

  **And the walk covers `core::Document`'s public methods that return
  graph-derived bytes**, not only `registry.commands()` and `registry.codecs()`.
  The case this sweep was written for - **attachment extraction** - touches
  `PluginRegistry` nowhere: it is a pane button going `panes/attachments.rs` to
  `canvas.rs` to `core::Document::attachment_bytes` to `attachments::read_bytes`.
  A registry walk misses it entirely. So the sweep also enumerates
  `attachment_bytes`, `page_svg`, `export_snapshot` and `page_text`, which a test
  over `core`'s public API can hold and keep holding, rather than a list of UI
  call sites nobody maintains. **Anything added to either surface that is in none
  of the three buckets fails this test by existing** - which is the only property
  that survives the next feature, after a hand list missed print-to-file at one
  pass and SVG export at the next.
- Pane rows and menu entries are in the accessibility tree; the pane's rows leave it when the pane closes.
- **Runs.** `cargo test -p onionskin-app --no-default-features --features shell,shell-test-support` and the matching clippy; `cargo test -p onionskin-core` for the widened reason type.
- **Corpus.** The fixture carrying comments Onionskin did not author is `external/`, behind P1c's fetch step and its mandatory re-run. It is the fixture that distinguishes a Comments pane from a view of the edit graph, so a skipped run leaves the package's central claim unmeasured.

**Review risk.** Whether the pane sees a comment the session just authored,
which is this package's central claim and which rests entirely on P6's reader
going through P3's `structure()` rather than `&self.cos` - a pane reading the
document as opened lists everything except what the user just did, and every
fixture-based assertion here still passes. Whether the pane reads annotations
through `core::annots` or
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

**Depends on.** P0b, P11.

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
`crates/app/src/shell/chrome/{rail,global_bar}.rs`,
`crates/app/src/shell/chrome/tabs/menu.rs`,
`crates/app/src/shell/{canvas,find_bar}.rs`, `crates/core/src/{viewport,search}.rs`,
`crates/render/src/base.rs` and `crates/render/Cargo.toml` (the fork rev, if the
Line Weights commit lands), new `plugins/codecs-common/src/rtf.rs`,
`crates/app/tests/kernel_emptiness.rs` (whose `codecs().count()` assertion `rtf`
changes, after P14a has already changed it).

**Depends on.** P0b.

Advanced Search's attachment half reads embedded files, which P13b also writes,
but it depends on that work only for symmetry of code and not for function: the
reader exists today. Stated here rather than in the line above, because section 5
derives the graph from Depends-on lines mechanically and a package named there is
an edge - naming P13b would put P22 at depth eleven and make it the sole critical
path, which is false.

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

**Derived, not drawn.** Every edge below comes from a package's own "Depends on"
line and every app-file claim from its own "Files" line. The first draft's graph
was drawn by hand and was wrong about the critical path, about which branches
were independent, and about whether `crates/app` had one occupant. Redraw it the
same way after any package's dependencies change; do not edit the picture.

For that to be mechanical, **a "Depends on" paragraph names only this package's
predecessors**, and anything else about ordering goes in a paragraph after it.
The umbrella sections (P0, P9, P13, P14) carry no edges of their own except
where P0 states its shared one.

Three roots startable on day one: **P0a** (relocating `ShellFrame`'s methods,
app only), **P1c** (the corpus CI pair, YAML and one test file) and **P1b**
(crypto, ruled in). **P1** (cos) follows P1c, because P1's own verification
sweeps `external/`, which CI does not fetch today, so P1 cannot be verified
before P1c lands even though it compiles without it. P0b follows P0a.

```
P1c corpus in CI          P0a relocate ShellFrame
 └── P1  cos               ├── P0b field restructuring  (gates every app package)
      └── P2  core::edit   └── P0c move the tests out of tabs/mod.rs
           │    └── P7  plugin-api edit contract (also P0b)
           │         └── P1b crypto, ruled in   (also P0b; the app seam)
           ├── P22 remaining shell rows (P0b only; drawn here for position)
           └── P4  core::structure
                ├── P5  core::pages (the transformation; also P1)
                └── P6  core::annots
                     └── P3  core::save  (also P1, P2; P6 is for AnnotationFilter)
                          ├── P13a document properties      (also P0b)
                          ├── P13c File and Edit remainder  (also P0b)
                          ├── P18 save/undo/dirty/recovery  (also P0b)
                          ├── P19 skins panel               (also P0b, P1)
                          ├── P15 print: imposition + file backend (also P1b, P6)
                          │    └── P16 print: macOS backend
                          │         └── P17 print dialog    (also P0b)
                          ├── P8  tools-comment: text markup (also P1c, P6, P7)
                          │    └── P9a notes, free text, overlay painters (also P0b, P6, P7)
                          │         ├── P9b ink    (also P6, P7, P8)
                          │         └── P9c shapes (also P6, P7) ──┐
                          └── P11 tools-organize (also P1b, P5, P7)
                               ├── P21 organize grid (also P0b)
                               └── P12 combine + split (also P0b, P1, P1b, P5)
                                    └── P14a image import/export (also P0b, P1)
                                         ├── P14b compress (also P0b, P1, P1b)
                                         └── P10 stamps, attach, summary
                                              │    (also P0b, P1b, P6, P7, P9c ──┘)
                                              ├── P13b bookmark + attachment authoring
                                              │        (also P0b, P3, P5)
                                              └── P20 comments pane
                                                       (also P0b, P1b, P8, P9a, P9b, P9c)
```

Reading it: **a package's position carries one edge, its parent; every other
dependency is in its `(also …)`.** Both together are the package's own Depends-on
paragraph, and the picture is checked against those paragraphs mechanically
rather than kept in step by hand - which is how the two earlier versions of it
came to be wrong about the critical path.

**Critical path: `P1c → P1 → P2 → P4 → P6 → P3 → P8 → P9a → P9c → P10 → P13b`,
eleven packages**, with three others tying at eleven: `→ P20` through the same
ten-package prefix, and both again through the **page** branch
`P3 → P11 → P12 → P14a → P10`, since a custom stamp needs P9c's shapes and the
comment summary needs P12's page assembly. **P10 is still the join**: it waits on the annotation branch and the page
branch both, and nothing downstream starts until the later lands.

**The path moved, and it moved because of a fix.** `preview_bytes`'s signature
names `AnnotationFilter`, which P6 defines, so **P3 depends on P6** and the whole
save-and-preview subtree sits one level below the annotation reader rather than
beside it. That single edge lifted P3 from four to six and everything under it
with it. It is not a scheduling choice to be undone: without it P3's central API
does not compile at P3's landing. The alternative is defining `AnnotationFilter`
in P3 and having P6 take it over, which splits one type across two packages for
no gain.

Two earlier readings of this path were wrong in the same direction, and both were
hand-drawn: the first draft called `P1 → P2 → P4 → P5 → P11 → P21` critical at
six, and the second `P1c → … → P12 → P14a → P10 → P13b` at ten. Re-derive from
the Depends-on paragraphs after any dependency changes; the picture above is
output, not input.

The picture hid it in two ways, both worth naming because they are how a drawn
graph lies. P10 was drawn as a child of P6, which is true and is not the whole
truth: P10 also depends on P12, P14a and P9c, which is what puts it at depth ten.
And P12, P13 and P14 were drawn as siblings while the text said outright that
they "are three independent branches", which is false twice over: **P14 depends
on P12**, and **P13 depends on P10, which depends on P14, which depends on P12**.
They are a chain of four, and it is the tail of the critical path.

The print chain, `P1c → P1 → P2 → P4 → P6 → P3 → P15 → P16 → P17`, is nine deep
on the same convention, so it is shorter than the critical path rather than "the
same depth" as the first draft said. It is still the natural second track, because it is the longest chain that
shares nothing with the first past P4.

**The lever, recorded rather than pulled.** P10 is at depth ten and both
eleven-length paths run through it, but a large part of why it is that deep is
`core::embedded`, the embedded-file writer, which this plan houses in P10 because
P10's attach-as-comment was its first named consumer. P13b then waits on P10 for
it, and P13b is one of the two deepest packages. **Move `core::embedded` into P6
or give it a kernel slot of its own and the critical path drops by three or
four**, because P13b would then depend on P3, P5 and that slot rather than on the
whole stamps-and-summary chain. This is an observation, not a defect: the
current placement is defensible (one writer, two consumers, and it does sit next
to the annotation that carries the file), and moving it is a scope change nobody
has asked for. It is written down so that whoever schedules M3 sees the lever
before deciding the milestone is ten deep and unavoidably so.

**The schedule's real hazard** is otherwise unchanged and is not the length: P5
and P11 sit in the middle of the critical path and are the two packages with the
highest correctness risk. Everything downstream of them, which is now seven packages
rather than one, waits on the two that are most likely to need a second review
round.

**Parallelism.**

- **P0 neither runs alone in `crates/app` nor blocks nobody.** Both of those
  claims were in the first draft and both are false. `crates/app` files
  appear in the Files line of P1b, P1c, P7, P8, P9a, P10, P12, P13a, P13b, P13c,
  P14a, P14b, P17, P18, P19, P20, P21 and P22, which is eighteen. Sixteen of them
  depend on P0b and land after it. **P1c and P8 are the two exceptions**, and for
  the same reason: their only `crates/app` files are under `crates/app/tests/`,
  which is a test target rather than shell source and which neither P0a nor P0b
  rewrites.
- P1b touches `crates/app/src/shell/chrome/tabs/mod.rs` directly for the open-time
  notice, which is the exact file P0 splits, so its app seam depends on P0b and
  lands as its own commit after it. Its `crates/crypto` and `crates/cos` work
  runs the whole milestone alongside everything else, off the critical path.
- Once P2 lands, P3, P4 and P7 are three independent branches in `crates/core`
  and `crates/plugin-api`.
- P5 and P6 are independent of each other; both need P4.
- P8 comes before all three of P9a, P9b and P9c, on purpose: it gives P6's
  appearance generator one consumer's worth of feedback before thirteen more
  arrive. **P9a then gates P9b and P9c**, not for taste but because it owns the
  `Overlay` and `OverlayPaint` seam both need: four of six `Overlay` variants
  have no painter today (`canvas.rs:1879-1895` returns `Err` for `Rect`,
  `Polyline`, `Line` and `Circle`), so an ink stroke or a rectangle drawn before
  P9a lands shows a status string instead of a preview. P9b and P9c are then
  independent of each other. P10 needs P9c's shapes before its stamps.
- P15 is windowless and in a crate nobody else touches, so it runs from the
  moment P3 and P6 land, and it is the head of the whole print track.

**Contention, and the order it forces.** M2's audit named
`crates/app/src/shell/chrome/tabs.rs` the recurring conflict point, and eighteen
M3 packages have `crates/app` files, sixteen of them under `src/shell`. After P0a splits it, each app package owns a
distinct new module, and the remaining shared files are append-only tables:
`chrome/tabs/menu.rs`'s command dispatch, `chrome/tabs/context.rs`'s
availability table, `shell/dialog.rs`'s `ShellDialog` variant list and its two
matches, `panes/mod.rs`'s `NavigationPane` list, `chrome/commands.rs`'s
`MenuCommand` table, and `Cargo.lock`. A rebase resolves all of those without
judgement.

**The paths above are P0a's nested `chrome/tabs/` tree, not sibling
`chrome/*.rs` files.** P0a shipped nested for the scope-preservation reason its
own entry gives, so `chrome/menu.rs`, `chrome/dialogs.rs` and
`chrome/context.rs` do not exist; `chrome/commands.rs`, `chrome/rail.rs`,
`chrome/side_panel.rs`, `chrome/global_bar.rs` and `chrome/accessible.rs` are
pre-existing siblings the split never touched. Any package naming a
`chrome/<concern>.rs` path is naming a file that is not there.

**`chrome/tabs/mod.rs` belongs on that list too**, which the first draft of this
paragraph left off even though contention on `tabs.rs` is P0's entire
justification. Three packages name it after the split: P1b (the open-time
encryption notice), P14a (the clipboard image helper, which P10 then calls rather
than duplicating) and P18 (the dirty indicator). What remains in `tabs/mod.rs` is
1407 production lines, the `ShellFrame` struct and its `Render` impl, so those
packages are adding **a field and a call site**, not a method to a 2500-line
`impl`. The resolution rule is the same as for the tables: a field addition and a
call site in `render` are append-only, a rebase resolves them without judgement,
and the landing order (P1b any time after P0b, then P14a, then P18) keeps them
serialised. If any of them finds itself adding a *method* to `tabs/mod.rs`, that
is the signal it belongs in one of the six concern modules instead.

**`tabs/mod.rs`'s 4669-line test module is the live conflict magnet until P0c
lands**, because every package adding a shell test edits it. That is why P0a's
parallelism claim is necessary and not sufficient, and why P0c is a package
rather than a tidy-up.

`shell/dialog.rs` deserves a note, because the first draft's reviewer suggested
reordering the app packages so that whichever one "introduces the dialog host
shape" lands first. **No package introduces it**: `ShellDialog`
(`crates/app/src/shell/dialog.rs:27`) already exists with a modal host, four
variants and its `accessible`/`render` pair, and `preferences_dialog.rs` is the
worked example. Every new dialog is one more variant in an append-only enum, in
the same class as the other five shared tables. There is nothing to sequence
around.

**The app landing order is dependency depth**, so no package rebases over one
that later moves beneath it:

1. **P7** (depth 4). It deletes `context_menu.rs`'s private `Requirement` and
   moves it into `plugin-api`, which every later availability query reads.
2. **P1b's app seam** (depth 5), whenever convenient after P7; it is one notice
   and one gate in one file.
3. **P13a**, **P13c**, **P18** and **P19** (all depth 7). P18 first among them:
   the dirty state, the saved mark's UI reading and four global commands that
   every other app package reads or extends. A save and undo model rebased under
   twelve branches is worse than twelve rebasing under it. P19 next, as the first
   consumer of the rail-plus-side-panel surface P20 also uses.
4. **P9a**, **P12** and **P21** (depths 8, 8 and 8). **P9a first among them**: it
   is the only app package that changes `plugin_api::Overlay` and adds four
   `OverlayPaint` variants with their painters, and it removes `map_overlay`'s
   `Err` arms, so anything else touching `canvas.rs` should rebase over it rather
   than under it. P12 then adds the first new `ShellDialog` variants and P21 the
   second pane-shaped surface.
5. **P14a** and **P17** (both depth 9).
6. **P10** and **P14b** (both depth 10).
7. **P13b** and **P20** (both depth 11). P20's is the one that also flips the
   quick action gates and owns the registry-exhaustive encryption sweep.
8. **P22 last**, out of depth order and deliberately: it is at depth three, but
   it is nine small changes across many files, which is exactly the shape that
   rebases cleanly under everything and painfully over anything.

**P1b's app seam is step 2 above**, at depth five, and can land any time after
P0b and P7. It is one notice and one gate in one file and it belongs to a package
that is otherwise off the critical path entirely, so sequencing it with the
others would couple the crypto track to the app track for no benefit. It is named
here so that "the app landing order" is not read as a list of everything that
touches `crates/app`.

Whoever rebases re-runs `cargo test -p onionskin-app --no-default-features
--features shell,shell-test-support` rather than trusting the merge, which is
the same rule M2's plan set and for the same reason.

**One split for ordering.** The corpus reaching CI is P1c's, and it is a day-one
root that lands **before P1**, because P1's own verification sweeps `external/`.
P4 then adds the `corpus/tagged/` derivation on top of the `verapdf` set P1c
already fetches, and adds its own suite to P1c's mandatory re-run list. Neither
is a "later package" problem: a suite that skips its corpus silently is exactly
how guarantee 6 stayed green and unmeasured for a milestone (section 8, item 2),
and this plan was in the middle of repeating it for twelve packages.

---

## 6. Parity row ledger: all 99 rows M3 was given, 97 it ships

Every M3 row belongs to exactly one package or is deferred with a reason. Row
text is abbreviated; the source of truth is `ACROBAT-PARITY.md`, and the M3 rows
are recoverable with:

```sh
awk -F'|' '/^\|/ {gsub(/^ +| +$/,"",$4); if ($4=="M3") print $2}' ACROBAT-PARITY.md
```

| # | Parity section | Row | Package |
|---|---|---|---|
| 1 | Application shell | Convert (global bar entry point) | P14a |
| 2 | Application shell | Undo / Redo icons on the global bar | P18 |
| 3 | Application shell | Save / Save As in the global bar | P18 |
| 4 | Application shell | Print button | P17 |
| 5 | Application shell | Home view: Starred | P22 |
| 6 | Application shell | Manage Tools / customize the tool rail | P22 |
| 7 | Application shell | Autosave and crash recovery | P18 |
| 8 | Application shell | Window menu | P22 |
| 9 | Menus | File > Create | P14a |
| 10 | Menus | File > Save | P18 |
| 11 | Menus | File > Save As | P18 |
| 12 | Menus | File > Save as Other | P13a |
| 13 | Menus | File > Revert | P18 |
| 14 | Menus | File > Properties | P13a |
| 15 | Menus | File > Print | P17 |
| 16 | Menus | File > Attach to Email | P13c |
| 17 | Menus | Edit > Undo / Redo | P18 |
| 18 | Menus | Edit > Cut / Copy / Paste / Delete | P13c |
| 19 | Menus | Edit > Copy File to Clipboard | P13c |
| 20 | Menus | Advanced Search > include attachments | P22 |
| 21 | Menus | Advanced Search > document-property criteria | P22 |
| 22 | Menus | View > Automatically Scroll | P22 |
| 23 | Menus | View > Show/Hide > Line Weights | P22 |
| 24 | Menus | View > New Window | P22 |
| 25 | Navigation panes | Bookmarks: create, rename, nest, destination, delete | P13b |
| 26 | Navigation panes | Attachments: add and delete | P13b |
| 27 | Navigation panes | Bookmarks pane context menu | P13b |
| 28 | Navigation panes | Attachments pane context menu | P13b |
| 29 | Navigation panes | Comments pane | P20 |
| 30 | Navigation panes | Comments list context menu | P20 |
| 31 | Viewer and reading | Copy with formatting / Export selected text | P22 |
| 32 | Viewer and reading | Initial View settings | P13a |
| 33 | Create a PDF | Create from a single image file | P14a |
| 34 | Create a PDF | Create from multiple files | P12 |
| 35 | Create a PDF | Create a blank page | P11 |
| 36 | Create a PDF | Create from the clipboard | P14a |
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
| 51 | Compress a PDF | Compress a PDF | P14b |
| 52 | Compress a PDF | Reduce File Size | P14b |
| 53 | Export a PDF | Export pages to JPEG / JPEG 2000 / TIFF | P14a |
| 54 | Export a PDF | Export all images in a document | P14a |
| 55 | Add comments | Sticky note | P9a |
| 56 | Add comments | Highlight text | P8 |
| 57 | Add comments | Underline text | P8 |
| 58 | Add comments | Strikethrough text | P8 |
| 59 | Add comments | Insert text at cursor (caret markup) | P8 |
| 60 | Add comments | Replace text | P8 |
| 61 | Add comments | Add text comment (typewriter) | P9a |
| 62 | Add comments | Text box | P9a |
| 63 | Add comments | Callout | P9a |
| 64 | Add comments | Draw freehand (ink) | P9b |
| 65 | Add comments | Erase ink | P9b |
| 66 | Add comments | Line | P9c |
| 67 | Add comments | Arrow | P9c |
| 68 | Add comments | Rectangle | P9c |
| 69 | Add comments | Oval | P9c |
| 70 | Add comments | Polygon | P9c |
| 71 | Add comments | Connected lines (polyline) | P9c |
| 72 | Add comments | Cloud | P9c |
| 73 | Add comments | Attach a file as a comment | P10 |
| 74 | Add comments | Comment properties | P20 |
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

**Every row-closing package names `ACROBAT-PARITY.md` in its Files line** and
flips its own rows in the commit that closes them, together with the `By
milestone` headline the executable contract recounts
(`guarantees.rs:2959`). That line is **not append-only**: two packages flipping
rows on two branches resolve their rebase only by recounting 403 rows, so the
recount `awk` below is part of landing a package rather than a periodic chore.
Nineteen packages close rows and none named the file.

**Totals**, recounted from the table above rather than carried forward. P8 5,
P9a 4, P9b 2, P9c 7, P10 9, P11 9, P12 6, P13a 3, P13b 4, P13c 3, P14a 6,
P14b 2, P15 7, P16 1, P17 7, P18 7, P20 5, P21 1, P22 9, moved to M4 2.
**Sum 99, of which M3 ships 97 and M4 gains 2.** P0a, P0b, P1, P1b, P1c, P2, P3,
P4, P5, P6, P7 and P19 close zero rows and are named against them in section 4.
**Thirty-two packages**, up from the first draft's twenty-four: P0 split into
P0a, P0b and P0c, P9 into three, P13 into three, P14 into two, and P1c is new.

**Every row is owned by the package that builds the surface it is scored on.**
That is a rule rather than an observation, and applying it moved four things.
Row 74's surface is P20's side-panel inspector, so the row moved from P10 to P20.
Rows 38, 39, 40 and 46 are a dialog, so P12 gained app files and a P0b dependency
rather than closing them from `plugins/commands-core` alone. Rows 1, 9, 36, 51
and 52 are a global-bar entry, a menu, a clipboard read and a destructive Save As
dialog, so P14a and P14b gained the same. Rows 83, 84 and 85 are a dialog and a clipboard
read, so P10 gained the same. Left alone, thirteen rows would have been marked
`implemented` against surfaces no package had scheduled, which is the
dishonest-scoreboard failure `ACROBAT-PARITY.md` exists to prevent.

**Rows outside M3's 99 that M3 changes**, which the scoreboard update has to
carry and which nobody should discover at review time:

- Three M2 rows flip from `partial` to `implemented`: the right-hand side panel
  (P20 gives it tool content), the quick action toolbar (P20 opens the Comment,
  Highlight and Draw gates), and the Page Thumbnails pane context menu (P21
  enables its M3 entries; Crop Pages stays disabled on M5).
- The Layers pane context menu's `Properties` entry goes live with P13a.
- `Open an encrypted document` moves from M6 to `partial` at M3, per ruling A,
  with Notes naming the read-only scope, the M6 write path and the accepted
  regression for documents whose `/P` bits allow modification.
- Rows 90 and 91 move from M3 to M4, per ruling B, with the reason recorded.
- The September 10 reconciled `implemented` count is 52, up from the planning
  baseline of 49; M3 adds only the rows its completed user paths support. The
  executable totals contract in `crates/app/tests/guarantees.rs` recounts it, so
  a mismatch fails the build rather than living in the preamble.

---

## 7. YAGNI ledger and deferrals

Every abstraction M3 introduces names the caller that exists in M3.

| Introduced | Consumer that exists in M3 |
|---|---|
| `cos::Document::next_object_number` | `core::edit`'s reservation counter |
| `cos::Document::sections` | P19's skins panel, P3's `revert_to` |
| `cos::Document::write_new` | six callers: P12 combine and split, P11 extract, P14a create-from-image, P14b compress, P10's comment summary, P15's `FileBackend` |
| `cos::{PendingEdit, Document::section_for, Document::save_overlay_to_path}` | P3's save and P3's `preview_bytes`, which are the same call with the same argument; `incremental_section` and `save_to_path` are re-expressed as callers so there is one serializer, not two |
| cos's save-time reference gate on `section_for` | every save in M3, as the cheap guard that T5's free-nothing rule was not broken |
| `cos::Document::audit_references` | the verification of P3, P5, P7, P11, P12, P13b, P14a and P14b - eight packages, and the definition of done says the same eight |
| `core::edit::{Overlay, History, DocumentEdit}` | every tool and command from P8 to P14b |
| `core::preview_bytes(filter)` | the canvas (committed edits, unfiltered) and P15's print filter, which is row 94. There is no third consumer: the "hide all comments" view the first draft named is not an M3 row (T7). |
| `core::generations` and `revert_to` | P19's skins panel, P18's `File > Revert` |
| `core::structure` | P5's page operations, P6's annotation authoring, and M5's guarantee 8 |
| `core::pages::rewrite_page_tree` | all nine P11 operations, P12's combine and split |
| `core::pages::import` | P11's insert and copy-between-documents, P12's combine |
| `core::pages::assemble` | P12, P10's summary, P11's extract, P14a's create-from-images |
| `core::annots` and its appearance generator | P8's five tools, P9a/P9b/P9c's thirteen, P10's stamps |
| `core::embedded` (the embedded-file writer) | P10's attach-as-comment, P13b's Attachments pane |
| `core::Document::edits()` | every M3 tool and command, reached through the `&mut Document` `ToolCtx` and `CommandCtx` already carry; there is no second field, because `EditSession` is a field of `Document` and two mutable borrows of it do not compile |
| `Requirement::Command` in `plugin-api` | the **seven** `Requirement::Milestone` arms in `context_menu.rs`'s `requirement()` (lines 115, 117-122), of which **four name M3** and become registry queries; the other three name M5 |
| `plugin_api::Overlay::Ellipse` (replacing `Circle`) and `Polyline { closed }` | P9c's Oval, Polygon and Cloud. `Circle` had no consumer at all, so this is a replacement, not an addition |
| `canvas::OverlayPaint::{Rect, Polyline, Line, Ellipse}` and their painters | P9a's text box and callout, P9b's ink, P9c's seven shapes. Today four of six `Overlay` variants render as a status string instead of a preview |
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
  content stream, and P9a must not blur that.
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
| Booklet (row 90) and Poster / tile (row 91) | **Moved to M4**, per ruling B. Both are pure imposition math over P15's `Sheet` model and land alongside M4's CUPS and Windows backends. | **P15 moves both rows to M4 in `ACROBAT-PARITY.md` with the reason, in the same commit as the `By milestone` headline**, since `guarantees.rs` recounts the totals from the rows. PLAN.md decision 13 and the `crates/print` crate entry are already corrected, at `989d8a7`. |
| Copy With Formatting to the clipboard (half of row 31) | Deferred. `gpui::ClipboardEntry` has only `String` and `Image`, so a rich-text flavour needs a fork addition (section 8, item 12). Export Selection As ships; the row is `partial`. | Add the cut to row 31's Notes; open a fork issue for a custom pasteboard flavour. |
| `New Bookmarks From Structure` | Not M3. Needs the tagged tree, and its parity row already puts it at M6. P4 makes it cheap when it arrives. | None; the row is already correct. |
| JPEG 2000 export, if no acceptable pure-Rust encoder exists | Row 53 ships `partial` naming JPEG and TIFF, with the reason. A C dependency is not an acceptable resolution (decision 4). | Split row 53's Notes if it happens; decided in P14a, never carried as both outcomes. |
| Line Weights, if the hayro fork commit does not land | The menu item stays disabled with a reason and the row moves to M4. M2's review already recorded the correct semantics (constant hairline width, not a width floor) so M3 does not repeat the wrong analysis. | Only if it happens; decided in P22. |
| Inline images (`BI`/`ID`/`EI`) in Export all images (row 54) | Decided in P14a and stated either way. Including them is a content-stream walk, excluding them is a documented scope line. | Whichever P14a takes goes in row 54's Notes. |
| `corpus/tagged/` beyond what P4's invariant needs | M5 owns the guarantee-8 fixture set. P4 populates enough to exercise the invariant and says how many files that is. | `corpus/README.md`'s guarantee-8 row is updated from "not built yet" to what P4 built. |

---

## 8. Candor: where PLAN.md's M3 text does not survive contact with the code

Each item needed a plan edit or an explicit acceptance before implementation
started. **Six of them landed at `989d8a7`** and are marked so rather than left
reading as pending, because a candor list that describes fixed things as broken
is the same defect it exists to prevent. Items 1, 3, 4, 5, 6 and 8 are done in
PLAN.md; item 2 is done in `guarantees.rs`; item 7 is half done. Evidence is
cited, and every "landed" claim below was re-checked against `main` at `fa5a194`
this session rather than carried forward.

1. **LANDED at `989d8a7`. The M3 paragraph named seven deliverables while the
   scoreboard put 99 rows in M3.** PLAN.md's M3 paragraph now covers the whole
   milestone, including stamps, document properties, bookmark and attachment
   authoring, compress, image creation and export, autosave, the Window menu,
   Line Weights and the Advanced Search extensions. The original finding, kept
   for the reasoning:

   This is M2's candor item 12 repeating: the paragraph names
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

2. **LANDED at `0f1c295`, and this item now describes what M3 inherits rather
   than what is missing.** The draft of this item said guarantees 1, 2 and 6 were
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
     `cos::set_info_field`. **P8** owns the new test and the tripwire edit, and
     `crates/app/tests/guarantees.rs` is named in P8's file list because nobody
     currently owns editing it. P8 rather than P7 because P8 ships the highlight
     the test drives, and because P8 already depends on P7, so putting it in P7
     would have been a cycle rather than a schedule.
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

3. **LANDED at `989d8a7`.** "What does not transfer" item 1 now reads "Dropping
   the overlay node is the common case, not the rule: an edit that overwrites an
   already-overlaid object has to put back what was there ... The stack holds a
   before and an after per changed object." The original finding:

   "What does
   not transfer" item 1 and parity row 17 both say undo is "dropping edit-graph
   overlay nodes". Dropping a node is correct only when the edit created the
   node. An edit that overwrites an object a previous edit already overlaid (a
   highlight recoloured twice) must restore the previous overlay state, and an
   edit that deletes an object present in the original has no node to drop.
   T2 resolves it with before-and-after states; the plan text should say so, and
   P2's property test covers exactly these two cases by name.

4. **LANDED at `989d8a7`.** The core invariant now carries an "Undo is not
   truncation" bullet that separates the two by name. The original finding:

   Point 2 of the
   invariant says generations "roll back by truncation", and the milestone list
   says M3 delivers "undo/redo". Read together they imply truncation is undo,
   which is wrong after any save that is not the last thing in the file, wrong
   after a Save As, and destructive of generations the user kept. T1 separates
   them into a session-scoped edit stack and a named, explicit generation
   rollback. PLAN.md carries that distinction now.

5. **LANDED at `989d8a7`.** The core invariant now carries "A document we author
   has nothing beneath it". The original finding:

   Combine,
   split, extract, create-from-image and the comment summary all produce a new
   file with nothing underneath to append to, and cos has no write-from-scratch
   API (its charter names a `flatten` that does not exist). T8 proposes the
   clause: a document Onionskin authors is written complete on its first save
   and the invariant applies from there. PLAN.md carries that clause now.

6. **LANDED at `989d8a7` in PLAN.md; the code half is still P4's.** The M3
   paragraph now ends "M3 also owes the tagged structure tree its reader, its
   maintenance hook and its invariant (decision 12), three milestones before
   guarantee 8." The repository-wide grep still returns nothing, which is what
   P4 changes. The original finding:

   A repository-wide grep for
   `StructTreeRoot`, `ParentTree`, `StructParents`, `MarkInfo` and `MCID`
   returns one incidental comment in `crates/content/src/interpret.rs:33`.
   PLAN.md assigns guarantee 8 to M5 and says nothing about M3's obligation,
   which reads as permission to ignore it. T6 states the obligation and P4 sizes
   it. The cost of not doing it now is not a delayed guarantee; it is M5
   inheriting shipped M3 builds that broke the tree, plus a revisit of every M3
   tool.

7. **HALF LANDED. PLAN.md is fixed; the ledger entry is not, and its wording now
   needs to change more than the first draft said.** PLAN.md's `core` crate entry
   now reads "organize, combine, split and redact all need the same repairs, so
   no plugin owns them". `known-issues.md` and `document.rs:868` still say "M3's
   `tools-organize` has to fix up the page tree itself", and under T5's
   free-nothing rule that is not merely mis-owned but **describes work M3 does
   not do**: the fix-up is not "after `delete_object`", it is "instead of it",
   and `cos::delete_object` gains no M3 caller at all. The ledger entry should
   say that, and the orchestrator lands it. The original finding:

   `known-issues.md` and `document.rs:868` both say "M3's
   `tools-organize` has to fix up the page tree itself". A plugin is the last
   place that knowledge should live: `commands-core`'s combine and split need
   exactly the same fixups, and so will `redact` at M5. This plan puts the
   transformation in `core::pages` (P5) and leaves `tools-organize` as the
   surface that calls it, and adds the guard in cos (P1) so the mistake is loud
   wherever it is made. The ledger entry's wording should move with it.

8. **LANDED at `989d8a7`.** PLAN.md's M3 paragraph now says `crates/print`
   "lands in the five parts its crate entry names, with the print-to-file backend
   first". The original finding:

   The five are a page-selection model,
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

15. **PLAN.md's workspace layout puts the print command in `commands-core`; this
    plan puts it in `crates/app`, and `crates/app` is right.** PLAN.md's
    `plugins/commands-core` crate comment lists its contents as "menu commands:
    Combine files, compress/flatten export, document properties, bookmark and
    attachment authoring, generation rollback, **print (via `crates/print`)**".
    P17 puts `file.print` in `crates/app/src/shell/chrome/commands.rs` and its
    entry points in `global_bar.rs` and `context_menu.rs`, with no
    `commands-core` file at all.

    The plan is right and PLAN.md's line is wrong, for the reason PLAN.md itself
    gives four lines earlier in the `print` crate entry: "Only the dialog is app
    work." Invoking print **is** opening the dialog, and a `CommandPlugin`
    cannot: no crate outside `crates/app` may import GPUI, so a command living in
    `commands-core` could do nothing but ask the shell to open a dialog, which is
    a shell command with an extra hop. The same argument applies to two more
    entries on that line: **generation rollback** is P19's skins panel, which is
    app surface, and the **Combine** and **compress** file-list dialogs are P12's
    and P14b's app files. `commands-core` keeps the document-level work behind
    all of them.

    **Resolution: a PLAN.md edit for the orchestrator**, narrowing that crate
    comment to the commands that act on the document rather than on a window.
    Not made here; recorded here so it is not discovered as a contradiction at
    P17's review.

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

### Ruling B. M3 ships twelve of the fourteen printing rows.

**Ruled: booklet and poster / tile move to M4.** M3 ships the `Sheet` imposition
model and the other twelve rows.

**The count, corrected, because the ruling's own arithmetic was wrong and the
reasoning below is not.** `ACROBAT-PARITY.md`'s Printing section has **eighteen
rows, fourteen of them M3**; the other four are Print on Linux (M4), Print on
Windows (M4), Print colour PDFs (out of scope) and Print a PDF Portfolio
(post-1.0). So the denominator is fourteen, not sixteen, and moving two leaves
**twelve**. Three more print-shaped rows live in other parity sections and are
all P17's (4 Print button, 15 File > Print, 77 Print comments), so the print
feature as a whole is seventeen rows of which fifteen ship: P15 7 + P16 1 +
P17 7. Recount either way with the `awk` snippet in section 6; do not carry the
number forward.

`crates/print`'s doc comment names page ranges, scaling, N-up, booklet and
print-as-image. Some of what the Printing section asks for is deep.

**The defensible M3 subset**, which is what P15 through P17 as written deliver:
the imposition engine, page range and subset (all, current, custom, odd and
even), page sizing and handling (Fit, Actual size, Shrink oversized, Custom
scale), N-up, orientation, duplex, Comments and Forms, Page Setup, Print as
Image, Print to File, the print dialog itself with a live preview, and the
Advanced Print Setup dialog carrying only its two in-scope items. Twelve of the
Printing section's fourteen M3 rows, plus the three print rows that live in other
sections.

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
rather than the silent cut.** PLAN.md decision 13 was corrected at `989d8a7`
("Booklet and poster/tile are imposition over the same sheet model and ship with
the later backends, not with the first one"), along with the `crates/print` crate
entry, so the deferral is recorded where the promise was made. What is still
outstanding is `ACROBAT-PARITY.md`, which still reads `M3` for both rows; that is
P15's atomic edit. The road
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

- **All 97 `ACROBAT-PARITY.md` rows M3 ships** are `implemented` or `partial`
  with a stated cut in their Notes; rows 90 and 91 are at M4 with ruling B's
  reason recorded. Ninety-seven, not ninety-nine: the milestone was given 99 and
  ruling B moved two, so M3 is 97 and M4 is 4. The scoreboard's executable totals
  contract (`acrobat_parity_headline_matches_every_inventory_row`,
  `guarantees.rs:2959`) recounts and passes, which means the "By milestone" line
  in `ACROBAT-PARITY.md`'s preamble moved with the two rows.
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
- **No crate outside `crates/app` imports GPUI, asserted by a test this
  milestone has to write**, because **there is no such test today**: the only
  artifacts are three doc comments. The DoD asserted "the existing test" two
  bullets after congratulating itself for catching exactly this class, which is
  the failure mode being loud about a failure mode does not prevent. `crates/print`
  imports AppKit directly and no GPUI, asserted by the same new test, which reads
  each kernel crate's manifest and its resolved dependency tree rather than
  grepping sources.
- **Guarantees 1, 2 and 6 pass at the level the guarantee means.** All three
  already run (`0f1c295`); what M3 owes them is the level, not the switch.
  Guarantee 2 is driven by an edit a **tool** made through `core`, which is P8's
  test in `crates/app/tests/`, and its tripwire in `guarantees.rs` names that
  test. Every corpus M3 makes load-bearing has P1c's fetch-and-re-run pair, so
  none of them can repeat guarantee 6's vacuous pass.
- P7's registry-exhaustive property tests pass over the real `build_registry()`:
  every registered tool and command is undoable, serializes to a section a fresh
  parse accepts, survives degenerate documents, and is deterministic.
- P4's structure invariant is clean after every M3 edit on every tagged fixture,
  and it **fails** on the deliberately broken fixture in the same suite.
- `audit_references` is clean on the output of every operation in **P3, P5, P7,
  P11, P12, P13b, P14a and P14b** over every named fixture, and `section_for`'s
  cheap gate accepts every section those packages emit.
- P16's manual print acceptance script has run on macOS and its result is
  recorded in that package, including what it failed at. **M3 is not done until
  it has.**
- Every new control appears in the AccessKit tree with a real label and state,
  and every dismissible surface removes its controls from the tree rather than
  leaving invisible tab stops. The macOS accessibility probe stays a required CI
  gate.
- Every new keyboard route is proven with `cx.simulate_keystrokes` on a real
  window.
- `known-issues.md` has every M3-deadline entry it actually contains removed or
  narrowed. **Two of the four the first draft named do not exist**: there is no
  Line Weights entry and no `Include Comments` entry in that file today, and
  listing them as things to remove would have produced a definition of done with
  two items nobody could satisfy. The two that do exist are the **cos
  dangling-reference entry** (`known-issues.md`, "M3's `tools-organize` has to
  fix up the page tree itself"), whose wording changes per section 8 item 7
  because under T5's rule the fix-up replaces `delete_object` rather than
  following it, and the **`SearchResult::Unavailable::reason` widening**, nested
  in the P10/foundation bullet and marked "widen it after P9 lands". The Line
  Weights and `Include Comments` deadlines live in their menu items' asserted
  disabled reasons, not in the ledger, and P22 and P20 flip those.

  Plus the new entries M3 earns: the encryption class split and ruling A's
  accepted regression for documents whose `/P` bits allow modification, the
  `/OC` annotation-visibility consequence, T5's free-nothing rule and what it
  means for "delete page" not being removal, and any deferral from section 7.
- P1b's measurement table is committed under `docs/evidence/` and its tally is
  asserted by a test, so the orchestrator can land the `known-issues.md`
  encryption entry from a number that cannot go stale silently.
- The dogfood claim carries its caveats. "M3 edits PDFs non-destructively" is
  stated with what it cannot do attached: no text editing, no form filling, no
  redaction, no signing, no booklet or poster printing, and, on an encrypted
  document, no editing at all even where its permission bits would allow it. A
  claim that omits them is a defect, not a simplification.
