# Source report: core, render, content, search, and performance audit

Provenance:

- Audit date: 2026-08-31
- Repository: `<repo-root>`
- Baseline: `main` at `7413186`
- Scope: read-only independent audit of merged M0-M2 core/render/content/search/performance work, especially P1-P6, P9 non-UI search core, and P14.
- Source state during audit: `git status --short --branch` returned `## main`.
- Mutation policy during audit: no edits, formatters, staging, commits, or worktree changes.

All `file:line` references in this frozen report refer to baseline `7413186` and
may shift in later audit-ledger edits.

## Findings

| ID | Severity | File:line | Finding | Evidence / reproduction | Existing-ledger status | Recommended fix / test |
|---|---|---|---|---|---|---|
| CR-001 | Medium | `crates/content/src/page.rs:56`, `crates/content/src/page.rs:58`, `crates/core/src/session.rs:204`, `crates/app/src/shell/canvas.rs:349` | Negative page-tree root `/Count` is silently converted to `0` pages. That lets a malformed PDF with real reachable pages open as an empty document instead of failing loudly or repairing. | `content::page_count` does `Ok(count.max(0) as usize)`. `core::Document::open_shared` trusts that value. `CanvasModel::new` returns `CanvasError::EmptyDocument` when `page_count() == 0`. A fixture with `/Pages << /Kids [3 0 R] /Count -1 >>` and a valid page at `3 0 R` would hit this path even though `cos::Document::page(0)` can still walk the kid. Existing `cos pages` tests cover count-skip correctness but not negative root count. | Not found in `known-issues.md` at audit time. | Validate root `/Count >= 0` in `cos::Document::page_count` or `content::page_count`; return an explicit malformed-page-tree error instead of clamping. Add a regression fixture asserting negative root count does not become `EmptyDocument`. |
| CR-002 | Low | `crates/cos/src/document.rs:863` | Stale consumer comment still says `M2 tools-organize` for object deletion, while the current ledger says this belongs to M3 tools-organize. | Live code comment: `Consumer: M2 tools-organize`. `known-issues.md` already says the stale tag should be corrected to M3. | Partially captured: ledger captures the stale tag class, but the source comment is still stale. | Update the comment when next touching COS deletion docs. No behavior test needed. |

## Validated existing-ledger findings

| ID | Severity | File:line | Finding | Evidence / reproduction | Existing-ledger status | Recommended fix / test |
|---|---|---|---|---|---|---|
| CR-003 | Low | `crates/core/benches/scroll.rs:230` | P14 scroll bench undercounts recomposites when a page is evicted and later reinserted. | The bench writes `self.composited.insert(page, cache.composites())` and later sums map values at lines `376-388`, so later samples overwrite prior counts for the same page. | Adequately captured in `known-issues.md` at audit time. | Accumulate per-page composite counts, or track `(page, generation)` samples. Add a test or mutation proving eviction plus revisit increments the total. |
| CR-004 | Medium | `corpus/fetch.sh:38`, `corpus/fetch.sh:212` | Hayro R2 corpus objects are fetched without checksums. | The script pins hayro manifest revisions, but downloads `$HAYRO_ASSETS_BASE/$kind/$id.pdf` directly to `*.partial` and moves it into place without hash verification. | Adequately captured in `known-issues.md` at audit time. | Add checksums from the pinned manifest or a checked-in lockfile. Refuse mismatches. |
| CR-005 | Low | `crates/app/tests/guarantees.rs:83` | Guarantee 9 CI enforcement is textual and can be evaded by workflow shape changes. | The test scans strings for bench declarations, corpus required env, no `continue-on-error`, and CI job text. This verifies current wiring but is not semantic workflow validation. | Adequately captured in `known-issues.md` at audit time. | Keep as current tripwire; later replace or supplement with live branch-protection / workflow validation once remote policy exists. |
| CR-006 | Low | `known-issues.md:91` | Branch protection requiring the bench job could not be validated from local source. | Local CI workflow has a `bench` job and the guarantee test passed, but branch protection is remote repository state. | Adequately captured in `known-issues.md` at audit time. | Verify with GitHub API once the remote policy exists. |
| CR-007 | Low | `crates/core/src/search.rs:598` | P9 cancel flake remains plausible but did not reproduce in this run. | The targeted core test `search::tests::a_cancel_drops_the_search_queued_behind_the_walk_it_stops` passed once. Ledger already records a rare flake. | Adequately captured in `known-issues.md` at audit time. | Keep ledger entry until a stress loop or deterministic cancellation hook closes it. |

## Areas reviewed with no new findings

- P1 COS/content page access: lazy page lookup, inherited attributes, crop box, stream decode path, page accessor tests.
- P2 core `Document` session: shared bytes, COS ownership, render/search worker lifecycle, bounded page/text caches.
- P3 geometry: `PageGeometry` mapping, hayro transform usage, crop/rotation glyph-to-pixel tests.
- P4 render store/cache: `TileStore`, `TileCache`, overlay validation/indexing, eviction tests.
- P5 render worker: coalescing, stale-generation filtering, placeholder/raster ordering, `Drop` shutdown join.
- P6 core layout/viewport: viewport math, visible pages, selection quad mapping, restore guards.
- P9 non-UI search core: worker constructs COS on the background thread, streams one page at a time, generation cancellation, any/all/phrase search modes.
- P14 performance wiring: bench source inspected, CI job inspected, guarantee test passed.

## Verification commands and results

- `cargo test -p onionskin-core --tests`: passed.
- `cargo test -p onionskin-render --tests`: passed.
- `cargo test -p onionskin-content search`: passed.
- `cargo test -p onionskin-cos pages`: passed.
- `cargo test -p onionskin-app --test guarantees open_and_scroll_stay_within_the_performance_budgets`: passed.

Not run:

- `cargo bench`: not run. The audit checked bench code and CI wiring, not current wall-clock budget numbers.
