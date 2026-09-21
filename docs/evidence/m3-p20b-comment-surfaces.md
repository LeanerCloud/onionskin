# M3 P20b verification: the comment surfaces around the pane

Date: 2026-09-21. Branch: `feat/m3-p2-edit-graph`. Linux x86-64, stable
toolchain, GPUI's test platform. No macOS, Windows or hosted-CI run is
claimed.

P20b is the rest of P20 after the pane itself (P20a). This file grows with
each part as it lands.

## Quick actions: Comment, Highlight and Draw

Row flipped: Quick action toolbar, M2 `partial` to `implemented`.

**What the user gets.** The floating toolbar's Comment, Highlight and Draw
buttons are live. Comment places a sticky note, which is Acrobat's Add
Comment quick action. Highlight is the highlighter and Draw is the pencil.
Fill text fields and Add Sign stay disabled with their M5 reason.

**How it is built.** The buttons were already resolved through the
registry. What changed is which tool a button picks: the first tool made
for the action, meaning its first listed capability, and only then the
first that lists it at all. Without that, Comment picked the highlighter,
which lists Comment second because a highlight is a comment, and is
registered before the sticky note.

**Runs.**

- `quick_actions::tests::comment_highlight_and_draw_are_live_on_the_tools_acrobat_uses`
  reads the registry the app builds. It asserts each button is enabled with
  no reason, and that the tools are `sticky-note`, `highlight` and `ink`.
- `multiple_capabilities_enable_every_match_and_a_tool_made_for_the_action_wins`
  replaces the old "first matching tool wins" test. A later tool made for
  drawing now wins Draw over an earlier one that only also draws.
- `cargo test -p onionskin-app --features shell,shell-test-support --lib --
  quick_actions`: 18 pass.
