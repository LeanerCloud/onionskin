# M2 app hardening backlog

Status: retained source backlog, line-by-line revalidation in progress under the
2026-08-31 forward audit. P8-P11, P13, and P14 have since integrated, so this file
is evidence rather than current package order. Live dispositions and B2/B3/B5
owners are recorded in `docs/audits/m2-forward-audit.md`; no bullet is deleted when
superseded or resolved.

## Canvas (crates/app/src/shell)

- View math leaked back into app: tile_rect and rotate_pixel duplicate
  core::ViewRotation::rotate_rect/rotate_point; viewport_center,
  window_point, rect_intersects_viewport, rotated_size, atlas_image_rect and
  the origin subtraction in input.rs (:52-53, :67, :113-116), canvas.rs
  window_point and mod.rs local_point are three copies of one transform.
  Move into core (or render) beside rotate_rect; give Viewport a
  window_to_view(point, origin) and a center(); input.rs ends with no float
  arithmetic, as P6b specified.
- begin_frame runs once per update(), and update() runs on every pointer
  move as well as every paint (two evictions per interactive frame); each
  update() queries viewport.visible_pages() four times plus paint_list a
  fifth, each allocating. Compute visible once per update and make
  prepare_paint the only begin_frame caller.
- paint_list requires &mut CanvasModel because TileStore::get is &mut self,
  defeating the interior mutability P4 added to TileCache::tile. Give
  TileStore a shared read path or split touch from get.
- sources: BTreeMap<PageIndex, BaseRaster> duplicates the store's bases under
  a different eviction policy; a page scrolled out of view loses its rescale
  source while the store still holds the raster, so returning at a new zoom
  paints blank instead of a rescaled raster (decision 11 asks for the
  rescale). Unify on the store.
- placeholders is dead state (written, cleared, never read outside tests).
- Two sources of truth for the canvas origin (model.canvas_origin from
  prepaint for hit testing vs the paint closure's bounds.origin); resize is
  the only refresh and never notifies.
- Invariant-by-coupling expects: canvas.rs:1035 (paint source has a cache,
  true only because oldest_evictable never evicts the last-touched entry)
  and :798. State or test the coupling, or remove the expects.
- Magic numbers: the 1 px atlas gutter encoded four ways (+2, -1, 2.0*gutter),
  px(8.0)/px(700.0), positional 0/false to paint_image; gpui::white() for the
  first-paint page rect where decision 11 says the document background.
- TileImageKey encodes rotation as a u8 via rotation_code; key on the enum.
- Single-consumer pub API: has_pending_render, generation, the two drain_*
  functions.

## Chrome (crates/app/src/shell/chrome)

- Document-search and command hits in the global search panel render as
  enabled rows and explain unavailability only after the click; reuse the
  disabled styling with an inline reason.
- Main menu omits later-milestone entries (File > Print, Edit > Find,
  Preferences, Select All, Open Recent, File > Properties) instead of
  disabled-with-reason; the same function does it right for Open, About,
  Keyboard Shortcuts.
- Quick-action reason strings are a hardcoded milestone table in chrome
  (DeliveryStage::reason); the plan wants the reason to come from the
  capability query. Carry deferred-stage metadata on the plugin-api side, or
  amend the plan to say the table is chrome-owned. Coordinate with P10's
  ToolCtx/capability changes.
- The ~40-line menu-row builder is copy-pasted three times (render_main_menu,
  render_tab_context_menu, page_controls action_button); one menu_row
  helper.
- No scroll bars exist (overflow_*_scroll only); parity row 179 requires the
  theme to reach scroll bars.
- Full Screen has no presentation semantics: theme visibility() ignores
  fullscreen; toggle_fullscreen is a bare window toggle.
- shell/mod.rs:411-417 duplicates TabState::new's active-tab rule to build the
  startup MenuState; MenuState::initial exists for that single caller.
- rail.rs:191 renders the icon asset name as visible text.
- Panics inside Render/click paths that are unreachable today by filtering
  (tabs.rs:345/350/370/683/942, tool_search.rs:742/744, rail.rs:101,
  quick_actions.rs:259/263); degrade instead.
- Zero comments and no module docs across 5,760 lines of chrome; at minimum
  chrome/mod.rs states what it owns and how P7a/b/c divide.
- Inline pixel literals (320, 420, 230, 82/76/52/24, 38/30/34/28) unnamed.
- SearchInput doubles as the page-number field with a hardcoded
  "OnionskinSearch" key context; rename and rehome the shared input.
- No zoom-to-percentage control (CanvasModel::zoom_to has only test callers);
  parity row 88 partly unmet.
- page_controls.rs:91 saturating cast turns a non-finite zoom into 0 silently.
- Side panel host renders the literal "Panel"; consumer arrives with P8.

## Accessibility readiness (feeds P12)

STATUS 2026-08-31: P12 shipped and closed most of this section - the tree,
focus handles, a tab order, Escape dismissal, text labels for the glyph-only
controls, checked state as state rather than a string prefix, and per-page
nodes with the document's own words. What it did not ship is arrow-key
navigation; see known-issues.md for that and the other P12 residuals.

Historical pre-P12 finding retained for provenance:

- The chrome is a GPUI element tree with stable ids, so a tree is derivable,
  but semantics are absent: one focus handle in the whole chrome
  (SearchInput), no tab order, no arrow-key navigation, no Escape dismissal
  (dismissal is a click layer); glyph-only controls ("☰", "⠿", "›", the page
  controls) would be announced as punctuation; checked state is a "✓ "
  string prefix. The document canvas paints pages and tiles into one
  gpui_canvas element with no focus handle; per-page nodes, page text and a
  focusable document node must be synthesised from PaintList, which carries
  no labels or ids today.

## Process

- The review trail for P2-P7 is thin: six of fourteen P7 feature commits
  have empty bodies, none records verification, and the plan's P7 item
  (layout parity checked against a screenshot, recorded in the PR) has no
  trace because everything landed directly on main. Rule going forward:
  every package passes the adversarial review gate before merge, and every
  commit body states what changed, how the defect was found, and how it was
  verified.
- Historical pre-P10 finding: the two registry-driven chrome assertions
  (rail.rs:419-436, quick_actions.rs:616-621) were vacuous before real tools
  registered. P10 merged at `00a7d29`; current rail and quick-action tests now
  exercise the live registry, resolving HARD-PROC-002.
- Historical pre-audit finding: ACROBAT-PARITY.md read 0 implemented. Audit Task 2
  reconciled all 403 rows and added an executable totals contract, resolving
  HARD-PROC-003 while private reference comparison remains open.
