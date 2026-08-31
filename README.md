# Onionskin

Onionskin is a local-first PDF viewer and editor written as a Rust workspace. Its
core invariant is non-destructive editing: future saves append valid incremental
PDF updates instead of rewriting the original bytes.

The M2 viewer is implemented and under hardening. It can open and repair PDFs,
render and navigate pages, search and select text, run the basic viewer tools,
export text/PNG/SVG, persist local preferences and recents, and publish a macOS
accessibility tree. M2 is not accepted yet. The remaining correctness, VoiceOver,
navigation-pane, release, and platform gates are tracked in the
[forward audit](docs/audits/m2-forward-audit.md) and [known issues](known-issues.md).
Editing milestones M3-M6 remain planned.

## Build and run

The workspace pins Git dependencies, so use the Git CLI for Cargo fetches:

```sh
export CARGO_NET_GIT_FETCH_WITH_CLI=true
cargo run -p onionskin-app --features shell --bin onionskin
cargo run -p onionskin-app --features shell --bin onionskin -- /path/to/file.pdf
```

Rust stable with `rustfmt` and `clippy` is declared in
[`rust-toolchain.toml`](rust-toolchain.toml). The shell also needs the native GPUI
toolchain for the host. CI installs the Linux Fontconfig, Vulkan, Wayland, X11/XCB,
and XKB development packages listed in [the shell job](.github/workflows/ci.yml),
and downloads Apple's Metal toolchain on macOS. Windows uses the standard Rust
MSVC toolchain supplied by the hosted runner.

Useful checks:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo test -p onionskin-app --no-default-features
cargo test -p onionskin-app --features shell,shell-test-support
```

The release workflow and packaging scripts are not an acceptance-ready release
path yet. See [packaging notes](packaging/README.md) and the B6 findings in the
forward audit before producing artifacts.

## Repository map

- [`PLAN.md`](PLAN.md) defines the product, architecture, invariants, and roadmap.
- [`crates/`](crates/) contains COS parsing, content extraction, session/core,
  rendering, the app shell, and later-milestone service crates.
- [`plugins/`](plugins/) contains feature-gated tools, commands, and codecs.
- [`.project-docs/INDEX.md`](.project-docs/INDEX.md) is the documentation index.
- [`ACROBAT-PARITY.md`](ACROBAT-PARITY.md) is the mechanically checked feature
  matrix.

Historical source artifacts, worktrees, plans, screenshots, and audit reports are
retained evidence. Do not remove, move, rewrite, or clean them up without explicit
per-item approval.
