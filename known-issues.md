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
- The P9 cancel test (a_cancel_drops_the_search_queued_behind_the_walk_it_stops)
  has a roughly 1-in-1000 false-failure race: start() and cancel() are two
  sends, and a worker drain landing between them legitimately emits one page
  of the queued generation before hearing the cancel. Harden by tolerating
  queued-generation updates that precede the cancel rather than forbidding
  them outright.
- One real VoiceOver session is the remaining M2 acceptance item: P12 built
  the tree, the focus ring and an automated probe, but the probe messages the
  view directly and never leaves the process, so the AX server path,
  notification delivery and speech itself are unproven. Follow the 20-minute
  script at docs/spikes/m2-voiceover-acceptance.md and record the result
  there. Role::Document is fixed (the M1 spike misread it: the missing piece
  was the role description, not the role) and verified by execution.
- P12 residuals: arrow-key navigation was not shipped, so the focus ring is
  flat and every visible pane row is a tab stop, which makes Tab cross an
  open thumbnails pane in dozens of presses; a screen-reader press on an
  occluded window is queued rather than honoured, because gpui runs a
  window's display link only while macOS reports it visible and
  refresh_windows merely marks the window dirty; the shell extracts every
  visible page's text on every frame whether or not a client is listening,
  so first-visit content-stream extraction runs on the UI thread during
  scroll and P14's headless scroll bench cannot see it; two probe tests
  (the platform half of the press, and the prepaint rectangles) are guarded
  only by the continue-on-error probe job, which ci.yml plans to harden
  after its first green run - hold that plan or those guards stay soft.
- P12 focus dispatch: an AccessKit `Focus` request updates the internal ring but
  does not clear stale GPUI text-field focus or focus the requested real input.
  B5 must regress text-field focus -> screen-reader focus on Zoom In -> Enter and
  prove Zoom In runs. Linux and Windows adapters are currently no-ops; either wire
  them or scope M2 app-accessibility acceptance to macOS with named follow-ups.
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
