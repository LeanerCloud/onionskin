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
│   │                       #   LAZY by construction: objects parse on demand
│   │                       #   via the xref, never the whole file up front.
│   │                       #   REPAIRING like Acrobat: broken xref, junk
│   │                       #   before the header, truncation all recover via
│   │                       #   a scan-and-rebuild path; repaired structures
│   │                       #   are written into the first incremental section
│   │                       #   while the corrupt original stays byte-intact
│   │                       #   beneath. Every object retains its source byte
│   │                       #   span. Writer emits incremental-update
│   │                       #   sections; full rewrite exists only behind an
│   │                       #   explicit `flatten` API.
│   ├── core                # Kernel: document model over cos - page tree, the
│   │                       #   edit graph (base nodes = original objects,
│   │                       #   overlay nodes = pending edits), history,
│   │                       #   selection, save. Owns the tagged-PDF structure
│   │                       #   tree: every edit that touches tagged content
│   │                       #   must keep the tree valid (accessible documents
│   │                       #   stay accessible through edits). Contains no
│   │                       #   features.
│   ├── content             # Content-stream interpretation: operators,
│   │                       #   graphics state, text runs; maps glyphs/paths
│   │                       #   back to byte spans so selection, search and
│   │                       #   redaction know their source. Full-text search
│   │                       #   lives here (viewer Ctrl+F, search-and-redact).
│   ├── text-engine         # Fonts: parsing (ttf-parser/skrifa),
│   │                       #   CMap/ToUnicode, shaping for new text
│   │                       #   (rustybuzz), subsetting. System-font matching
│   │                       #   and fallback for glyphs missing from embedded
│   │                       #   subsets (the thing that makes text editing
│   │                       #   work on real documents, not demos), and
│   │                       #   fsType embedding-permission enforcement.
│   ├── scripting           # PDF JavaScript via Boa (pure Rust): the Acrobat
│   │                       #   forms API subset - field calculation,
│   │                       #   validation, formatting (AFNumber_Format,
│   │                       #   AFSimple_Calculate and friends). Sandboxed: no
│   │                       #   I/O, no network, fuel-budgeted like Schist's
│   │                       #   WASM host. Real-world AcroForms compute or
│   │                       #   they are filled wrong; this is not optional
│   │                       #   for a drop-in claim.
│   ├── print               # Printing. GPUI has none (Zed never prints), so
│   │                       #   this is ours end to end: render pages via
│   │                       #   `render`, hand off to NSPrintOperation (macOS),
│   │                       #   CUPS (Linux), the Windows print pipeline.
│   │                       #   Acrobat print-dialog parity: page ranges,
│   │                       #   scaling, N-up, booklet, print-as-image.
│   │                       #   GPUI-free; `app` supplies only the dialog UI.
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
│   ├── tools-edit          # Edit PDF: line-level text edit (backed by
│   │                       #   text-engine font matching), image
│   │                       #   replace/transform, links, header & footer,
│   │                       #   watermark, Bates numbering, crop
│   ├── tools-organize      # Organize Pages: rotate, reorder, insert, delete,
│   │                       #   extract, split
│   ├── tools-fill-sign     # Fill & Sign
│   ├── tools-form          # Prepare Form: AcroForm fields, appearances,
│   │                       #   scripting-driven calculate/validate/format
│   ├── redact              # Redact: content-stream rewriting, image region
│   │                       #   scrub, metadata scrub - plus a VERIFIER that
│   │                       #   re-extracts text/images from the output and
│   │                       #   proves the target is absent. The verifier is
│   │                       #   part of the feature, not the test suite.
│   ├── tools-protect       # Protect: passwords, permissions; signature UX
│   │                       #   incl. platform keystores (Keychain / CNG /
│   │                       #   PKCS#11) for signing identities
│   ├── tools-accessibility # Accessibility toolset: checker (rule-based, like
│   │                       #   Acrobat Pro's), reading-order view/repair,
│   │                       #   Read Out Loud via platform TTS (AVSpeech /
│   │                       #   SAPI / speech-dispatcher)
│   ├── tools-measure       # Measure: distance, perimeter, area with scale
│   ├── commands-core       # menu commands: Combine files, compress/flatten
│   │                       #   export, document properties, generation
│   │                       #   rollback, print (via crates/print)
│   └── codecs-common       # Create PDF from images; PNG/SVG page export
├── corpus/                     # test PDFs incl. a malformed set and a
│                               # JS-forms set (fetch script; not vendored)
└── PLAN.md
```

Post-1.0 slots, deliberately absent until their first consumer exists (YAGNI):
`plugin-host-wasm` + `plugin-sdk` (Schist's split makes this clean - the trait
surface exists from day one, the sandbox comes later), `neural` (tract-based
OCR for scanned PDFs), XFDF form-data interchange, reflowing text edit,
Compare Files, scanner capture (ICA/TWAIN/WIA), PDF portfolios, UI
localization plus bidi/vertical text, auto-update (Schist's Check for Updates
path as template). Preflight/print production is out of scope permanently (a
product in itself); it gets a scoreboard row saying so.

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
| 9 | PDF JavaScript ships, via Boa | Real AcroForms compute; ignoring JS fills them wrong, silently. Scope: the Acrobat forms API subset (calculate, validate, format), sandboxed with no I/O and a fuel budget. Document-level and interactive JS beyond forms: out of scope, surfaced as a visible notice. |
| 10 | Repair-on-open, Acrobat-grade | A large share of real PDFs are invalid; a drop-in replacement opens them. Recovery is a scan-and-rebuild path in `cos`; repaired structures are written into the first incremental section so the corrupt original survives byte-intact underneath. |
| 11 | Lazy, xref-driven parsing with pinned budgets | Acrobat opens 2000-page files instantly because it never parses ahead of need. Budgets (enforced by benches, not aspiration): time-to-first-page under 200 ms on the 1000-page corpus file, memory proportional to viewed pages rather than file size, 60 fps scroll on the M2 viewer. |
| 12 | Accessibility is first-class, both senses | App side: AccessKit wired into the GPUI fork so the shell exposes a real accessibility tree (Section 508 / European Accessibility Act buyers are exactly Acrobat's institutional base). Document side: `core` owns the tagged-PDF structure tree and every edit keeps it valid. |
| 13 | Printing is our own pipeline | GPUI has none, so `crates/print` renders pages and drives NSPrintOperation / CUPS / Windows print APIs directly, with Acrobat print-dialog parity. Treated as a milestone deliverable, not a stretch goal. |

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
6. **Repair:** every file in the malformed corpus set opens; saving appends an
   incremental section containing the repaired structures; the corrupt
   original bytes are preserved beneath, byte-intact.
7. **Forms compute:** every file in the JS-forms corpus set fills like Acrobat
   does - computed fields recalculate, formats apply, validation fires -
   verified against Acrobat-produced expected values.
8. **Tag integrity:** editing a tagged corpus document leaves its structure
   tree valid and consistent with the edited content (checked by
   `tools-accessibility`'s own checker, eating its own dog food).
9. **Performance:** the decision-11 budgets run as benches in CI; a regression
   past budget fails the build like any other test.

Corpus: hayro's regression corpus, the PDF Association sample set, veraPDF
test files, a malformed set (broken xref, junk header, truncation), a
JS-forms set with Acrobat-verified expected values, and tagged-PDF
accessibility samples. `cos` gets cargo-fuzz targets from M1 onward.

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
   of the whole suite (the byte-level guarantee tests 1–4 and 6 run per corpus
   file, the behavioral ones 7–9 per specialty corpus), because bytes, not
   pixels, are the product.
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
- **M1 - Spikes (de-risk the four bets).**
  (a) `cos` parse→incremental-save round-trip over the corpus, lazy via the
  xref from the first line, including the scan-and-rebuild repair path over
  the malformed set;
  (b) hayro base raster + tiny-skia overlay composited into damage-tracked
  tiles;
  (c) GPUI window (IAmJSD fork) rendering that tile cache with pan/zoom/pinch;
  (d) AccessKit attached to a GPUI window with VoiceOver reading a focused
  element - proves the accessibility bet before the shell is built on it.
  Spikes are throwaway-permitted; what survives is the decision record.
- **M2 - Viewer.** Open (including repaired files), render, navigate,
  full-text search (Ctrl+F, highlight-all, next/previous), text selection via
  `content` byte-span mapping, delivered as the first plugins (`tools-basic`,
  `codecs-common` export) - already wearing the Acrobat shell: quick-action
  top bar, left navigation panes (thumbnails, bookmarks, attachments,
  signatures), tool rail, Acrobat shortcuts, AccessKit tree live from the
  first release. Performance budgets (decision 11) enforced from here on.
  Ships as a usable fast PDF viewer - first dogfoodable artifact, first
  parity screenshots compared, and the registry's proof of shape.
- **M3 - First edits, first print.** `tools-comment`, `tools-organize`,
  `commands-core` (Combine files, split), incremental save, undo/redo, the
  skins panel. `crates/print` lands with the macOS backend and the Acrobat
  print dialog. Guarantee tests 1–2 and 6 pass. This is the identity release.
- **M4 - MCP server, print everywhere.** Sessions, `describe`, generic
  invokers over the registry - Schist's `mcp` crate as the template.
  Everything M3 can do, agent-driven, plus `render` returning inline PNG.
  `print` gains the CUPS and Windows backends.
- **M5 - Forms, text edit, redaction.** `tools-fill-sign` and `tools-form`
  with `scripting` live (guarantee test 7: forms compute like Acrobat);
  `tools-edit` with line-level text editing backed by system-font
  matching/fallback and fsType enforcement (reflow is explicitly out of scope
  pre-1.0), Bates numbering, and tagged-PDF maintenance on every edit
  (guarantee test 8); `redact` with its verifier (guarantee test 3).
- **M6 - Trust and accessibility.** `tools-protect`: signature verification
  and preservation (guarantee test 4), platform-keystore signing identities,
  encrypted-document open/save, PAdES signing. `tools-accessibility`: checker,
  reading-order repair, Read Out Loud. `tools-measure`.
- **Post-1.0.** `plugin-host-wasm` + `plugin-sdk`, OCR for scanned documents,
  reflowing text edit, XFDF, Compare Files, scanner capture, portfolios,
  localization/bidi/vertical text, auto-update.

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
- **GPUI accessibility.** Neither upstream GPUI nor the fork exposes an
  accessibility tree today; AccessKit integration is upstream-sized work
  inside the fork, and the M1 spike exists to size it before the shell
  depends on it. If the spike fails, this risk escalates to
  reconsider-the-framework, which is why it is a spike and not an M6 line.
- **Boa completeness and performance.** Boa is the only viable pure-Rust JS
  engine; the Acrobat forms API subset is small, but engine gaps or slow
  paths would surface as forms that misbehave. Mitigation: the JS-forms
  corpus with Acrobat-verified expected values is the contract, and any form
  whose scripts fail to run gets a visible notice rather than silent wrong
  values.
- **Printing is hand-rolled per platform.** No framework help, three
  platform APIs, and print bugs are trust-killers in exactly the offices a
  drop-in replacement targets. Mitigation: print-to-PDF-file backend first
  (testable in CI against expected output), platform backends behind the same
  trait.
- **Dropbox-synced repo.** The working copy lives under Dropbox/Maestral; git
  and sync can race. Consider a `~/devel` checkout with the Dropbox copy as a
  mirror, matching sibling projects.
