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

## P1c representative capture, 2026-09-11

At committed parser revision `521ca17`, the native app opened
`isartor-6-1-12-t01-fail-a.pdf` and exposed `Page 1 of 10000` in its accessibility
tree. Fit Page showed 85 percent zoom and a white first page. This records the
native consumer, not a pixel comparison or a passing I/O budget.

| Evidence ID | Captured UTC | Private filename | SHA256 |
|---|---|---|---|
| M3-P1C-A-001 | 2026-09-11T13:26:17Z | `p1c-isartor-521ca17-20260911T132700Z.png` | `8590e24501f56cc89ff91c4feae191376805bb5fc303557d6c6f6262c495bb99` |

The time is the file modification time in UTC; the filename time is approximate.
Capture used `screencapture -x -o -l 21275` for Onionskin PID 25322 and was visually
inspected as window-only. Capture permission was granted and the locked flag was
absent. The binary was explicitly relinked from the P1c worktree with
`cargo rustc --locked -p onionskin-app --features shell --bin onionskin --
-C metadata=p1c_native_20260911`; its SHA256 was
`b80b344c17c10774c135ec9796c537f6a14d14d999d018e52dd645c669372a62`.
Private AX/window records are at `/tmp/claude/onionskin-p1c-native-20260911.1RdLPW/`.
The matching executable is retained there as `onionskin-shell-b80b344c`;
the shared target's top-level binary was subsequently replaced by a test build.
The representative-capture item is closed; P1c and integration remain incomplete.

A follow-up capture tied the same document to committed validation-reuse revision
`e7778898724fe732b6fb79cff63d4ccbf59c21a7`. The fresh shell booted headlessly
with exit 0 and SHA-256
`741d61af47e830afca02c796cb6a9994de8a55668cbe48151371e5bb75a15078`.
Onionskin PID 98936 owned window ID 22648 with bounds
`{X = 350, Y = 155, Width = 1100, Height = 893}`; the earlier PID 25322 was
left untouched. Capture permission was true, the login session was active and
the lock-state key was unavailable. Exact-window capture succeeded without a
desktop or rectangle fallback. AX inspection selected PID 98936 and exposed
the Isartor document, `Page 1 of 10000`, page entry 1 and 10,000 total pages.
The image mtime was `2026-09-11T15:59:20Z` UTC, size 191,653 bytes, and its
SHA-256 is shown below. The image was inspected as an Onionskin-only window;
its same-layout hash does not claim Acrobat pixel equivalence, broader
page-rendering, VoiceOver or milestone acceptance.

| Evidence ID | Captured UTC | Private filename | SHA256 |
|---|---|---|---|
| M3-P1C-B-001 | 2026-09-11T15:59:20Z | `p1c-reuse-e777889-20260911T155913Z.png` | `8590e24501f56cc89ff91c4feae191376805bb5fc303557d6c6f6262c495bb99` |

## P0c native verification, 2026-09-11

The tested source revision `d34b29f` now passes native open, Find,
tab switching, and export modal/input smoke. The binary was explicitly relinked
from the P0c worktree with `cargo rustc --locked -p onionskin-app --features shell
--bin onionskin -- -C metadata=p0c_native_20260911`. Its SHA256 is
`c793492942cc0aa6156fed258a6fd180875ecc5acfc8344595cbb2719d454b74`.
PID 18211 owned the captured window 21242. Capture permission was granted, the
locked-session flag was absent, and the window was on screen. Each capture uses
`screencapture -x -o -l 21242`, was visually inspected, and contains only Onionskin.
Times below are file modification times in UTC; filename times are approximate.

| Evidence ID | Verified state | Captured UTC | Private filename | SHA256 |
|---|---|---|---|---|
| M3-P0C-A-001 | Native Cmd+F, query Hello, one match visibly highlighted | 2026-09-11T13:16:24Z | `p0c-task-a-find-d34b29f-20260911T1320Z.png` | `e4963ba0b96674aa8159f973c4fb1b4209bde01c6c213c2c726d13dbfb0a7e85` |
| M3-P0C-A-002 | Page 0 rejected with a visible error, Export focused | 2026-09-11T13:17:53Z | `p0c-export-invalid-d34b29f-20260911T1325Z.png` | `94c1f3a36eed576d14aeb1c3cde75fcc0ecf64b6269907741255c05d702dff4d` |
| M3-P0C-B-001 | Representative Task B capture: page 2 only, 144 DPI, Export focused | 2026-09-11T13:18:07Z | `p0c-task-b-export-d34b29f-20260911T1327Z.png` | `719668fab99a3e1fd14e0b98c3d8a2fff7fc27eec9eb35b0671bca4982800c7c` |

AX dumps record Find `Hello` with `1 of 1`, switching from `hello.pdf` to
`two-page.pdf` with two page nodes, export defaults `1/2/150`, rejection of
First page `0`, then corrected `2/2/144` with Export focused. Cmd+A replaces
First/DPI values; native Edit > Select All replaces First after correction.
The resulting PNG is 160 by 360 pixels and visibly contains only `Page two`.
Its SHA256 is `db576c0bd09ccb5f7e0d74645d884a9736b987a27043f3c01f06fa7b5947c088`.

The save-panel automation entered the intended scratch-directory path as a
filename instead of navigating there. The newly generated file was identified
in `corpus/seeds`, verified, then moved into the private evidence directory;
no pre-existing file was moved or overwritten. Raw AX dumps, output PNG and
command/selector limitations are retained at
`/tmp/claude/onionskin-p0c-native-20260911.DrJCIZ/`.
These checks close P0c's pending native smoke and Task A/B capture items, not
its final integration gate, real VoiceOver acceptance, or any Acrobat feature row.

## Historical capture blockers

- M3 P0c source checkpoint, 2026-09-10: test relocation is committed at
  `fdf875c` and automated verification passed. Fresh CoreGraphics probes
  report capture permission granted, `CGSSessionScreenIsLocked=1`, and no
  Onionskin windows. Native open/Find/tab/export smoke and the Task A
  window-only capture remain pending. The subsequent evidence/status task
  also needs its representative capture. No screenshot or native acceptance
  was claimed at that checkpoint, and no full-screen fallback was taken.
  Resolved for P0c by the 2026-09-11 verification above.
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
- M3 P1c read-window checkpoint, 2026-09-10T22:00Z: representative Onionskin
  window capture pending. Parser/corpus checks, application build and headless
  startup are recorded in `m3-p1c-read-windows.md`; they are not native UI proof.
  Fresh preflight reports `capturePermission=true` and
  `CGSSessionScreenIsLocked=1`, with no Onionskin window. No full-screen fallback
  was taken, and no screenshot was claimed then. The representative capture was
  completed at `521ca17` on 2026-09-11 as recorded above.
