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
parity"). That phrasing is the INTERNAL engineering target; public-facing
language follows the discipline in "Legal posture".

A reference checkout of Schist lives at `/tmp/claude/schist-src` (re-clone from
GitHub if gone). Its `docs/architecture.md` is the canonical statement of the
patterns adopted below.

## Core invariant

**The original bytes are never rewritten.** Every save appends a PDF incremental
update section (ISO 32000 §7.5.6). Opening a document, editing it, and saving it
N times produces a file that still contains the byte-exact original, recoverable
by truncation. Flattening (a full rewrite that discards history) is a separate,
explicit export operation, never the default save path.

Three things the model has to be precise about before anything writes:

- **One section per save, not per edit.** A save materializes the net effect of
  every edit since the last one, so ten edits and one save append one section.
  An edit undone before the save appends nothing: a save with nothing to write
  writes nothing.
- **Undo is not truncation.** Truncation rolls a *generation* back and is an
  explicit command (the skins panel, `File > Revert`). Undo is a session-scoped
  edit stack held above the document and never touches disk. Undoing past the
  last save makes the document dirty again and the next save appends a section
  expressing the reversal. Truncation as undo is wrong whenever the section is
  not the file's last, and wrong after a Save As.
- **A document we author has nothing beneath it.** Combining, splitting,
  extracting and creating from images produce new files, so their first save is
  a complete write and the invariant applies from there: the file we wrote is
  the base sheet. This is not an exception, because nothing existed to preserve.

This is not a self-imposed handicap; it is what the PDF spec itself provides
for, and it buys three things outright:

1. **Lossless round-trip.** Nothing the parser didn't understand is ever lost,
   because unparsed objects are never re-serialized - they're simply not
   touched. Schist states the same rule for PSD blocks: "unimplemented features
   therefore mean *untouched*, never *corrupted*." For PDF, incremental updates
   make that rule structural rather than disciplinary.
2. **Free versioning.** Each incremental section is a document generation. The
   UI shows generations as sheets ("skins") and rolls one back by truncating to
   it, which is a named command rather than the undo key.
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
  storage, AI Assistant, Liquid Mode, web-based review flows. Also: rich
  media and 3D (no crate could own them), geospatial PDFs, and a virtual PDF
  printer driver (the OS already prints to PDF). Onionskin's counter-pitch is
  local and private. Export-to-Office, RTF and HTML land post-1.0 at best and
  are marked partial forever (full-fidelity document export is a product in
  itself). The print dialog's Advanced Print Setup is deliberately split:
  Print as Image and Print to File are in scope; Output, Marks and Bleeds,
  PostScript options and print color management stay with the out-of-scope
  print-production surface.
- **Legal line.** Layout geometry, workflows, shortcuts and tool naming are
  clonable; Adobe's icon artwork and trademarks are not. All icons are redrawn
  in-house - Schist's generated-logo discipline (`tools/logo.py`, constants in,
  SVGs out) applies to the whole icon set. The word "Acrobat" appears in
  comparisons, never in branding. Like Schist, Onionskin registers `.pdf` as
  openable, never as the default handler: it joins the "Open with" menu rather
  than taking files off Acrobat. The full rules, including clean-room
  discipline and public-language discipline, are in "Legal posture" below.

## Legal posture

Not legal advice; an hour with a German IP lawyer clears the mark and the
posture before any domain, org, or commercial launch. Until then these are the
working rules. Background: PDF is ISO 32000 (open since 2008, the 2.0 spec is
free via the PDF Association) and Adobe's Public Patent License grants
royalty-free rights to read, write, modify and process compliant files - the
reason Poppler, MuPDF, PDFium, pdf.js and qpdf have existed for twenty years
unsued. The realistic worst case is a trademark cease-and-desist, which the
rules below are designed to make moot.

1. **Spec-first clean room.** The reference for behavior is ISO 32000-2, never
   Acrobat's internals. No decompiling or disassembling Acrobat, ever. Black-box
   observation of the running product (how it fills a form, how it repairs a
   broken file, what a dialog looks like) is fine and is EU-lawful (SAS v World
   Programming; Software Directive 2009/24/EC Art. 5(3), with Art. 8 voiding
   EULA terms to the contrary) - but every corpus artifact derived from
   observing Acrobat (the JS-forms expected values, repair-behavior notes)
   documents that black-box provenance where it lives.
2. **Trademarks.** Off limits everywhere: Adobe, Acrobat, Reader, Distiller,
   the red A, any Adobe logo, and any name or logo that evokes them. "Acrobat"
   in prose as nominative comparison is fine. "PDF" as a descriptive word is
   fine; not styled into a logo that resembles Adobe's PDF marks.
3. **Public-language discipline.** README, website, release notes and app
   store copy never say "clone", "identical to Acrobat", or "drop-in
   replacement for Acrobat". Describe what Onionskin does (non-destructive
   editing, familiar workflows, local and private); let reviewers draw the
   comparison. The parity scoreboard is fine to publish - it's a factual
   feature comparison - but its preamble follows this rule.
4. **Reference screenshots stay private.** `parity/reference/` captures are
   Adobe's copyrighted UI artwork. They are gitignored, local-only, and never
   part of any public repo, issue, or marketing material. The scoreboard and
   parity process are public; the reference images are not.
5. **XFA is permanently out of scope.** Adobe-specified, deprecated in PDF
   2.0, and outside the clean ISO patent story. Onionskin detects XFA and
   shows a read-only notice, nothing more. Same rule for Adobe Supplement
   extensions beyond ISO 32000: not implemented.
6. **Fonts.** Adobe's font files are never shipped or embedded by us. The
   Base 14 are handled via metric-compatible free substitutes (the
   URW/Nimbus set, as Ghostscript and Poppler do); `text-engine`'s fsType
   enforcement covers third-party fonts.
7. **Patents, ours and theirs.** The Adobe patent grant has a defensive
   termination clause: never assert patents against anyone's compliant PDF
   implementation. Before adding any exotic codec, check its patent status
   (JPEG 2000 and JBIG2 essentials have expired; anything newer gets checked
   first).
8. **Code provenance.** Adapting Schist is fine (MIT, with attribution in
   LICENSE). Never copy code from Acrobat SDKs, Adobe plugins, or
   decompilation dumps posted by others.

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
│   │                       #   sections and enumerates the section chain, so
│   │                       #   generations are addressable and truncatable.
│   │                       #   Deleting an object must not leave a dangling
│   │                       #   reference: the writer validates what a section
│   │                       #   points at before emitting it. A full write from
│   │                       #   scratch exists only behind an explicit `flatten`
│   │                       #   API, used by authored documents, compression
│   │                       #   and redaction.
│   ├── core                # Kernel: document model over cos - page tree, the
│   │                       #   edit graph (base nodes = original objects,
│   │                       #   overlay nodes = pending edits), history,
│   │                       #   selection, save. Owns every page-set mutation
│   │                       #   and the fix-ups one forces (page tree, page
│   │                       #   labels, destinations, annotations): organize,
│   │                       #   combine, split and redact all need the same
│   │                       #   repairs, so no plugin owns them. Also owns the
│   │                       #   tagged-PDF structure
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
│   │                       #   this is ours end to end, in five parts: a page
│   │                       #   selection model, an imposition engine turning
│   │                       #   settings into sheets, a sheet renderer over
│   │                       #   `render`, a backend trait, and the dialog. Only
│   │                       #   the dialog is app work. Backends: print-to-file
│   │                       #   first, because it is the only one CI can check,
│   │                       #   then NSPrintOperation (macOS), CUPS (Linux), the
│   │                       #   Windows print pipeline.
│   │                       #   Acrobat print-dialog parity: page ranges,
│   │                       #   scaling, N-up, print-as-image; booklet and
│   │                       #   poster/tile are imposition over the same sheet
│   │                       #   model and ship with the later backends.
│   │                       #   GPUI-free; `app` supplies only the dialog UI.
│   ├── render              # The trait seam. CPU reference: hayro renders
│   │                       #   the base raster per (page, zoom) whole-page
│   │                       #   (hayro has no sub-rect rendering; ratified in
│   │                       #   docs/spikes/m1-render-hayro.md); tiny-skia
│   │                       #   overlays composite above it into 256x256
│   │                       #   damage-tracked tiles that cache COMPOSITES.
│   │                       #   M2 adds tile eviction. GPU backend (vello)
│   │                       #   later behind the same trait, parity-tested.
│   ├── plugin-api          # The trait surface every feature implements:
│   │                       #   ToolPlugin / CommandPlugin / CodecPlugin,
│   │                       #   PointerInput in page space, Overlay out,
│   │                       #   PluginManifest + PluginRegistry. GPUI-free.
│   ├── crypto              # Signature verification (PAdES sign later),
│   │                       #   encryption/decryption handlers used by cos.
│   ├── mcp                 # Post-1.0. Headless MCP server: sessions instead of windows,
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
│   │                       #   watermark, background, Bates numbering, crop
│   │                       #   (advanced page boxes in M5)
│   ├── tools-organize      # Organize Pages: rotate, reorder, insert, delete,
│   │                       #   extract, split, replace pages, page labels.
│   │                       #   The page-tree repairs live in `core`, not here
│   ├── tools-fill-sign     # Fill & Sign
│   ├── tools-form          # Prepare Form: AcroForm fields, appearances,
│   │                       #   scripting-driven calculate/validate/format.
│   │                       #   XFA: detect and show a read-only notice only
│   │                       #   (Legal posture, rule 5)
│   ├── redact              # Redact: content-stream rewriting, image region
│   │                       #   scrub, metadata scrub, and the full sanitize
│   │                       #   sweep (Acrobat's remove-hidden-information:
│   │                       #   scripts, hidden layers, deleted/cropped
│   │                       #   content, attachments, actions) - plus a
│   │                       #   VERIFIER that re-extracts text/images from the
│   │                       #   output and proves the target is absent. The
│   │                       #   verifier is part of the feature, not the test
│   │                       #   suite.
│   ├── tools-protect       # Protect: passwords, permissions; signature UX
│   │                       #   incl. platform keystores (Keychain / CNG /
│   │                       #   PKCS#11) for signing identities
│   ├── tools-accessibility # Accessibility toolset: checker (rule-based, like
│   │                       #   Acrobat Pro's), reading-order view/repair,
│   │                       #   Read Out Loud via platform TTS (AVSpeech /
│   │                       #   SAPI / speech-dispatcher)
│   ├── tools-measure       # Measure: distance, perimeter, area with scale
│   ├── commands-core       # menu commands: Combine files, compress/flatten
│   │                       #   export, document properties, bookmark and
│   │                       #   attachment authoring, generation rollback,
│   │                       #   print (via crates/print)
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
| 9 | PDF JavaScript ships, via Boa | Real AcroForms compute; ignoring JS fills them wrong, silently. Scope: the Acrobat forms API subset (calculate, validate, format), sandboxed with no I/O and a fuel budget. Document-level and interactive JS beyond forms: out of scope, surfaced as a visible notice. A user-facing preference disables document JavaScript entirely, mirroring Acrobat's. |
| 10 | Repair-on-open, Acrobat-grade | A large share of real PDFs are invalid; a drop-in replacement opens them. Recovery is a scan-and-rebuild path in `cos`; repaired structures are written into the first incremental section so the corrupt original survives byte-intact underneath. |
| 11 | Lazy, xref-driven parsing with pinned budgets | Acrobat opens 2000-page files instantly because it never parses ahead of need. Two budgets, enforced by benches: (1) lazy open - time-to-first-page under 200 ms on the 1000-page bench file (the corpus has none; synthesize one), guarding xref-driven laziness; (2) first paint - something visible under 200 ms on ANY page, with the full raster completing on a background thread, because a correct transparency-heavy page can cost 700+ ms in the CPU interpreter (spike-measured). Plus: memory proportional to viewed pages (tile eviction policy lands M2), 60 fps scroll on the M2 viewer. |
| 12 | Accessibility is first-class, both senses | App side: AccessKit wired into the GPUI fork so the shell exposes a real accessibility tree (Section 508 / European Accessibility Act buyers are exactly Acrobat's institutional base). Document side: `core` owns the tagged-PDF structure tree and every edit keeps it valid. That obligation starts at the first edit, not at guarantee 8: M3 builds the reader, the per-edit maintenance hook and an invariant (the `/ParentTree` resolves, no `/K` names a removed page, annotations get a `/StructParent`). Retrofitting it at M5 would mean revisiting every M3 tool and repairing documents our own earlier builds broke. M5 adds the checker on top; M3 owes the tree, not the checker. |
| 13 | Printing is our own pipeline | GPUI has none, so `crates/print` renders pages and drives NSPrintOperation / CUPS / Windows print APIs directly, with Acrobat print-dialog parity. Imposition is a pure function producing sheets, and every backend consumes those sheets, so print behaviour is checkable without a printer. Treated as a milestone deliverable, not a stretch goal. Booklet and poster/tile are imposition over the same sheet model and ship with the later backends, not with the first one. |

## What does not transfer from Schist

Choices that are right for a raster editor and would be cargo-culting here:

1. **COW tiles as the document substrate.** Schist's document *is* pixels;
   tiles are the data. Onionskin's document is an object graph and content
   streams; pixels exist only in the render cache. Tiles and damage tracking
   apply there, and undo restores an object's previous overlay state rather
   than a tile snapshot, so it costs memory proportional to changed objects,
   strictly cheaper. Dropping the overlay node is the common case, not the
   rule: an edit that overwrites an already-overlaid object has to put back
   what was there, and an edit that deletes an object present in the original
   has no node to drop. The stack holds a before and an after per changed
   object.
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

1. **Round-trip:** for every well-formed corpus file, open → save-unchanged
   produces byte-identical output (no appended section for a no-op save). The
   deliberately broken files (the malformed set, hayro's fuzzed crash
   regressions) are excluded here; they exercise test 6 instead.
2. **Onionskin:** open → edit → save produces `original bytes ++ one incremental
   section`; truncating the section yields the byte-exact original. One section
   per save however many edits it carries, and an edit undone before the save
   appends nothing.
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

Each guarantee is proved at the layer that owns the behaviour, and its test
says which layer that is; a guarantee named at one layer and proved at another
is a guarantee nobody is checking. Every guarantee whose corpus is generated or
fetched rather than tracked owes CI two steps, not one: a step that produces the
set, and a step that re-runs the enforcing test with the corpus made mandatory.
A suite that skips a missing corpus is green and measuring nothing, which is
what guarantee 6 was until the malformed set got both steps.

Corpus: hayro's regression corpus, the PDF Association sample set, veraPDF
test files, a malformed set (broken xref, junk header, truncation), a
JS-forms set with Acrobat-verified expected values, and a tagged-PDF
accessibility set derived in-repo from veraPDF's PDF/UA files, since no
ready-made one exists. No public corpus of validly signed PDFs exists, so the
test-4 set is generated in-repo: sign fixture documents with our own test CA
(M6), which also gives the verifier known-good and known-tampered cases.
`cos` gets cargo-fuzz targets from M1 onward.

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
6. **MCP as the end-to-end harness (post-1.0, with the MCP server).** Until
   then the registry is driven in-process by the app's window tests. Schist's "verified end-to-end under a
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
  rows "planned", "partial", or "out of scope"), first `parity/reference/`
  screenshots captured from the installed Acrobat Reader 25.
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
- **M2 - Viewer.** Authoritative decomposition: docs/plans/m2-viewer.md
  (14 packages, reviewed). Scope, matching the 67 M2 scoreboard rows: open
  including repaired files; the full Acrobat shell chrome (global bar,
  document tabs, quick actions, tool rail, side panel, theme, Full Screen
  and Read Mode, preferences, recents/Home view); navigation panes
  (thumbnails with context-menu page commands, bookmarks, attachments,
  layers with OCG visibility toggles via a fork patch, signatures
  read-only); the canvas with Acrobat's scroll and zoom modes and a
  dedicated coordinate-mapping layer (rotation and crop boxes - the
  milestone's biggest correctness risk); `tools-basic` through the
  registry; find bar with document-level incremental search (Unicode
  presentation-form folding ships; visual-order-to-logical reordering is
  deferred); text/PNG/SVG export via `codecs-common`; AccessKit tree live
  from the first release with correct document role mapping, and one real
  VoiceOver session as a user-gated acceptance item. Performance budgets
  (decision 11, both) enforced by benches: first paint is a placeholder or
  rescaled cached raster with the full render completing on the worker
  (coarse-then-fine buys nothing; per-page cost is interpretation), plus
  the tile eviction policy. Encrypted documents stay closed with a typed,
  fail-loud message naming the milestone; every "usable viewer" claim
  carries that caveat; reassess pulling empty-user-password decryption into
  `cos` at M3 planning (reassessed and taken, read side only; see M3).
  Line-weights view toggle moved to M3 (its fork
  patch was cut in plan review; correct semantics are constant hairline
  width, not a width floor). Ships as the first dogfoodable artifact, with
  the first parity screenshot comparison and the registry's proof of shape.
- **M3 - First edits, first print.** Authoritative decomposition:
  docs/plans/m3-edits-and-print.md (32 packages, reviewed). This is the
  identity release: the first milestone that writes a byte, and the largest by
  scoreboard weight: 99 rows assigned, 97 once ruling B moved booklet and
  poster/tile to M4, and fewer again where a package moved a row on with its
  reason (Line Weights to M4 in P22); the headline in ACROBAT-PARITY.md is the
  count. Scope, matching those rows. In `core`: the edit
  graph, transactions and a session-scoped undo stack; incremental save with
  one section per save, Save As, Revert, generations and the skins panel; a
  preview buffer, so the canvas renders committed edits from the bytes a save
  would write and the preview cannot disagree with the output; page-set
  mutation with the page-tree, page-label, destination and annotation fix-ups
  it forces. Tools and commands: `tools-organize` (rotate, reorder, insert,
  delete, extract, replace, blank pages, page labels, copy between documents);
  `tools-comment` (text markup, sticky notes, free text and callouts, ink with
  stylus pressure, shapes, stamps including dynamic ones drawn in-house,
  attach-as-comment, comment summaries); `commands-core` (Combine files and its
  file list, split by count, size or bookmark, document properties, bookmark
  and attachment authoring, and the compress/flatten export, which is the one
  deliberately destructive path M3 ships and says so in the UI);
  `codecs-common` gains image import and JPEG/TIFF export. `crates/print`
  lands in the five parts its crate entry names, with the print-to-file backend
  first and the macOS NSPrintOperation backend and dialog on top; booklet and
  poster/tile move to M4. Shell: undo/redo and save on the global bar, autosave
  and crash recovery, the Comments pane, the Organize Pages grid, Manage Tools,
  the Window menu, and the Advanced Search extensions (Line Weights moved to
  M4 with the renderer option it needs). `cos`
  gains empty-user-password decryption, read-only: those documents open,
  render, print and export but stay uneditable until M6, a stated regression
  against Acrobat for files whose permission bits would allow editing, accepted
  in exchange for no longer refusing the class outright. Guarantee tests 1, 2
  and 6 pass at the layer that owns them, with guarantee 2 driven by an edit a
  tool made through `core` rather than by a synthetic object write, and every
  guarantee M3 switches on carries the two CI steps the guarantee section
  requires. M3 also owes the tagged structure tree its reader, its maintenance
  hook and its invariant (decision 12), three milestones before guarantee 8.
  Status (2026-09-22): landed. No M3 row is left planned; the partial ones
  name their gaps, and P16's manual Mac print run is the one acceptance item
  outstanding (docs/evidence/m3-closeout.md).
- **M4 - Print everywhere.** `print` gains the CUPS and Windows backends,
  plus booklet and poster/tile imposition over M3's sheet model. The MCP
  server that M4 used to carry moved to post-1.0 (decided 2026-09-21): it is
  Onionskin-only surface, not Acrobat parity, and nothing before 1.0 depends
  on it.
  Status (2026-09-24): every row built. Booklet, poster/tile, the CUPS
  backend and View > Show/Hide > Line Weights have landed; the Windows
  backend is written, tested where it can be and type-checked, and stays
  partial until it has run on Windows (docs/evidence/m4-*.md).
- **M5 - Forms, text edit, redaction.** `tools-fill-sign` and `tools-form`
  with `scripting` live (guarantee test 7: forms compute like Acrobat),
  including form auto-complete and the JS-disable preference; `tools-edit`
  with line-level text editing backed by system-font matching/fallback and
  fsType enforcement (reflow is explicitly out of scope pre-1.0), spell
  check, find-and-replace, Bates numbering, advanced page boxes, and
  tagged-PDF maintenance on every edit (guarantee test 8); `redact` with
  sanitize and its verifier (guarantee test 3).
  Status (2026-09-24): started. In `tools-edit`, Crop Pages and Set Page
  Boxes (advanced page boxes, Change Page Size, Remove White Margins, the
  crop tool), the page marks (header and footer, watermark, background,
  Bates numbering, each with Update and Remove), and links (the Link tool,
  Create Links from URLs, Remove Web Links, following links, the Trust
  Manager's web-link policy) have landed. `tools-fill-sign` has landed:
  Add Text, checkmark, cross, dot, circle and line, and Sign with a typed,
  drawn or image signature or initials kept locally
  (docs/evidence/m5-crop-pages.md, m5-page-marks.md, m5-links.md,
  m5-fill-sign.md). `redact` has landed: marking text, regions, pages and
  search results, properties and code sets, Apply Redactions as a verified
  flattening rewrite, and Remove Hidden Information; guarantee test 3 runs
  (m5-redaction.md). Filling forms has landed: `scripting` runs the
  Acrobat forms API subset on Boa, sandboxed, and the Hand tool fills
  text fields, dropdowns, list boxes, check boxes and radio buttons with
  their keystroke, validate, calculate and format scripts, Tab between
  fields, Clear Form, and the JavaScript preference. Preparing forms has
  landed: field tools for every kind but image, and the Properties dialog
  with Format, Validate and Calculate written as Acrobat writes them, and
  Auto-Complete (Basic) with its entry list, image fields, and Detect Form
  Fields (m5-forms.md). Editing images has landed: the Edit Image tool
  moves, resizes and deletes the selected image, the Edit menu turns,
  flips, replaces and saves it, and the Add Image tool places a picture
  (m5-images.md). Editing text has landed: the Edit Text tool rewrites a
  line where it is, in its own font or a standard one of the same family,
  the Add Text tool draws a new line, and the find bar replaces one match
  or all of them (m5-text-editing.md); system-font matching and embedding
  in `text-engine` are still to come. Guarantee test 7 waits on the
  JS-forms corpus's values recorded in Acrobat. Spell check has landed:
  Edit > Check Spelling over the comments and text fields, with a user
  dictionary, in US English (m5-spelling.md). The rest of M5 (changing
  edited text's font, size and colour, and tagged-PDF maintenance
  checking) is planned.
- **M6 - Trust and accessibility.** `tools-protect`: signature verification
  and preservation (guarantee test 4), platform-keystore signing identities,
  signature appearance management, trusted-identity management, timestamping
  and LTV, encrypted-document open/save, PAdES signing.
  `tools-accessibility`: checker, reading-order repair, Read Out Loud.
  `tools-measure`.
- **Post-1.0.** The MCP server (`crates/mcp`): sessions, `describe`,
  generic invokers over the registry and `render` returning inline PNG, with
  Schist's `mcp` crate as the template; moved here from M4. Then
  `plugin-host-wasm` + `plugin-sdk`, OCR for scanned documents
  plus scan enhancement (deskew, descreen, background removal), reflowing
  text edit, XFDF, Compare Files, scanner capture, portfolios, guided
  actions (Action Wizard - a natural fit over the registry and MCP), layer
  editing (import/merge/flatten OCGs), articles, named destinations,
  embedded search indexes, barcode form fields, Loupe and Pan & Zoom
  windows, localization/bidi/vertical text, auto-update.

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
  the start (M1 parses encryption, M3 decrypts empty-user-password files for
  reading, M6 writes it), not bolted on.
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
- **Trademark letter.** The plausible legal worst case is a cease-and-desist
  over a name or mark, cheap for Adobe to send. Mitigation: the name has zero
  collision surface, no Adobe marks or lookalike iconography anywhere
  (Legal posture rules 2-4), and IP counsel clears the mark before anything
  commercial ships.
- **Dropbox-synced repo.** The working copy lives under Dropbox/Maestral; git
  and sync can race. Consider a `~/devel` checkout with the Dropbox copy as a
  mirror, matching sibling projects.
