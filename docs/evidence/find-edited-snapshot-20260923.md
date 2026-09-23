# Edited Find snapshot evidence

Recorded 2026-09-23 in the isolated treatment worktree
`codex/search-edited-snapshot`. Native Find UI and screenshots remain deferred
while the Mac is locked.

## Source-provenance matrix

The preserved baseline worktree is
`/Users/cristi/Dropbox_Maestral/devel/onionskin-search-edited-snapshot-baseline`,
branch `codex/search-edited-snapshot-baseline`, with production sources still
at pre-fix `cd02bfe`. Its integration test file was synchronized byte-for-byte
with the treatment integration test file. Only that baseline test file was
changed; no baseline production source was edited.

Both runs used the existing isolated target directories, the shared `lockf`
build lock, low-debug settings, and this command shape:

```text
cargo test -vv --manifest-path <worktree>/Cargo.toml \
  --config 'profile.test.package.onionskin-core.opt-level=1' \
  --locked -p onionskin-core --lib --test search edited_search -- --nocapture
```

The fresh logs include rustc commands with the absolute source and artifact
paths:

- Baseline: `baseline-final-synchronized-fresh-library.log`, exit 101. The
  pre-fix library compiled from the baseline source path. Seven integration
  regressions ran and all seven failed for stale edited-snapshot behavior:
  delete/move/undo/redo, saved bytes, one-shot invalidation, changed text,
  changed comments, preparation failure, and overcounted-comment preparation.
- Treatment: `treatment-final-synchronized-fresh-library.log`, exit 0. The
  treatment library compiled from the treatment source path; all seven
  integration regressions passed. The private snapshot-open unit test also
  passed in `treatment-snapshot-open-final.log`.
- Treatment canvas filter: three edited-search tests passed in
  `treatment-final-canvas.log`.

The earlier `baseline-final-edited-search.log` is retained but superseded: it
used a contaminated cached library artifact and must not be used as proof.
No baseline failure claim is made from that log, and no baseline production
source was changed to obtain the fresh result.

## Scope and controls

The treatment source uses a document source stamp to refresh searches over the
current preview, reuses the worker for unchanged source snapshots, clears stale
results once, and keeps preparation failures visible without per-frame retry.
The tests cover page edits, saves, comments, changed text, cancellation,
preparation errors, and the per-canvas refresh path. The evidence does not claim
pre-fix failures for any native UI behavior, canvas behavior, open-failure unit
case, or overcounted-comment case beyond the fresh baseline run described above.

Strict Clippy, formatting, and diff checks were run separately under the same
lock. No commit, push, deletion, cache cleanup, or native UI acceptance is
claimed.
