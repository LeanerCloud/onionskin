# M1 spikes (c) and (d): GPUI shell and AccessKit

Decision record for the two M1 spikes that de-risk the UI framework bet:
(c) a GPUI window rendering a page with pan/zoom/pinch, and (d) AccessKit
attached to that window. Spike code lives in `crates/app/src/bin/`
behind the `shell` cargo feature.

## Verdict: GO on AccessKit, no fork changes needed

The gpui fork pinned at rev `84bdb01` already implements
`raw_window_handle::HasWindowHandle` for `gpui::Window`
(`src/window.rs:4867`), returning an `AppKitWindowHandle` whose `ns_view`
is the real NSView. `accesskit_macos::SubclassingAdapter` is built for
views the caller did not create, so it attaches from application code:
the view's class becomes `AccessKitSubclassOfGPUIView`, and the first
accessibility query invokes the activation handler.

Zero changes to the fork were required. Upstream gpui is converging on
the same shape, exposing AccessKit behind `ZED_EXPERIMENTAL_A11Y` and
using `SubclassingAdapter::for_window`, so the eventual upstream path
matches what `crates/app` does today.

This settles the plan's GPUI accessibility risk: the escalation clause to
reconsider the framework is not triggered.

## What the probe actually proved, and what it did not

`a11y_spike` sends the `NSAccessibility` messages VoiceOver sends
(`accessibilityChildren`, `accessibilityFocusedUIElement`,
`accessibilityTitle`) straight to the view. Result:

```
probe: NSView class after attach = AccessKitSubclassOfGPUIView
a11y: accessibility client requested the initial tree
probe:   child role=AXGroup title=<nil> roleDescription=group
probe:     grandchild role=AXGroup title=Page 1 of 1, Onionskin accessibility spike
probe:     grandchild role=AXButton title=Zoom in roleDescription=button
probe:   focused role=AXGroup title=Page 1 of 1, Onionskin accessibility spike
```

Honesty findings, recorded because they bound the verdict:

- **VoiceOver itself was never run.** This machine grants no accessibility
  trust, so Accessibility Inspector and `AXUIElement` queries both return
  nothing (confirmed against Finder, not assumed).
- **Messaging the view directly is narrower than a real session, not
  stronger.** It isolates adapter correctness, but it skips the AX server
  path, cross-process marshalling, notification delivery and the actual
  announcement. **One real VoiceOver session is an M2 acceptance item.**
- **The focus read is partly self-inflicted.** The probe calls
  `update_view_focus_state(true)` before the window is provably key, so
  `accessibilityFocusedUIElement` reflects state the probe set itself.
  Real focus tracking, driven by window key state and by the shell's own
  focus model, is untested.

## Caveats for M2

- The adapter must be constructed before anything queries the view.
  Attaching on first render works today, but it is an ordering constraint,
  not a coincidence.
- A fork rebase that adds accessibility selectors to gpui's view class
  would collide with the runtime subclass. There is no conflict at
  `84bdb01`, where the view implements none of them. Re-check on every
  fork bump.
- AccessKit maps `Node::label` to `accessibilityTitle` and never sets
  `accessibilityLabel`. Code reading labels back must use the former.
- `Role::Document` currently flattens to `AXGroup`, and AccessKit
  deliberately suppresses the label on the root `Role::Window` node, so
  VoiceOver would announce a page as "group". Correct role mapping for
  document content has to be resolved in M2.

## Shell spike state

Fit-to-window on launch, drag pan, two-finger scroll pan, and zoom
anchored on the pointer via ctrl/cmd+scroll and via the fork's
`PinchEvent`. Verified by screenshot at fit and after zoom.

Live event injection needs an accessibility grant this machine does not
have, so `CGEventPost` is silently dropped. The view transitions the GPUI
handlers delegate to are therefore pinned by unit tests
(`cargo test --features shell --bin shell_spike`), mutation-checked by
breaking the anchor math and confirming the anchor tests fail. What
remains unproven by execution is the listener registration itself.

Known spike-quality nits, to fix when this becomes the real canvas:

- `drag_from` is not cleared when the mouse is released outside the
  window, so a drag can appear to continue.
- `fit()` runs once on first paint and is not re-run on window resize.

## Environment notes for CI and onboarding

- `CARGO_NET_GIT_FETCH_WITH_CLI=true` is needed to fetch the fork.
  Cargo's bundled libgit2 falls back to ssh and fails with
  "revision not found" even though the rev is the tip of `main`.
- Xcode 26 ships without the Metal toolchain. gpui's shader build script
  fails until `xcodebuild -downloadComponent MetalToolchain` has run
  (704 MB).
- Cold gpui build is roughly 4 minutes; incremental spike rebuilds are a
  few seconds.
