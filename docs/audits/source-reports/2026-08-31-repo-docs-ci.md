# Source report: repository, documentation, CI, packaging, and retained state audit

Provenance:

- Report ID: `2026-08-31-repo-docs-ci`
- Scope: read-only independent repository/process audit for Onionskin.
- Repository: `<repo-root>`
- Baseline: `main` at `7413186` on 2026-08-31.
- Constraints observed: no edits, staging, commits, deletes, moves, or worktree cleanup; no browsing; live repository evidence only.

## Verification limitations

- No Git remote was configured, so remote CI, branch protection, PR status, and hosted release checks could not be verified.
- Platform release validation could not be asserted from this read-only audit.
- Real VoiceOver acceptance could not be asserted from repository evidence.
- Acrobat reference screenshots were not present in the repository, and private local screenshots were not inspected.
- Findings below are based on live files, Git history, and retained worktree state visible at the baseline.
- All `file:line` references refer to baseline `7413186` and may shift in later
  audit-ledger edits.

## Authoritative milestone completion map at baseline

| Milestone/package | Baseline status | Evidence |
|---|---|---|
| M0 scaffold | Mostly landed, incomplete evidence | `PLAN.md` requires first `parity/reference/` screenshots, but no `parity/` directory was present. |
| M1 spikes | Landed as spikes/docs, incomplete acceptance | AccessKit spike evidence exists, but no real VoiceOver run was verifiable. |
| P1 foundation audit | Landed on main | `crates/cos/src/document.rs`, `crates/cos/src/source.rs`; P1 represented on main before later M2 merges. |
| P2 session | Landed on main | First-parent M2 foundation evidence. |
| P3 geometry | Landed on main | First-parent M2 foundation evidence. |
| P4 render package | Landed on main | First-parent merge `aa4d9b8`. |
| P5 render worker | Landed on main | First-parent M2 foundation evidence. |
| P6a layout | Landed on main | First-parent M2 foundation evidence. |
| P6b shell | Landed on main | First-parent M2 foundation evidence. |
| P7 shell chrome | Landed on main, retained plan stale | First-parent M2 foundation evidence; external plan still appeared active. |
| P8 navigation panes | Landed on main | First-parent merge `452f575`. |
| P9 search | Landed on main, retained plan stale | First-parent merge `72d0a21`; external plan still appeared planned. |
| P10 tools basic | Landed on main via alternate branch, stranded branch/worktree remain | First-parent merge `00a7d29`; retained `feat/m2-tools-basic` and `../onionskin-m2-p10-tools-basic` contain unmerged or uncommitted state. |
| P11 commands/preferences/recents | Landed on main | First-parent merge `c3c6576`. |
| P12 accessibility | Not landed | No merge commit; active worktree had a large uncommitted P12 diff and marked `crates/app/src/bin/a11y_spike.rs` deleted. |
| P13 codecs/export | Landed on main, retained plan stale | First-parent merge `74b60ec`; external plan still appeared planned. |
| P14 performance budgets | Landed on main, retained plan stale | First-parent merge `64829a0`; external plan still appeared draft. |
| M2 overall | Not complete | P12, VoiceOver acceptance, parity scoreboard reconciliation, and screenshot evidence remained open. |

## Findings

### REPO-001: release workflow builds the wrong artifact

- Severity: Critical
- Artifact: `.github/workflows/release.yml:49-50`, `crates/app/Cargo.toml:64-100`, `crates/app/src/main.rs:16-27`
- Finding: The release workflow builds packaged artifacts without the `shell` feature, so released binaries can take the headless/non-viewer path instead of the production UI.
- Evidence: `onionskin-app` gates the GPUI shell behind the `shell` feature. The release workflow build lines did not pass `--features shell`.
- Ledger status: Must be tracked as an open release blocker.
- Recommended action: Update release packaging to build `onionskin-app --features shell`, install the same platform prerequisites as CI, and smoke-test that the produced artifact launches the viewer.

### REPO-002: P12 accessibility is not merged, so M2 is incomplete

- Severity: Critical
- Artifact: `docs/plans/m2-viewer.md:713-745`, `known-issues.md:135-137`, Git history
- Finding: The roadmap requires an M2 accessibility tree and one real VoiceOver session, but P12 was not merged.
- Evidence: No P12 merge commit was present on main; the active P12 worktree contained uncommitted changes.
- Ledger status: Open M2 blocker.
- Recommended action: Complete P12 in an isolated package, preserve the existing spike, verify AccessKit probes and real VoiceOver acceptance, and keep the item visibly pending until the user-gated session passes.

### REPO-003: Acrobat parity scoreboard is stale

- Severity: High
- Artifact: `ACROBAT-PARITY.md:29-31`, `ACROBAT-PARITY.md:60`, `known-issues.md:108`
- Finding: The parity matrix still reported zero implemented rows despite P1-P11, P13, and P14 being represented on main.
- Evidence: The headline summary said zero implemented while merged M2 rows had live code paths.
- Ledger status: Open documentation/acceptance blocker.
- Recommended action: Reconcile every row against live code, real callers, UI behavior, tests, caveats, and evidence; mechanically recalculate totals.

### REPO-004: M2 viewer plan status is stale

- Severity: High
- Artifact: `docs/plans/m2-viewer.md:3`
- Finding: The M2 plan status still described P9, P10, and P13 as next and P8, P11, P12, and P14 as later, although all except P12 had landed.
- Evidence: Git history showed P8, P9, P10, P11, P13, and P14 merge commits on main.
- Ledger status: Open documentation status defect.
- Recommended action: Update the M2 plan header and package status table to match live Git history without claiming P12 or VoiceOver acceptance.

### REPO-005: retained branches and worktrees contain stranded unmerged work

- Severity: High
- Artifact: `feat/m2-tools-basic`, `../onionskin-m2-p10-tools-basic`, `worktree-agent-a9ddd622c0af1f8c0`, `worktree-agent-ae42da07ac02bfaa4`
- Finding: Retained branches/worktrees remain after main has advanced, including a P10 branch with commits and uncommitted diff that do not exactly match main.
- Evidence: `feat/m2-tools-basic` had seven commits and a large diff versus main; retained agent worktrees were still present.
- Ledger status: Retained evidence, not safe cleanup.
- Recommended action: Inventory and preserve the state until the user gives explicit per-item cleanup approval; reconcile any useful unmerged work additively in new reviewed packages.

### REPO-006: external project metadata is stale

- Severity: Medium
- Artifact: `~/.claude/projects.md`
- Finding: The Onionskin project row still described the project as planning/no code despite the Rust viewer work on main.
- Evidence: Main contained landed Rust crates, M2 viewer packages, CI, and packaging files.
- Ledger status: Open external metadata defect.
- Recommended action: Update the project entry to describe the live Rust/PDF viewer state and current M2 completion caveats.

### REPO-007: baseline documentation and development entry points are missing

- Severity: Medium
- Artifact: repository root, `.project-docs/`
- Finding: The repository lacked root `README.md`, `.project-docs/`, `Makefile`, and `.editorconfig`.
- Evidence: No root README, project docs index, Makefile, or editorconfig was present at baseline.
- Ledger status: Partially actionable. README, `.project-docs/INDEX.md`, and `.editorconfig` are appropriate; a Makefile should not be added only for convention.
- Recommended action: Add minimal verified entry points for scope, build/test/run commands, architecture pointers, known issues, and retained-state notes; do not invent a task runner.

### REPO-008: CI lacks dependency and security gates

- Severity: High
- Artifact: `.github/workflows/ci.yml:21-30`, `.github/workflows/ci.yml:65`, `.github/workflows/ci.yml:88-103`
- Finding: CI lacked advisory/license/secret/dependency policy gates such as cargo audit, cargo deny, gitleaks, Dependabot, or equivalent reviewed policies.
- Evidence: Existing jobs covered build/test/bench style gates, but no checked-in security/dependency policy was present.
- Ledger status: Open CI/security posture issue.
- Recommended action: Add a dependency/security hardening package with advisory, license, secret scanning, Dependabot or equivalent dependency tracking, and explicit reviewed exceptions.

### REPO-009: R2 corpus artifacts lack cryptographic checksum enforcement

- Severity: High
- Artifact: `corpus/fetch.sh:212-215`, `corpus/README.md:243-247`, `known-issues.md:91-95`
- Finding: Merge-gating corpus or benchmark downloads were not protected by cryptographic checksums.
- Evidence: The fetch script and corpus docs did not enforce object hashes for all required remote corpus artifacts.
- Ledger status: Open reproducibility/security issue.
- Recommended action: Add SHA-256 manifests for required corpus objects and fail loud on mismatch.

### REPO-010: parity screenshot acceptance evidence is absent

- Severity: High
- Artifact: `PLAN.md:100-102`, `PLAN.md:497`, `known-issues.md:142-143`, repository `parity/` state
- Finding: The roadmap requires parity screenshot comparison, but no `parity/` directory or reference screenshot evidence was present.
- Evidence: `PLAN.md` requires reference UI screenshots and first parity comparison; repository listing found no parity evidence.
- Ledger status: Open acceptance blocker.
- Recommended action: Capture private Acrobat reference screenshots, compare corresponding Onionskin states, keep private images out of public version control, and record evidence IDs and Onionskin screenshot manifests.

### REPO-011: repository-level guarantee tests are mostly ignored stubs

- Severity: Medium
- Artifact: `crates/app/tests/guarantees.rs:6-65`, CI configuration
- Finding: Several guarantee tests remained ignored/unimplemented, and there was no coverage threshold or equivalent proof that roadmap guarantees are enforced as they land.
- Evidence: Guarantee tests 1-4 and 6-8 were ignored stubs at baseline; only guarantee 9 had active assertions in the viewed file.
- Ledger status: Open test coverage/tracking issue.
- Recommended action: Replace stale ignored M1/M2 stubs with real crate-level guarantees or direct references to their active tests; keep future milestone guarantees as explicit pending contracts.

### REPO-012: packaging documentation is stale

- Severity: Medium
- Artifact: `packaging/README.md:34`
- Finding: Packaging docs still said Linux dependencies were waiting for M1, while later CI/release work existed and the release workflow still lacked correct shell packaging.
- Evidence: Packaging README text did not match the current viewer feature graph or release workflow needs.
- Ledger status: Open packaging documentation defect.
- Recommended action: Update packaging docs to match actual macOS/Linux/Windows prerequisites, release feature flags, and validation limitations.
