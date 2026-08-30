# M2 implementation plan: the viewer

Status: draft, awaiting adversarial review. No code written.

M2 turns three isolated M1 spikes into one application: open a PDF (repaired
if needed), render it, navigate it, search it, select text in it, wearing the
Acrobat shell, with an accessibility tree and two enforced performance
budgets. It ships as a usable fast PDF viewer and as the registry's proof of
shape.

Scope boundary, held throughout: `crates/mcp` and `crates/cli` are M4 and
later and get no code here. `cargo test -p onionskin-app --no-default-features`
(guarantee test 5) must stay green at every commit, and no crate outside
`crates/app` may import GPUI.

---

## 1. Ground truth: what exists today

Read before reviewing the packages. Every claim below was checked against the
tree, not against PLAN.md.

| Component | State | The part that matters for M2 |
|---|---|---|
| `crates/cos` | real, 3300-file corpus green | `Document::open` / `open_repairing` / `open_path*`, `get`, `resolve`, `catalog`, `page_count`, `first_page`, `escalate_to_scan`, `set/add/delete_object`, `incremental_section`, `save_to_writer/path/vec`. Filters are `pub(crate)`. No indexed page accessor. `Document` is `!Send` because of one `Rc<ObjectStream>` (`document.rs:68`). Encrypted files are refused with `Error::Encrypted`. |
| `crates/content` | real, oracle-tested | `page`, `extract_page`, `extract`, `content`, `search`/`search_flattened`/`flatten`, `PageText`/`TextRun`/`Glyph`/`Mapping`/`ByteProvenance`, `SearchOptions{case_sensitive, whole_word}`. Owns a private page-tree walk (`page.rs:87 descend`) and a private 524-line filter decoder (`filter.rs`). `Page` carries `media_box` and `rotate`, **no `crop_box`**, and `base_ctm` translates by the media box only. Search is per page; there is no document-level search. |
| `crates/render` | real spike, ratified | `Document::open(Vec<u8>)` / `render_page(index, zoom)`, `BaseRaster`, `PageRender{raster, warnings}`, `TileCache{new, tile(&mut self), damage, add_overlay, page_image, composites}`, `Overlay::{Highlight, Ink}`, `TILE_SIZE = 256`. No eviction, no trait, one `RenderCache::new()` per `render_page` call, `..Default::default()` on `InterpreterSettings` so annotations render. |
| `crates/core` | a unit struct | `pub struct Document;`. Its plan-assigned job (session, page model, edit graph, history, selection, save, tagged-PDF tree) is entirely unbuilt. `plugin-api` already hands `&mut core::Document` to every tool, so the signature is pinned. |
| `crates/plugin-api` | real | `ToolPlugin`, `CommandPlugin`, `Command`, `PluginRegistry`, `PointerInput`, `PagePoint`/`PageRect`/`PageQuad`, `Overlay`. `ToolCtx` carries **only** `doc: &mut Document`. |
| `crates/app` | registry assembly plus two spike binaries | `build_registry`, `boot_summary`, `--headless-boot`. `shell_spike.rs` (pan/zoom/pinch over a fake page, unit-tested view math), `a11y_spike.rs` (`SubclassingAdapter` on gpui's NSView). Everything GPUI sits behind the `shell` feature, which **CI never builds**. |
| `plugins/tools-basic` | empty manifest | `register` is a no-op. No tool is registered anywhere in the workspace. |
| corpus | 3300+ files | `seeds/` committed, `malformed/` and `external/` generated or fetched. No 1000-page file. `external/hayro-corpus/0041790.pdf` is the 738 ms transparency-heavy page from the render spike. |

Facts verified in the pinned hayro fork (`33d9caf`) that shape several
packages:

- `PdfData` wraps `Arc<dyn AsRef<[u8]> + Send + Sync>` and `From<Arc<T>>`
  exists, so cos and hayro can share one byte buffer without a copy.
- `Page::initial_transform(invert_y)` is **public** and returns the exact
  page-space to render-space transform, crop box and `/Rotate` included.
  `render_dimensions()` is the intersected crop box, rotated.
- `InterpreterSettings::render_annotations: bool` already exists and defaults
  to `true`. The render spike's "render_annotations knob" to-do is a local
  one-field change, not upstream work.
- OCG visibility **is** honoured, but `OcgState::from_catalog` is built inside
  `Context::new_with` from the document's own `/OCProperties /D`. There is no
  override on `InterpreterSettings`, so a viewer cannot toggle a layer.
- `hayro-svg` 0.7.0 exists in the same repo, so SVG page export is available
  from the fork we already pin.

---

## 2. The nine tensions, resolved

**T1. Three `Document` types, one session.** `core::Document` becomes the
single session and owns, for M2: the shared byte buffer, the `cos::Document`,
a lazily populated page model (geometry per page), selection state, search
state, and the render worker handle. It does **not** get an edit graph,
history, or a save path in M2: the viewer never mutates the document, and
building an edit graph with no editing plugin to exercise it is the exact
abstraction-without-a-consumer this plan is supposed to refuse. M3 adds them.
`render::Document` stays what it is (display only) and is owned by the render
worker, never by the session. `content` keeps operating on `cos::Document`,
reached through the session.

On the double parse: hayro parses the file independently of cos and will keep
doing so. **Accepted for M2, with two mitigations and one measurement.**
(a) The *bytes* are shared, not duplicated: the session reads the file once
into an `Arc<Vec<u8>>`, hands it to `cos` through a new
`BytesSource::from_shared` and to hayro through `PdfData: From<Arc<T>>`.
(b) The measurement is a bench asserting `Pdf::new` on the 1000-page bench
file stays inside its share of the open budget; the M1 spike measured
`Pdf::new` at ~1.4 us/page, so 1000 pages is ~1.4 ms, and the bench pins that
rather than trusting it. Feeding hayro from cos's parsed objects would mean
reimplementing hayro's reader against our object model, which is the "never
re-implement full PDF interpretation" line in decision 4. Reject.

Consequence to state plainly: a document cos refuses (`Error::Encrypted`,
`Error::Unrecoverable`) cannot be viewed even when hayro could render it. See
the candor list, item 4.

**T2. The cos API work items land first.** `decode_stream` and the indexed
page accessor are package P1, before anything else, because `content` deletes
524 lines of duplicated filter code against them, `core`'s page model is built
on the accessor, and both are cheaper to change now than after three consumers
exist. P1 also carries `from_shared` (T1), because all three are one crate,
one review, one commit series.

**T3. Who owns the render thread: `core`.** The kernel may not import GPUI;
a worker living in `app` would be untestable headless, and M4's MCP wants the
same page delivery. So `crates/core` owns it. The worker thread owns its own
`render::Document` built from the *same* `Arc<Vec<u8>>`, so **no cos state
crosses the render thread boundary at all in M2**, and hayro's `Pdf` is
`Send + Sync` (asserted, not assumed, in P4's verification). The same pattern
covers P9's search worker: it constructs a `cos::Document` inside its own
spawned closure from the same shared bytes, so no document ever moves across a
thread and nothing in M2 needs `cos::Document: Send`. The
`RenderCache` self-reference problem disappears with it: the cache is created
inside the worker's loop scope, where the `Pdf` provably outlives it, so no
yoke or ouroboros is needed.

Progressive first paint needs a correction the plan text does not carry. The
render spike measured per-page cost as *interpretation*, not rasterization
area, so rendering coarse-then-fine does not make first paint faster. First
paint therefore comes from something that is not a hayro render of that page:
a correct-size page rectangle at the document's background, plus the most
recent raster for that page at any other zoom, rescaled, if one is cached.
The full raster replaces it when the worker delivers. That is what the
200 ms budget is measured against.

**T4. Viewer layout is kernel code; only painting and input routing are GPUI.**
The page layout, scroll metric, zoom modes and viewport state live in
`crates/core` (GPUI-free, headless-testable), and `app`'s canvas is a thin
GPUI element over them. Three consequences, all of which the first review
round demanded: the moved `shell_spike` view-math tests get a home that CI
already builds, the scroll bench in `crates/core/benches/` can compile at all
(`core` cannot depend on `app`), and the canvas package shrinks to something
reviewable.

The shell therefore splits into: **P6a** layout and viewport math in `core`,
**P6b** the GPUI canvas and input routing, **P7a/P7b/P7c** frame chrome in
three staged review points, **P8** navigation panes, **P11** commands, keymap
and preferences. Pixel parity is **not** verifiable in M2: `parity/reference/`
is empty and capturing it needs Acrobat driven on screen (Legal posture rule
4, local only). M2 verifies *layout* parity against the row descriptions in
`ACROBAT-PARITY.md` and pins shell logic as unit tests per the testing
strategy's item 4. Pixel comparison is a stated follow-up, filed as its own
issue at the end of M2.

**T5. `tools-basic` through `plugin-api`, with one contract change.**
`ToolCtx` today carries only `doc: &mut Document`, which means a hand tool
cannot pan and a zoom tool cannot zoom: both are viewport state, not document
state. `ToolCtx` gains `viewport: &mut Viewport`, a GPUI-free type owned by
`core` (pan offset, zoom, page layout mode, rotation). Named consumers:
`tools-basic`'s hand, zoom and marquee-zoom tools. Snapshot does not get a
render handle; it sets a region selection and raises a core-side
`SnapshotRequest` that `app` fulfils, so the plugin surface stays document
and viewport only. Text selection consumes `content::TextRun` quads through
the session's page-text cache.

**T6. AccessKit from spike to real.** P12 builds the tree for the shell chrome
and the page text, fixes the `Role::Document` to `AXGroup` flattening, and
drives focus from real window key state rather than a probe's own
`update_view_focus_state(true)`. The VoiceOver acceptance session needs the
user to grant accessibility trust on their machine; it is planned up to the
point of "run this script, here is the expected announcement", and marked as a
user-gated acceptance item. M2 is not done until it passes.

**T7. Search: wired, with the bidi gap narrowed and the rest deferred.**
`content::search` is per page and there is no document-level entry point, so
`core` gets an incremental document search that walks pages, yields matches,
and discards `PageText` it is not holding for the viewport (otherwise
whole-document find breaks the memory budget on a 1000-page file). It runs on
its own worker thread, which *constructs* its `cos::Document` inside the
spawned closure from the shared `Arc<Vec<u8>>`, exactly as the render worker
constructs its `render::Document`. Only the bytes cross the thread boundary,
and `Arc<Vec<u8>>` is already `Send`, so no document is ever moved between
threads and `cos::Document` needs no `Send` bound. On bidi:
the cheap half is folding Arabic Presentation Forms-A/B to their base letters
inside `search::fold`, which already maintains a byte-offset map back to the
source text, so a user-typed Arabic word matches when it sits in one run. The
expensive half, visual-to-logical reordering across runs, is **deferred**;
`known-issues.md` keeps the entry, narrowed to what remains.

**T8. Budgets as CI benches: P14.** A `crates/core/benches/` harness, a
`corpus/make-bench.py` that synthesizes `corpus/bench/pages-1000.pdf`
(gitignored, deterministic, varied per page so nothing caches its way out),
and the bench gating job. The bench asserts bytes read through
`CountingSource`, not only wall clock, because wall clock alone does not guard
laziness (see candor, item 3). First paint is benched against the real
`external/hayro-corpus/0041790.pdf`. The *shell* CI job (building and testing
`--features shell`, the Metal toolchain step, `CARGO_NET_GIT_FETCH_WITH_CLI`)
does **not** wait for P14; it lands with P6b, the package that first writes
GPUI code, so no shell code is ever written without CI covering it.

**T9. No MCP, no CLI.** Not planned, not touched. `crates/core` gains no API
"for MCP later". Kernel emptiness and feature gating are verified in every
package's checklist.

---

## 3. Work packages

Format per package: goal, files, depends-on, what exists to build on,
verification, review risk.

### P1. cos API completion

**Goal.** The three cos changes every later package needs, landed together and
reviewed once.

1. `Document::decode_stream(&Stream) -> Result<Vec<u8>>`, promoting the
   private `filters::decode`. One decoder owns Flate, predictors, ASCIIHex and
   ASCII85; LZW and RunLength move in from `content/src/filter.rs`.
2. `Document::page(index: usize) -> Result<PageNode>` where `PageNode` carries
   `objref`, the page dict, and the four inheritable attributes
   (`/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`) already resolved against
   ancestors. Structural only: no normalization, no typing. That stays in
   `content::Page`.
3. `BytesSource::from_shared(Arc<Vec<u8>>)` so one buffer serves cos and hayro.

`content/src/filter.rs` is deleted and `content/src/page.rs`'s `descend` walk
is replaced by a call into the accessor; `content::Page` keeps its
normalization layer and gains a `crop_box` field (used by P3).

**Files.** `crates/cos/src/document.rs`, `crates/cos/src/filters.rs`,
`crates/cos/src/source.rs`, `crates/cos/src/lib.rs`, `crates/cos/src/object.rs`;
delete `crates/content/src/filter.rs`; edit `crates/content/src/page.rs`,
`crates/content/src/lib.rs`, `crates/content/Cargo.toml` (drop `flate2`,
`weezl`), `crates/cos/Cargo.toml` (add `weezl`); new
`crates/cos/tests/pages.rs`.

**Depends on.** Nothing.

**What exists to build on.** `filters::decode` handles Flate with predictors,
ASCIIHex and ASCII85 already and is bomb-guarded. `content::filter` has the
LZW and RunLength cases. `content::page::descend` is the working page walk,
including the `/Count` subtree skip and the cycle guard, and moves nearly
verbatim. `first_page` is the same descent for index 0.

**Verification.**
- `cargo test -p onionskin-cos` and `ONIONSKIN_CORPUS_REQUIRED=1 cargo test -p onionskin-content`: the content oracle tally must not regress by a single file. Capture the tally before and after and diff it; that is the real proof the decoder swap is behaviour preserving.
- New `crates/cos/tests/pages.rs`: for every seed and every `external/` file with more than one page, `Document::page(i)` for all `i` agrees with a naive full walk, and `page(0)` agrees with `first_page()`.
- A lazy assertion: `Document::page(500)` on the P14 bench file, through `CountingSource`, reads fewer bytes than a full page-tree walk would.
- `cargo clippy --workspace --all-targets -- -D warnings`.

(The `hayro::hayro_syntax::Pdf: Send + Sync` assertion T3 relies on is a plain
test in `crates/render`, landing with P4.)

**Review risk.** Whether the promoted decoder is byte-identical to both
predecessors on every corpus file (the oracle tally is the only credible
answer). Whether `PageNode` leaks interpretation into cos's charter. Whether the inheritable-
attribute resolution matches `descend`'s `/Count`-skip semantics exactly,
including the "a node with `/Kids` is internal even when it claims
`/Type /Page`" rule.

### P2. `core::Document`: the session

**Goal.** One type the viewer holds: bytes, cos document, provenance, page
count, per-page geometry cache, selection, search state. No edit graph, no
history, no save.

**Files.** `crates/core/src/lib.rs`, new `crates/core/src/session.rs`,
`crates/core/src/page.rs`, `crates/core/src/selection.rs`,
`crates/core/Cargo.toml` (adds `onionskin-cos`, `onionskin-content`,
`onionskin-plugin-api` is *not* addable, see risk).

**Depends on.** P1.

**What exists to build on.** `cos::Document::open_repairing` and its
`Provenance`; `content::page_count`; `content::extract_page`.

**Note on the dependency direction.** `plugin-api` depends on `core`, so
`core` cannot depend on `plugin-api`. `PagePoint`, `PageRect` and `PageQuad`
live in `plugin-api` today and `content` returns `PageQuad`. Resolution:
move those four geometry types (`PageIndex`, `PagePoint`, `PageRect`,
`PageQuad`, `Modifiers`) down into `core` and re-export them from
`plugin-api`, so every existing `use onionskin_plugin_api::PageQuad` keeps
compiling. This is a P2 sub-step and it must be in the diff.

**Verification.**
- `cargo test -p onionskin-core`: open every `corpus/seeds/` and `corpus/malformed/` file, assert `provenance()` matches the file's known damage, assert `page_count()` matches the seed.
- A session opened on a malformed file reports `Repaired` and still serves page geometry.
- An encrypted corpus file fails with an error whose `Display` names encryption and the milestone, not a debug dump.
- `cargo test -p onionskin-app --no-default-features` still passes.

**Review risk.** Scope creep into the edit graph. Whether the geometry-type
move broke any `PartialEq`/`Copy` derive a caller relied on. Whether the
page-geometry cache is bounded (a 50k-page document must not cache 50k
geometries eagerly). Whether `Session` is honestly `!Sync` and says so.

### P3. Page geometry and the one coordinate mapping

**Goal.** A single, tested transform between `content`'s user space and the
rendered raster, and the crop-box fix that makes it correct.

`content::Page` uses `/MediaBox` and deliberately does not apply `/Rotate`;
hayro renders the intersected crop box with `/Rotate` applied. Every text
selection, search highlight and pointer hit test crosses that gap. M2 gets
exactly one place where it is crossed: `render` re-exports hayro's own
`Page::initial_transform(true)` as `render::page_transform(index)`, and `core`
exposes `PageGeometry { media_box, crop_box, rotate, render_size }` plus
`user_to_device(quad, zoom)` built on it. Nothing reimplements the transform.

**Files.** `crates/content/src/page.rs` (`crop_box` field, populated from P1's
accessor), `crates/render/src/base.rs` (expose the transform and
`render_dimensions` per page), new `crates/core/src/geometry.rs`.

**Depends on.** P1, P2.

**What exists to build on.** `Page::initial_transform` and
`intersected_crop_box` are public in hayro. `content::Page::base_ctm` is the
current, media-box-only version being replaced.

**Verification.**
- A corpus sweep that lists every `external/` file where `/CropBox` differs from `/MediaBox`, or `/Rotate` is non-zero; pick three of each as fixtures. This sweep is part of the package, not an afterthought: if the plan cannot name the fixture files, the fix is unproven.
- For each fixture, extract the first text run, map its first glyph quad through `user_to_device` at zoom 1 and 3, and assert the device rect contains non-background pixels in the rendered raster. This fails on today's code, which is the point.
- A rotated fixture at `/Rotate 90` and `/Rotate 270`, because the two rotation transforms in hayro differ and a sign error passes one and fails the other.

**Review risk.** The highest-correctness-risk package in M2. A reviewer will
probe: the y-flip direction, the crop-box origin offset, all four rotations,
whether `zoom` is applied before or after rotation, and whether the mapping is
used consistently by selection, search highlight and hit testing or whether
one of them quietly does its own arithmetic.

### P4. `render`: eviction, the spike to-dos, and the OCG fork patch

**Goal.** Close the render spike's M2 list and add the tile eviction policy
decision 11 requires.

- `TileStore`: an LRU over `(page, zoom)` tile caches with a byte budget,
  evicting whole caches and individual tiles, so resident memory is
  proportional to viewed pages rather than visited pages.
- `TileCache::tile` behind interior mutability, so a paint pass holds `&self`.
- Reject degenerate overlays at `add_overlay` instead of re-filtering on every
  composite.
- Per-tile overlay index, so a page with 500 annotations does not walk all 500
  per tile.
- `render_annotations` exposed on our own settings struct (the hayro field
  already exists).
- `Document::from_shared(Arc<Vec<u8>>)`.
- `RenderError::Load(Decryption)` `Display` stops leaking `{e:?}`.
- The highlight test compares the full region outside the quad, not one pixel.

**The fork patch.** One M2 parity row needs a hayro capability its public
settings do not expose: attempt the patch, decide by the end of P4, carry
exactly one outcome, never both.

`OcgState` is built from `/OCProperties /D` inside
`Context::new_with` and there is no override, so a Layers pane cannot toggle
anything through hayro as it stands. Chosen resolution: one more commit on the
fork we already carry, adding `InterpreterSettings::ocg_overrides:
HashMap<ObjectIdentifier, bool>` consulted by `OcgState`, offered upstream the
way the `UnresolvedAnnotationAppearance` variant was. Fallback if the patch
does not land or does not work: the Layers pane lists OCGs read-only in M2 and
the parity row moves to M3, which is a scoreboard change, not a silent
downgrade.

Line Weights needed a second fork commit and **the review cut it**; the
fallback is taken up front. See section 6 for the taken outcome and the M3
design guidance.

**Files.** `crates/render/src/tile.rs`, `src/base.rs`, `src/overlay.rs`,
`src/lib.rs`, new `crates/render/src/store.rs`, `crates/render/tests/tiles.rs`,
`crates/render/tests/eviction.rs`; the fork commit lives outside this repo
and is referenced by a new rev in `crates/render/Cargo.toml`.

**Depends on.** Nothing. `render::Document::from_shared` needs only hayro's
`PdfData: From<Arc<T>>`, not anything from P1, so this package is **day-one
parallel** with P1 and P2.

**What exists to build on.** `TileCache` with working damage tracking and the
seam test that pins Schist's tile-edge bug at zero boundary pixels.

**Verification.**
- `cargo test -p onionskin-render`, including a new test that scrolling a 200-page document through the store keeps resident bytes under the budget and that the budget is a named constant, not a literal.
- A test that an overlay with fewer than two points, or non-finite coordinates, is refused at `add_overlay` and never reaches a composite.
- A test that `TileCache::composites()` after a one-tile ink damage is 1, preserving the M1 finding.
- If the OCG patch lands: a fixture with two OCGs where toggling one changes the rendered pixels and toggling it back is bit-identical.
- `hayro::hayro_syntax::Pdf` is `Send + Sync`, asserted as a plain test here rather than assumed by T3.

**Review risk.** Whether eviction can drop a tile the current frame is about
to paint (a correctness question, not a memory one). Whether interior
mutability introduces a re-entrancy hazard when `page_image` calls `tile` in a
loop. Whether the byte budget is derived from something real or is a magic
number. Whether the fork rev bump silently pulled other upstream changes.

### P5. The render worker

**Goal.** A background thread that turns page-render requests into rasters,
plus the first-paint path, owned by `core` and testable headless.

Shape: `core::render::Worker::spawn(Arc<Vec<u8>>) -> WorkerHandle`. The
handle takes `Request { page, zoom, generation }` and returns
`Response::{Placeholder, Raster, Failed}` over a channel. Requests are
coalesced by page and superseded by generation, so a fast scroll does not
queue 200 stale renders. The worker owns a `render::Document` and creates its
`RenderCache` inside its loop scope. Placeholders are produced synchronously
on the caller's thread from `PageGeometry` plus any cached raster for that
page at another zoom.

**Files.** New `crates/core/src/render/mod.rs`, `worker.rs`, `placeholder.rs`;
`crates/core/Cargo.toml` gains `onionskin-render`.

**Depends on.** P2, P3, P4.

**What exists to build on.** `render::Document::render_page` and its warning
sink; the M1 measurement that a fresh `RenderCache` per call throws away font
caching, which the worker-owned cache fixes.

**Verification.**
- Headless test: request pages 0..20 of a seed, assert every response arrives, in any order, with the right dimensions.
- Headless test: request page 5 at zoom 1, then immediately at zoom 4; assert exactly one raster is delivered for the superseded request or that it is dropped, and that generations never interleave wrongly.
- Headless test on `0041790.pdf`: a placeholder response arrives before the raster, and the raster carries the interpreter warnings hayro emitted.
- `cargo test -p onionskin-core` must not require a window or a display.

**Review risk.** Channel deadlock on shutdown; whether the worker thread is
joined or detached and what happens if it panics mid-render (a poisoned viewer
that silently stops painting is the failure mode). Whether coalescing can
starve a page that is permanently in view. Whether "placeholder" ever gets
mistaken for the real raster by the snapshot or export paths.

### P6a. Layout and viewport math, in `core`

**Goal.** Everything about *where* pages go and *what* the view is showing,
GPUI-free and headless-testable. `app` gets no view math of its own.

Covers the state behind these parity rows: single page, single page
continuous (default, and the mode the 60 fps budget is measured in), two page,
two page scrolling, show cover page, zoom in/out/to, actual size, fit page,
fit width, fit height, fit visible, dynamic zoom, rotate view, page navigation
(first/previous/next/last, page..., previous/next view). P6b renders and
drives them.

**Continuous scroll versus lazy open.** Laying out a scroll of N pages needs
N page heights, which contradicts "never parse ahead of need". Resolution:
the scroll metric is an estimate from the first page's geometry, refined as
pages are measured, with the scrollbar and page-number box reading from the
estimate. Documented in code where the estimate is formed, and unit-tested
(a jump to page 900 lands on page 900 even though only 12 pages have been
measured).

**Files.** New `crates/core/src/layout.rs` (page placement per mode, the
scroll metric, visible-page query), `crates/core/src/viewport.rs` (`Viewport`:
pan offset, zoom, mode, view rotation, `fit`, `zoom_at`, `pan_by`, `scroll`,
`pinch`, `page_point_at`), `crates/core/src/history.rs` (previous/next view
stack, view positions only, not an edit history).

**Depends on.** P2, P3.

**What exists to build on.** `shell_spike`'s `fit`, `zoom_at`, `pan_by`,
`scroll` and `pinch` and their seven unit tests move here nearly verbatim,
with `gpui::Pixels`/`Point` replaced by plain `f32` pairs. Two known nits are
fixed as part of the move: `drag_from` is not cleared on an outside-window
mouse-up (P6b's half), and `fit()` is not re-run on resize.

**Verification.**
- The moved spike tests plus new ones per layout mode, sentence-named, each naming the behaviour it pins: `two_page_view_with_a_cover_page_puts_page_one_alone`, `a_jump_to_page_900_lands_there_with_twelve_pages_measured`, `fit_width_is_re_applied_when_the_viewport_resizes`, `previous_view_returns_to_the_zoom_and_offset_it_left`.
- `cargo test -p onionskin-core`, on CI's default job, with no window and no `shell` feature.
- A test that `core` still compiles with no GPUI in its dependency tree, which the workspace's default job already proves by building it.

**Review risk.** Whether the estimate-and-refine scroll metric can produce a
scrollbar that jumps under the user. Whether `fit_visible` is implemented
against rendered content bounds or faked as `fit_width`. Whether `Viewport`
grew fields only `app` needs, which would mean the split was drawn in the
wrong place.

### P6b. The GPUI canvas and input routing

**Goal.** Paint P6a's layout, route input into `plugin-api` types, and put the
shell under CI.

Covers: page rendering on screen, pan, pinch-to-zoom and stylus pressure, and
the pointer path from a GPUI event to `PointerInput` in page user space.

**Files.** New `crates/app/src/shell/mod.rs`, `canvas.rs`, `input.rs`; delete
`crates/app/src/bin/shell_spike.rs` once its view-math tests have moved to
P6a; `crates/app/src/main.rs` gains the windowed path;
`.github/workflows/ci.yml` gains the shell job (F5).

**The shell CI job**, landing here rather than with the benches:

- All three runners get `CARGO_NET_GIT_FETCH_WITH_CLI: true` at the job level. It is needed on any runner whose git config rewrites https to ssh; harmless where it does not, and both the hayro and gpui pins are git revs.
- macOS: `xcodebuild -downloadComponent MetalToolchain` (704 MB) before the build, or gpui's shader build script fails.
- **Scope, per OS, stated rather than assumed.** The job is `cargo build -p onionskin-app --features shell` plus `cargo test -p onionskin-app --features shell` on all three, and **no test in it may open a window**: window-opening verification stays manual on the developer's macOS machine. GPUI on Linux needs a system dependency set (at minimum Vulkan loader and headers, `libxkbcommon`, wayland and xcb development packages) that nobody in this project has yet installed; **P6b's first task is to determine that set empirically on `ubuntu-latest` and write it into the workflow**, and if the Linux shell build cannot be made to work inside this package, the job is scoped to macOS and Windows with an issue filed, not left red.

**Depends on.** P6a, P5.

**Verification.**
- `cargo test -p onionskin-app --features shell` green on every runner the job covers.
- `a_drag_released_outside_the_window_stops_panning`, as a unit test on the input state machine (the nit the spike left open).
- A test that pointer coordinates reach `PointerInput` through P3's transform and not a local copy: assert the canvas calls `core`'s mapping, e.g. by making the mapping the only public path and having no float arithmetic in `input.rs`.
- Manual: open a seed, a 152-page real file and `0041790.pdf`, screenshot at fit, at 400%, and mid-scroll. Screenshots go in the PR, not the repo.

**Review risk.** Whether any view math leaked back into `app`. Whether the
canvas holds `TileCache` across frames in a way that fights P4's interior
mutability. Whether the Linux dependency set was determined by running CI or
by copying Zed's. Whether a windowless CI job silently passes while the real
window is broken, and what manual step covers that gap.

### P7. Shell frame chrome (three staged review points)

**Goal.** The Acrobat frame around the canvas. Too large for one review, so it
lands and is reviewed in three stages, each independently mergeable.

- **P7a. Top frame.** Global bar, hamburger main menu, document tabs, tab context menu, global search field.
- **P7b. Tool surfaces.** All-tools pane (left tool rail) and its "view more" state, quick action toolbar (floating, draggable, customizable), right-hand side panel host.
- **P7c. View chrome.** Bottom page controls, display theme (system/light/dark), Full Screen mode, Read Mode, show/hide navigation panes, show/hide toolbar items and page controls, show/hide line weights.

**Two rows that cannot be fully live at M2, and ship disabled with a reason:**

- **Quick action toolbar defaults.** Acrobat's defaults are Select, Comment, Highlight, Draw, Fill text fields, Add Sign or Initials. Only Select exists at the integrated M2 gate; Comment, Highlight and Draw are `tools-comment` (M3), Fill and Sign are `tools-fill-sign` (M5). P7 derives every slot from a registry capability query, not a hardcoded enabled list, and disables missing capabilities with an explicit reason. This leaves Select honestly unavailable in P7's pre-P10 registry; P10's selection tool later exposes `ToolCapability::Select` and makes it live without a P7 application-code change. P7 owns the backward-compatible `ToolCapability` metadata and default-empty `ToolPlugin::capabilities()` contract that supplies this query. Its customization row (which actions show) is live, over the same set.
- **Line Weights.** The fork patch it would have needed was cut in review, so this menu item ships **disabled with a reason** and the parity row moves to M3. P7c must not invent a tile-layer approximation.

**Global search field (row 89) has two halves**, both here: the tool-lookup
half is a substring query over `PluginRegistry::tools()` and `commands()` by
name and id, and lives in P7a; the document-text half routes the query to
P9's find and does no searching of its own.

**Files.** New `crates/app/src/shell/chrome/{mod,global_bar,tabs,rail,
quick_actions,side_panel,page_controls,theme}.rs`; new
`crates/app/src/shell/chrome/tool_search.rs`; existing
`crates/plugin-api/src/lib.rs` (`ToolCapability` and the default-empty
`ToolPlugin::capabilities()` metadata contract).

**Depends on.** P6b.

**What exists to build on.** `PluginRegistry::tools()` and `plugins()` already
give the rail its content, including `ToolPlugin::group`, `in_rail`, `icon`
and `shortcut`, which exist precisely for this pane.

**Verification.**
- Unit tests on the logic with no window: rail grouping puts same-group tools in one slot with the last-used one showing; tab close-others leaves exactly one tab; theme resolution maps system to light or dark and never to a third state.
- The Line Weights menu item is disabled and its reason string names M3, asserted; nothing in `render` is toggled by it.
- The rail's contents equal `build_registry().tools()` filtered by `in_rail`, asserted, so a new plugin appears without a chrome edit.
- Layout parity checked by reading each `ACROBAT-PARITY.md` M2 row description against a screenshot, recorded in the PR. Pixel parity is deferred (see the follow-up issue in section 6).
- Tool search: a query matching a registered tool's name returns it; a query matching nothing returns nothing rather than falling through to document search silently.
- Every disabled quick action carries a reason string naming its milestone, asserted, and the reason comes from a capability query so it cannot go stale.

**Review risk.** File sprawl and a god `shell.rs`. Whether Full Screen and
Read Mode are two states or one state with a flag (they differ: Read Mode
keeps the top bar reachable). Whether theming reaches menus, context menus and
scroll bars, which the parity row explicitly calls out. Whether the three
stages are genuinely independently mergeable or share a half-written module.

### P8. Navigation panes

**Goal.** The left navigation panes.

Covers: page thumbnails pane and its context menu (menu present, page-editing
entries disabled with a reason until `tools-organize` at M3), bookmarks pane
(view and navigate), attachments pane (list, open, save), layers pane with OCG
toggles (or read-only listing per P4's decision), layers pane context menu,
signatures pane (listing only, validation at M6), search results pane.

**Files.** New `crates/app/src/shell/panes/{mod,thumbnails,bookmarks,
attachments,layers,signatures,results}.rs`; new `crates/core/src/outline.rs`,
`attachments.rs`, `layers.rs`, `signatures.rs` for the GPUI-free readers.

**Depends on.** P7c, P5 (thumbnails are worker renders at a small zoom), P9
(the results pane).

**What exists to build on.** `cos::Document::catalog()` plus `resolve` reach
`/Outlines`, `/Names /EmbeddedFiles`, `/OCProperties`, `/AcroForm`. Nothing
above cos exists yet for any of them; each reader is new, small and
independently testable.

**Verification.**
- Per reader, a headless test over corpus files known to carry the structure: an outline file from `pdf-association`, a file with embedded attachments, a file with OCGs, a signed file. Name the files in the package, do not say "some corpus file".
- A cyclic `/Outlines` tree terminates (this is the classic hostile-document case and cos's own descent already caps depth; the reader must too).
- Thumbnails for a 1000-page document render lazily: assert the worker received requests only for visible rows.
- The thumbnails context menu shows every Acrobat entry, with the M3 ones disabled and carrying a reason string, not hidden.

**Review risk.** Whether attachment "save" writes outside a user-chosen path.
Whether the signature pane claims anything about validity (it must not; that
is M6). Whether the layers pane silently no-ops when the OCG override is
unavailable rather than disabling the control.

**Note from P4 review (layer toggles).** `TileStore::clear()` drops overlays
together with the cached rasters, by the documented ownership model on
`insert`: the caller that re-rendered owns re-adding overlays, because only
it knows what the new raster already contains. So toggling an OCG layer in
the Layers pane costs a re-render of the affected pages plus re-adding every
annotation overlay on them. P8's layers implementation must do both; a
toggle that re-renders and forgets the overlays is the bug to test for.

### P9. Find bar and document search

**Goal.** Ctrl+F with Acrobat's options, wired to `content::search`, plus the
incremental document-level search `content` does not have.

Covers: Edit > Find, the find toolbar (highlight all, next, previous),
whole-word and case options, the search results pane's data source, and the
"Return Results Containing" subset of Advanced Search (Match Exact Word Or
Phrase, Any Of The Words, All Of The Words). Stemming is deferred, see
section 6.

`core::search::DocumentSearch` runs on its own worker thread. The thread
receives the shared `Arc<Vec<u8>>` and **constructs its own `cos::Document`
inside the spawned closure**, the same pattern the render worker uses for
`render::Document`: only the bytes cross the boundary, so nothing here needs
`cos::Document` to be `Send`. It walks pages from the current one, extracts,
searches, yields `Match` values with their page index over a channel, and
drops `PageText` for pages outside the viewport so a 1000-page find does not
hold the document in memory. Highlight-all draws through
`render::Overlay::Highlight` using P3's transform.

Bidi: `content::search::fold` also folds Arabic Presentation Forms-A and -B to
their base letters, using the offset map it already builds. Visual-to-logical
reordering stays out.

**Include Bookmarks / Include Comments (row 149): both deferred at M2.**
Comments need `tools-comment` (M3). Bookmarks are readable, but the outline
reader lands in P8, which depends on this package for the results pane; adding
the reverse edge makes a cycle, and pulling the reader forward for one
checkbox is not worth restructuring two packages. Row 149's Notes column says
so, and both checkboxes ship disabled with a reason rather than absent.

**Files.** `crates/content/src/search.rs` (the fold change, plus the
any-of/all-of modes), new `crates/core/src/search.rs`, new
`crates/app/src/shell/find_bar.rs`.

**Depends on.** P2, P3, P6b. The dependency on P5 is transitive through P6b
and intentional: the find bar needs rendered pages to highlight onto, and P6b
already pulls the worker in, so listing P5 again would only duplicate an edge
the graph already carries.

**What exists to build on.** `search`, `search_flattened`, `flatten`,
`Flattened::runs_for`, `TextRun::quads_for`, `SearchOptions`, and the
existing 10 unit tests including the Turkish dotted-I folding case.

**Verification.**
- New `content` unit tests: an Arabic word written as presentation forms matches the same word typed in base letters, within one run; a case where it does not (two runs, reordered) is asserted as a known limitation with a test that documents the current behaviour rather than a skip.
- `core` test: search a 1000-page bench file for a term on page 900, assert the match is found and that peak resident `PageText` count stayed under a named bound.
- App test: next/previous wrap at the ends; highlight-all count equals the match count; the options change the count in the direction the option implies.
- The search worker is proven off-thread: a test that starts a find on the bench file and asserts the calling thread returns before the search completes.

**Review risk.** Whether the incremental search holds `PageText` alive through
a `Match` (the `Match` carries quads and provenance, not borrows, so it should
not, but a reviewer will check). Whether "all of the words" is implemented as
N searches intersected per page or something looser. Whether the presentation-
form fold breaks the existing offset-map invariants that the Turkish test
pins. Whether the second `cos::Document` doubles the open cost (it should not:
opening is 19 KB and ~2 ms, but a reviewer will want the number).

### P10. `tools-basic`

**Goal.** The registry's first real tools, and the `ToolCtx` change that lets
them exist.

Tools: hand (pan), select text, select region, marquee zoom, snapshot.
`ToolCtx` gains `viewport: &mut core::Viewport`. Snapshot sets a region
selection and raises `core::SnapshotRequest`; `app` renders the region and
puts it on the clipboard.

The selection tool exposes `ToolCapability::Select`, making P7b's Select
quick action live through the registry without a P7 application-code change.

Also the page canvas and text-selection context menu. Live at M2: Copy, Copy
With Formatting, Export Selection As (through P13), Take A Snapshot, Rotate
(view rotation, P6a), Add Bookmark is **not** live (bookmark authoring is
`commands-core`, M3). **Print is not live either**: File > Print is parity row
139, M3, and needs `crates/print`, which does not exist. Every non-live entry
ships disabled with a reason naming its milestone.

**Files.** `crates/plugin-api/src/lib.rs` (`ToolCtx`), `crates/core/src/viewport.rs`
(the `Viewport` P6a created; this package adds the mutation surface tools
need), `plugins/tools-basic/src/lib.rs` and
`src/{hand,select_text,select_region,zoom,snapshot}.rs`,
`plugins/tools-basic/Cargo.toml` (adds `onionskin-core`),
`crates/app/src/shell/context_menu.rs`.

**Depends on.** P2, P3, P6a (owns `Viewport`), P6b.

**What exists to build on.** `ToolPlugin` with its full lifecycle
(`on_activate`, `on_pointer_*`, `on_commit`, `on_cancel`, `on_deactivate`,
`overlays`), `PluginRegistry::register_tool` with its duplicate-id assertion,
and `Overlay::{AntsRect, Quads}` which are exactly what region and text
selection need.

**Verification.**
- Headless gesture tests in the Schist style, sentence-named, against a real document: `a_drag_across_glyphs_selects_them_in_document_order`, `a_tiny_drag_selects_nothing`, `shift_extends_an_existing_text_selection`, `a_region_drag_produces_one_ants_rect`, `marquee_zoom_fits_the_dragged_rectangle`.
- Registry-exhaustive test extended: every registered tool has a non-empty id, name, icon and group, and survives a degenerate document (zero pages, one empty page).
- `cargo test -p onionskin-app --no-default-features --features tools-basic` still passes and the registry reports the right counts.

**Review risk.** Whether adding `viewport` to `ToolCtx` was the minimum change
or whether it opened a door to viewport-mutating tools nobody wants. Whether
text selection uses P3's transform. Whether snapshot's clipboard path is in
`app` and not in the plugin. Whether the disabled context-menu entries are
disabled by a real capability query or by a hardcoded list that will rot.

### P11. Commands, keymap, preferences, recents, file association

**Goal.** The command layer and the app-level plumbing the parity rows name.

Covers: File Open (including the repair path and its user-visible notice),
Open Recent, Close/Close All, Exit/Quit, Edit Select All/Deselect All, Edit
Take a Snapshot, Edit Preferences, View > Tools, Help menu (About, keyboard
shortcuts), keyboard shortcut remapping via `keymap.json`, Home view Recents
with its list/thumbnail toggle, display theme preference, preferences dialog
(partial: the categories Onionskin has), registering `.pdf` as openable.
Quick-action toolbar customization (row 105) belongs to P7b, not here.

**Files.** New `crates/app/src/keymap.rs`, `commands.rs`, `recents.rs`,
`preferences.rs`, `shell/home.rs`, `shell/preferences_dialog.rs`;
`packaging/macos/Info.plist`, `packaging/linux/onionskin.desktop`,
`packaging/windows/installer.nsi` for the association.

**Depends on.** P7c, P10.

**What exists to build on.** `Command { id, title, keybind, run }` and
`PluginRegistry::commands()` already carry keybinds in GPUI keystroke syntax
with the documented cmd-to-ctrl mapping. Packaging manifests exist and need
association entries, not new files.

**Verification.**
- Unit tests: a `keymap.json` that rebinds a command takes effect; an unknown command id in the file is an error, not a silent skip; a duplicate binding is reported.
- Every command in `build_registry()` has either a keybind or a menu home, asserted, so a command cannot be unreachable.
- Repair notice: opening a `corpus/malformed/` file shows the notice and names what was repaired from `RepairReport`, verified by a headless test on the notice text.
- Association: verified by inspection on macOS via `lsregister -dump`, and stated as inspection-only on Linux and Windows, matching the existing packaging confidence note.

**Review risk.** Preferences scope: the parity row says "partial" and lists
about 30 Acrobat categories. A reviewer will check that the dialog carries
only categories with a real setting behind them and that the row's Notes
column says what is cut. Whether recents stores absolute paths that leak in a
shared home directory.

### P12. AccessKit

**Goal.** A real accessibility tree, live from the first release, and the
`Role::Document` fix.

- Tree for the shell chrome: global bar, tabs, rail, panes, page controls,
  find bar, each with role, label and bounds.
- Tree for page content: the page node plus its text, from
  `content::PageText`, so a screen reader reads the document rather than
  announcing "group".
- `Role::Document` flattens to `AXGroup` today; fix the mapping so a page
  reads as a document. If the fix belongs in AccessKit rather than in our
  node construction, say so in the package and carry the workaround with the
  reason.
- Focus driven by real window key state, not by
  `update_view_focus_state(true)` called before the window is key.
- Adapter constructed before anything queries the view, which the spike flags
  as an ordering constraint rather than a coincidence.

**Files.** New `crates/app/src/a11y/{mod,tree,focus}.rs`; delete
`crates/app/src/bin/a11y_spike.rs` once its probe moves into a test helper.

**Depends on.** P6b, P7c, P8, P9.

**What exists to build on.** The spike proves `SubclassingAdapter` attaches
to gpui's NSView with zero fork changes at rev `84bdb01`, and its
`dump_accessibility` probe is a working, permission-free way to read the tree
back. Reuse the probe as the automated test.

**Verification.**
- Automated, no permission needed: the spike's direct-messaging probe, promoted to a test, asserts the chrome tree's roles and labels and that the page node's role is no longer `AXGroup`. Labels are read via `accessibilityTitle`, never `accessibilityLabel` (AccessKit sets only the former).
- A re-check that the fork rev still implements no accessibility selectors on its view class, so the runtime subclass does not collide. This must run on every fork bump, so it is a test, not a note.
- **User-gated acceptance:** one real VoiceOver session on the user's machine, following a written script in this package: open a seed, Ctrl+Option+arrow through the chrome, land on the page, confirm the announcement names the document and reads text. Everything up to "run this" is planned; the session itself needs the user and their accessibility grant. **M2 is not done until it passes**, and its result is recorded here.

**Review risk.** Whether the page text node is one giant label (unusable) or
structured. Whether the tree updates on navigation or is built once. Whether
focus follows the shell's own focus model or is asserted by the adapter.
Whether the acceptance session is honestly reported, including what it failed
at.

### P13. `codecs-common` export

**Goal.** The export rows the plan assigns to `codecs-common` at M2.

Covers: export to plain text (from `content`), export pages to PNG, export
pages to SVG.

SVG comes from `hayro-svg` 0.7.0 in the fork we already pin, which means one
more dependency entry, not a new pin.

**Files.** `plugins/codecs-common/src/lib.rs` and
`src/{text,png,svg}.rs`, `plugins/codecs-common/Cargo.toml`,
`crates/render/Cargo.toml` (add `hayro-svg` at the same rev),
`crates/render/src/svg.rs`.

**Depends on.** P1, P3, P5.

**What exists to build on.** `content::extract_page` and `PageText::flatten`
give plain text with the separator rules already tested. `render::PageRender`
gives the pixels; PNG encoding needs an encoder (`image` is already a
dependency of `app` behind `shell`; put the encoder in `codecs-common`, not in
`render`, so the seam stays about rasters).

**Verification.**
- Round-trip-ish test: export a seed to text, assert the output equals `flatten(&extract_page(..)).text`, so the codec adds no interpretation.
- PNG export of a seed page at zoom 2 has the expected dimensions and is decodable.
- SVG export of a seed page parses as XML and contains a glyph path.
- `cargo test -p onionskin-app --no-default-features --features codecs-common`.

**Review risk.** Whether `CodecPlugin` gets invented here (`plugin-api`'s doc
comment says it "joins these in M2"). If it does, its shape must be named by a
real consumer, and there are exactly three. Whether accessible-text ordering
is claimed (it should not be; the parity row says that improves at M6).

### P14. Benches and the budgets in CI

**Goal.** Decision 11's two budgets, plus the 60 fps scroll and the memory
line, running as benches that fail the build on regression. Guarantee test 9
stops being `#[ignore]`.

- `corpus/make-bench.py` synthesizes `corpus/bench/pages-1000.pdf`:
  deterministic, 1000 pages, per-page varied content so nothing caches its way
  out, gitignored like the rest of the generated corpus.
- Lazy-open bench: time to first page **and** bytes read through
  `CountingSource`. The byte assertion is the one that guards laziness; wall
  clock alone does not (see candor, item 3).
- First-paint bench on `external/hayro-corpus/0041790.pdf`: something visible
  under 200 ms, full raster delivered afterwards.
- Scroll bench: frame time over a scripted continuous scroll, headless, driving
  `core`'s layout and worker without a window.
- Memory bench: resident tile bytes after scrolling 200 pages stays under
  `TileStore`'s budget.
- CI: the bench gating job only. The shell job, the Metal toolchain step and
  `CARGO_NET_GIT_FETCH_WITH_CLI` land with P6b, not here (F5), so no GPUI code
  is ever written without CI covering it.

The scroll bench compiles because P6a put layout and viewport math in `core`;
`crates/core/benches/scroll.rs` drives `core::layout` and the render worker
with no window and no dependency on `app`. If the review reverses P6a and puts
layout back in `app`, this bench moves to `crates/app/benches/` behind the
`shell` feature and the CI job that runs it becomes the shell job.

**Files.** New `corpus/make-bench.py`, `crates/core/benches/open.rs`,
`benches/paint.rs`, `benches/scroll.rs`, `crates/core/Cargo.toml`,
`.github/workflows/ci.yml`, `corpus/.gitignore`, `corpus/README.md`,
`crates/app/tests/guarantees.rs` (delete one `#[ignore]`).

**Depends on.** P4, P5, P6a. Not P6b: every bench here is windowless.

**What exists to build on.** `CountingSource` and `ReadStats` exist and were
what proved M1's lazy claim. `corpus/make-seeds.py` is the template for a
deterministic generator. The corpus skip-loudly convention and
`ONIONSKIN_CORPUS_REQUIRED=1` already exist in both `cos` and `content` test
harnesses.

**Verification.**
- Each bench fails when its budget is tightened by 20%, proving it measures something. Run that mutation and record it.
- The bench job green on the runners it is scoped to, with the shell job (P6b) already green independently.
- `corpus/make-bench.py` run twice produces byte-identical output.

**Review risk.** Whether the synthetic bench file exercises anything (1000
identical trivial pages measure the xref, not the viewer). Whether the benches
are stable enough on shared CI runners to gate a build, or whether they need a
tolerance that makes them meaningless. Whether the byte-read assertion has a
number derived from the file or a number that happened to pass.

---

## 4. Dependency graph

Two roots, both startable on day one: **P1** (cos) and **P4** (render).

```
P1  cos API                      P4  render eviction + spike to-dos + the OCG fork patch
 └── P2  core::Document               │
      └── P3  geometry + mapping      │
           ├──────────┬───────────────┘
           │          └── P5  render worker
           │               ├── P6b GPUI canvas + input + the shell CI job  (also needs P6a)
           │               │    ├── P7a top frame
           │               │    │    └── P7b tool surfaces
           │               │    │         └── P7c view chrome
           │               │    │              ├── P8  navigation panes   (also needs P9)
           │               │    │              └── P11 commands/keymap/prefs (also needs P10)
           │               │    ├── P9  find bar + document search   (P5 transitively, intentional)
           │               │    └── P10 tools-basic                  (also needs P6a)
           │               ├── P13 codecs-common export   (also needs P1, P3; headless, no P6b edge)
           │               └── P14 benches + bench CI job (also needs P4, P6a; windowless)
           └── P6a layout + viewport math  (in core, headless; needs only P2, P3)

P12 AccessKit  needs P6b, P7c, P8, P9  (last, then the user-gated session)
```

Critical path: P1 → P2 → P3 → P5 → P6b → P7a → P7b → P7c → P8 → P12.

Parallelism: P4 runs alongside P1 and P2 from day one. **P6a depends only on
P2 and P3**, so it runs parallel with P4 and P5 rather than waiting behind
them, and is ready when P6b needs it. P13 is headless and needs no canvas, so
it can start as soon as P5 lands. Once P6b lands, P9, P10 and P13 run in
parallel with the P7 chain. P14 needs P6a but not P6b, so it can start while
the chrome is still being built.

One split for ordering: P14's bench file generator (`corpus/make-bench.py`) is
needed by P1's laziness test, so **the generator ships with P1, the benches
with P14.**

**Integration order for the three parallel packages after P6b.** P9, P10 and
P13 all touch shared files, so they land in a fixed order to keep conflicts
mechanical rather than semantic:

1. **P10 first.** P7 has already added the backward-compatible `ToolCapability` metadata contract to `crates/plugin-api/src/lib.rs`. Of these three parallel packages, P10 is the only one that changes `ToolCtx` in that file and `crates/core/src/viewport.rs`, and a `ToolCtx` change rebased under two other branches is worse than either of them rebasing under it.
2. **P9 second.** It adds `crates/core/src/search.rs` to `core`'s module list and `find_bar` to `shell/mod.rs`.
3. **P13 third.** It touches `crates/render/Cargo.toml` (adding `hayro-svg`), which also moves `Cargo.lock`; landing it after P4's rev bump and after the other two keeps lock-file churn to one merge.

The recurring conflict points are `crates/core/src/lib.rs`'s module list,
`crates/app/src/shell/mod.rs`'s module list, and `Cargo.lock`. All three are
append-only in these packages, so a rebase resolves them without judgement;
whoever rebases re-runs `cargo test --workspace` rather than trusting the
merge.

---

## 5. YAGNI ledger

Every abstraction introduced here names its consumer.

| Introduced | Consumer that exists in M2 |
|---|---|
| `cos::Document::decode_stream` | `content::page::content`, `codecs-common` |
| `cos::Document::page(index)` | `content::page`, `core` page model, P8's readers |
| `BytesSource::from_shared` | the session feeding cos and hayro one buffer |
| `core::layout` / `core::Viewport` | P6b's canvas, P14's scroll bench, and `tools-basic`'s hand, zoom and marquee zoom |
| `core::Viewport` in `ToolCtx` | `tools-basic` hand, zoom, marquee zoom |
| `core::SnapshotRequest` | `tools-basic` snapshot, `app` clipboard |
| `render::TileStore` | the canvas scrolling more pages than fit in memory |
| `core::render::Worker` | the canvas, and the first-paint budget |
| `core::search::DocumentSearch` | the find bar and the results pane |
| geometry types moved into `core` | `core` cannot depend on `plugin-api` |

Deliberately **not** built in M2, and why:

- Edit graph, history, undo, save in `core`. No M2 feature mutates a document.
  M3 adds them with `tools-comment` and `tools-organize` as the first
  consumers.
- A `Render` trait. The seam is documented and cut in the right place; making
  it a trait with one implementor buys nothing until vello exists.
- `CodecPlugin` beyond what P13's three exports need.
- Any MCP or CLI affordance.
- **`cos::Document: Send`** (the `Rc<ObjectStream>` to `Arc` swap). Both M2
  workers construct their document inside the spawned closure from the shared
  `Arc<Vec<u8>>`, so nothing in this milestone moves a constructed document
  across a thread. The swap lands in **M3 (the edit graph, where a mutated
  document may need to leave the UI thread) or M4 (MCP sessions), whichever
  first needs it**. Cheap either way; not M2's to justify.

One thing built early to avoid painting M3 into a corner: `core::Session`
holds the `cos::Document` by value and never hands out a `&cos::Document` that
outlives a call, so M3 can wrap it in the edit graph without changing every
caller. Stated here so a reviewer can check the constraint is real and not
speculative generality.

---

## 6. Deferred, with reasons, and the ledger updates

| Item | Decision | Ledger action |
|---|---|---|
| Bidi visual-to-logical reordering in search | Deferred. Presentation-form folding ships in P9 and covers single-run matches; reordering needs a bidi implementation and is post-1.0 localization work. | Narrow the `known-issues.md` entry to reordering only. |
| Advanced Search stemming | Deferred. No stemmer is in scope, and the parity row is a judgment call. | Move the row's stemming clause to post-1.0 in `ACROBAT-PARITY.md`, with the reason. |
| Pixel parity against `parity/reference/` | Deferred. The corpus does not exist and capturing it needs Acrobat driven on screen, local-only per Legal posture rule 4. M2 verifies layout parity from the row descriptions. | File a follow-up issue at the end of M2: capture the corpus, then run the comparison. Keep the `known-issues.md` entry. |
| Signature validation | M6, as the parity row already says. M2 lists signature fields only. | None; correct the PLAN.md M2 line (candor item 5). |
| OCG toggles, if the fork patch fails | Layers pane becomes read-only listing, parity row moves to M3. | Only if it happens; decided in P4. |
| Line Weights (View > Show/Hide > Line Weights) | **Cut in review; the fallback is the taken outcome.** The menu item ships disabled with a reason and the parity row moves to M3, rather than carrying a second speculative fork commit. M3 design guidance, recorded now so the M3 implementer does not repeat the wrong analysis: Acrobat's toggle draws **all strokes at a constant hairline width when off**, not a minimum-width floor on their true widths, so the fork field to add is constant-hairline-width semantics and not `min_stroke_width`. Batch it with whatever fork bump M3 takes anyway. | Move the row to M3 in `ACROBAT-PARITY.md` with this reason. |
| Find bar: Include Bookmarks, Include Comments (row 149) | Both deferred. Comments need `tools-comment` (M3); bookmarks would need P8's outline reader, and P8 depends on P9 for the results pane, so pulling it forward inverts two packages for one checkbox. Both ship disabled with a reason. | Add the cut to row 149's Notes in `ACROBAT-PARITY.md`. |
| Encrypted documents | M6. M2 fails loud with a message naming encryption and the milestone. | Add a `known-issues.md` entry: M2 cannot open the ~34 encrypted corpus files, and hayro can. **The entry must distinguish permissions-only encryption** (an `/Encrypt` dict with an empty user password, which Acrobat opens silently without prompting and which dominates real-world encrypted PDFs) from password-protected files, and record how many of the ~34 fall in each class. Losing the first class is a much worse viewer defect than losing the second, and the current entry does not separate them. |
| Empty-password decryption in `cos` | Not M2. **Reassessment point recorded at M3 planning**: `crates/crypto`'s own charter calls encryption/decryption handlers a kernel concern used by cos from the start, and PLAN.md's risk list says "M1 parses encryption; M6 writes it", so *reading* an empty-password file is arguably already inside the plan's own boundary and only the write path is M6. Decide at M3 with the class counts from the entry above in hand. | Add the reassessment point to `known-issues.md` alongside the entry. |
| `tools-organize` page-tree fixup after delete | M3. PLAN.md puts `tools-organize` at M3, so this is not M2 work; P8's thumbnail context menu ships those entries disabled. | **`known-issues.md` currently tags it "M2's tools-organize", which is stale.** Correct the tag to M3 so the ledger stops claiming an M2 deadline the milestone does not own. |
| cos fuzz on nightly in CI | Not M2 scope, but P14 touches CI. If nightly is cheap to add there, add it; if not, leave the ledger entry. | Unchanged either way. |

**One action for the orchestrator, not for this plan.** Candor item 12 says
`ACROBAT-PARITY.md` puts 67 rows in M2 that PLAN.md's M2 paragraph never
mentions. The packages above cover all 67, so the resolution is to **grow
PLAN.md's M2 paragraph** to match the scoreboard, not to move rows out. That
is a PLAN.md edit and is deliberately not made here.

---

## 7. Candor: where PLAN.md's M2 text does not survive contact with the code

Listed with evidence. Each needs a plan edit or an explicit acceptance before
implementation starts.

1. **"Progressive first paint" as written cannot work.** The plan and decision
   11 imply rendering something coarse first. The render spike's own
   measurement says per-page cost is interpretation, not rasterization area
   (`docs/spikes/m1-render-hayro.md`, "Architecture decision"), so a coarse
   render of `0041790.pdf` costs roughly what the fine one costs. First paint
   must come from a placeholder or a previously cached raster. Resolved in T3;
   the plan text should say so.

2. **"Layers with OCG visibility toggles" is not deliverable through hayro
   today.** `OcgState::from_catalog` is built inside `Context::new_with` from
   `/OCProperties /D` and `InterpreterSettings` carries no override
   (`hayro-interpret/src/ocg.rs`, `context.rs:89`). Either we extend the fork
   or the row is read-only at M2.

3. **The lazy-open budget as stated guards nothing.** Decision 11 asks for
   time-to-first-page under 200 ms on a 1000-page file. M1 already measured
   2.2 ms to first page on a 66 MB, 72-page file reading 0% of it. A wall-clock
   bench with three orders of magnitude of headroom will stay green through a
   regression that reads the whole file. The bench has to assert bytes read
   through `CountingSource`, and the plan should say bytes, not milliseconds.

4. **M2 cannot open encrypted documents, and the plan does not say so.**
   `cos::Document` returns `Error::Encrypted` and does not decrypt; hayro
   decrypts AES-256 fine (the render spike found this, contradicting hayro's
   own docs). The corpus tally in `docs/spikes/m1-cos.md` counts 34 encrypted
   files across the corpora. "Ships as a usable fast PDF viewer" is true only
   with that caveat, and a drop-in claim is not. The sharp edge is
   **permissions-only encryption**: an `/Encrypt` dict with an empty user
   password, which Acrobat opens without ever prompting and which is what most
   encrypted PDFs in the wild are. To a user those files are not "encrypted",
   they are ordinary documents that Onionskin refuses. Section 6 records the
   class split as a ledger action and an M3 reassessment point.

5. **The M2 shell list names a signatures pane; the parity row says listing at
   M2, validation at M6.** The parity row is right and the plan line
   over-promises. Fix the plan line.

6. **The plan never names the coordinate mapping, which is M2's biggest
   correctness risk.** `content::Page` uses `/MediaBox` and deliberately does
   not apply `/Rotate` (`page.rs:29-33`); hayro renders the intersected crop
   box with `/Rotate` applied (`hayro-syntax/src/page.rs:289`, `:345`). Any
   document where `/CropBox` differs from `/MediaBox` will have text selection
   and search highlights offset from what is on screen. `content::Page` does
   not even read `/CropBox`. P3 exists because of this.

7. **`cos::Document` is `!Send`, and the plan never says so.** One
   `Rc<ObjectStream>` at `document.rs:68` is the reason. It does not bite M2:
   both workers construct their document inside the spawned closure from the
   shared `Arc<Vec<u8>>`, so only bytes cross a thread boundary. It will bite
   the first design that hands an already-open document to another thread,
   which is M3's edit graph or M4's MCP sessions. Worth knowing before that
   design is drawn; not worth changing now.

8. **`content` has no document-level search.** The plan says "full-text search
   lives here (viewer Ctrl+F, search-and-redact)", and what exists is per-page
   `search(&PageText, ..)`. Whole-document find has to be driven from above,
   and doing it naively (extract all pages, hold all `PageText`) breaks the
   "memory proportional to viewed pages" line on a 1000-page file. P9 owns
   this; the plan should name it.

9. **Continuous and two-page scroll contradict xref-driven laziness.** Laying
   out a scroll needs every page's size. Acrobat estimates and refines; the
   plan says neither. P6a resolves it, but this is a real design decision the
   plan text hides.

10. **`plugin-api`'s `ToolCtx` cannot express half of `tools-basic`.** It
    carries `doc: &mut Document` only, so hand, zoom and marquee zoom, all of
    which act on the viewport, have nothing to act on. The plan's "delivered
    as the first plugins (`tools-basic`...)" assumes a contract that does not
    exist yet. P10 adds `viewport`.

11. **CI does not build the shell.** `.github/workflows/ci.yml` runs
    `cargo test --workspace` with default features, and `shell` is off by
    default, so every line of GPUI and AccessKit code M2 writes is currently
    untested by CI. macOS runners also need
    `xcodebuild -downloadComponent MetalToolchain` before gpui's shaders
    compile (`docs/spikes/m1-shell-accesskit.md`), and nobody has ever built
    gpui on Linux in this project, so that runner's system dependency set is
    unknown. P6b fixes all three, landing with the first line of GPUI code
    rather than with the benches; it is not optional polish.

12. **`ACROBAT-PARITY.md` puts 67 rows in M2, several of which the plan's own
    M2 paragraph never mentions**: document tabs, Read Mode, two-page views,
    preferences, display theme, recents, home view, `.pdf` registration,
    export to text/PNG/SVG, advanced search. They are in this plan (P7, P11,
    P13, P9) because the scoreboard is the checklist, but the plan's M2
    paragraph reads much smaller than M2 actually is. **Resolution: grow the
    paragraph**, since the packages already cover all 67 rows and moving rows
    out would shrink the milestone for no engineering reason. That is a
    PLAN.md edit for the orchestrator, recorded in section 6, not made here.

---

## 8. Definition of done for M2

- All 67 `ACROBAT-PARITY.md` M2 rows are either `implemented`, `partial` with
  a stated cut, or moved to a later milestone with a reason recorded in the
  review.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` green on macOS, Linux and Windows.
- `cargo build -p onionskin-app --features shell` and
  `cargo test -p onionskin-app --features shell` green on the runners P6b
  scoped the shell job to. (`--features shell` is not a valid workspace-level
  flag: the feature exists only on `onionskin-app`, so it must be `-p`.)
- `cargo test -p onionskin-app --no-default-features` green: kernel emptiness
  still holds.
- No crate outside `crates/app` imports GPUI, asserted by a test.
- Guarantee test 9 no longer `#[ignore]`, and its benches fail when their
  budgets are tightened.
- The VoiceOver acceptance session has run and its result is recorded in P12.
- `known-issues.md` has every M2-deadline entry either removed or narrowed,
  the stale "M2's tools-organize" tag corrected to M3, and the new entries
  this milestone earned (encryption class split, empty-password reassessment
  point, the Line Weights row moved to M3, whatever the OCG fork patch
  settles into).
- **The dogfood claim carries its caveat.** "M2 ships a usable fast PDF
  viewer" is stated with "except encrypted documents, including the
  permissions-only files Acrobat opens without prompting" attached, wherever
  it is stated: release notes, the parity scoreboard preamble, and any
  announcement. A claim that omits it is a defect, not a simplification.
