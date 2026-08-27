# Known issues

Living ledger; remove entries when resolved. Details in docs/spikes/ where
referenced.

## Upstream (hayro) - blocking or shaping our work

- Appearance-state dictionaries (/AP dict + /AS) render blank, silently.
  Blocks M5 forms rendering; we hold a fallback (resolve /AS ourselves).
  File upstream at LaurenzV/hayro. See docs/spikes/m1-render-hayro.md.
- InterpreterWarning cannot express skipped annotation appearances (the gap
  above is invisible to warning_sink). Second upstream report.
- RenderSettings has no origin, so no sub-rectangle base rendering; ratified
  workaround: tiles cache composites. Upstream feature request.
- RenderCache borrows the Pdf (self-referential storage); viewer-lifetime
  caching needs an owned cache upstream or yoke.

## Ours - accepted debt with a deadline

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

- gpui fork fetch needs CARGO_NET_GIT_FETCH_WITH_CLI=true (libgit2 ssh
  fallback fails); Xcode 26 needs `xcodebuild -downloadComponent
  MetalToolchain` before gpui shaders compile. Belongs in CI docs when the
  shell feature reaches CI.
