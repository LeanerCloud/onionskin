# Known issues

Living ledger; remove entries when resolved. Details in docs/spikes/ where
referenced.

## Upstream (hayro) - blocking or shaping our work

- Appearance-state rendering (/AP dict + /AS): FIXED on hayro's unreleased
  main (upstream commit 6af63be9, 2026-07-08); crates.io 0.7.1 predates it.
  We pin the cristim/hayro fork (upstream main + one commit) until a release
  ships; then return to the registry version. No M5 blocker remains.
- InterpreterWarning silent-skip gap: resolved by our fork commit 33d9caf8
  (UnresolvedAnnotationAppearance variant). Candidate for a small upstream
  PR; note in any PR body that the enum is not #[non_exhaustive], so the
  variant breaks exhaustive matchers (it broke our own example), and that
  onionskin-render re-exports the enum, forwarding the same hazard.
- RenderSettings has no origin, so no sub-rectangle base rendering; ratified
  workaround: tiles cache composites. Upstream feature request.
- RenderCache borrows the Pdf (self-referential storage); viewer-lifetime
  caching needs an owned cache upstream or yoke.

## Ours - accepted debt with a deadline

- cos (see docs/spikes/m1-cos.md): silent endstream recovery records no
  RepairReason - must be plumbed before M5 so redaction knows a stream
  boundary was recovered, not authoritative; save materializes the whole
  file (stream it in the M1 build); no delete API / free-list handling yet
  (M2); mid-session escalation from clean open to full scan is designed but
  not built; the fuzz target builds on stable but needs a nightly toolchain
  to actually run - install one when wiring fuzz into CI.

- One real VoiceOver session is an M2 acceptance item; the AccessKit GO was
  proven by direct view messaging only. Role::Document currently surfaces as
  AXGroup and must be fixed in M2. See docs/spikes/m1-shell-accesskit.md.
- Linux packaging script and Windows NSIS installer have inspection-only
  confidence; the first tagged release is their real test.
- Render M2 to-dos: tile eviction policy, degenerate-overlay rejection at
  add_overlay, full-region highlight test, decryption error Display leaks
  debug formatting, per-tile overlay index, render_annotations knob.
- Shell spike nits for M2: drag state not cleared on outside-window
  mouse-up; fit-to-window not re-run on resize.
- Decision 11's bench file: the corpus has no real 1000-page document;
  fetch or synthesize one before wiring the perf gates.
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
