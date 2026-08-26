# Onionskin — Project Plan

Non-destructive PDF editor in pure Rust, architected after Schist (Astrid Gealer's
layered image editor). The name is the architecture: the original document is the
bottom sheet, every edit generation is a translucent sheet laid on top, and you can
always see through to what's underneath.

## Core invariant

**The original bytes are never rewritten.** Every save appends a PDF incremental
update section (ISO 32000 §7.5.6). Opening a document, editing it, and saving it N
times produces a file that still contains the byte-exact original, recoverable by
truncation. Flattening (a full rewrite that discards history) is a separate,
explicit export operation, never the default save path.

This is not a self-imposed handicap; it is what the PDF spec itself provides for,
and it buys three things outright:

1. **Lossless round-trip.** Nothing the parser didn't understand is ever lost,
   because unparsed objects are never re-serialized — they're simply not touched.
   This sidesteps the classic PDF-editor failure mode of corrupting what you
   didn't model.
2. **Free versioning.** Each incremental section is a document generation. The UI
   can show generations as sheets ("skins") and roll back by truncation.
3. **Signature compatibility.** An incremental update is the *only* legal way to
   annotate or counter-sign an already-signed PDF without invalidating its
   signature. Editors that rewrite on save break signatures; Onionskin can't.

The one deliberate exception is **redaction**, which must destroy data. It gets its
own crate, its own explicit save path (a flattening rewrite), and a built-in
verifier that proves removed content is gone (see `redact` below).

## What "Schist's architecture" concretely is

Derived from the shipped binaries (`Schist.app` 0.4.0 and `schist-mcp`), which
embed the workspace layout and dependency list:

- Pure Rust, single Cargo workspace, ~15 focused crates, one shipped binary per
  frontend. No C/C++ dependencies.
- A **headless `core` document model** (a graph: base content plus ordered edit
  nodes) with zero UI dependencies.
- **Two frontends over the same core**: a GPUI desktop app (`crates/app`) and a
  headless MCP server (`crates/mcp`) so AI agents are first-class users, not an
  afterthought.
- **Codec crates per foreign format** (`codec-psd`, `codec-affinity`) that import
  competitors' files into the native model.
- **Split CPU and GPU compositors** (`compositor`, `compositor-gpu` on wgpu).
- Domain-specific operation crates (`pixel-ops`, `adjustments`, `fx`, `layer-fx`,
  `vector`, `text-engine`, `colormgmt`).
- `plugin-host-wasm` (wasmtime + cranelift) for sandboxed plugins.
- `neural` (tract/ONNX) for on-device ML.

Onionskin adopts the same shape: headless core, multiple thin frontends, one crate
per domain, pure Rust, agents as first-class users.

## Workspace layout

```
onionskin/
├── crates/
│   ├── cos               # COS object layer: lexer/parser, xref tables & streams,
│   │                     #   object streams, filters, encryption handlers.
│   │                     #   Every object retains its source byte span. Writer
│   │                     #   emits incremental-update sections; full rewrite
│   │                     #   exists only behind an explicit `flatten` API.
│   ├── core              # Document model over cos: page tree, the edit graph
│   │                     #   (base nodes = original objects, overlay nodes =
│   │                     #   pending edits), snapshots for undo/redo, save.
│   ├── content           # Content-stream interpretation: operators, graphics
│   │                     #   state, text runs; maps glyphs/paths back to byte
│   │                     #   spans so selection and redaction know their source.
│   ├── text-engine       # Fonts: parsing (ttf-parser/skrifa), CMap/ToUnicode,
│   │                     #   shaping for new text (rustybuzz), subsetting.
│   ├── render            # Page rasterization. Untouched base pages via hayro;
│   │                     #   overlay edits via vello (GPU) / tiny-skia (CPU),
│   │                     #   mirroring Schist's compositor / compositor-gpu split.
│   ├── ops               # User-level operations: annotations, page
│   │                     #   insert/delete/reorder/rotate, merge/split,
│   │                     #   stamps/watermarks, attachments, form fill.
│   ├── forms             # AcroForm model, appearance-stream generation,
│   │                     #   XFA detection (read-only warning, not support).
│   ├── redact            # True redaction: content-stream rewriting, image
│   │                     #   region scrubbing, metadata scrub, plus a VERIFIER
│   │                     #   that re-extracts text/images from the output and
│   │                     #   proves the target content is absent. The verifier
│   │                     #   is part of the feature, not the test suite.
│   ├── crypto            # Digital signatures (verify first, PAdES sign later),
│   │                     #   encryption/decryption handlers.
│   ├── mcp               # Headless MCP server binary over core: open, inspect,
│   │                     #   annotate, fill, assemble, redact, save.
│   ├── cli               # Thin CLI over core (merge/split/redact/verify).
│   │                     #   Doubles as the dev-loop verification harness.
│   └── app               # GPUI desktop shell: canvas, thumbnails, tool panels,
│                         #   and the signature "skins" panel — one row per
│                         #   incremental-update generation, toggleable.
├── corpus/               # Test PDFs (submodule or fetch script; not vendored).
└── PLAN.md
```

Post-1.0 slots, mirroring Schist, deliberately absent from the initial tree
(YAGNI — each gets created when its first consumer exists):
`plugin-host-wasm` (wasmtime), `neural` (tract-based OCR for scanned PDFs),
`codec-xfdf` (form data interchange), image→PDF import.

## Key decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Incremental-update-native model | The differentiator; see core invariant. |
| 2 | Headless core, three consumers (app, mcp, cli) | Schist parity; agents and scripts are first-class. |
| 3 | Pure Rust, no pdfium/mupdf | Schist parity; avoids AGPL (mupdf) and a C build (pdfium). Base-page rendering delegated to **hayro** (LaurenzV, pure Rust, ~1000-file regression corpus, active in 2026, vello GPU backend in progress) so Onionskin never re-implements full PDF interpretation. |
| 4 | GPUI for the desktop shell | Schist parity (it ships on gpui 0.2.x from crates.io). Risk: Zed-controlled API churn — pin versions, keep `app` thin so a framework swap only touches one crate. |
| 5 | Byte-span fidelity everywhere | Every model object knows its source bytes. Enables the round-trip guarantee test (below), selection→source mapping, and redaction verification. |
| 6 | Redaction is a flattening export with a built-in verifier | Redaction under incremental update is a lie (old bytes remain). It must be the one destructive path, and it must prove itself. |

## Guarantee tests (the spec, as executable checks)

1. **Round-trip:** for every corpus file, open → save-unchanged produces byte-
   identical output (no appended section for a no-op save).
2. **Onionskin:** open → edit → save produces `original bytes ++ one incremental
   section`; truncating the section yields the byte-exact original.
3. **Redaction:** redact text T → verifier extracts all text and images from the
   output and finds no trace of T; raw byte scan of the output finds no trace of
   the original object bytes.
4. **Signature preservation:** annotating a signed corpus file keeps its
   signature valid; the UI/MCP report it as such.

Corpus: hayro's regression corpus, the PDF Association sample set, and veraPDF
test files. `cos` gets cargo-fuzz targets from M1 onward.

## Milestones

- **M0 — Scaffold.** Workspace, CI (GitHub Actions, like Schist), corpus fetch
  script, guarantee tests wired up as failing/ignored.
- **M1 — Spikes (de-risk the three bets).**
  (a) `cos` parse→incremental-save round-trip over the corpus;
  (b) hayro base raster + vello overlay composited in one texture;
  (c) GPUI window rendering that texture with pan/zoom.
  Each spike is throwaway-permitted; what survives is the decision record.
- **M2 — Viewer.** Open, render, navigate, text selection via `content` byte-span
  mapping. Ships as a usable fast PDF viewer (first dogfoodable artifact).
- **M3 — First edits.** Annotations (highlight, note, ink, stamp), page ops
  (rotate/reorder/delete/insert/merge/split), incremental save, undo/redo, the
  skins panel. Guarantee tests 1–2 must pass. This is the identity release.
- **M4 — MCP server.** `onionskin-mcp` over core: everything M3 can do,
  agent-driven. Small surface, big leverage.
- **M5 — Forms, text edit, redaction.** Form fill; line-level text editing
  (glyph runs re-shaped within a line — no reflow; reflow is explicitly out of
  scope pre-1.0); redaction + verifier (guarantee test 3).
- **M6 — Trust features.** Signature verification and preservation (guarantee
  test 4), encrypted-document open/save, PAdES signing.
- **Post-1.0.** WASM plugins, OCR for scanned documents, reflowing text edit,
  image→PDF import, XFDF.

## Risks

- **PDF breadth.** Mitigated by delegating interpretation/rendering of base
  content to hayro and never re-serializing what we don't model.
- **Text editing.** The famous tar pit. Scoped to line-level pre-1.0; the
  incremental model means a bad edit never damages the original.
- **GPUI churn / platform reach.** macOS-first (matches Schist, macOS 11+);
  Linux later, Windows when GPUI is ready. `app` stays thin.
- **Encryption × incremental updates.** Appending to an encrypted file requires
  encrypting new objects with the existing key material — handled in `cos` from
  the start (M1 parses encryption; M6 writes it), not bolted on.
- **Dropbox-synced repo.** The working copy lives under Dropbox/Maestral; git and
  sync can race. Consider moving the checkout to `~/devel` with the Dropbox copy
  as a mirror, matching the pattern used by sibling projects.
