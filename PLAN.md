# Onionskin - Project Plan

Non-destructive PDF editor in pure Rust, architected after
[Schist](https://github.com/IAmJSD/schist) (Astrid Gealer's layered image
editor, MIT-licensed, v0.6.0). The name is the architecture: the original
document is the bottom sheet, every edit generation is a translucent sheet laid
on top, and you can always see through to what's underneath.

The product goal mirrors Schist's relationship to Photoshop, applied to
Acrobat: a near pixel-level clone of Acrobat's interface and workflows, so an
Acrobat user can switch without relearning anything - a drop-in replacement
with as much of the functionality implemented as feasible (see "GUI: Acrobat
parity").

A reference checkout of Schist lives at `/tmp/claude/schist-src` (re-clone from
GitHub if gone). Its `docs/architecture.md` is the canonical statement of the
patterns adopted below.

## Core invariant

**The original bytes are never rewritten.** Every save appends a PDF incremental
update section (ISO 32000 §7.5.6). Opening a document, editing it, and saving it
N times produces a file that still contains the byte-exact original, recoverable
by truncation. Flattening (a full rewrite that discards history) is a separate,
explicit export operation, never the default save path.

This is not a self-imposed handicap; it is what the PDF spec itself provides
for, and it buys three things outright:

1. **Lossless round-trip.** Nothing the parser didn't understand is ever lost,
   because unparsed objects are never re-serialized - they're simply not
   touched. Schist states the same rule for PSD blocks: "unimplemented features
   therefore mean *untouched*, never *corrupted*." For PDF, incremental updates
   make that rule structural rather than disciplinary.
2. **Free versioning.** Each incremental section is a document generation. The
   UI shows generations as sheets ("skins") and rolls back by truncation.
3. **Signature compatibility.** An incremental update is the *only* legal way to
   annotate or counter-sign an already-signed PDF without invalidating its
   signature. Editors that rewrite on save break signatures; Onionskin can't.

The one deliberate exception is **redaction**, which must destroy data. It gets
its own plugin, its own explicit save path (a flattening rewrite), and a
built-in verifier that proves removed content is gone (see `redact` below).

## Schist's architecture (from source)

Five patterns carry the design; Onionskin adopts all of them.

1. **Microkernel plus plugins.** The kernel owns state and contracts; every
   user-facing feature - all 55 tools, 57 filters, every codec and menu
   command - is a plugin. First-party plugins are workspace crates compiled in
   through the same `plugin-api` trait surface that third-party sandboxed WASM
   plugins target. The honesty test, verbatim from its docs: "Delete every
   `plugins/` entry and the app still builds and boots — to an empty workspace
   that can do nothing." (Em-dash preserved: direct quotation.) A feature cannot reach around the API because there is
   nothing to reach for.
2. **The UI boundary.** Kernel and plugins never import GPUI. Tools receive
   `PointerInput` already transformed into document space and return `Overlay`
   primitives; `crates/app` translates in both directions. Consequences: every
   tool is unit-testable headless, and a GPUI upgrade touches exactly one
   crate.
3. **CPU reference as semantic contract.** `pixel-ops` defines correct output;
   the wgpu compute backend sits behind a `Compositor` trait seam and is
   parity-tested against the CPU to ±1 RGBA8 step, falling back per call for
   anything it can't express. Two seams (`Compositor`, `FxBackend`), one rule:
   the CPU path is the spec.
4. **Copy-on-write tiles.** All raster data lives in 256×256 COW tiles: undo
   costs memory proportional to changed pixels, duplication is free until
   written, and damage tracking falls out - only dirty, visible tiles
   recomposite.
5. **MCP over the same registry.** `schist-mcp` links the kernel and the same
   first-party plugin set the app assembles - no window, no GPU. The surface is
   deliberately generic: `describe` enumerates what's installed and four
   invokers (`run_command`, tool gestures, `apply_filter`, `apply_adjustment`)
   cover all of it, so a third-party plugin is as reachable as a built-in.
   Sessions replace windows.

Supporting choices worth copying outright (MIT license permits it): the GPUI
fork pinned by revision ([IAmJSD/gpui](https://github.com/IAmJSD/gpui), adds
pinch-to-zoom and stylus pressure over gpui 0.2.2), the packaging scripts for
all three platforms with tag-triggered signed/notarized CI builds, the
remappable `keymap.json`, and the parity-test methodology.

## GUI: Acrobat parity

What Schist is to Photoshop - its keyboard defaults, its tool taxonomy, its
context menus, "two Photoshop-parity passes" - Onionskin is to Acrobat, taken
further: the interface is a near pixel-level clone so an Acrobat user can treat
Onionskin as a drop-in replacement.

- **Reference target: the current unified Acrobat interface** (the 2023+
  redesign: quick-action top bar, left navigation panes for
  thumbnails/bookmarks/attachments/signatures, tool rail with the named
  toolsets), not the classic DC layout. That is what today's Acrobat users see
  daily; classic can become a theme later if demand shows. Acrobat Reader
  25.001 is installed at `/Applications/Adobe Acrobat Reader.app` and serves as
  the live reference; Acrobat Pro screenshots/trials cover the Pro-only
  toolsets.
- **Parity is measured, not vibed.** A screenshot corpus of the reference UI
  (each toolset, panel, dialog and state) lives in `parity/reference/`;
  `ACROBAT-PARITY.md` is the public scoreboard - every Acrobat tool and
  command, one row each: implemented / partial / planned / out of scope, with
  the reason. Schist's README counts ("55 tools, 57 filters, all nine layer
  effects") are this scoreboard's precedent.
- **The plugin taxonomy IS Acrobat's tool rail** (see workspace layout): one
  first-party plugin crate per Acrobat toolset, same names, same grouping, same
  ordering. The registry's `describe` output should read like Acrobat's tools
  list.
- **Keyboard and mouse defaults are Acrobat's**, remappable via `keymap.json`
  exactly as in Schist.
- **Out of scope, stated up front:** everything cloud-tethered - Adobe cloud
  storage, AI Assistant, Liquid Mode, web-based review flows. Onionskin's
  counter-pitch is local and private. Export-to-Office lands post-1.0 at best
  and is marked partial forever (full-fidelity DOCX export is a product in
  itself).
- **Legal line.** Layout geometry, workflows, shortcuts and tool naming are
  clonable; Adobe's icon artwork and trademarks are not. All icons are redrawn
  in-house - Schist's generated-logo discipline (`tools/logo.py`, constants in,
  SVGs out) applies to the whole icon set. The word "Acrobat" appears in
  comparisons, never in branding. Like Schist, Onionskin registers `.pdf` as
  openable, never as the default handler: it joins the "Open with" menu rather
  than taking files off Acrobat.

## Workspace layout

```
onionskin/
├── crates/                     # the kernel and its contracts - no features here
│   ├── cos                 # COS object layer: lexer/parser, xref tables &
│   │                       #   streams, object streams, filters, encryption.
│   │                       #   Every object retains its source byte span.
│   │                       #   Writer emits incremental-update sections; full
│   │                       #   rewrite exists only behind an explicit
│   │                       #   `flatten` API.
│   ├── core                # Kernel: document model over cos - page tree, the
│   │                       #   edit graph (base nodes = original objects,
│   │                       #   overlay nodes = pending edits), history,
│   │                       #   selection, save. Contains no features.
│   ├── content             # Content-stream interpretation: operators,
│   │                       #   graphics state, text runs; maps glyphs/paths
│   │                       #   back to byte spans so selection and redaction
│   │                       #   know their source.
│   ├── text-engine         # Fonts: parsing (ttf-parser/skrifa),
│   │                       #   CMap/ToUnicode, shaping for new text
│   │                       #   (rustybuzz), subsetting.
│   ├── render              # The trait seam. CPU reference: hayro for base
│   │                       #   pages + tiny-skia overlays, composited into
│   │                       #   COW render tiles with damage tracking. GPU
│   │                       #   backend (vello) added later behind the same
│   │                       #   trait, parity-tested against the CPU.
│   ├── plugin-api          # The trait surface every feature implements:
│   │                       #   ToolPlugin / CommandPlugin / CodecPlugin,
│   │                       #   PointerInput in page space, Overlay out,
│   │                       #   PluginManifest + PluginRegistry. GPUI-free.
│   ├── crypto              # Signature verification (PAdES sign later),
│   │                       #   encryption/decryption handlers used by cos.
│   ├── mcp                 # Headless MCP server: sessions instead of windows,
│   │                       #   describe + generic invokers over the registry,
│   │                       #   inline PNG page rendering. Schist's design,
│   │                       #   near-verbatim.
│   ├── cli                 # Thin CLI over the same registry (merge, split,
│   │                       #   redact, verify). Dev-loop verification harness.
│   └── app                 # GPUI shell: window, canvas, thumbnails, panels,
│                           #   keymap; translates GPUI events ⇄ plugin-api
│                           #   types. The only crate that imports GPUI.
├── plugins/                    # first-party features = Acrobat's toolsets,
│                               #   same names, grouping and order as its tool
│                               #   rail; each optional at compile time
│   ├── tools-basic         # hand, select (text/region), zoom, snapshot
│   ├── tools-comment       # Comment: highlight, underline, strikeout, sticky
│   │                       #   note, ink (stylus pressure), text box, stamps,
│   │                       #   attach-as-comment
│   ├── tools-edit          # Edit PDF: line-level text edit, image
│   │                       #   replace/transform, links, header & footer,
│   │                       #   watermark, crop
│   ├── tools-organize      # Organize Pages: rotate, reorder, insert, delete,
│   │                       #   extract, split
│   ├── tools-fill-sign     # Fill & Sign
│   ├── tools-form          # Prepare Form: AcroForm fields, appearances
│   ├── redact              # Redact: content-stream rewriting, image region
│   │                       #   scrub, metadata scrub - plus a VERIFIER that
│   │                       #   re-extracts text/images from the output and
│   │                       #   proves the target is absent. The verifier is
│   │                       #   part of the feature, not the test suite.
│   ├── tools-protect       # Protect: passwords, permissions; signature UX
│   ├── commands-core       # menu commands: Combine files, compress/flatten
│   │                       #   export, document properties, generation
│   │                       #   rollback, print
│   └── codecs-common       # Create PDF from images; PNG/SVG page export
├── corpus/                     # test PDFs (fetch script; not vendored)
└── PLAN.md
```

Post-1.0 slots, deliberately absent until their first consumer exists (YAGNI):
`plugin-host-wasm` + `plugin-sdk` (Schist's split makes this clean - the trait
surface exists from day one, the sandbox comes later), `neural` (tract-based
OCR for scanned PDFs), XFDF form-data interchange, reflowing text edit.

## Key decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Incremental-update-native model | The differentiator; see core invariant. |
| 2 | Microkernel, plugin-first from day one | Schist's load-bearing pattern. The registry is what makes the MCP surface generic and keeps contracts honest. First-party plugins are native crates; the WASM host is post-1.0. |
| 3 | Headless kernel, three registry consumers (app, mcp, cli) | Agents and scripts are first-class. MCP session/registry design adapted from Schist's `docs/mcp.md`, with structured verbs primary (see "What does not transfer", item 7). |
| 4 | Pure Rust, no pdfium/mupdf | Schist parity; avoids AGPL (mupdf) and a C build (pdfium). Base-page rendering delegated to **hayro** (pure Rust, ~1000-file regression corpus, active, vello backend in progress) so Onionskin never re-implements full PDF interpretation. |
| 5 | GPUI via the IAmJSD fork, pinned by rev | Schist ships on it; pinch-to-zoom matters for a document canvas too. The plugin-api boundary means a framework swap touches only `app`. |
| 6 | Serialization is the semantic contract; rendering is display-only | The round-trip guarantee tests play the role Schist's `pixel-ops` plays. The renderer keeps Schist's trait-seam discipline (CPU reference, GPU parity-tested, per-call fallback) but only for display consistency (see "What does not transfer", item 2). |
| 7 | Byte-span fidelity everywhere | Every model object knows its source bytes. Enables the round-trip guarantee tests, selection→source mapping, and redaction verification. |
| 8 | Redaction is a flattening export with a built-in verifier | Redaction under incremental update is a lie (old bytes remain). It must be the one destructive path, and it must prove itself. |

## What does not transfer from Schist

Choices that are right for a raster editor and would be cargo-culting here:

1. **COW tiles as the document substrate.** Schist's document *is* pixels;
   tiles are the data. Onionskin's document is an object graph and content
   streams; pixels exist only in the render cache. Tiles and damage tracking
   apply there, and undo is dropping edit-graph overlay nodes, not restoring
   tile snapshots - strictly cheaper.
2. **The renderer as semantic contract.** In Schist, compositing is the
   product: the composited pixels are what gets saved, so `pixel-ops` must be
   the spec and the GPU must be parity-tested against it. In Onionskin,
   rendering is display-only - what gets saved is operators and bytes. The
   semantic contract moves to `cos` serialization and the round-trip guarantee
   tests; CPU/GPU render parity is demoted to a display-consistency concern.
3. **The `FxBackend` seam and GPU compute kernels.** Onionskin has no filters,
   warps, or seam carving. Its hot paths - parsing, content-stream
   interpretation, text shaping - are branchy CPU work, not GPU-shaped. There
   is no `fx` crate; the GPU appears only inside vello rasterization.
4. **The pixel-buffer plugin ABI.** `schist_filter!` is essentially
   `fn(&mut [f32])` - a perfect sandbox surface, and meaningless for document
   edits. Onionskin's WASM plugin API must expose a structured document
   surface (serialized page selection in, edit operations out), which is the
   real reason `plugin-host-wasm` is post-1.0 instead of copied: the trait
   registry transfers, the WASM boundary must be redesigned.
5. **Competitor-format codecs.** Schist's interop problem is reverse-engineered
   PSD/Affinity readers and writers - a third of its engineering. PDF *is* the
   interchange format; there is no equivalent problem. The codec surface
   shrinks to image import/export and, later, XFDF.
6. **`colormgmt`, `adjustments`, `neural` as kernel crates.** ICC handling for
   display is hayro's job; adjustment layers and LUT compilation have no PDF
   analogue (optional content groups already exist natively, as do Form
   XObjects where Schist needs smart objects); ML is only ever future OCR.
7. **Gesture-first MCP.** Schist drives tools through synthesized pointer
   strokes because painting is inherently gestural. Onionskin's MCP surface is
   structured-first - `add_annotation` with a rect, `set_field_value`,
   `organize_pages` - with gestures only where the operation truly is one
   (ink). Same registry design, different primary verbs.

## Guarantee tests (the spec, as executable checks)

1. **Round-trip:** for every corpus file, open → save-unchanged produces
   byte-identical output (no appended section for a no-op save).
2. **Onionskin:** open → edit → save produces `original bytes ++ one incremental
   section`; truncating the section yields the byte-exact original.
3. **Redaction:** redact text T → verifier extracts all text and images from the
   output and finds no trace of T; a raw byte scan finds no trace of the
   original object bytes.
4. **Signature preservation:** annotating a signed corpus file keeps its
   signature valid; the UI/MCP report it as such.
5. **Kernel emptiness (Schist's honesty test):** with every `plugins/` entry
   deleted, the app still builds and boots to a workspace that can do nothing.

Corpus: hayro's regression corpus, the PDF Association sample set, and veraPDF
test files. `cos` gets cargo-fuzz targets from M1 onward.

## Testing strategy (Schist's practice, adopted)

Schist backs its parity claims with 540 tests and a specific set of mechanisms,
all visible in its source. Onionskin adopts each, shifted where the
what-does-not-transfer section demands:

1. **Registry-exhaustive property tests.** Schist's `all_filters.rs` iterates
   the whole plugin registry - "checked against all of them at once so a new
   filter cannot quietly skip them" - asserting every filter is registered with
   a name and category, leaves the buffer finite and in range, survives
   degenerate sizes, does something at its defaults, and is deterministic.
   Onionskin's equivalents, run over every registered tool and command: has
   name/icon/shortcut/group; survives degenerate documents (empty page,
   zero-object file, 50k-page file); **every edit is undoable** (apply + undo
   is identity on the edit graph); **every edit serializes** to an incremental
   section that a fresh re-parse accepts; deterministic given the same inputs.
   Completeness is enforced by construction - a new plugin inherits the
   contract by being registered.
2. **Headless UI-behavior tests.** Because tools are GPUI-free, Schist tests
   real gestures (`PointerInput` drags with modifiers) against a real document:
   `dragging_creates_an_artboard`, `a_tiny_drag_creates_nothing`. Same pattern
   here: a highlight drag over glyphs creates a highlight annotation with the
   right quad points; a tiny drag creates nothing; modifier behavior matches
   Acrobat's.
3. **Round-trip fixture tests as the semantic contract.** Schist's codecs
   re-serialize every fixture byte-for-byte. For Onionskin this is the center
   of the whole suite (guarantee tests 1–4 run per corpus file), because bytes,
   not pixels, are the product.
4. **App-shell logic pinned as unit tests**, sentence-style names, each comment
   naming the regression it pins (Schist: menu separator shape, crash-recovery
   snapshot ranking, "the preview is never magnified"). Applies to Onionskin's
   menus, keymap, tab strip, crash recovery - the shell logic that has no
   window dependency.
5. **CI gates on every push**: `cargo fmt --check`, `clippy -D warnings`,
   `cargo test --workspace`, on macOS, Linux and Windows (Schist's `ci.yml`,
   reused nearly verbatim).
6. **MCP as the end-to-end harness.** Schist's "verified end-to-end under a
   real window" is release discipline, not an automated harness - but its MCP
   server makes the entire registry drivable headless with inline PNG renders,
   which is what makes that discipline cheap. Onionskin does the same and goes
   one step further: a scripted pre-release pass drives every registry entry
   through MCP, renders before/after, and diffs the after-render against a
   stored expectation. UI parity itself is checked visually against the
   `parity/reference/` Acrobat screenshot corpus per release.
7. **The public scoreboard.** `ACROBAT-PARITY.md` plays the role of Schist's
   README counts ("55 tools, 57 filters, all nine layer effects") - parity is
   a number that can go up, not an adjective.

## Milestones

- **M0 - Scaffold.** Workspace with `crates/` + `plugins/` split, `plugin-api`
  with registry and manifest (adapted from Schist's `crates/plugin-api`), CI
  (adapt Schist's `ci.yml`, `packaging/` and tag-triggered release workflow),
  corpus fetch script, guarantee tests wired up as failing/ignored,
  `ACROBAT-PARITY.md` seeded with the full Acrobat tool/command inventory (all
  rows "planned" or "out of scope"), first `parity/reference/` screenshots
  captured from the installed Acrobat Reader 25.
- **M1 - Spikes (de-risk the three bets).**
  (a) `cos` parse→incremental-save round-trip over the corpus;
  (b) hayro base raster + tiny-skia overlay composited into damage-tracked
  tiles;
  (c) GPUI window (IAmJSD fork) rendering that tile cache with pan/zoom/pinch.
  Spikes are throwaway-permitted; what survives is the decision record.
- **M2 - Viewer.** Open, render, navigate, text selection via `content`
  byte-span mapping, delivered as the first plugins (`tools-basic`,
  `codecs-common` export) - already wearing the Acrobat shell: quick-action
  top bar, left navigation panes, tool rail, Acrobat shortcuts. Ships as a
  usable fast PDF viewer - first dogfoodable artifact, first parity
  screenshots compared, and the registry's proof of shape.
- **M3 - First edits.** `tools-comment`, `tools-organize`, `commands-core`
  (Combine files, split), incremental save, undo/redo, the skins panel.
  Guarantee tests 1–2 pass. This is the identity release.
- **M4 - MCP server.** Sessions, `describe`, generic invokers over the
  registry - Schist's `mcp` crate as the template. Everything M3 can do,
  agent-driven, plus `render` returning inline PNG.
- **M5 - Forms, text edit, redaction.** `tools-fill-sign` and `tools-form`;
  `tools-edit` with line-level text editing (glyph runs re-shaped within a
  line - reflow is explicitly out of scope pre-1.0); `redact` with its
  verifier (guarantee test 3).
- **M6 - Trust features.** `tools-protect`: signature verification and
  preservation (guarantee test 4), encrypted-document open/save, PAdES
  signing.
- **Post-1.0.** `plugin-host-wasm` + `plugin-sdk`, OCR for scanned documents,
  reflowing text edit, XFDF.

## Risks

- **PDF breadth.** Mitigated by delegating interpretation/rendering of base
  content to hayro and never re-serializing what we don't model.
- **Text editing.** The famous tar pit. Scoped to line-level pre-1.0; the
  incremental model means a bad edit never damages the original.
- **GPUI churn / platform reach.** The fork is pinned by rev; Schist proves
  macOS + Linux + Windows all ship from it. `app` is the only GPUI importer.
  Watch for the fork lagging upstream gpui.
- **Encryption × incremental updates.** Appending to an encrypted file requires
  encrypting new objects with the existing key material - handled in `cos` from
  the start (M1 parses encryption; M6 writes it), not bolted on.
- **The parity target moves.** Adobe reshapes Acrobat's UI continuously (and
  ships two coexisting interfaces already). Pin each release cycle to a dated
  `parity/reference/` screenshot set from a named Acrobat version rather than
  chasing the live product.
- **Dropbox-synced repo.** The working copy lives under Dropbox/Maestral; git
  and sync can race. Consider a `~/devel` checkout with the Dropbox copy as a
  mirror, matching sibling projects.
