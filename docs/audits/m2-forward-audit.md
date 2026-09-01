# M2 forward audit

Date: 2026-08-31
Baseline reviewed: `main` at `7413186`
Audit status: findings captured; remediation in progress

## Outcome

M2 is substantially implemented but not complete. P1 through P14 are represented
on main. P12's automated accessibility tree and probe merged at `07ebc93`; real
VoiceOver acceptance and the documented residuals remain open. M2 also has
confirmed correctness, lifecycle, release, performance-accounting, navigation,
and documentation defects that must close before final acceptance.

At the reviewed baseline, the Acrobat feature matrix still reported zero
implemented rows even though most M2 packages had landed. Audit Task 2 has since
reconciled all 403 rows to 43 implemented, 24 partial, 256 planned, and 80 out of
scope. B7 still owns private reference comparison and final acceptance.

## Method

Three independent read-only audits covered core/render/content/search/performance,
the app shell and merged feature packages, and repository/docs/CI/packaging state.
Their immutable source reports are:

- [`2026-08-31-core-render.md`](source-reports/2026-08-31-core-render.md)
- [`2026-08-31-app-features.md`](source-reports/2026-08-31-app-features.md)
- [`2026-08-31-repo-docs-ci.md`](source-reports/2026-08-31-repo-docs-ci.md)

`docs/plans/m2-app-hardening.md`, every live `known-issues.md` entry, Git history,
worktree state, and prior-session deletion history were additional inputs. A
finding is not considered fixed because it has an owner. It stays open until its
regression test and real user path pass.

## Live package map

| Unit | Main evidence | Audit disposition |
|---|---|---|
| M0 | scaffold/CI/corpus/matrix files | Mostly landed; private reference screenshots and first comparison remain open. |
| M1 | `docs/spikes/*`, retained `a11y_spike.rs` | Technical spikes landed; real VoiceOver acceptance remains open. |
| P1-P7 foundation | `1eed2de` | Merged; hardening findings below remain. |
| P4 render | `aa4d9b8` | Merged. |
| P8 navigation | `452f575` | Merged; layer order and thumbnail epoch defects remain. |
| P9 search | `72d0a21` | Merged; rare cancel race and Arabic visual/logical order remain. |
| P10 tools | `00a7d29` | Merged; snapshot/context-menu defects remain. |
| P11 commands/preferences/recents | `c3c6576` | Merged; keymap and preference-cap defects remain. |
| P12 accessibility | `07ebc93` | Merged; real VoiceOver acceptance, focus dispatch, platform coverage, and listed residuals remain. |
| P13 codecs/export | `74b60ec` | Merged; export lifecycle and UI-thread debt remain. |
| P14 budgets | `64829a0` | Merged; eviction-churn accounting and corpus integrity defects remain. |
| M2 overall | no completion commit | Not complete. |

## Retained and concurrent state

- B0 restored all three deleted source artifacts at `d683def` before current main
  was merged additively into this retained audit branch. The frozen repository
  source-report bytes are tracked and preserved; no rebase, removal, move, or
  checkout-overwrite was used.
- External worktree `.claude/worktrees/agent-a48bac55cf7a6b8b9` is clean retained
  P12 evidence. This audit does not modify it.
- `../onionskin-m2-p10-tools-basic` retains uncommitted P10-era changes even though
  the completed P10 commits are ancestors of main. It is historical evidence, not
  the forward implementation target.
- Other retained worktrees, branches, plans, and diffs remain preserved. No cleanup
  is authorized.
- `origin` is configured as `git@github.com:LeanerCloud/onionskin.git`. No
  remote-tracking refs or enforceable hosted gates have been verified locally;
  hosted CI, branch protection, release jobs, and platform release status remain
  unverified.

## Independent finding ingestion

### Core, render, content, search, and performance

| Source ID | Severity | Evidence at `7413186` | Disposition | Owner | Known-issues state | Required proof |
|---|---|---|---|---|---|---|
| CR-001 | Medium | `crates/content/src/page.rs`; `crates/core/src/session.rs`; focused COS/content/core fixtures | Resolved in B1.1 | B1 | Resolved; historical row retained | Negative root `/Count` returns `invalid-page-count` through COS and content, core refuses the session, and affected crate suites pass. |
| CR-002 | Low | `crates/cos/src/document.rs` | Resolved in B1.1 | B1 | Resolved; historical row retained | The object-deletion consumer comment now assigns page organization to M3. |
| CR-003 | Low | `crates/core/benches/scroll.rs`; `crates/core/tests/scroll_accounting.rs` | Resolved in B1.3 | B1 | Resolved; removed from known issues | Evict/reinsert mutation increments, rather than overwrites, composite totals; repeated observations add only their delta; decreasing counters fail loud. |
| CR-004 | Medium | `corpus/fetch.sh`; `corpus/verify-sha256.py`; checksum guarantees | Resolved in B1.4 | B1 | Resolved; historical row retained | Corrupted or incomplete stamped R2 corpus fails before cached or fresh R2 sets are accepted, R2 destinations are safely staged and published, and checksum verification rejects root swaps. |
| CR-005 | Low | `crates/app/tests/guarantees.rs:83` | Confirmed limitation | B6 | Already ledgered | Replace or supplement textual tripwires with verified hosted workflow policy; a configured `origin` alone is not proof. |
| CR-006 | Low | no configured remote at baseline; local bench job only | Externally blocked | B6 | Already ledgered | Verify required hosted checks and branch protection through the remote API; a configured `origin` alone is not proof. |
| CR-007 | Low | `crates/core/src/search.rs:598` | Confirmed open | B4 | Already ledgered | Deterministic cancel hook or stress proof closes the race without weakening semantics. |

### App shell and merged feature packages

| Source ID | Severity | Evidence at `7413186` | Disposition | Owner | Known-issues state | Required proof |
|---|---|---|---|---|---|---|
| APP-001 | Medium | `crates/app/src/shell/chrome/tabs.rs:873,891,907`; `crates/app/src/shell/panes/attachments.rs:65,78,84` | Confirmed open | B4 | Added by this audit | Resolve prompt after close/switch and prove no stale-tab export/attachment write occurs. |
| APP-002 | Medium | `crates/app/src/shell/chrome/tabs.rs:2357,2360,2376` | Confirmed open | B4 | Existing export entry, corrected | Confirm every derived overwrite and derive numbering width from the exported page count. |
| APP-003 | Medium | `crates/core/src/layers.rs:53,81` | Resolved in B3.2 | B3 | Removed from known issues | Nested `/D /Order` fixture renders hierarchy and hides omitted groups. |
| APP-004 | Low | `crates/app/src/shell/panes/thumbnails.rs:296,333`; `crates/app/src/shell/canvas.rs:556,595` | Resolved before B3.3; reverified in B3.3 | B3 | Removed from known issues | Existing regression proves old-size thumbnail responses are rejected after Reduce/Enlarge. |
| APP-005 | Medium | `crates/app/src/shell/mod.rs:287-317`; `crates/app/src/shell/canvas.rs:1270-1313` | Resolved in B2.4 | B2 | Updated by B2.4 | First error is preserved, stale worker wait timing is reset after poll update errors, oversized snapshots are refused before allocation, and PNG encoding runs on the background executor with stale completion protection. |
| APP-006 | Low | `crates/app/src/shell/chrome/tabs.rs:2177,2189` | Resolved in B2.3 | B2 | Updated by B2.3 | Second right-click inside document bounds repositions the open canvas context menu; right-click outside document bounds dismisses it. |
| APP-007 | Low | `crates/app/src/keymap.rs:267,287` | Confirmed open | B3 | Already ledgered | `f19`-`f35`, `back`, and `forward` parse through the canonical key path. |
| APP-008 | Low | `crates/app/src/preferences.rs:437,462` | Confirmed open | B3 | Already ledgered | Exactly 64 unknown preferences survive regardless of key ordering. |
| APP-009 | Low | `crates/app/src/shell/panes/attachments.rs:78,80` | Confirmed open | B4 | Added by this audit | Attachment prompt error becomes visible and is distinct from cancel. |
| APP-010 | Medium | `crates/app/src/shell/chrome/tabs.rs:2344`; `crates/app/src/shell/canvas.rs:415` | Confirmed open | C1 | Existing export entry, split from APP-002 | Move streaming/page-range/progress work off the UI thread and prove the UI stays responsive. |
| APP-011 | High | `crates/app/src/shell/panes/mod.rs:439-444`; live Task 2 verification | Resolved in B3.1 | B3 | Removed from known issues | Shared navigation flex item uses its existing open-state width; focused GPUI coverage proves every pane body has rendered bounds. |

The app audit also proved the native action-dispatch ledger entry stale: current
`RunCommand`, close-tab, and view-menu paths defer correctly and have shell tests.
B2.1 reverified the deferred route at `483e512` with focused tests, the full
shell target, and a real Command-F app run recorded as `M2-B2-T1-001`; the
retained entry remains resolved.
B2.2 preserves the first status error produced by one input cycle and clears the
render-worker wait deadline when the poll loop exits through a model-update
error. Verification passed focused regressions, the full shell-support target,
strict clippy for `onionskin-app`, and a shell build. The local macOS screenshot
API refused direct Onionskin window and region captures during B2.2, and full
screen fallback captures were black, so no B2.2 image is recorded as evidence.
B2.3 reuses the existing canvas-menu opener from the dismiss layer's right-click
handler when an open canvas menu receives another right-click inside document
bounds. The generic dismiss click handler ignores right-click click events so it
cannot close the repositioned menu after the right-mouse-down path. Verification
passed focused context-menu tests, the full shell-support target, strict clippy
for `onionskin-app`, and a shell build. The local macOS screenshot API still
refused direct Onionskin window capture, so no B2.3 image is recorded as
evidence.
B2.4 splits snapshot pixel preparation from PNG encoding. Snapshot preparation
now refuses a crop exceeding one 3840x2160 RGBA screenful before allocating its
owned pixels, and the shell schedules PNG encoding on GPUI's background executor
with one generation guard so stale completions cannot overwrite newer snapshot
requests. Focused snapshot regressions passed, including oversize, encoder
error, UI-yield, stale-completion, failed-request invalidation, generation-wrap,
and same-cycle primary-error preservation cases. Full shell-support tests, strict
clippy, format check, and shell build passed. Local macOS screenshot capture and
native synthetic drag smoke remain environment-blocked, so B2.4 records no new
window screenshot.
B3.1 applies `NavigationPanesState::width()` to the rendered pane column and
records the column's rendered children. The focused GPUI regression opens every
M2 pane and proves the strip plus body have rendered bounds, closing APP-011 and
restoring the rows whose only remaining gap was the hidden pane body.
B3.2 reads nested `/D /Order` as the pane order, carries layer depth, and omits
groups not named by `/Order`; the focused core regression covers nested ordering
and omitted groups while existing app layer tests cover pane behavior.
B3.3 reverified the existing
`a_thumbnail_rendered_at_the_previous_size_is_dropped_when_it_arrives`
regression, proving APP-004 was stale against current code.

### Repository, documentation, CI, packaging, and retained state

| Source ID | Severity | Evidence at `7413186` | Disposition | Owner | Known-issues state | Required proof |
|---|---|---|---|---|---|---|
| REPO-001 | Critical | `.github/workflows/release.yml:49-50`; `crates/app/Cargo.toml:64-100` | Confirmed release blocker | B6 | Added by this audit | Packaged artifact builds with `--features shell` and launches the real viewer. |
| REPO-002 | Critical | `07ebc93`; `docs/spikes/m2-voiceover-acceptance.md` | Partially resolved; M2 acceptance blocker remains | B5 | Existing VoiceOver/P12 entry, narrowed | Keep the restored spike, fix focus/platform residuals, harden the soft probe gate, and pass one real VoiceOver session. |
| REPO-003 | High | `ACROBAT-PARITY.md:29-31,60` | Resolved in Audit Task 2 | B7 | Added and resolved by this audit | All 403 rows are reviewed and the summary totals mechanically match the rows; B7 retains REPO-010 and final acceptance. |
| REPO-004 | High | `docs/plans/m2-viewer.md:3` at baseline | Resolved in this task | B7 | Added and resolved by this audit | M2 plan header now matches live merge history and open gates. |
| REPO-005 | High | retained P10/agent branches and worktrees | Preserve, no cleanup | B0 | Added by this audit | Inventory remains reproducible; useful work reconciles additively only. |
| REPO-006 | Medium | `~/.claude/projects.md` | Resolved in Audit Task 3 | B7 | Added and resolved by this audit | External project registry now describes the live Rust viewer and open hardening gates. |
| REPO-007 | Medium | root `README.md`, `.project-docs/INDEX.md`, `.editorconfig` | Resolved in Audit Task 3 | B6 | Added and resolved by this audit | Minimal build, status, documentation, and editor entry points exist without a Makefile or task runner; B6 retains its separate release and packaging-doc work. |
| REPO-008 | High | `.github/workflows/ci.yml`; no checked-in policy | Confirmed open | B6 | Added by this audit | Checked-in advisory/license/secret/dependency policy passes with reviewed exceptions. |
| REPO-009 | High | `corpus/checksums/hayro-corpus.sha256`; fresh 41-file acceptance download | Resolved in B1.4 | B1 | Resolved; historical row retained | Every merge-gating `hayro-corpus` PDF has an enforced SHA-256 verified for cached and freshly downloaded sets after strict R2 manifest ID validation and safe publication pass. |
| REPO-010 | High | `PLAN.md:100-102,497`; no private corpus evidence | Confirmed acceptance blocker | B7 | Existing parity screenshot entry | Private reference corpus and first Onionskin comparison are captured and ledgered. |
| REPO-011 | Medium | `crates/app/tests/guarantees.rs:6-65` | Confirmed open | B6 | Added by this audit | Landed guarantees execute or point to real enforcing tests; future ones stay explicit. |
| REPO-012 | Medium | `packaging/README.md:34` | Confirmed status defect | B6 | Added by this audit | Packaging docs exactly match current feature flags and prerequisites. |

## Prior-session deletions

The user explicitly required preserved files to be restored, so B0 keeps the exact
historical sources without selecting obsolete code in normal builds. Git history
alone does not satisfy that user instruction.

| Artifact | Evidence | Disposition | Owner |
|---|---|---|---|
| `crates/content/src/filter.rs` | Deleted on main by `cb75deea` after decoder consolidation; retained branch history also contains `f60b285`. | Restored byte-exact at `d683def` as an uncompiled retained reference. | B0 complete |
| `crates/app/src/bin/shell_spike.rs` | Deleted by `8573fad` and included in foundation merge `1eed2de`. | Restored byte-exact at `d683def` behind `shell-spike`. | B0 complete |
| `crates/app/src/bin/a11y_spike.rs` | Deleted by P12 commit `6077d7c`. | Restored byte-exact at `d683def` behind `a11y-spike`. | B0 complete |

## Existing ledger disposition

Every `known-issues.md` item is retained and assigned as follows:

- Upstream hayro gaps remain tracked external dependencies; they do not become
  false local completion claims.
- COS repair/save/page-tree debt belongs to M3 organize/save unless B1 fixes a
  current viewer correctness path.
- P8/P11/P14 and shell correctness items belong to B1-B4.
- Export background streaming, page range, progress, and cancellation belong to
  C1's M3 export-dialog work; M2 correctness/lifecycle defects belong to B4.
- Arabic visual/logical ordering belongs to the later localization/text package;
  M2 matrix rows remain partial where it affects behavior.
- P12/VoiceOver belongs to B5. Linux/Windows first-release validation belongs to
  B6 and stays explicitly unavailable until run on those hosts.
- The private Acrobat screenshot corpus and first comparison belong to B7.

`docs/plans/m2-app-hardening.md` is retained as a source ledger. Its live-item
disposition table follows from a line-by-line revalidation against `7413186`.

## M2 app-hardening backlog revalidation

| ID | Disposition | Owner | Live evidence / action |
|---|---|---|---|
| HARD-CAN-001 | Confirmed | B2 | App still duplicates view transforms across `input.rs`, `shell/mod.rs`, and `canvas.rs`; reuse core rotation/mapping helpers. |
| HARD-CAN-002 | Confirmed | B2 | `begin_frame` and visible-page calculation still repeat across update/paint; make paint preparation the single frame boundary. |
| HARD-CAN-003 | Confirmed | B2 | `paint_list` still needs mutable store access because `TileStore::get` touches/pins; separate shared read from touch. |
| HARD-CAN-004 | Confirmed | B2 | `CanvasModel::sources` still duplicates store bases under a different eviction policy; unify the source of truth. |
| HARD-CAN-005 | Confirmed | B2 | `placeholders` has writes/clears but no production read; remove only the dead state after tests prove behavior. |
| HARD-CAN-006 | Confirmed | B2 | Model and GPUI paint bounds still provide two canvas origins; establish one authoritative mapping. |
| HARD-CAN-007 | Confirmed, stale lines | B2 | Current coupled expects are `canvas.rs:1426` and `canvas.rs:1824`; remove the coupling or pin it explicitly. |
| HARD-CAN-008 | Confirmed | B2 | Atlas gutter and paint defaults remain encoded by repeated literals; name the real invariant without broad style churn. |
| HARD-CAN-009 | Confirmed | B2 | `TileImageKey` still encodes rotation as `u8`; use the enum directly. |
| HARD-CAN-010 | Confirmed | B2 | Several public canvas methods have no external consumer; narrow visibility when the B2 refactor touches them. |
| HARD-CHR-001 | Confirmed, narrowed | B3 | Document-search rows can still render enabled with no active document and then no-op; render the unavailable reason inline. |
| HARD-CHR-002 | Superseded | C1 | Find, Preferences, Select All, and Open Recent landed; Print/Properties remain correctly owned by M3. |
| HARD-CHR-003 | Confirmed, design check | B3 | Quick-action milestone reasons remain a chrome table; first decide whether plugin metadata has a second real consumer. |
| HARD-CHR-004 | Confirmed, narrowed | B3 | Some menu/context rows remain duplicated; page controls already reuse a helper. Refactor only the live repetition. |
| HARD-CHR-005 | Confirmed | B3 | Overflow scrolling exists, but no themed scrollbar surface satisfies parity. |
| HARD-CHR-006 | Confirmed | B3 | Full Screen remains a bare window toggle; visibility/presentation semantics are absent. |
| HARD-CHR-007 | Resolved | B3 | Startup now uses `MenuState::new`; the duplicate active-tab rule is gone. |
| HARD-CHR-008 | Confirmed | B3 | The rail displays asset IDs such as `hand` as text instead of drawing icon assets. |
| HARD-CHR-009 | Confirmed, stale lines | B3 | Live render/click expects remain in tabs/tool-search/rail/quick-actions; degrade reachable failure paths. |
| HARD-CHR-010 | Confirmed, narrowed | B3 | `chrome/mod.rs` still lacks ownership/module documentation; the old "zero comments" claim is stale and no comment quota is justified. |
| HARD-CHR-011 | Partially accepted | B3 | Repeated layout dimensions need names only where they encode a shared invariant; naming every visual literal would be over-engineering. |
| HARD-CHR-012 | Confirmed | B3 | Page entry still reuses `SearchInput` and its search key context; give the shared input an honest neutral contract. |
| HARD-CHR-013 | Confirmed | B3 | Page controls show zoom text but offer no percentage input even though `zoom_to` exists. |
| HARD-CHR-014 | Confirmed | B3 | Non-finite zoom still casts silently to zero in `page_controls.rs:91`; validate before formatting. |
| HARD-CHR-015 | Confirmed | B3 | The side-panel empty host still displays the literal `Panel`; render an honest empty/context state. |
| HARD-A11Y-001 | Partially resolved | B5 | P12 shipped the tree, roles, labels, page text, focus ring, actions, bounds, and probe. Real VoiceOver acceptance, AccessKit-to-GPUI focus transfer, arrow navigation, occluded-window actions, UI-thread text extraction, probe gating, and non-macOS adapters remain. |
| HARD-PROC-001 | Historical | Process | The thin P2-P7 review trail cannot be repaired in source; the current package/review/evidence gates prevent recurrence. |
| HARD-PROC-002 | Resolved | B3 | P10 registered real tools and current rail/quick-action tests exercise the live registry. |
| HARD-PROC-003 | Resolved in Audit Task 2 | B7 | All 403 rows are reconciled and mechanically recounted; B7 retains private reference comparison and final acceptance. |

Resolved and superseded bullets stay in this table to preserve why they no longer
justify code changes. B2/B3 plans must use these current lines and reject the broad
comment/literal refactors that would add churn without behavior.

## Previously encountered resolved defects

- The content oracle previously inferred extraction errors from a hand-maintained
  subset of category slugs and treated page-count errors as empty documents.
  B1.2 records error events at their call sites, rejects nonzero `pdftotext`
  exits, and pins new-category, mismatch, negative-count, and tool-failure cases.
- The black horizontal/vertical lines seen inside rendered PDF pages were tile-atlas
  sampling seams, not PDF content corruption. Commit `fdaf657` replicated one-pixel
  tile gutters and clipped sampling at atlas allocation edges. The focused seam
  regression, shell suite, independent review, and window-only evidence at
  `<local-screenshot-store>/m2-p7-render-artifact-fix-two-page-20260830-0126.png`
  (SHA-256 `0d88848c0f41b42ce7468e4b95384bbcac3b5c399718cc6e370d9319982f407f`)
  verified the fix. This audit inspected the capture and confirmed it contains only
  the Onionskin window. Later P7 screenshots and live gates continued to show clean
  page rendering. This audit found no evidence that the artifact returned.

## Verification performed for this audit

The core auditor passed:

- `cargo test -p onionskin-core --tests`
- `cargo test -p onionskin-render --tests`
- `cargo test -p onionskin-content search`
- `cargo test -p onionskin-cos pages`
- the focused guarantee-9 app test

The app and repository auditors performed source/test/doc/history inspection only.
No cargo bench, hosted CI, platform release, real VoiceOver, or Acrobat screenshot
comparison was run. Those remain open and must not be inferred from this report.

### Current integration verification

After P12 and B0 reached `main`, this retained audit branch merged current `main`
without removing or overwriting retained state. The reconciliation pass proved:

- all 403 matrix rows recount to 43 implemented, 24 partial, 256 planned, and 80
  out of scope;
- `cargo test -p onionskin-app --test guarantees` passes, including the executable
  matrix-headline and milestone-total contract;
- the three B0 source files still match their recorded SHA-256 values; and
- the production-source diff from `main` is empty. The only code difference is the
  audit branch's matrix contract in `crates/app/tests/guarantees.rs`.

Fresh shell and accessibility-probe reruns were attempted from both the audit and
shared main target directories, but the filesystem had only 140 MiB available and
Rust failed with `No space left on device`. No pass is inferred from those failed
runs; their last green P12/B0 results remain recorded in the corresponding package
plans, and final acceptance must rerun them after space is available.

Audit Task 3 verified `cargo fmt --all -- --check`, Cargo workspace metadata, every
new repository documentation link, package-merge ancestry, and the external project
registry update. The README's workspace, headless, shell-test, and run commands were
not recompiled in this task because only 137 MiB remained after the earlier ENOSPC
failures. Their prior package results stay cited, but no fresh pass is inferred.
