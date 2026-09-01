# Parity Reference Evidence

Status as of 2026-09-02: scaffolded, no private reference corpus captured in
this slice. REPO-010 remains open until the private Acrobat screenshots and the
first Onionskin comparison are captured and recorded.

## Privacy Rule

Acrobat reference screenshots and derived comparison images stay local-only under
ignored `parity/` subdirectories. The repository records metadata and results,
not Adobe UI image bytes.

## Required M2 Reference Set

| Evidence ID | State | Acrobat version | Onionskin commit | Reference file | Reference SHA-256 | Onionskin file | Onionskin SHA-256 | Result |
|---|---|---|---|---|---|---|---|---|
| B7-REF-001 | Unified shell baseline with a seed PDF open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-002 | Find bar plus search results state | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-003 | Page thumbnails pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-004 | Bookmarks pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-005 | Attachments pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-006 | Layers pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-007 | Signatures pane open | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-008 | Home recents list and thumbnail layouts | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-009 | Preferences dialog live M2 subset | pending | pending | pending | pending | pending | pending | pending |
| B7-REF-010 | Read Mode and Full Screen entry states | pending | pending | pending | pending | pending | pending | pending |

## Acceptance Checklist

- [ ] `parity/reference/` contains the private Acrobat reference screenshots.
- [ ] `parity/onionskin/` contains matching Onionskin captures from a committed
      build.
- [ ] `parity/comparison/` contains the local comparison output for each pair.
- [ ] Every row above names an Acrobat version, Onionskin commit, local
      filenames, SHA-256 values, and pass/fail result.
- [ ] The `parity_privacy` test passes before any commit that updates this
      ledger.
