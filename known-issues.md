# Known issues

Living ledger; remove entries when resolved. Details in docs/spikes/ where
referenced.

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

- Export runs on the UI thread and buffers every page in memory: a
  whole-document PNG export of a large file freezes the shell for the full
  render and holds all pages' bytes at once. The memory cost is documented on
  the codec contract; the UI blocking is not. M3's export dialog should move
  it to a background task with a page range. Also from the P13 review:
  export_entries reports "codecs plugin not installed" when no document is
  open (latent, a zero-tab window is currently unreachable); a failed save
  dialog is swallowed like a cancel; numbered export files pad to {:03} so
  above 999 pages they stop sorting in page order; derived numbered files
  overwrite without the prompt the base name gets.
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
- content residual nits from review: the UTF-8 BOM path in pdf_text_string
  uses from_utf8_lossy, which can introduce U+FFFD into an /ActualText
  string (a hair against no-invented-characters; tighten when touched);
  the oracle's ERROR_CATEGORIES list tracks Error::category slugs by hand
  and would silently narrow the error ceiling if they drift.
- Search must normalize bidi text: content extraction yields Arabic as
  visual-order presentation forms (a faithful per-glyph transcript), but a
  user's search query arrives in logical order, so M2's find bar cannot
  match user-typed Arabic until search folds presentation forms and
  reorders. Surfaced by the content review's poppler comparison.
- One real VoiceOver session is an M2 acceptance item; the AccessKit GO was
  proven by direct view messaging only. Role::Document currently surfaces as
  AXGroup and must be fixed in M2. See docs/spikes/m1-shell-accesskit.md.
- Linux packaging script and Windows NSIS installer have inspection-only
  confidence; the first tagged release is their real test.
- Shell spike nit for M2: drag state not cleared on outside-window
  mouse-up.
- parity/reference/ screenshot corpus not yet captured (needs Acrobat driven
  on-screen; local-only per Legal posture rule 4).

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
