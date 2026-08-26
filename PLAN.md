# Onionskin — Project Plan

Non-destructive PDF editor in pure Rust, architected after
[Schist](https://github.com/IAmJSD/schist) (Astrid Gealer's layered image
editor, MIT-licensed, v0.6.0). The name is the architecture: the original
document is the bottom sheet, every edit generation is a translucent sheet laid
on top, and you can always see through to what's underneath.

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
   because unparsed objects are never re-serialized — they're simply not
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
   user-facing feature — all 55 tools, 57 filters, every codec and menu
   command — is a plugin. First-party plugins are workspace crates compiled in
   through the same `plugin-api` trait surface that third-party sandboxed WASM
   plugins target. The honesty test, verbatim from its docs: "Delete every
   `plugins/` entry and the app still builds and boots — to an empty workspace
   that can do nothing." A feature cannot reach around the API because there is
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
   written, and damage tracking falls out — only dirty, visible tiles
   recomposite.
5. **MCP over the same registry.** `schist-mcp` links the kernel and the same
   first-party plugin set the app assembles — no window, no GPU. The surface is
   deliberately generic: `describe` enumerates what's installed and four
   invokers (`run_command`, tool gestures, `apply_filter`, `apply_adjustment`)
   cover all of it, so a third-party plugin is as reachable as a built-in.
   Sessions replace windows.

Supporting choices worth copying outright (MIT license permits it): the GPUI
fork pinned by revision ([IAmJSD/gpui](https://github.com/IAmJSD/gpui), adds
pinch-to-zoom and stylus pressure over gpui 0.2.2), the packaging scripts for
all three platforms with tag-triggered signed/notarized CI builds, the
remappable `keymap.json`, and the parity-test methodology.

## Workspace layout

```
onionskin/
├── crates/                     # the kernel and its contracts — no features here
│   ├── cos                 # COS object layer: lexer/parser, xref tables &
│   │                       #   streams, object streams, filters, encryption.
│   │                       #   Every object retains its source byte span.
│   │                       #   Writer emits incremental-update sections; full
│   │                       #   rewrite exists only behind an explicit
│   │                       #   `flatten` API.
│   ├── core                # Kernel: document model over cos — page tree, the
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
├── plugins/                    # first-party features, each optional at compile time
│   ├── tools-annotate      # highlight, underline, strikeout, note, ink, stamp
│   ├── tools-select        # text selection, region select, snapshot
│   ├── tools-pages         # rotate, reorder, insert, delete, extract
│   ├── tools-form          # AcroForm field model, fill, appearance streams
│   ├── redact              # true redaction: content-stream rewriting, image
│   │                       #   region scrub, metadata scrub — plus a VERIFIER
│   │                       #   that re-extracts text/images from the output
│   │                       #   and proves the target is absent. The verifier
│   │                       #   is part of the feature, not the test suite.
│   ├── commands-core       # menu commands: merge/split, flatten-export,
│   │                       #   document properties, generation rollback
│   └── codecs-common       # image→page import; PNG/SVG page export
├── corpus/                     # test PDFs (fetch script; not vendored)
└── PLAN.md
```

Post-1.0 slots, deliberately absent until their first consumer exists (YAGNI):
`plugin-host-wasm` + `plugin-sdk` (Schist's split makes this clean — the trait
surface exists from day one, the sandbox comes later), `neural` (tract-based
OCR for scanned PDFs), XFDF form-data interchange, reflowing text edit.

## Key decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Incremental-update-native model | The differentiator; see core invariant. |
| 2 | Microkernel, plugin-first from day one | Schist's load-bearing pattern. The registry is what makes the MCP surface generic and keeps contracts honest. First-party plugins are native crates; the WASM host is post-1.0. |
| 3 | Headless kernel, three registry consumers (app, mcp, cli) | Agents and scripts are first-class. MCP invoker design copied from Schist's `docs/mcp.md`. |
| 4 | Pure Rust, no pdfium/mupdf | Schist parity; avoids AGPL (mupdf) and a C build (pdfium). Base-page rendering delegated to **hayro** (pure Rust, ~1000-file regression corpus, active, vello backend in progress) so Onionskin never re-implements full PDF interpretation. |
| 5 | GPUI via the IAmJSD fork, pinned by rev | Schist ships on it; pinch-to-zoom matters for a document canvas too. The plugin-api boundary means a framework swap touches only `app`. |
| 6 | CPU render path is the semantic contract | GPU (vello) arrives later behind the `render` trait seam, parity-tested, falling back per call — Schist's compositor discipline, applied to page rendering. |
| 7 | Byte-span fidelity everywhere | Every model object knows its source bytes. Enables the round-trip guarantee tests, selection→source mapping, and redaction verification. |
| 8 | Redaction is a flattening export with a built-in verifier | Redaction under incremental update is a lie (old bytes remain). It must be the one destructive path, and it must prove itself. |

Where Onionskin structurally differs from Schist: the document is streams and
objects, not raster, so COW tiles apply to the *render cache* (damage-tracked
page tiles) rather than to document data, and undo is dropping edit-graph
overlay nodes rather than restoring tile snapshots — strictly cheaper.

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

## Milestones

- **M0 — Scaffold.** Workspace with `crates/` + `plugins/` split, `plugin-api`
  with registry and manifest (adapted from Schist's `crates/plugin-api`), CI
  (adapt Schist's `packaging/` and tag-triggered release workflow), corpus
  fetch script, guarantee tests wired up as failing/ignored.
- **M1 — Spikes (de-risk the three bets).**
  (a) `cos` parse→incremental-save round-trip over the corpus;
  (b) hayro base raster + tiny-skia overlay composited into damage-tracked
  tiles;
  (c) GPUI window (IAmJSD fork) rendering that tile cache with pan/zoom/pinch.
  Spikes are throwaway-permitted; what survives is the decision record.
- **M2 — Viewer.** Open, render, navigate, text selection via `content`
  byte-span mapping, delivered as the first plugins (`tools-select`,
  `codecs-common` export). Ships as a usable fast PDF viewer — first
  dogfoodable artifact, and the registry's proof of shape.
- **M3 — First edits.** `tools-annotate`, `tools-pages`, `commands-core`
  (merge/split), incremental save, undo/redo, the skins panel. Guarantee tests
  1–2 pass. This is the identity release.
- **M4 — MCP server.** Sessions, `describe`, generic invokers over the
  registry — Schist's `mcp` crate as the template. Everything M3 can do,
  agent-driven, plus `render` returning inline PNG.
- **M5 — Forms, text edit, redaction.** `tools-form`; line-level text editing
  (glyph runs re-shaped within a line — reflow is explicitly out of scope
  pre-1.0); `redact` with its verifier (guarantee test 3).
- **M6 — Trust features.** Signature verification and preservation (guarantee
  test 4), encrypted-document open/save, PAdES signing.
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
  encrypting new objects with the existing key material — handled in `cos` from
  the start (M1 parses encryption; M6 writes it), not bolted on.
- **Dropbox-synced repo.** The working copy lives under Dropbox/Maestral; git
  and sync can race. Consider a `~/devel` checkout with the Dropbox copy as a
  mirror, matching sibling projects.
