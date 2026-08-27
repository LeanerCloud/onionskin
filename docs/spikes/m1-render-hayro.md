# M1 spike (b): hayro base rendering, overlays, tile cache

Verdict: the hayro-under / overlay-over pipeline HOLDS. Keep it. Reviewed and
approved with all load-bearing claims verified by execution; the plan
adjustments below were ratified.

## What was proven

- hayro 0.7.1 renders the corpus correctly across Type 1/CID fonts, RTL and
  CJK scripts, shadings, JPX, CalRGB, PDF 2.0 samples, and (contradicting its
  own docs) AES-256 encrypted files. Zero interpreter warnings on the
  evidence set.
- tiny-skia overlays (multiply highlight in /QuadPoints order, round-join
  ink) composite over the base with no tile seams; a boundary-diff test pins
  the Schist seam bug at zero differing boundary pixels.
- Damage tracking works and is cheap: ~0.09 ms per 256x256 tile composite; a
  one-tile ink recomposite costs ~0.1 ms and never re-enters the interpreter.
- Time-to-first-page on a 10000-page file: ~17 ms. Interop is free: hayro's
  premultiplied RGBA8 rows memcpy straight into tiny-skia.

## Architecture decision (ratified)

Tiles cache COMPOSITES, not base rasters. hayro cannot render a
sub-rectangle (RenderSettings has no origin; its transform-taking Renderer is
crate-private), and measurement shows per-page cost is interpretation, not
rasterization area, so per-tile base rendering would buy little. The base
raster is one whole-page allocation per (page, zoom); tiles sit above it and
overlay edits never touch the base. The Render trait seam cuts under
"produce the base raster for page P at zoom Z"; grid, damage and overlay
compositing stay backend-independent for the future vello path.

Riders: memory is base + tiles, roughly 2x page pixels per (page, zoom), so
M2 needs an explicit eviction policy; ask upstream for origin/transform
rendering, which would let the vello backend restore per-tile base renders.

## Performance finding (ratified as a second budget)

Most pages render in 1-53 ms. One real-world transparency-heavy page
(0041790.pdf: 394 /Group, 90 /SMask) takes ~738 ms at 1x, correctly. This is
interpreter cost, unfixable at the tile layer. The plan therefore carries TWO
budgets: the lazy-open budget (unchanged, it guards xref-driven laziness) and
a first-paint budget (something visible under 200 ms, full raster completes
in background). The corpus has no real 1000-page document (largest real file:
152 pages); the bench file must be fetched or synthesized.

## hayro gaps found (upstream candidates)

1. BLOCKS M5: appearance-state dictionaries ignored. /AP << /N << dict >> >>
   selected by /AS renders BLANK with zero warnings; only direct-stream /N
   draws. Every checkbox and radio button in the wild is invisible.
   Fallback we control: resolve /AS ourselves and hand hayro the selected
   stream, or draw widget appearances as overlays.
   UPDATE 2026-08-27: accurate for the 0.7.1 release we tested, but already
   fixed on upstream main (commit 6af63be9, merged 2026-07-08, unreleased,
   with annotation_checkbox.pdf as their own regression fixture). We now pin
   the cristim/hayro fork of current main; the fallback is unnecessary.
2. InterpreterWarning has two variants and cannot express "annotation
   appearance skipped"; the gap above is silent. Second upstream report.
   UPDATE 2026-08-27: resolved by fork commit 33d9caf8, which adds
   UnresolvedAnnotationAppearance (verified to fire on an unresolvable /AS
   and stay quiet on annotation_checkbox.pdf). Also learned: the enum's
   UnsupportedFont variant has zero emit sites upstream.
3. No sub-rectangle rendering (origin request, above).
4. RenderCache<'a> borrows the Pdf: self-referential storage; a viewer needs
   an owned cache upstream or yoke/ouroboros.
5. u16 pixmap axes: a letter page overflows around 80x zoom (guarded).
6. Pdf::new eagerly resolves the page tree (~1.4 us/page). Cheap today;
   `cos` owns laziness in the real design.

## M2 to-dos from review

- Highlight test should compare the full region outside the quad, not one
  pixel.
- Decryption Display leaks debug formatting into a user-facing message.
- Reject degenerate overlays at add_overlay instead of re-filtering forever.
- Per-tile overlay index before annotation counts grow.
- Interior mutability for tile(); render_annotations knob for M3 edit mode.
