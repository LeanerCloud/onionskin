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
  runs on the UI thread and buffers every page in memory: a
  whole-document PNG export of a large file freezes the shell for the full
  render and holds all pages' bytes at once. The memory cost is documented on
  the codec contract; the UI blocking is not. M3's export dialog should move
  it to a background task with a page range. Also from the P13 review:
  export_entries reports "codecs plugin not installed" when no document is
  open (latent, a zero-tab window is currently unreachable); a failed save
  dialog is swallowed like a cancel; numbered export files pad to {:03} so
  above 999 pages they stop sorting in page order; derived numbered files
  overwrite without the prompt the base name gets.
  **2026-08-31 disposition:** export prompt failures are no longer swallowed;
  `start_export` handles `Ok(Err(error))`. Attachment save still swallows prompt
  failure and is tracked separately as APP-009. The remaining export UI-thread,
  memory, numbering, and derived-overwrite findings stay open.
- P10/foundation review follow-ups (app shell): a snapshot error can
  overwrite a pointer error in the single status slot (keep-first or queue);
  PNG snapshot encode is synchronous on the UI thread with no size cap; a
  second right-click while the canvas context menu is open is swallowed
  entirely by the occluding dismiss layer rather than repositioning as
  Acrobat does; the NaN-quad guard in raster_crop and the corner-by-corner
  overlay mapping both lack tests that would fail a bounding-rect
  implementation; zoom_limits derives the raster ceiling from the current
  page only, so a larger visible neighbour can still hit UnrenderableSize at
  an allowed zoom (handled gracefully via failed_renders and the status
  line); RowIndex::widest_by_gaps is a two-element array that would panic if
  a future layout mode put three pages in a row (loud, currently
  unreachable); the canvas poll's waiting set survives an error exit from
  the loop, so a much later re-arm can name just-requested pages in
  WorkerSilent; SearchResult::Unavailable::reason is &'static str so the
  tool palette shows a fixed pointer to the status line rather than the
  actual activation error - widen it after P9 lands, since P9 owns that file.
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
  tests cover the real find/close/view keystrokes. No code change is justified
  unless B2 re-verification disproves that evidence.
- P8/P14 follow-ups: the outline destination map sweeps cos::Document::page(i)
  once to resolve bookmarks that name a page by reference, which is O(pages)
  walks on a large outlined document; attachment save writes on the UI thread
  like export does; the P14 scroll bench overwrites rather than accumulates a
  page's composite count on evict-and-reinsert, so an eviction-churn variant
  of the frame-pinning regression could under-count (the primary mode is
  caught); guarantee 9's CI checks are textual tripwires, evadable by a
  softened harness with a stray assert. Branch protection must REQUIRE the
  bench job once a remote exists, or the budget gate binds the job and not
  the merge, and corpus/fetch.sh verifies no checksum for the R2-hosted
  objects the bench job now puts on the PR path.
- P8 residuals: Reduce/Enlarge does not advance the thumbnail epoch, so a
  page pending at the old size that falls outside the new band keeps a
  wrong-size (content-correct) picture until eviction - one-line fix is to
  clear pending or advance the epoch on a size change. The layers pane lists
  /OCGs in flat file order rather than /D /Order's tree, so nesting and the
  spec's intent that groups omitted from /Order stay hidden are both lost;
  record this against parity row 202 when it flips to shipped.
- P11 residuals: keymap's NAMED_KEYS whitelist is a subset of gpui's real key
  set (f19 through f35, back and forward are missing), so binding f19 on an
  Apple extended keyboard is refused with "names a key this build cannot
  produce", which is false on that hardware; the comment claiming the table
  comes from gpui's own platform table overstates it. The preferences
  carry-forward cap is 64..=71 rather than 64, because take() runs over a
  prefix that may sort all unknown keys first - filter out the build's own
  keys before take(). Informational: a pre-existing 0755 config directory
  keeps its mode (only newly created ones get 0700), and the save-time rescue
  copy is silent because keep_unreadable's note is discarded there.
- P8 residual: six dead-code warnings under --features shell alone
  (fixtures.rs x4, panes/mod.rs, panes/thumbnails.rs) - CI lints only
  --workspace --all-targets and the app under shell,shell-test-support, so
  they are unlinted today; fix the cfg shapes if CI ever adds that
  configuration, so "clippy is clean" stays honest.
- content residual nits from review: the UTF-8 BOM path in pdf_text_string
  uses from_utf8_lossy, which can introduce U+FFFD into an /ActualText
  string (a hair against no-invented-characters; tighten when touched);
  the oracle's ERROR_CATEGORIES list tracks Error::category slugs by hand
  and would silently narrow the error ceiling if they drift.
- Search normalizes presentation forms but not order: P9 ships Unicode
  presentation-form folding on both needle and haystack, so single-run Arabic
  now matches a user-typed query. What remains is visual-to-logical
  reordering, deferred per T7: extraction yields Arabic in visual order, so a
  query spanning a reordered run still misses. Surfaced by the content
  review's poppler comparison.
- The P9 cancel test (a_cancel_drops_the_search_queued_behind_the_walk_it_stops)
  has a roughly 1-in-1000 false-failure race: start() and cancel() are two
  sends, and a worker drain landing between them legitimately emits one page
  of the queued generation before hearing the cancel. Harden by tolerating
  queued-generation updates that precede the cancel rather than forbidding
  them outright.
- One real VoiceOver session is an M2 acceptance item; the AccessKit GO was
  proven by direct view messaging only. Role::Document currently surfaces as
  AXGroup and must be fixed in M2. See docs/spikes/m1-shell-accesskit.md.
- Linux packaging script and Windows NSIS installer have inspection-only
  confidence; the first tagged release is their real test.
- Shell spike nit for M2: drag state not cleared on outside-window
  mouse-up.
- parity/reference/ screenshot corpus not yet captured (needs Acrobat driven
  on-screen; local-only per Legal posture rule 4).

## 2026-08-31 forward-audit additions

- **Release blocker (REPO-001, owner B6):** `.github/workflows/release.yml`
  builds `onionskin-app` without the `shell` feature. A produced artifact can take
  the headless/non-viewer path instead of opening the production UI. Build/package
  with `--features shell`, mirror platform prerequisites, and smoke-launch the
  packaged viewer before calling a release usable.
- **Malformed page-tree correctness (CR-001, owner B1):** a negative root `/Count`
  is clamped to zero in `content::page_count`, so a malformed file with reachable
  page kids can be reported as an empty document instead of failing or repairing
  explicitly. Add a real negative-count fixture and typed error/repair behavior.
- **Stale-tab asynchronous writes (APP-001, owner B4):** export and attachment-save
  completions retain a canvas while a path prompt is pending and do not prove the
  originating tab/document still exists before writing. Close/switch-before-prompt
  regression tests must prove no stale write occurs.
- **Attachment prompt errors (APP-009, owner B4):** attachment save treats a prompt
  failure like cancel, unlike export. Surface the typed prompt error and test it.
- **M2 parity acceptance (REPO-003/010; REPO-004 resolved):** this audit corrected
  the M2 plan header. Audit Task 2 reconciles all 403 Acrobat rows and adds an
  executable totals contract. B7 still owns private reference comparison and final
  acceptance.
- **Stale context-menu milestone reasons (owner B3):** the canvas labels Edit Text,
  Redact Text, and Create Link as M3 although all three are M5, and the thumbnails
  menu's shared M3 reason also covers the M5 Crop command. Derive or test these
  reasons against owning command metadata instead of duplicating milestone strings.
- **Dependency/release policy (REPO-008/011/012, owner B6):** checked-in advisory,
  license, secret, and dependency-update policy is missing; repository guarantee
  stubs and packaging docs do not yet match landed behavior and release needs.
- **Retained prior deletions (owner B0):** `crates/content/src/filter.rs` and
  `crates/app/src/bin/shell_spike.rs` were deleted in prior sessions. Restore their
  exact blobs additively without selecting obsolete code in normal builds.
- **Active P12 spike preservation (owner B5):** main retains `a11y_spike.rs`; the
  active P12 owner must remove its pending deletion before handoff.
- **Resolved dispatch report retained:** the prior global-action-listener warning
  is stale at `7413186`. Current `RunCommand`, close-tab, and view-menu paths defer
  correctly and shell tests cover real find/close/view keystrokes. No code change
  is justified unless B2 re-verification disproves this evidence.
- **Current-session retained duplicate (owner B0):** source-report freezing accidentally
  created an untracked main-checkout copy of
  `docs/audits/source-reports/2026-08-31-repo-docs-ci.md`. It is byte-identical to
  the audit-worktree copy (SHA-256
  `7ddb97efad7d554ebfbfc67b94a003248e9357d38e6a214b61cab170c8daf0a7`) and is
  preserved. Before feature integration, track those exact existing bytes in an
  additive main commit and rebase the audit package, so no removal or move is
  needed.

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
