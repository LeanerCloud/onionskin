# Known issues

Living ledger; remove entries when resolved. Details in docs/spikes/ where
referenced.

The first sentence's removal instruction is superseded from the 2026-08-31 forward
audit onward: resolved entries are retained with an explicit resolution note so
their history is not erased. See `docs/audits/m2-forward-audit.md` for stable
source IDs, severity, ownership, and required proof.

## Upstream (hayro) - blocking or shaping our work

- Appearance-state rendering (/AP dict + /AS): FIXED on hayro's unreleased
  main (upstream commit 6af63be9, 2026-07-08); crates.io 0.7.1 predates it.
  We pin the cristim/hayro fork (upstream main + one commit) until a release
  ships; then return to the registry version. No M5 blocker remains.
- InterpreterWarning silent-skip gap: resolved by our fork commit 33d9caf8
  (UnresolvedAnnotationAppearance variant); filed upstream as
  LaurenzV/hayro#1359 (2026-08-27, with reproducer, screenshots and the
  #[non_exhaustive] semver note). Track the PR: if upstream takes it or the
  #[non_exhaustive] alternative, re-pin accordingly; onionskin-render
  re-exports the enum, forwarding the exhaustive-match hazard either way.
- Annotation-level optional content: hayro's annotation loop never reads an
  annotation dict's /OC (only the /F hidden flag), so no OCG override can
  hide an annotation whose visibility is layer-controlled; form XObject /OC
  on appearance streams is honoured. /VE visibility expressions unsupported
  upstream. Affects the M2 Layers pane; upstream feature request.
- RenderSettings has no origin, so no sub-rectangle base rendering; ratified
  workaround: tiles cache composites. Upstream feature request.
- RenderCache borrows the Pdf (self-referential storage); viewer-lifetime
  caching needs an owned cache upstream or yoke.

## Ours - accepted debt with a deadline

- Historical corpus proof scripts are retained references, not acceptance tools:
  `tripwire.sh` mutates its checkout's workflow and masks the guarantee test's
  exit status, `rerun.sh` deletes each command's raw log and treats an empty
  suite list as success, and `record.sh` does not assert expected scenario
  outcomes. This integration preserves them without running them. Current
  acceptance uses direct commands with retained raw logs and subprocess exit
  statuses.

- Native menu availability: Select All is disabled without a document
  or the commands-core plugin, even when Home search has text focus. Correct
  native menu availability and focus-driven refresh in the next menu pass.
  The separate focused-input dispatch and missing AXValue defects found during
  C1.2 acceptance are resolved by `62576fb` and `c7f1afa`; native Cmd+A,
  Edit > Select All, and macOS field-value reads were verified on 2026-09-10.

- Dialog accessibility geometry, found 2026-09-10: About, Keyboard Shortcuts,
  Zoom To, and Preferences publish controls without position bounds. The modal
  branch in `shell/chrome/tabs/accessible.rs` returns before geometry placement;
  `Surface::Dialog` measurements from the older dialog bodies are not consumed.
  Keyboard focus identity is published, but older modal buttons also lack a
  visible keyboard-focus style. Correct and verify those bodies during their
  next accessibility pass; C1.2's export-modal work is scoped separately.

- B0 restored `crates/content/src/filter.rs`, `shell_spike.rs`, and
  `a11y_spike.rs` byte-for-byte as retained historical references after earlier
  sessions deleted them. Production decoding continues through
  `cos::Document::decode_stream`; the content file is outside the module tree,
  and both spike binaries require explicit opt-in features. Keep these artifacts
  preserved without making production depend on them. Commands embedded in the
  historical spike sources may name their former feature gates; current checks use
  `shell-spike` and `a11y-spike` as declared in `crates/app/Cargo.toml`.

- cos: the fuzz target builds on stable but needs a nightly toolchain to
  actually run - install one when wiring fuzz into CI. Until then the
  robustness suite drives the same paths on stable. The four other
  carry-forwards in docs/spikes/m1-cos.md are built: streaming save,
  per-object recovered stream boundaries, object deletion with a chained
  free list, and caller-driven mid-session escalation to a scan.
- cos: Document::recovered_boundaries reports what has been parsed, because
  parsing is lazy. A cross-reference stream is always covered (opening parses
  it) and M5 redaction reads every stream it rewrites, so both see what they
  touch; a caller wanting a whole-file answer has to walk the xref itself.
- cos lazy I/O budget, reproduced 2026-09-10 at `65d29c6`: the default-only
  corpus enables the 10,000-page Isartor fixture and reads 1,857,768 of
  4,010,934 bytes through first-page access, exceeding the 25% budget.
  Growing windows reread prefixes, and structural validation parses a large
  Pages dictionary that first-page access parses again. P1c owns the fix;
  see `docs/evidence/m3-p1c-read-windows.md`. Do not relax the budget or mark
  corpus CI ready from test-result counts alone.
  **Read-window checkpoint:** retaining prefixes reduces the same scenario to
  1,335,528 bytes (33.3%), with unchanged unique coverage and successful
  first-page access. Duplicate validation parsing and window overfetch remain;
  the 25% budget still fails and P1c is not complete.
  **Validated-dictionary reuse checkpoint (2026-09-11):** retaining exact
  in-file dictionaries parsed during clean validation reduces the same
  through-first-page scenario to 1,072,232 bytes (26.73%), down 263,296 bytes
  (19.7%) from the suffix-read checkpoint and 785,536 bytes (42.3%) from the
  original main baseline. The unchanged budget assertion checks through-first-page
  reads, which remain over budget; open alone is 1,070,056 bytes (26.68%) and
  also exceeds 25%. Corpus CI, required-input enforcement, the shared helper,
  hosted timing/mutation evidence, P1c integration and the remaining 25%
  performance work remain open. This COS checkpoint does not close the named
  M2 carry-forwards: VoiceOver, Linux/Windows accessibility, private B7 states
  003-010, hosted release/required-checks/platform-package work, or three-pass
  acceptance.
  **Integrated Task B checkpoint (2026-09-14):** commit `432e1f7` measures the
  exact Isartor consumer at 941,160/4,010,934 returned bytes (23.46%) through
  first-page access, with 10,000 pages; 32 of 39 size-qualified lazy candidates
  were measured and seven existing exclusions remain. Required corpus targets,
  the COS library and `write_new` passed directly, and the local amd64
  Docker/`act` simulation exited 0. This is not hosted CI, Windows or native
  acceptance. Independently reviewed native macOS scenario evidence is recorded
  in `docs/evidence/milestone-screenshots.md`; the interrupted chooser failure
  was not reproduced and has no broader stability claim. The measured local 25%
  regression is resolved on this corpus; hosted timing/branch enforcement,
  Windows acceptance and final milestone acceptance remain open.
- cos: only the copy loop of a save is bounded memory. A repaired or
  escalated document assembles a section carrying a full table, which
  materializes every compressed object it has to copy forward, and
  escalate_to_scan reads the whole file to scan it. Both are repair-path
  costs on a file that is already damaged; revisit if M2 meets a large one.
- cos: deleting the object the trailer names as /Root is refused, but
  deleting a /Pages node or a page still referenced by one leaves a dangling
  reference. M3's tools-organize (per PLAN.md's milestone map) has to fix up
  the page tree itself.

- Historical P13 audit wording retained, with current disposition below. Export
  previously ran on the UI thread and buffered every page in memory: a
  whole-document PNG export of a large file froze the shell for the full render
  and held all pages' bytes at once. Also from the P13 review:
  export_entries reports "codecs plugin not installed" when no document is
  open (latent, a zero-tab window is currently unreachable); the historical
  prompt, numbering, and derived-overwrite findings are resolved below.
  **2026-09-01 disposition:** export and attachment prompt failures are visible,
  cancellation remains silent, numbered export width follows the page count,
  and derived destinations are reserved without overwrite before writing. The
  export UI-thread and whole-document memory findings remained APP-010.
  **2026-09-02 C1.1 disposition:** `f0cbbcb` adds a shared-byte export snapshot;
  `3ac647b` moves codec and output work to a background worker. Single streams
  page chunks into one temporary file and publishes it atomically at completion;
  PerPage publishes completed page files incrementally. Single keeps one
  temporary-file writer; PerPage keeps at most one destination writer open.
  Visible and accessible progress,
  cancellation, and cleanup-before-guard-release are proved. APP-010 is now
  limited to the C1.2 user-facing page-range/settings dialog at that checkpoint.
  **2026-09-10 C1.2 disposition:** `4ddec23` adds validated page ranges and PNG
  DPI before destination selection. `c7f1afa` and `62576fb` fix native field
  values and focused Select All. APP-010 is resolved: 624 shell/support tests
  passed (7 ignored), and native page-2 PNG/text subset exports were verified.
  See `docs/evidence/milestone-screenshots.md` for window-only evidence.
- P10/foundation review follow-ups (app shell): B2.2 resolved the first-error
  and stale worker-wait portions by preserving a primary pointer/update error
  over a same-cycle snapshot failure and resetting the poll wait deadline before
  recording an update-error exit. B2.3 resolved canvas context-menu
  repositioning: a second right-click inside document bounds reuses the existing
  open path, while a right-click outside document bounds still dismisses the
  menu. B2.4 resolved the remaining snapshot lifecycle gap by bounding snapshot
  pixel allocation, moving PNG encoding to GPUI's background executor, reporting
  encode errors, preserving same-cycle primary errors, and ignoring stale
  snapshot completions, including completions older than a failed request at
  generation wrap. Still open: the NaN-quad guard in raster_crop and the
  corner-by-corner overlay mapping both
  lack tests that would fail a bounding-rect implementation;
  zoom_limits derives the raster ceiling from the current page only, so a larger
  visible neighbour can still hit
  UnrenderableSize at an allowed zoom (handled gracefully via failed_renders and
  the status line); RowIndex::widest_by_gaps is a two-element array that would
  panic if a future layout mode put three pages in a row (loud, currently
  unreachable); SearchResult::Unavailable::reason is &'static str so the tool
  palette shows a fixed pointer to the status line rather than the actual
  activation error - widen it after P9 lands, since P9 owns that file.
- **Resolved at `7413186`; historical finding retained:** Global action listeners
  run synchronously inside a window update, during
  which gpui takes the window off cx.windows, so any listener body calling
  window_handle.update() fails with "window not found" and drops the command
  with only an eprintln. P9 hit this on Ctrl+F and fixes its own listener with
  cx.defer; main's pre-existing CloseTab and RunViewMenu listeners on the
  native-menu path have the same latent shape and want the same treatment plus
  a keystroke-dispatch test. Found by executing a real ctrl-f in a gpui test,
  not by reading the code.
  Current `RunCommand`, close-tab, and view-menu paths defer correctly, and shell
  tests cover the real find/close/view keystrokes. B2.1 reverified this at
  `483e512` with focused real-window Find, Close Tab, and Actual Size keystroke
  tests plus the full shell target. A live Command-F smoke opened the document
  Find bar in the shell; screenshot `M2-B2-T1-001` records the window-only
  evidence. No code change is justified.
- P8/P14 follow-ups: the outline destination map sweeps cos::Document::page(i)
  once to resolve bookmarks that name a page by reference, which is O(pages)
  walks on a large outlined document; attachment save writes on the UI thread
  like export does; guarantee 9's CI checks are textual tripwires, evadable by
  a softened harness with a stray assert. Branch protection must REQUIRE the
  bench job through an enforceable hosted rule; `origin` now exists but the
  rule is not verified, so the budget gate binds the job and not the merge.
- P11 residuals: a pre-existing 0755 config directory keeps its mode (only newly
  created ones get 0700), and the save-time rescue copy is silent because
  keep_unreadable's note is discarded there.
- P8 residual: six dead-code warnings under --features shell alone
  (fixtures.rs x4, panes/mod.rs, panes/thumbnails.rs) - CI lints only
  --workspace --all-targets and the app under shell,shell-test-support, so
  they are unlinted today; fix the cfg shapes if CI ever adds that
  configuration, so "clippy is clean" stays honest.
- Content residual nit from review: the UTF-8 BOM path in pdf_text_string
  uses from_utf8_lossy, which can introduce U+FFFD into an /ActualText
  string (a hair against no-invented-characters; tighten when touched).
- **Resolved in B1.2:** the content oracle records page-count, extraction, and
  `pdftotext` failures explicitly. Its error-rate ceiling no longer depends on
  a duplicated `ERROR_CATEGORIES` slug list, and nonzero `pdftotext` exits are
  errors carrying their status and diagnostic output.
- Search normalizes presentation forms but not order: P9 ships Unicode
  presentation-form folding on both needle and haystack, so single-run Arabic
  now matches a user-typed query. What remains is visual-to-logical
  reordering, deferred per T7: extraction yields Arabic in visual order, so a
  query spanning a reordered run still misses. Surfaced by the content
  review's poppler comparison.
- One real VoiceOver session is the remaining M2 acceptance item: P12 built
  the tree, the focus ring and an automated probe, but the probe messages the
  view directly and never leaves the process, so the AX server path,
  notification delivery and speech itself are unproven. Follow the 20-minute
  script at docs/spikes/m2-voiceover-acceptance.md and record the result
  there. Role::Document is fixed (the M1 spike misread it: the missing piece
  was the role description, not the role) and verified by execution.
- **Resolved in B5:** P12 now groups the focus ring so Tab crosses surfaces and
  arrows move within one, publishes queued presses from occluded macOS windows
  on the main queue, and extracts page text on demand rather than every frame.
  The macOS accessibility probe is a required CI step after its local pass.
- **Resolved in B5:** AccessKit `Focus` requests and GPUI text-field focus now
  synchronize through the published cursor. Linux and Windows adapters remain
  no-ops, and one real VoiceOver session is still required for acceptance.
- Linux packaging script and Windows NSIS installer have inspection-only
  confidence; the first tagged release is their real test.
- Shell spike nit for M2: drag state not cleared on outside-window
  mouse-up.
- **Partially resolved in B7 (REPO-010):** the Reader 25.001.20438 unified-shell
  and document-Find baselines and matching Onionskin states are captured,
  compared, hashed, and ledgered under the ignored parity directories. The clean
  Reader Page Thumbnails reference is pinned. Its black-placeholder defect was
  fixed at `4132a99` by rearming canvas polling after deferred thumbnail requests,
  with a GPUI-loop-only regression. The corrected exact-window Onionskin capture
  remains blocked while the macOS GUI session is locked and both displays are
  asleep/inactive. Capture permission is allowed, but WindowServer rejects the
  rectangle against the inactive displays, so eight required states remain
  incomplete. Private image bytes stay local-only per Legal posture rule 4.

## 2026-08-31 forward-audit additions

- **Release blocker (REPO-001, owner B6):** `.github/workflows/release.yml`
  builds `onionskin-app` without the `shell` feature. A produced artifact can take
  the headless/non-viewer path instead of opening the production UI. Build/package
  with `--features shell`, mirror platform prerequisites, and smoke-launch the
  packaged viewer before calling a release usable.
- **Resolved in B1.1 (CR-001):** a negative root `/Count` now returns COS's
  typed `invalid-page-count` error. Content propagates it, core refuses to open
  an empty session, and corpus sweeps no longer turn page-count errors into zero
  pages. The COS, content, and core regressions use a reachable page kid, so the
  malformed count is the only reason the document is refused.
- **Resolved in B1.4 (CR-004 / REPO-009):** every merge-gating
  `hayro-corpus` PDF now has a tracked SHA-256. Cached and freshly downloaded R2
  sets are verified before success, malformed manifest IDs fail before network
  access, checked and unchecked R2 sets publish through private staging, and
  checksum verification rejects symlinks, root swaps, and incomplete trees.
- **Resolved in B4.2 (APP-001):** export and attachment-save prompts retain the
  originating canvas identity. Four GPUI regressions prove switching or closing
  the tab before either prompt completes writes nothing.
- **Resolved in B4.3 (APP-009):** attachment save reports typed prompt failures
  through pane feedback while cancellation remains silent.
- **M2 parity acceptance (REPO-003/010; REPO-004 resolved):** this audit corrected
  the M2 plan header. Audit Task 2 reconciles all 403 Acrobat rows and adds an
  executable totals contract. B7 has the privacy scaffold, the first two private
  comparisons, and the pinned Reader side plus code/test fix for B7-REF-003; it
  still owns the eight incomplete states and final acceptance.
- **Dependency/release policy (REPO-008/011/012, owner B6):** checked-in advisory,
  license, secret, and dependency-update policy is missing; repository guarantee
  stubs and packaging docs do not yet match landed behavior and release needs.
- **Resolved retained prior deletions (B0):** `d683def` restored
  `crates/content/src/filter.rs`, `shell_spike.rs`, and `a11y_spike.rs` with their
  exact pre-deletion hashes. The content reference remains outside the module tree;
  the spike binaries remain behind explicit opt-in features.
- **Resolved dispatch report retained:** the prior global-action-listener warning
  is stale at `7413186`. Current `RunCommand`, close-tab, and view-menu paths defer
  correctly and shell tests cover real find/close/view keystrokes. B2.1
  reverified the same routes at `483e512`; no production code change was needed.
- **Resolved retained source-report duplicate (B0):**
  `docs/audits/source-reports/2026-08-31-repo-docs-ci.md` is tracked on `main`
  with the preserved SHA-256
  `7ddb97efad7d554ebfbfc67b94a003248e9357d38e6a214b61cab170c8daf0a7`.
  The audit branch accepted the current main tree additively; no removal, move,
  checkout overwrite, or history rewrite was used.

## Environment

- ANY git dependency fetch needs CARGO_NET_GIT_FETCH_WITH_CLI=true: the
  user's gitconfig rewrites https to ssh, which libgit2 cannot authenticate
  (verified with a cold CARGO_HOME). Since crates/render now pins the hayro
  fork by git rev, every cold default build needs it, not just the shell
  feature. Xcode 26 needs `xcodebuild -downloadComponent MetalToolchain`
  before gpui shaders compile. Both belong in CI configuration.
- The hayro git pin drags vello_cpu/vello_common to 0.0.9 at the git rev
  hayro main itself pins (registry had 0.0.8). Rev-locked so reproducible;
  if M2's vello backend picks its own vello version cargo will carry two
  copies (bloat, not breakage). Revisit when hayro cuts a release.
