# Milestone screenshot evidence

Actual Onionskin screenshots are stored in a private local screenshot directory
outside the repository. Each image contains only the Onionskin window. Private
Acrobat reference images are never committed; their future evidence IDs may be
recorded here without public paths. The user-facing progress report provides local
absolute paths when needed; this tracked manifest records filenames only.

| Evidence ID | Task | Scenario | Build commit | Evidence time (UTC) | Local image | SHA-256 |
|---|---|---|---|---|---|---|
| M2-P7-ARTIFACT-001 | P7 rendering artifact fix | Rotated second page after atlas-gutter seam fix; inspected as window-only | `fdaf657` | 2026-08-29T23:26Z, inferred from filename; filesystem mtime 23:28:57Z | `m2-p7-render-artifact-fix-two-page-20260830-0126.png` | `0d88848c0f41b42ce7468e4b95384bbcac3b5c399718cc6e370d9319982f407f` |
| M2-AUDIT-T1-001 | Forward audit Task 1 | Committed shell on `two-page.pdf`, continuous two-page canvas and quick actions visible; inspected as window-only | `0f36d86` | 2026-08-31T11:27:30Z | `m2-forward-audit-ledger-task1-20260831T112730Z.png` | `db55b3ebc15fa6ff5ef5895696975ce11ab96e8316a0212754e58c1c427bfc45` |
| M2-AUDIT-T2-001 | Forward audit Task 2 | Committed shell on `two-page.pdf`; Find shows `Page`, result 1 of 2, and real highlights while Page Thumbnails is selected but its shared pane body remains collapsed; inspected as window-only | `20f8326` | 2026-08-31T12:45:29Z | `m2-forward-audit-ledger-task2-20260831T124529Z.png` | `5eca211b41f33fb617a3052d8d022713ddff339466ecedd94e101549bbbf5289` |
| M2-B0-001 | B0 retained source preservation | Committed shell opened `two-page.pdf`, then returned to Home with the resulting Recents entry visible; inspected as window-only | `d683def` | 2026-08-31T16:55:48Z | `m2-b0-preserve-deleted-sources-20260831T165548Z.png` | `5769b4e185500ff52204e2cedef593ca0071ddfd4ae0a4ed3508c13a33595167` |
| M2-AUDIT-T3-001 | Forward audit Task 3 | Home/Recents after the non-production Task 3 commit; the retained `d683def` shell stayed open because production sources are unchanged at `e401b18`; inspected as window-only | `e401b18` | 2026-08-31T17:43:21Z | `m2-forward-audit-ledger-task3-20260831T174321Z.png` | `5769b4e185500ff52204e2cedef593ca0071ddfd4ae0a4ed3508c13a33595167` |
| M2-B1-T1-001 | B1.1 negative page-count correctness | Committed shell opened the valid `two-page.pdf` seed at page 2 of 2 after malformed negative `/Count` rejection landed; both page surfaces render without the prior black-line artifact; inspected as window-only | `876af0a` | 2026-08-31T18:51:00Z | `m2-b1-task1-negative-page-count-20260831T185100Z.png` | `b1de5c363210a9fc73a3c3e15da3e121f8c0a106351a800e32fde93b33166832` |
| M2-B1-T2-001 | B1.2 explicit oracle error accounting | Shell opened `hello.pdf` after the test-only oracle changes at `09ecb9b`; the production sources and shell binary are unchanged from B1.1, and the expected `Hello Onionskin` text renders without black-line artifacts; inspected as window-only | `09ecb9b` | 2026-08-31T19:17:40Z | `m2-b1-task2-oracle-accounting-20260831T191740Z.png` | `3d25da1eca5595ab45bf0f0c8106da3fa1a2adc2afd1b98c0b56472b7d8d6ca1` |
