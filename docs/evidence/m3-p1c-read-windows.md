# M3 P1c: parser read-window checkpoint

## Scope

This checkpoint addresses repeated prefix reads in
`crates/cos/src/reader.rs` and `crates/cos/src/xref.rs`. It does not complete
P1c, change the 25% total-byte budget, or close an Acrobat feature row.
The corpus CI fetch/rerun gates and shared test helper remain unimplemented.

## Baseline

Verified on main `65d29c6`, using `ONIONSKIN_CORPUS=/tmp/claude/ci_corpus`.
That root contains the default hayro, pdf-association and veraPDF sets,
tracked seeds and generated malformed files, without optional hayro-corpus
or generated bench data. Cargo used `--locked` and debug-0 profiles.

Command:

```text
cargo test --locked -p onionskin-cos --test lazy opening_a_large_document_reads_far_less_than_the_whole_file -- --exact --nocapture
```

Observed subprocess exit: 101, with 0 passed, 1 failed and 2 filtered tests.
The failing file is
`external/verapdf/Isartor test files/PDFA-1b/6.1 File structure/6.1.12 Implementation Limits/isartor-6-1-12-t01-fail-a.pdf`.
Its 10,000-page document opens cleanly, but first-page access reads 1,857,768
bytes of its 4,010,934 bytes (46.3%), exceeding the existing 25% budget.

An independent consumer of `Document::open_repairing` and `first_page`
reproduced the byte count through `FileSource` and `CountingSource`:

| Phase | Total bytes returned | Unique file bytes covered |
|---|---:|---:|
| Open | 1,505,256 | 904,092 |
| Through first page | 1,857,768 | 905,116 |

Unique coverage is diagnostic only. `CountingSource::total()` and the
acceptance test continue to count every returned byte, including rereads.
The classic xref rereads growing prefixes totaling 1,148,648 bytes for an
800,488-byte tail. The 97,282-byte flat Pages dictionary is parsed twice,
each time returning 349,184 bytes through growing windows.

Raw records, retained outside Git:

- `/tmp/claude/onionskin-p1c-lazy-baseline-status-65d29c6-20260910T2355/`
- `/tmp/claude/onionskin-p1c-range-trace.gtPkuZ/`

The corrected baseline records an actual subprocess status. An earlier
pipeline-masked run is retained but is not used as exit-code proof.

## Verified change

Both parsing loops retain their buffer and read only the missing suffix on
growth. Initial window sizes, quadrupling, target-window EOF handling and
parser error mapping are unchanged. Short-read offsets use the actual buffer
length; no inner refill loop or new cache is introduced. Retaining the prefix
alongside an owned suffix may increase transient allocation; no peak-memory
improvement is claimed.

Six focused regressions failed on the original loops (exit 101), then passed
afterward (exit 0). They cover parsed stream bytes/spans and xref entries,
nonzero offsets, several growth steps, truncated inputs, partial/empty reads
and source errors. The complete parser unit suite passes 27 tests.
Raw logs: `/tmp/claude/onionskin-p1c-focused.g16NiT/`.
The red-source hash was recorded after unchanged test modules moved to EOF,
so it is not an exact hash of the red executable's source layout. The raw red
log retains its original line locations; production loops were still unchanged.

The unchanged diagnostic consumer, linked to a freshly compiled patched
library, measured:

| Phase | Original total | Patched total | Unique bytes, unchanged |
|---|---:|---:|---:|
| Open | 1,505,256 | 1,070,056 | 904,092 |
| Through first page | 1,857,768 | 1,335,528 | 905,116 |

The document still opens cleanly and resolves its first page. This saves
522,240 returned bytes (28.1%) without changing unique coverage or the 29
read calls. The original budget still fails: 33.3% of the file exceeds 25%.
Raw trace and library/source hash checks:
`/tmp/claude/onionskin-p1c-after-trace.3LfP79/`.

## Validated dictionary reuse checkpoint, 2026-09-11

The committed Task 1 source revision `e7778898724fe732b6fb79cff63d4ccbf59c21a7`
(staged diff SHA-256
`71df8959f500418a067853a0fbfebcb2d90be77bd84e98bf03f2fa03e96e025f`)
retains exact in-file dictionary results parsed during clean structural
validation. A fresh range trace of the same Isartor input verified clean
provenance and successful first-page access; its raw log is retained as
`/tmp/claude/onionskin-p1c-reuse-evidence-20260911.dmMXEo/trace.log`.

| Phase | Read calls | Total bytes returned | Unique file bytes covered |
|---|---:|---:|---:|
| Open | 17 | 1,070,056 | 904,092 |
| Through first page | 21 | 1,072,232 | 905,116 |

The fresh corpus run reports 10,000 pages. The timed `cos-lazy` command took
0.301745 s; the trace itself reports only read ranges, provenance and phase
totals. The input SHA-256 is
`a0d9fa2404be34c8ef098f6b6320523dbacf1d5d964c3c29e30a8f391210ded9`.

Compared with the preceding suffix-read checkpoint, through-first-page reads
fall from 1,335,528 to 1,072,232 bytes: 263,296 bytes, or 19.7%, fewer. The
combined reduction from the original main baseline is 785,536 bytes, or
42.3%. Unique coverage remains unchanged. The unchanged lazy-budget assertion
checks through-first-page reads: 1,072,232 bytes, or 26.73% of the
4,010,934-byte file, so it still exceeds 25%. Open alone is unchanged at
1,070,056 bytes, or 26.68%, and already exceeds that budget as well.

The fresh 17-command runtime
(`/tmp/claude/onionskin-p1c-reuse-fresh-diagnostic-20260911.AEDQwd/run.tvxy7aes/`)
completed 16 commands with exit 0. The unchanged lazy-budget assertion
remained the one failure (5 passed, 1 failed), so this
is a verified local improvement, not P1c completion. Corpus CI gates,
required-input enforcement, the shared helper, hosted timing and mutation
evidence, P1c integration and the remaining 25% performance work remain open.
The named M2 carry-forwards also remain open: VoiceOver, Linux/Windows
accessibility, private B7 states 003-010, hosted release/required-checks/
platform-package work, and three-pass acceptance.

## Broader checks: preceding suffix-read checkpoint

The preceding 17-command local matrix completed without timeouts or source
drift: 16 exits were zero, and only the unchanged lazy-budget assertion failed.
Every case used the explicit corpus root above; corpus-required mode was
scoped to the selected corpus consumers. Results are test counts, not unique
fixture counts.

| Check | Result |
|---|---|
| cos unit | 27 passed |
| cos roundtrip | 6 passed, optional hayro-corpus test filtered |
| cos incremental / delete / repair / robustness | 9 / 6 / 8 / 4 passed |
| cos boundaries / pages | 7 / 5 passed; two bench tests filtered |
| cos lazy | 2 passed, 1 budget failure |
| core panes / session | 6 / 7 passed |
| app guarantees / privacy | 39 passed, 4 ignored / 1 passed |
| app no-default-features | 113 passed, 7 ignored |
| cos all-target clippy, shell build, actual headless binary | Exit 0 each |

Measured corpus work includes all 15 malformed repairs and 22 damaged
external repairs, a 671-file boundary sample, and 63 multi-page files in the
indexed-page comparison (8 nested page trees). Round-trip tallies report
zero failures; encrypted and repair-required files are explicitly counted as
skipped by that suite, not claimed as successful clean opens. The
pdf20examples subset overlaps the full pdf-association set.

Exact commands, environment, per-suite elapsed times, raw tallies and source
guards are retained in
`/tmp/claude/onionskin-p1c-diagnostic-runner-20260911/run.o2u8n3_p/`.
The application binary reported:
`onionskin: 12 plugins, 6 tools, 2 commands, 3 codecs`.
Formatting and whitespace checks also pass. These are macOS local results,
not hosted Linux CI timing or native-window acceptance.

Verified SHA-256 fingerprints:

- reader.rs: `273cf9ca8e8be7dd7db18004ad1aaf44185dc2d8007bbdc45de2319155c6b3ff`
- xref.rs: `b9f7287438c57978a4b1378525ad359a2086442cc4847881509cd5084d8f2c9a`
- Cargo.lock, unchanged: `f6eb1ef2babfa2a318aa85df591ad58b429c1457cf749ea75f347aa2a6c5a67c`

## Still pending

Window overfetch remains. Validated dictionary reuse now covers the repeated
page-tree dictionary parse, while validation-parsed streams remain excluded so
ordinary access can resolve indirect Length values afresh. Compressed objects
also remain deferred until ordinary access. No further source change is part
of this checkpoint.

P1c's corpus CI gates, required-input enforcement, shared helper and hosted
timing/mutation proof remain outstanding. No feature rows are promoted.
The native representative capture passed at `521ca17` on 2026-09-11: the app
opened the same Isartor PDF and exposed page 1 of 10,000. Window-only capture,
binary hash and private AX records are in `milestone-screenshots.md`, evidence
`M3-P1C-A-001`. This supersedes the locked-session capture blocker from
2026-09-10T22:00Z, not the remaining 25-percent budget or integration gate.

A follow-up capture tied the same document to committed validation-reuse
revision `e7778898724fe732b6fb79cff63d4ccbf59c21a7`. The freshly relinked shell
booted headlessly with exit 0 and SHA-256
`741d61af47e830afca02c796cb6a9994de8a55668cbe48151371e5bb75a15078`.
Onionskin PID 98936 owned window ID 22648 with bounds
`{X = 350, Y = 155, Width = 1100, Height = 893}`; the earlier PID 25322 was
left untouched. Capture permission was true, the login session was active and
the lock-state key was unavailable. Exact-window capture succeeded without a
desktop or rectangle fallback.

AX inspection selected PID 98936 and exposed the Isartor document, `Page 1 of
10000`, page entry 1 and 10,000 total pages. The image mtime was
`2026-09-11T15:59:20Z` UTC, size 191,653 bytes, and SHA-256
`8590e24501f56cc89ff91c4feae191376805bb5fc303557d6c6f6262c495bb99`.
Private AX/window records and native logs are retained at
`/tmp/claude/onionskin-reuse-native-e777889.r0ANsy/`. The image was inspected
as an Onionskin-only window showing the blank first page and page 1 of 10,000.
Its hash matches the earlier same-layout representative image, but this is a
new process/window capture and makes no Acrobat pixel-equivalence claim. It
does not claim broader page-rendering, VoiceOver or milestone acceptance.

| Evidence ID | Captured UTC | Private filename | SHA256 |
|---|---|---|---|
| M3-P1C-B-001 | 2026-09-11T15:59:20Z | `p1c-reuse-e777889-20260911T155913Z.png` | `8590e24501f56cc89ff91c4feae191376805bb5fc303557d6c6f6262c495bb99` |
