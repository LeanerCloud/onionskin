# M2 forward audit

Date: 2026-08-31
Baseline reviewed: `main` at `7413186`
Audit status: findings captured; remediation in progress

## Outcome

M2 is substantially implemented but not complete. P1 through P14 are represented
on main. P12's automated accessibility tree and probe merged at `07ebc93`, with
focus and background-publication residuals closed by `11d5751`; real VoiceOver
acceptance and Linux/Windows adapters remain open. M2 also has
confirmed correctness, lifecycle, release, performance-accounting, navigation,
and documentation defects that must close before final acceptance.

At the reviewed baseline, the Acrobat feature matrix still reported zero
implemented rows even though most M2 packages had landed. Audit Task 2 has since
reconciled all 403 rows. The 2026-09-10 source reconciliation at `755842f`, including
the view/zoom merge `cb87b72`, brings the live totals to 52 implemented,
16 partial, 255 planned, and 80 out of scope. B7's privacy scaffold
is landed, and its first two private comparisons are complete with public metadata
ledgered. It still owns the remaining eight states and final acceptance.

## Integration verification checkpoint, 2026-09-14

Task B corpus/parser integration is committed locally at
`432e1f7eb5facdc51239472d28c748388b445550`. The required consumer targets,
COS library, and `write_new` passed as separate direct commands. The exact
Isartor trace opened page 1 of 10,000 and returned 941,160/4,010,934 bytes
through first-page access. A local amd64 Docker/`act` simulation exited 0;
its clean control and three reviewed workflow mutations produced their
expected statuses. These results are local qualification only, not hosted CI,
or Windows acceptance. Independent review approved the native macOS scenario
evidence recorded in `docs/evidence/milestone-screenshots.md`, including the
stable Isartor window, Find, tab switching, invalid page rejection, native
Select All, and the 160 by 360 pixel export. Final-main and milestone gates
remain separate, and these results do not change the Acrobat matrix totals or
feature statuses.

The stale `f1fe658` P12 ledger is retained as historical evidence. Its current
disposition is the live `HARD-A11Y-001` row: P12 remains partially resolved,
the required macOS probe is not a VoiceOver session, and real VoiceOver plus
Linux/Windows adapters remain open. The old missing-codecs export claim in that
ledger is stale and is not carried forward; the current regression uses a
custom blocked codec. Historical global-search ordering and probe-lint claims
remain unverified and are not imported as fresh findings. The retained source
inventory is likewise
unchanged: the integrated primary sets (`c516734`, `c933642`, `0340cdb`, and
`dd3e404`) contribute only their approved history; dirty tools-basic, zoom-to
and C12 experiments, equivalent old M1/page-index work, the `a284` whole-tree
checkpoint, historical P12 and parity worktrees, local corpus artifacts, and
the three workflow-mutation worktrees remain preserved as follow-ups or
controls. None is promoted or silently discarded.

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

| Unit | Implementation evidence | Audit disposition |
|---|---|---|
| M0 | scaffold/CI/corpus/matrix files | Mostly landed; the private evidence protocol and first two Reader/Onionskin baselines are complete, with eight required states still open. |
| M1 | `docs/spikes/*`, retained `a11y_spike.rs` | Technical spikes landed; real VoiceOver acceptance remains open. |
| P1-P7 foundation | `1eed2de` | Merged; hardening findings below remain. |
| P4 render | `aa4d9b8` | Merged. |
| P8 navigation | `452f575` | Merged; B3 resolved layer order and stale-size thumbnail classification, and B7 resolved deferred idle-poll thumbnail delivery at `4132a99`. |
| P9 search | `72d0a21`, B4.1 `d5a0836` | Merged; B4.1 resolved queued-search cancellation. Arabic presentation forms match, but visual-to-logical run reordering remains unsupported. |
| P10 tools | `00a7d29`, B2.2-B2.4 evidence below | Merged; B2 resolved snapshot allocation/encoding/error/stale-completion handling and context-menu repositioning. Canvas geometry regression gaps and the current-page-only raster zoom ceiling remain. |
| P11 commands/preferences/recents | `c3c6576`, B3 `c3d0444` | Merged; B3 resolved extended-key acceptance and the unknown-preference carry-forward cap. Existing config-directory modes and silent save-time rescue-copy notices remain. |
| P12 accessibility | `07ebc93`, residuals `11d5751` | Merged; grouped traversal, GPUI/AccessKit focus synchronization, background publication, and demand-driven page text are implemented. Real VoiceOver acceptance and Linux/Windows adapters remain. |
| View/zoom residuals | `cb87b72` | Merged; Fit Visible, Dynamic Zoom, and Read Mode now meet their matrix scope. Zoom To has preset choices but no custom percentage entry; Full Screen hides chrome but still lacks presentation semantics. |
| P13 codecs/export | `74b60ec`, C1.1 `f0cbbcb` and `3ac647b` | Merged; C1.1 moved encoding and output I/O to a background worker. Single publishes one completed temporary file atomically; PerPage publishes completed page files incrementally. Progress, cancellation, and one-writer bounds are proved. C1.2 is resolved at the commits below. |
| C1.2 export settings and native input | `4ddec23`, `c7f1afa`, `62576fb` | Implemented and verified at the cited commits. Page range/PNG resolution, modal focus/bounds, editable AX values, and native Select All routing for enabled menu entries are implemented. Home/no-document and missing-commands-core native availability remain outside this fix. |
| P14 budgets | `64829a0`, `be39bae`, B1.4 `c32ae05` and `be3789e` | Merged; composite accounting now accumulates across cache incarnations, and merge-gating corpus integrity is verified for cached and fresh downloads. Headless benches do not measure shell raster ownership; hosted required-check enforcement remains unverified. |
| M2 overall | no completion commit | Not complete. |

The September 10 view/zoom reconciliation inspected merged source and test
definitions; C1.2's later automated and native proof is recorded separately below.
Current shell paths are
`crates/app/src/shell/chrome/tabs/{mod,menu,accessible,dialogs,export,context,frame_state}.rs`.
The `tabs.rs` citations under "Evidence at `7413186`" below remain historical.

### C1.1 post-rebase verification

The exact rebased commits are the shared-byte worker snapshot at `f0cbbcb` and
the background streaming export at `3ac647b`. Verification against that state
passed 100 focused tab tests; all 501 shell app tests; plugin-api 11/11;
codecs-common unit 3/3, export 11/11, and roundtrip 1/1; and the complete
no-default app targets (56 library, 4 file-association, 30 guarantee with 7
expected ignored, 4 kernel-emptiness, 1 privacy, and 9 registry with 3 expected
ignored). The 179-line no-default normal dependency graph contains no
`tempfile`. Formatting, diff checks, and scoped strict clippy also passed. The
clippy command allows only the pre-existing unrelated
`clippy::single-char-add-str` baseline in `crates/app/src/a11y/probe.rs`.

### C1.2 verification and native acceptance, 2026-09-10

`4ddec23` implements First/Last page settings, PNG resolution, validation,
modal keyboard traversal, and rendered accessibility bounds. `c7f1afa` publishes
raw editable values for export and existing Search/Find/Page inputs. `62576fb`
routes enabled native Select All commands to the visible focused field while
preserving document selection when a modal is open.

The latest full shell/support run passed 624 tests with 7 expected ignored.
Focused no-codecs checks passed all 3 native-input and 2 published-value tests;
strict app clippy, formatting, and diff checks passed. Regressions cover real
menu/settings/prompt/worker subset output, absolute numbering, PNG dimensions,
immutable Single-output requests, invalid inputs, stale origins/prompts, modal
focus/bounds, and Unicode/empty/invalid raw values. The input regressions failed
before their fixes and passed afterwards.

Native checks on committed `62576fb` read AXValue defaults `1`, `2`, `150`.
Cmd+A replaced the First value with `bad`, Enter produced validation, and native
Edit > Select All visibly selected `bad`; typing `2` after restoring foreground
replaced it. Setting DPI to `144` yielded AX values `2`, `2`, `144`; Tab visibly
focused Export, and submission opened the native destination Save dialog. The
final build exported page 2 at 144 DPI as a 160 by 360 pixel PNG. Earlier
native text output contained only `Page two`; output hashes and captures are in
`docs/evidence/milestone-screenshots.md`.

APP-010 is resolved. File > Export To remains partial for later format targets,
and accessible-text export still lacks tagged reading order. Native Select All
remains disabled on Home/without a document or commands-core. Older modal
geometry/focus omissions, real VoiceOver acceptance, and Linux/Windows adapters
and packaged-platform acceptance remain open.

## Retained and concurrent state

- M3 P0c is a local test-only checkpoint at `fdf875c`, based on `65d29c6`.
  It moves 94 inline tests beside their shell implementation and retains 24
  at the root, without changing production code or the three external C1.2 files.
  Eleven configurations pass with 3,060 test-result entries, including ignored
  cases, preserved in the before/after census. Missing-test/import mutations
  and a fresh 624-test control pass. Native open/Find/tab/export smoke and Task A/B
  window-only captures passed at `d34b29f` on 2026-09-11; details and hashes are in
  `docs/evidence/milestone-screenshots.md`. Final integration remains pending. This changes no matrix
  status. Detailed proof is in `crates/app/tools/item-inventory/p0c-test-relocation-*.txt`.
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
| CR-007 | Low | `crates/core/src/search.rs:598` | Resolved in B4.1 | B4 | Resolved; removed from known issues | Deterministic queue-drain regression at `d5a0836` proves cancel abandons a queued search behind a worker walk without weakening cancel semantics. |

### App shell and merged feature packages

| Source ID | Severity | Evidence at `7413186` | Disposition | Owner | Known-issues state | Required proof |
|---|---|---|---|---|---|---|
| APP-001 | Medium | `crates/app/src/shell/chrome/tabs.rs:873,891,907`; `crates/app/src/shell/panes/attachments.rs:65,78,84` | Resolved in B4.2 | B4 | Resolved; historical row retained | Export and attachment prompts retain the originating canvas identity. Four GPUI regressions prove switch/close before successful prompt completion writes nothing; the native-error branches use the same exercised identity predicate because GPUI's test prompt cannot inject an error result. |
| APP-002 | Medium | `crates/app/src/shell/chrome/tabs.rs:2357,2360,2376` | Resolved in B4.4 | B4 | Resolved; historical row retained | Derived export paths use page-count width and are reserved without overwrite before any bytes are written. |
| APP-003 | Medium | `crates/core/src/layers.rs:53,81` | Resolved in B3.2 | B3 | Removed from known issues | Nested `/D /Order` fixture renders hierarchy and hides omitted groups. |
| APP-004 | Low | `crates/app/src/shell/panes/thumbnails.rs:296,333`; `crates/app/src/shell/canvas.rs:556,595` | Resolved before B3.3; reverified in B3.3 | B3 | Removed from known issues | Existing regression proves old-size thumbnail responses are rejected after Reduce/Enlarge. |
| APP-005 | Medium | `crates/app/src/shell/mod.rs:287-317`; `crates/app/src/shell/canvas.rs:1270-1313` | Resolved in B2.4 | B2 | Updated by B2.4 | First error is preserved, stale worker wait timing is reset after poll update errors, oversized snapshots are refused before allocation, and PNG encoding runs on the background executor with stale completion protection. |
| APP-006 | Low | `crates/app/src/shell/chrome/tabs.rs:2177,2189` | Resolved in B2.3 | B2 | Updated by B2.3 | Second right-click inside document bounds repositions the open canvas context menu; right-click outside document bounds dismisses it. |
| APP-007 | Low | `crates/app/src/keymap.rs:267,287` | Resolved in B3.4 | B3 | Removed from known issues | `f19`-`f35`, `back`, and `forward` parse through the canonical key path. |
| APP-008 | Low | `crates/app/src/preferences.rs:437,462` | Resolved in B3.5 | B3 | Removed from known issues | Exactly 64 unknown preferences survive regardless of key ordering. |
| APP-009 | Low | `crates/app/src/shell/panes/attachments.rs:78,80` | Resolved in B4.3 | B4 | Resolved; historical row retained | Attachment prompt failure uses the pane feedback path while cancellation remains silent. |
| APP-010 | Medium | `crates/app/src/shell/chrome/tabs.rs`; `crates/core/src/session.rs` | Resolved by C1.1 and C1.2 (`4ddec23`, `c7f1afa`, `62576fb`) | C1.2 | Resolved; historical row retained | Background streaming, progress, cancellation, bounded publication, and cleanup-before-guard-release remain covered. Page-range/PNG-resolution settings, modal validation/focus/bounds, raw AX values, and native input replacement are verified as recorded above. |
| APP-011 | High | `crates/app/src/shell/panes/mod.rs:439-444`; live Task 2 verification | Resolved in B3.1 | B3 | Removed from known issues | Shared navigation flex item uses its existing open-state width; focused GPUI coverage proves every pane body has rendered bounds. |
| APP-012 | High | `crates/app/src/shell/panes/thumbnails.rs`; pre-fix B7-REF-003 Onionskin capture | Resolved in B7 at `4132a99` | B7 | Resolved before ledger; no live entry | Opening Page Thumbnails after initial canvas work settles rearms polling for deferred thumbnail requests. The regression drives only the normal GPUI timer/observer path and receives every picture without direct collection. |

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
B3.4 extends the canonical key whitelist through `f35` and adds `back` and
`forward`; focused keymap tests prove the names bind and unknown names remain
refused.
B3.5 filters known settings before applying the unknown-key carry-forward cap;
focused preferences tests prove exactly 64 unknown settings survive regardless
of key ordering.
B3.6 gives canvas and thumbnail context menu entries command-specific milestone
reasons, including M5 for Edit Text, Redact Text, Create Link, and Crop Pages.
B3.7 resolves the matrix-visible chrome hardening items: document search no-doc
rows now render their unavailable reason, the shared page field reports
`NumberInput`, invalid zoom displays as unavailable, and the empty side panel
renders a Tool details empty state. The remaining HARD-CHR rows are non-B3
matrix blockers or deferred rows already represented in `ACROBAT-PARITY.md`.
B3.8 maps tool rail icon IDs to compact visual marks at render time while
keeping screen readers on tool names.
B4.1 replaces the timing-sensitive P9 search-cancel quiet-window test with a
deterministic regression at the worker queue-drain seam. Focused core search
tests, the search integration target, strict core clippy, and the format check
passed after the change rebased onto `d5a0836`.
B4.2 carries the originating canvas identity through export and attachment path
prompts and refuses successful write completions after the tab is switched or
closed. Four GPUI regressions cover both actions and both write paths. Native
prompt-error reporting uses the same identity predicate; GPUI's test prompt has
no error-injection seam, so that branch is retained as explicit code-path proof
rather than claimed as an end-to-end regression. B4.3 reports attachment prompt
failures through the existing pane feedback path while preserving silent
cancellation. B4.4 derives numbered export width from the page count and reserves
all derived destinations without overwrite before writing. C1.1 later resolved
the export UI-thread and whole-document buffering work. C1.2 subsequently resolved
APP-010's page-range/settings remainder at `4ddec23`, with native-input follow-ups
at `c7f1afa` and `62576fb`.
B7 traced the Page Thumbnails black placeholders to deferred `Show` handling:
thumbnail work was queued after the canvas poll loop had gone idle, so completed
worker responses were never collected. `4132a99` rearms the existing idempotent
poll loop after queueing the visible band. The regression first settles initial
canvas work, opens the pane through its normal action, and waits through GPUI's
timer path until all thumbnails arrive without calling the collector directly.

### Repository, documentation, CI, packaging, and retained state

| Source ID | Severity | Evidence at `7413186` | Disposition | Owner | Known-issues state | Required proof |
|---|---|---|---|---|---|---|
| REPO-001 | Critical | `.github/workflows/release.yml:49-50`; `crates/app/Cargo.toml:64-100` | Resolved in B6; hosted release run still unproven | B6 | Resolved; historical row retained | Every release artifact builds with `--features shell` on every matrix platform, with the CI shell job's prerequisites mirrored step for step. `release_artifacts_build_the_windowed_viewer` refuses a conditioned build step and any later featureless rebuild of the same binary. Proved locally on macOS: the shell build links AppKit, CoreGraphics, QuartzCore and Metal where the featureless build links only `libSystem`, and `bundle.sh` packages either without complaint. No hosted release run has happened, so the Linux tarball and Windows installer remain unlaunched. |
| REPO-002 | Critical | `07ebc93`, `11d5751`; `docs/spikes/m2-voiceover-acceptance.md` | Partially resolved; M2 acceptance blocker remains | B5 | Existing VoiceOver/P12 entry, narrowed | Focus residuals are implemented. Keep the restored spike and required macOS probe gate, pass one real VoiceOver session, and implement/accept Linux and Windows adapters. |
| REPO-003 | High | `ACROBAT-PARITY.md:29-31,60` | Resolved in Audit Task 2 | B7 | Added and resolved by this audit | All 403 rows are reviewed and the summary totals mechanically match the rows; B7 retains REPO-010 and final acceptance. |
| REPO-004 | High | `docs/plans/m2-viewer.md:3` at baseline | Resolved in this task | B7 | Added and resolved by this audit | M2 plan header now matches live merge history and open gates. |
| REPO-005 | High | retained P10/agent branches and worktrees | Preserve, no cleanup | B0 | Added by this audit | Inventory remains reproducible; useful work reconciles additively only. |
| REPO-006 | Medium | `~/.claude/projects.md` | Resolved in Audit Task 3 | B7 | Added and resolved by this audit | External project registry now describes the live Rust viewer and open hardening gates. |
| REPO-007 | Medium | root `README.md`, `.project-docs/INDEX.md`, `.editorconfig` | Resolved in Audit Task 3 | B6 | Added and resolved by this audit | Minimal build, status, documentation, and editor entry points exist without a Makefile or task runner; B6 retains its separate release and packaging-doc work. |
| REPO-008 | High | `.github/workflows/ci.yml`; `deny.toml` | Resolved in B6; hosted run still unproven | B6 | Resolved; historical row retained | `deny.toml` reports `advisories ok, bans ok, licenses ok, sources ok` against cargo-deny 0.20.2, with six reviewed per-advisory exceptions and a tightened license list. The policy's own settings and the exact key set of every section are pinned by `supply_chain_policy_is_checked_in_and_gated`, which the supply-chain job runs beside cargo-deny: `cargo deny check` reports all four sections green against a blanket policy, so the action's verdict alone is not evidence. The action's `with:` block is pinned whole so it cannot be redirected at another file or narrowed to one section. |
| REPO-009 | High | `corpus/checksums/hayro-corpus.sha256`; fresh 41-file acceptance download | Resolved in B1.4 | B1 | Resolved; historical row retained | Every merge-gating `hayro-corpus` PDF has an enforced SHA-256 verified for cached and freshly downloaded sets after strict R2 manifest ID validation and safe publication pass. |
| REPO-010 | High | `PLAN.md:100-102,497`; `B7-REF-001/002` ledgered; B7-REF-003 Reader side pinned | Partially resolved; acceptance blocker remains | B7 | Existing parity screenshot entry narrowed | Capture the corrected B7-REF-003 Onionskin state, then compare, hash, and ledger B7-REF-003 through B7-REF-010; keep every private image ignored and untracked. |
| REPO-011 | Medium | `crates/app/tests/guarantees.rs:6-65` | Resolved in B6 | B6 | Resolved; historical row retained | Guarantees 1, 2 and 6 now execute, naming every `crates/cos` test that carries their sentence and proving each still states its clauses in an assertion rather than in a comment. Their old reasons named M1 twice over wrongly: M1 has passed, and PLAN.md places these three at M3. Guarantee 6 was additionally green and vacuous, because `corpus/malformed` is gitignored and no CI step generated it; CI now generates the set and reruns the repair suite with `ONIONSKIN_CORPUS_REQUIRED=1` so an absent corpus fails instead of skipping. The remaining four ignores match PLAN.md's milestone map: redaction, forms and tagged-PDF at M5, signatures at M6. |
| REPO-012 | Medium | `packaging/README.md:34` | Resolved in B6 | B6 | Resolved; historical row retained | `packaging/README.md` states what `--features shell` buys and every prerequisite the workflows install: `CARGO_NET_GIT_FETCH_WITH_CLI`, the macOS Metal toolchain, the GPUI Linux packages, and NSIS on Windows. The stubbed-artifact row now records what was proved on macOS and that Linux and Windows remain unvalidated. |

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
- Export background streaming, progress, and cancellation landed in C1.1 at
  `f0cbbcb` and `3ac647b`. C1.2 settings and native-input follow-ups are resolved
  at `4ddec23`, `c7f1afa`, and `62576fb`; APP-010 is retained as history.
  Remaining format and tagged-reading-order gaps keep their matrix owners.
- Arabic visual/logical ordering belongs to the later localization/text package;
  M2 matrix rows remain partial where it affects behavior.
- P12/VoiceOver belongs to B5. Linux/Windows first-release validation belongs to
  B6 and stays explicitly unavailable until run on those hosts.
- B7 has the private evidence protocol and first two comparisons; the remaining eight
  reference states still belong to B7.

`docs/plans/m2-app-hardening.md` is retained as a source ledger. Its live-item
disposition table follows from a line-by-line revalidation against `7413186`.

## M2 app-hardening backlog revalidation

| ID | Disposition | Owner | Live evidence / action |
|---|---|---|---|
| HARD-CAN-001 | Resolved in B2 | B2 | One `CanvasModel::canvas_point` is the only place the canvas origin is subtracted; `pointer_input` takes a `ViewPoint`; `scroll`/`pinch` take window points. The pan path never subtracted the origin at all, so a mid-drag origin move lost the grabbed content: pinned by `a_pan_follows_the_canvas_origin_when_it_moves_mid_drag`. `tile_rect` now calls core's `ViewRotation::rotate_rect_within`. `rotate_pixel` and `rotated_size` stay: `rotate_pixel` is that helper on a unit rect exactly, but runs once per pixel in a double loop, so calling it would cost per-pixel work for an identical result. |
| HARD-CAN-002 | Partly stale; live half resolved in B2 | B2 | `begin_frame` did not repeat across update and paint: there was one `begin_frame` in `update` and one `end_frame` in `paint_list`. The two evictions per interactive frame come from `handle_change` running a second full `update`, a duplicated update rather than a duplicated frame; left as it is, since moving it would move request queueing, poll arming and snapshot draining off the event that triggers them. Live half fixed: each of `update` and `paint_list` now opens and closes its own frame, on error paths too, and the drains stopped re-deriving the signature. Five visible-page queries per painted frame became three, not the two the B2 plan asked for; see that plan for why. |
| HARD-CAN-003 | Resolved in B2 | B2 | `TileStore::paint_source` touches and pins and hands back a shared `&TileCache`; `collect_tiles` takes `&TileCache`. `TileStore::base` is the pure read beside it. |
| HARD-CAN-004 | Resolved in B2 | B2 | `CanvasModel::sources` and `retain_visible_state` are gone; the store is the only raster owner. A page returning to view at a new zoom paints the resident raster scaled instead of blank. External review then found the deletion had removed an eviction-immune owner with nothing replacing its protection, so an over-budget zoom change painted nothing; fixed by having every visible page claim what it will paint from, before the early exits. Both regressions are pinned at a budget where eviction actually runs. |
| HARD-CAN-005 | Resolved in B2 | B2 | `placeholders` deleted. The poll-arming it appeared to carry is carried by `requests`, which the surviving test pins. |
| HARD-CAN-006 | Resolved in B2, unproven | B2 | `prepare_paint` returns the origin the model recorded and the paint closure uses it, so the model's mapping is the only one. Correct and worth having, but not proven by any test: `resize` assigns `canvas_origin` unconditionally before the fallible `viewport.resize`, so the two origins are equal on every path including the error path, and offsetting the painted origin breaks no test. Recorded as unproven rather than proven-by-the-suite. |
| HARD-CAN-007 | Resolved in B2 | B2 | Both coupled expects are gone. `collect_tiles` no longer looks the cache up a second time, and `schedule_visible_renders` selects on the geometry it needs rather than filtering on `PagePlacement::measured` and then asserting the geometry is there; the layout answers both from the same map, so the two forms select the same pages. |
| HARD-CAN-008 | Resolved in B2, narrowed | B2 | `ATLAS_GUTTER_PX` names the gutter that was encoded three ways across two functions. The rest of the bullet is rejected under this audit's closing note: `paint_image`'s trailing `0` and `false` are gpui's positional parameters, not a project invariant, and `px(8.0)`, `px(700.0)` and `gpui::white()` each appear once. |
| HARD-CAN-009 | Resolved in B2 | B2 | `ViewRotation` derives `PartialOrd, Ord` in declaration order, which is the numbering `rotation_code` assigned, so map order is unchanged and the hand-written table is gone. The rotation's place in the key had no test; adding one closed a real coverage gap rather than a live defect. |
| HARD-CAN-010 | Resolved in B2 | B2 | `has_pending_render`, `drain_geometry_responses` and `drain_render_responses` are private; `generation` is deleted, having drawn a dead-code warning the moment it stopped being `pub`. `paint_list`, `update` and `accessible_pages` stay public: the a11y package needs them as seams. |
| HARD-CHR-001 | Resolved in B3.7 | B3 | Document-search rows render the no-document unavailable reason inline instead of no-oping. |
| HARD-CHR-002 | Superseded | C1 | Find, Preferences, Select All, and Open Recent landed; Print/Properties remain correctly owned by M3. |
| HARD-CHR-003 | Reclassified in B3.7 | C1 | Quick-action milestone reasons have no second consumer today; defer plugin metadata until a real consumer exists. |
| HARD-CHR-004 | Resolved in B3.6 | B3 | Canvas and thumbnail context rows have command-specific milestone reasons. |
| HARD-CHR-005 | Deferred after B3.7 | C1 | No local themed-scrollbar API or distinct Acrobat matrix row blocks B3; keep default overflow scrolling. |
| HARD-CHR-006 | Partially resolved by `cb87b72` | post-1.0 | Full Screen now hides all chrome and fills the viewport, with Escape handling covered in `crates/app/src/shell/chrome/tabs/mod.rs`; presentation semantics remain partial and outside B3.7. |
| HARD-CHR-007 | Resolved | B3 | Startup now uses `MenuState::new`; the duplicate active-tab rule is gone. |
| HARD-CHR-008 | Resolved in B3.8 | B3 | The rail maps icon IDs to compact visual marks instead of drawing raw asset IDs such as `hand`. |
| HARD-CHR-009 | Deferred after B3.7 | C1 | Remaining live expects are internal hardening, not matrix-visible B3 parity. |
| HARD-CHR-010 | Deferred after B3.7 | C1 | Module ownership docs are internal hardening, not a matrix-visible B3 blocker. |
| HARD-CHR-011 | Reclassified after B3.7 | B3 | No shared-invariant literal change is required for matrix honesty. |
| HARD-CHR-012 | Resolved in B3.7 | B3 | The shared input reports the page field as `NumberInput`. |
| HARD-CHR-013 | Partially resolved by `cb87b72` | M3 | Dedicated Zoom To now opens a 12-preset chooser through `crates/app/src/shell/chrome/tabs/menu.rs`; custom percentage entry remains missing. Page zoom input is outside B3.7. |
| HARD-CHR-014 | Resolved in B3.7 | B3 | Invalid zoom renders as unavailable instead of 0%. |
| HARD-CHR-015 | Resolved in B3.7 | B3 | The empty side-panel host renders Tool details with a fitted empty-state prompt. |
| HARD-A11Y-001 | Partially resolved | B5 | P12 shipped the tree, roles, labels, page text, focus ring, actions, bounds, and probe; B5 made the macOS probe a required CI gate. Four of the six residuals are now closed. The focus ring is grouped, so Tab moves between surfaces and the arrows move inside one, proved by a keyboard sweep that reaches all 38 published stops. An AccessKit focus request moves GPUI's focus, and GPUI's focus moving into a text field moves the published cursor onto it. A press on a window macOS reports as not visible is drained and published on the main queue rather than on the window's display link, which gpui runs only while the window is visible. A page's text is extracted once a client has asked for the tree instead of on every frame; a page nobody has read says so rather than reading as empty. Remaining: one real VoiceOver session, per `docs/spikes/m2-voiceover-acceptance.md`, whose steps 7, 7b and 9 cover this work, and the Linux and Windows adapters, which stay no-ops. |
| HARD-PROC-001 | Historical | Process | The thin P2-P7 review trail cannot be repaired in source; the current package/review/evidence gates prevent recurrence. |
| HARD-PROC-002 | Resolved | B3 | P10 registered real tools and current rail/quick-action tests exercise the live registry. |
| HARD-PROC-003 | Resolved in Audit Task 2 | B7 | All 403 rows are reconciled and mechanically recounted; B7 retains private reference comparison and final acceptance. |

Resolved and superseded bullets stay in this table to preserve why they no longer
justify code changes. B2/B3 plans must use these current lines and reject the broad
comment/literal refactors that would add churn without behavior.

### What the P14 benches do not say about B2

`cargo bench -p onionskin-core` reports the same six counters before and after B2
(71281 bytes to open, 15570 tile fetches, 8394 tiles composited, 4 pages laid out
in one frame, 213674496 peak resident bytes while painting, 211577344 resident
after two hundred pages). That agreement is **not** evidence that HARD-CAN-004's
change of raster ownership is memory-safe, and must not be cited as such.
`crates/render/src/store.rs` is +51 -0 across the package, purely additive, and
the benches drive `TileStore` through their own harness: no bench constructs a
`CanvasModel`, and none calls `paint_source` or `base`. The counters match because
the code they exercise did not change. The raster-ownership change is unmeasured,
not measured-and-unchanged.

What can be said is reasoning rather than measurement. `BaseRaster` holds its
pixels in an `Arc<[u8]>` (`crates/render/src/base.rs:257`), so the deleted
`CanvasModel::sources` never duplicated a raster's bytes; it duplicated a handle
and, with it, a second lifetime. Removing it makes resident bytes track
`TileStore::resident_bytes` more honestly, which is neutral to better. Measuring
it would need a bench that drives the canvas, which does not exist.

`benches/paint.rs` skips for a missing `corpus/external/hayro-corpus/0041790.pdf`
and reported nothing either side.

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

At the 2026-08-31 audit, the app and repository auditors performed
source/test/doc/history inspection only. No cargo bench, hosted CI, platform
release, real VoiceOver, or Acrobat screenshot comparison was run then. B7 later
completed and ledgered B7-REF-001/002; the other eight reference states, along
with the remaining verification named above, stay open and must not be inferred
from this report.

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
