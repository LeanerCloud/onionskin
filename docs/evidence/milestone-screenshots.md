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
| M2-B1-T3-001 | B1.3 composite accounting across cache incarnations | Shell scrolled `two-page.pdf` until both the unrotated and rotated page surfaces were visible after the bench-only accounting change at `be39bae`; production sources and the shell binary are unchanged from B1.1, and neither page contains the prior black-line artifact; inspected as window-only | `be39bae` | 2026-09-01T10:16:47Z | `m2-b1-task3-composite-accounting-20260901T101647Z.png` | `509f7220ade0da604cce97aefe73069875ab61671da2b01a097f964ffeab20d1` |
| M2-B1-T4-SUPERSEDED-001 | B1.4 seed launch check, superseded by corpus evidence | Committed shell opened `hello.pdf` after the R2 corpus helper hardening at `be3789e`; window-only capture preserved, but the official B1.4 task evidence is the verified corpus PDF row below | `be3789e` | 2026-09-01T16:33:24Z | `m2-b1-task4-r2-hardening-window8577-20260901T163324Z.png` | `2b2f4115d7639e05bc9f9037190ec5edb5b6dea077b406672520c13dd41ff310` |
| M2-B1-T4-001 | B1.4 R2 checksum and safe publication hardening | Shell opened verified retained `hayro-corpus` file `0899694.pdf` after the checksum-enforced R2 corpus helper landed at `be3789e`; the 16-page corpus PDF renders without black-line artifacts; inspected as window-only | `be3789e` | 2026-09-01T16:35:29Z | `m2-b1-task4-r2-checksums-corpus-20260901T163529Z.png` | `57802b144ba27656ddc5a06ec56ad607c3c73cf885eb4f21feae02e81a615398` |
| M2-B2-T1-001 | B2.1 native action dispatch re-verification | Shell opened verified retained `hayro-corpus` file `0899694.pdf`; the real macOS Command-F shortcut reached the deferred `RunCommand` route and opened the Find bar in the app window; inspected as window-only | `483e512` | 2026-09-01T16:50:45Z | `m2-b2-task1-dispatch-reverify-20260901T165045Z.png` | `131ce2959fe2c5348501d2140be15e3236bcad7cb935e1baabd05eba16c26ee0` |
| M2-B2-T1-ALT-001 | B2.1 native action dispatch re-verification alternate | B1-integrated shell opened `hello.pdf` and `two-page.pdf`; a live Command-F route opened the document Find bar in the window, matching the focused GPUI keystroke tests for Find, Close Tab, and Actual Size; inspected as window-only | `483e512` | 2026-09-01T16:49:00Z | `m2-b2-task1-dispatch-reverification-20260901T184900Z.png` | `3536dd758e003f47d8d023c815adcb40b9f7f211d0e704a607c8806abd3e8711` |
| M2-B3-T7-001 | B3.7 chrome hardening | Home/Recents shell after B3.7; the right panel shows the Tool details empty state with fitted text after the no-document search, numeric page-field, and invalid-zoom regressions passed; inspected as window-only | `c85dea2` | 2026-09-01T19:41Z | `m2-b3-task7-chrome-hardening-rebased-20260901T1941Z.png` | `ba3f688847468a3df8068fd5583b384f2adcda494eba066e994d28a5dcff6861` |
| M2-B4-T2-4-001 | B4.2-B4.4 stale prompt completion and export hardening | Rebased shell opened `two-page.pdf` after stale export and attachment prompt completions were bound to the originating canvas and derived export destinations were hardened; both page surfaces render without the prior black-line artifact; inspected as window-only | `34e4632` | 2026-09-01T23:22:05Z | `m2-b4-stale-prompt-completions-20260901T232205Z.png` | `64ddead406d9cc41eb597c9421e7ae0e1e42f202183924c89544ee0e98849aba` |
| M2-B5-001 | B5 required macOS accessibility probe | Shell built from the B5 CI-only revision and opened `two-page.pdf`; the production viewer remains unchanged from B6, both page surfaces render without the prior black-line artifact, and the exact B5 revision makes the 14-case macOS accessibility probe a required CI gate; inspected as window-only | `c948e42` | 2026-09-01T23:27:10Z | `m2-b5-required-a11y-probe-20260901T232710Z.png` | `ae6a17038341fbcaaee381d21392d52ba8c4e273c6f5cc5d3ebb36cfaf035bec` |
| M2-B7-SCAFFOLD-001 | B7 private parity evidence scaffold | Shell opened `two-page.pdf` after the public privacy protocol and executable ignore/tracked-state regression landed; the production viewer is unchanged from B4 and both page surfaces render without the prior black-line artifact; inspected as window-only | `78bd120` | 2026-09-01T23:39:16Z | `m2-b7-parity-privacy-scaffold-20260901T233916Z.png` | `c277567ac4ddd4b90759d46aa82ae5af0c41cdbec75e1b149f3aaa9396c5da6c` |

## C1.2 native progress, 2026-09-10

The first three captures contain only the Onionskin window, ID 20322, from exact build
`4ddec23` (PID 59412). Times below use file modification times in UTC because
the filename times are approximate. They prove the new dialog's native menu
route, visible validation, and keyboard-focused Export button. Native testing
also exported only page 2 of `two-page.pdf`: PNG at 144 DPI was 160 by 360 pixels,
and the single text output contained only `Page two`.

The final build `62576fb` (PID 82243, window 20433) also passed native Cmd+A
and Edit > Select All replacement, raw AXValue reads (`1`, `2`, `150`, then
`2`, `2`, `144`), visible validation, and keyboard focus on Export. The existing
Page Number field reported AXValue `1`. Its page-2 PNG at 144 DPI was again
160 by 360 pixels, SHA-256
`db576c0bd09ccb5f7e0d74645d884a9736b987a27043f3c01f06fa7b5947c088`.
The final shell/support suite passed 624 tests with 7 ignored. This closes the
C1.2 acceptance gate, not the remaining whole-app accessibility work.

| Evidence ID | Scenario | Time (UTC) | Local image | SHA-256 |
|---|---|---|---|---|
| C1.2-NATIVE-001 | Default PNG settings opened from the native menu | 2026-09-10T14:41:45Z | `c12-export-settings-4ddec23-20260910T1442Z.png` | `31ca80f3370c81117e05b6bd3a3882e2f92987f0283244828c21cbb7a4a5e9df` |
| C1.2-NATIVE-002 | Invalid First page produces a visible error without a destination prompt | 2026-09-10T14:42:07Z | `c12-export-validation-4ddec23-20260910T1443Z.png` | `45c92707a0e163f7e7bc76cb2028e3e7d354d0297d5a0563c60a54c00eb4041e` |
| C1.2-NATIVE-003 | Page 2 only, 144 DPI, keyboard-focused Export button | 2026-09-10T14:43:44Z | `c12-export-page2-144dpi-focus-4ddec23-20260910T1445Z.png` | `b616bfcb1fbaa5fe0a65e80f591c4566bfebc69cc098d3c8ebace79175ebc041` |
| C1.2-NATIVE-004 | Final build after native field replacement, page 2 at 144 DPI, Export focused | 2026-09-10T15:05:05Z | `c12-native-input-verified-62576fb-20260910T1505Z.png` | `a9660eb25fe281332910be7b0ce5492e9407f085cb7b4f5320f62675acaec842` |
| C1.2-NATIVE-005 | Final build, invalid First page visibly rejected | 2026-09-10T15:06:37Z | `c12-native-alert-62576fb-20260910-final.png` | `45382a38b00269ac807f9c59a951d686f50c8b5e22378f226390d9921671acae` |

## Historical capture blockers

- C1.1 Task 1 shared-byte export snapshot, 2026-09-02: the required exact-build
  `f0cbbcb` Onionskin-window-only capture remains pending. It was deferred while
  B7 owned the overlapping evidence files, and the GUI session is now locked
  with the displays asleep/inactive. No Task 1 launch evidence is claimed.
  Retry after the session is awake and unlocked; do not use a full-screen
  fallback.
- C1.1 Task 2 background streaming export, 2026-09-02T03:23:35Z: exact build
  `3ac647b` was launched as PID 12887 on generated ignored
  `corpus/bench/pages-1000.pdf`. CoreGraphics reported the Onionskin-only window
  as ID 12655 with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. The required exact-window
  screenshot is pending because the GUI session is locked and the displays are
  asleep/inactive despite capture permission. Retry the exact window after the
  session is awake and unlocked. No full-screen fallback was taken.
- B7 Page Thumbnails lifecycle fix, 2026-09-02T01:42Z: exact committed build
  `4132a99` opened `two-page.pdf` after the focused GPUI lifecycle regression,
  strict clippy, format check, and shell build passed. CoreGraphics reported the
  Onionskin-only window as ID 12612 with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. `screencapture -l` and
  exact-region capture failed, ScreenCaptureKit returned stream error -3811,
  and the legacy CoreGraphics window-image path produced an all-black image.
  Follow-up diagnostics found capture permission allowed, but the GUI session
  locked and both displays asleep/inactive; WindowServer reported that the
  capture rectangle intersected no display. No full-screen fallback was taken
  and no B7 Task 1 screenshot is recorded. Retry the exact window after the
  session is awake and unlocked.
- B2.2 status/lifecycle hardening, 2026-09-01T19:15Z: focused regressions,
  full shell-support tests, strict clippy, and shell build passed. The fresh
  app window was PID 33128, CoreGraphics window ID 8802. `screencapture -l`
  failed with `could not create image from window`; region capture failed with
  `could not create image from rect`; full-screen fallback captures were black.
  No B2.2 screenshot is recorded as evidence.
- B2.3 canvas context-menu repositioning, 2026-09-01T19:31Z: focused
  second-right-click tests, full shell-support tests, strict clippy, and shell
  build passed. The committed app launched as PID 67358, CoreGraphics window ID
  8828. `screencapture -l` failed with `could not create image from window`. No
  B2.3 screenshot is recorded as evidence.
- B2.4 bounded background snapshot encoding, 2026-09-01T20:00Z: focused
  snapshot regressions, full shell-support tests, strict clippy, format check,
  and shell build passed. The fresh app window was PID 91767, CoreGraphics
  window ID 8838. `screencapture -l` failed with
  `could not create image from window`; `screencapture -R` failed with
  `could not create image from rect`. The native synthetic drag smoke left the
  pasteboard empty. No B2.4 screenshot is recorded as evidence.
- B3.1 navigation pane body bounds, 2026-09-01T18:24Z: focused rendered-bounds
  GPUI regression, pane test filter, matrix-count contract, format check, and
  shell build passed. The fresh app window was PID 12022, CoreGraphics window
  ID 8844, with bounds `{X = 361, Y = 163, Width = 1078, Height = 877}`.
  `screencapture -l` failed with `could not create image from window`;
  exact window-bounds `screencapture -R` failed with
  `could not create image from rect`. No full-screen fallback was taken. No
  B3.1 screenshot is recorded as evidence.
- B3.2 layer order hierarchy, 2026-09-01T18:35Z: focused core layer-order
  regression, full core layer filter, app layer filter, matrix-count contract,
  format check, and shell build passed. The fresh app window was PID 19470,
  CoreGraphics window ID 8851, with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. `screencapture -l` failed
  with `could not create image from window`; exact window-bounds
  `screencapture -R` failed with `could not create image from rect`. No
  full-screen fallback was taken. No B3.2 screenshot is recorded as evidence.
- B3.3 thumbnail stale-size reclassification, 2026-09-01T18:38Z: focused
  regression `a_thumbnail_rendered_at_the_previous_size_is_dropped_when_it_arrives`
  passed, proving APP-004 is stale against current code. The fresh app window
  was PID 23650, CoreGraphics window ID 8856, with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. `screencapture -l` failed
  with `could not create image from window`; exact window-bounds
  `screencapture -R` failed with `could not create image from rect`. No
  full-screen fallback was taken. No B3.3 screenshot is recorded as evidence.
- B3.4 canonical key names, 2026-09-01T18:43Z: focused extended-key regression,
  keymap filter, format check, and shell build passed. The fresh app window was
  PID 25860, CoreGraphics window ID 8861, with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. `screencapture -l` failed
  with `could not create image from window`; exact window-bounds
  `screencapture -R` failed with `could not create image from rect`. No
  full-screen fallback was taken. No B3.4 screenshot is recorded as evidence.
- B3.5 preference carry-forward cap, 2026-09-01T18:47Z: focused unknown-key cap
  regression, preferences filter, format check, and shell build passed. The
  fresh app window was PID 26491, CoreGraphics window ID 8866, with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. `screencapture -l` failed
  with `could not create image from window`; exact window-bounds
  `screencapture -R` failed with `could not create image from rect`. No
  full-screen fallback was taken. No B3.5 screenshot is recorded as evidence.
- B3.6 command-specific disabled reasons, 2026-09-01T18:45Z: focused context
  menu regressions, matrix headline check, format check, and shell build passed.
  The fresh app window was PID 27984, CoreGraphics window ID 8871, with bounds
  `{X = 361, Y = 163, Width = 1078, Height = 877}`. `screencapture -l` failed
  with `could not create image from window`; exact window-bounds
  `screencapture -R` failed with `could not create image from rect`. No
  full-screen fallback was taken. No B3.6 screenshot is recorded as evidence.
- B3.7 chrome follow-up, 2026-09-01T18:50Z:
  `m2-b3-task7-chrome-followup-20260901T1850Z.png` captured only the Onionskin
  window by CoreGraphics window ID 8942. Bounds were
  `{X = 350, Y = 155, Width = 1100, Height = 893}`. SHA-256:
  `8893280ecfbb7de8e1581bfe4e7b651773809228ddb0eed65ec836b8217bda39`.
- B3.8 rail icon rendering, 2026-09-01T19:08Z:
  `m2-b3-task8-rail-icons-20260901T1908Z.png` captured only the Onionskin
  window by CoreGraphics window ID 9009. Bounds were
  `{X = 350, Y = 155, Width = 1100, Height = 893}`. SHA-256:
  `f29036ddc3cadbf2608f535210ca1d2ca516b1d0585a66413e9e1f1415d0ae89`.
