# Parity Reference Evidence

Status as of 2026-09-02: the first two private Reader/Onionskin baselines are
captured and compared. The clean Reader side of B7-REF-003 is pinned, and its
idle-polling defect is fixed and regressed at `4132a99`; the corrected Onionskin
capture remains blocked while the macOS GUI session is locked and both displays
are asleep; WindowServer rejects the window rectangle against the inactive
displays even though capture permission is allowed.
REPO-010 remains open until all eight incomplete states are captured and recorded.

## Privacy Rule

Acrobat reference screenshots and derived comparison images stay local-only under
ignored `parity/` subdirectories. The repository records metadata and results,
not Adobe UI image bytes.

## Required M2 Reference Set

| Evidence ID | State | Acrobat version | Onionskin commit | Reference file | Reference SHA-256 | Onionskin file | Onionskin SHA-256 | Result |
|---|---|---|---|---|---|---|---|---|
| B7-REF-001 | Unified shell baseline with a seed PDF open | Reader 25.001.20438 | `34e4632` | `unified-shell-two-page-actual-size-20260901T234420Z.png` | `e24801668e50cf30f93b3dcc15d70688929d4006931a794693047191abecd0c6` | `unified-shell-two-page-actual-size-20260901T234343Z.png` | `aecc1ddc6cc3126c87081e50d593df3abd841bd888f9a01d5f4087d37560a77e` | Pass for M2 shell/rendering baseline; wider tool gaps remain matrixed |
| B7-REF-002 | Find bar plus search results state | Reader 25.001.20438 | `34e4632` | `find-page-results-20260901T235431Z.png` | `433183c433150ac56ac555de905dcf06fd9b5351302adc71528de27bc987c799` | `find-page-results-20260901T235458Z.png` | `abf74209498e0a8c299484c3f09b0471d7f10480509a0e48d73473aa77d2ca6e` | Pass for M2 document Find; Reader tool/AI suggestions remain outside this claim |
| B7-REF-003 | Page thumbnails pane open | Reader 25.001.20438 | `4132a99` | `page-thumbnails-pane-20260902T002149Z.png` | `e9d2b0485192cf2618dc1cc2af94742cf4d64abca72a055b21a9c116ebe55f83` | pending | pending | Blocked on corrected exact-window capture; the deferred idle-poll lifecycle regression passes |
| B7-REF-004 | Bookmarks pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-005 | Attachments pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-006 | Layers pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-007 | Signatures pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-008 | Home recents list and thumbnail layouts | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-009 | Preferences dialog live M2 subset | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-010 | Read Mode and Full Screen entry states | pending | pending | pending | pending | pending | pending | pending |

## Acceptance Checklist

- [x] `parity/reference/` contains the first three private Acrobat reference screenshots.
- [ ] `parity/onionskin/` contains matching Onionskin captures from committed
      builds; B7-REF-003 still needs its corrected capture.
- [ ] `parity/comparison/` contains the local comparison output for each pair.
- [ ] Every row above names an Acrobat version, Onionskin commit, local
      filenames, SHA-256 values, and pass/fail result.
- [x] The `parity_privacy` test passes before this commit updates the
      ledger.
