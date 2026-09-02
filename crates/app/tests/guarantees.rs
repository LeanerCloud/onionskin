//! PLAN.md's guarantee tests, the spec as executable checks. A guarantee whose
//! capability has landed either runs here or names the test that enforces it
//! (guarantee 5 in `kernel_emptiness.rs`, 9 in `crates/core/benches/`, 1, 2 and
//! 6 in `crates/cos/tests/`). One whose capability has not landed stays
//! `#[ignore]`d, and its reason names the milestone PLAN.md gives it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, fs, process};

const ALPHA_SHA256: &str = "b6a98d9ce9a2d9149288fa3df42d377c3e42737afdcdaf714e33c0a100b51060";
const REPLACEMENT_SHA256: &str = "1d054714357ce5ee01723ed91fcaa69206e221faaf9c1fad64f73be2e5d051da";
const TINY_PDF_SHA256: &str = "98704aee8801c3738f9b38577f4c7917b82da5d770a8f4c1c162293e49c0d172";

/// Guarantee 1, round-trip: for every corpus file, open then save with no
/// edit produces byte-identical output. A no-op save appends nothing.
///
/// Proved in `crates/cos/tests/roundtrip.rs`, where the parser and the writer
/// are. `onionskin-app` neither depends on `cos` nor has a save path of its
/// own: PLAN.md puts the app-level save at M3, so running the round-trip from
/// here would mean a dependency and a second corpus walk added for a test that
/// already exists. What this guarantee owns until M3 is that the enforcing
/// test is still there, still switched on, still states both halves of the
/// sentence, and is still reached by CI. Any of those quietly going away would
/// otherwise leave guarantee 1 green and unmeasured.
#[test]
fn a_save_with_no_edit_is_byte_identical_to_the_original() {
    let source = enforcing_suite("roundtrip.rs", 1, &["seeds_round_trip_exactly"]);
    for (marker, missing) in [
        (
            "document.incremental_section()",
            "a no-op save is no longer asked what it appended",
        ),
        (
            "non-empty-noop-save",
            "a no-op save that appended bytes no longer fails",
        ),
        (
            "save_to_vec",
            "nothing is saved, so byte-identity is no longer compared",
        ),
    ] {
        assert!(
            source.contains(marker),
            "roundtrip.rs no longer proves guarantee 1: {missing}"
        );
    }
    assert_ci_reaches_the_cos_suite();
}

/// Guarantee 2, onionskin: open, edit, save produces the original bytes
/// followed by exactly one incremental section, and truncating that
/// section yields the byte-exact original back.
///
/// Proved in `crates/cos/tests/incremental.rs` over the tracked seeds, for the
/// same reason guarantee 1 is proved in `roundtrip.rs`. This checks that the
/// enforcing test still asserts each clause: the original prefix survives, the
/// append is exactly one section, and the document's own `original_len` is the
/// cut that undoes the edit.
#[test]
fn an_edit_appends_one_incremental_section_that_truncates_away() {
    let source = enforcing_suite(
        "incremental.rs",
        2,
        &["editing_a_seed_appends_exactly_one_section"],
    );
    for (marker, missing) in [
        (
            "the original bytes must survive an edit untouched",
            "the original prefix is no longer compared",
        ),
        (
            "an edit must append exactly one incremental section",
            "the section count is no longer pinned to one",
        ),
        (
            "document.original_len()",
            "the truncation point is no longer the one the document reports",
        ),
        (
            "truncating the section must undo the edit",
            "the roll-back half of the sentence is no longer checked",
        ),
    ] {
        assert!(
            source.contains(marker),
            "incremental.rs no longer proves guarantee 2: {missing}"
        );
    }
    assert_ci_reaches_the_cos_suite();
}

/// Guarantee 3, redaction: after redacting text T, the verifier extracts
/// all text and images from the output and finds no trace of T, and a raw
/// byte scan finds no trace of the original object bytes.
#[test]
#[ignore = "lands with the redact plugin and its verifier in M5"]
fn redacted_content_survives_neither_extraction_nor_a_byte_scan() {
    unimplemented!("needs the redact plugin's verifier")
}

/// Guarantee 4, signature preservation: annotating a signed corpus file
/// leaves its signature valid, and the UI and MCP report it as valid.
#[test]
#[ignore = "lands with crypto signature verification in M6"]
fn annotating_a_signed_document_keeps_its_signature_valid() {
    unimplemented!("needs crypto verification and a signed corpus file")
}

/// Guarantee 6, repair: every file in the malformed corpus set opens;
/// saving appends an incremental section carrying the repaired
/// structures; the corrupt original bytes stay byte-intact beneath.
///
/// Proved in `crates/cos/tests/repair.rs`. That test needs `corpus/malformed`,
/// which is gitignored, and a corpus-less run of it returns early and passes -
/// so this guarantee also owns the CI step that generates the set. Without it
/// the enforcing test is present, green, and measuring nothing.
#[test]
fn every_malformed_file_opens_and_repairs_into_a_new_section() {
    let source = enforcing_suite(
        "repair.rs",
        6,
        &["every_malformed_file_repairs_and_saves_over_intact_original_bytes"],
    );
    for (marker, missing) in [
        (
            "guarantee test 6 requires the whole malformed set",
            "a subset of the malformed files may now pass for the whole set",
        ),
        (
            "the corrupt original bytes were not preserved",
            "the original bytes beneath the section are no longer compared",
        ),
        (
            "the appended section holds no cross-reference table",
            "the appended section is no longer required to carry the repair",
        ),
    ] {
        assert!(
            source.contains(marker),
            "repair.rs no longer proves guarantee 6: {missing}"
        );
    }
    assert_ci_reaches_the_cos_suite();

    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("the CI workflow is readable");
    assert!(
        job(&ci, "test").contains("./corpus/make-malformed.sh"),
        "CI never generates corpus/malformed, so repair.rs skips it and guarantee 6 passes unmeasured"
    );
}

/// Guarantee 7, forms compute: every file in the JS-forms corpus set
/// fills the way Acrobat fills it - computed fields recalculate, formats
/// apply, validation fires - against Acrobat-produced expected values.
#[test]
#[ignore = "lands with scripting and tools-form in M5"]
fn js_form_fields_compute_the_way_acrobat_computes_them() {
    unimplemented!("needs scripting live and the JS-forms corpus")
}

/// Guarantee 8, tag integrity: editing a tagged corpus document leaves
/// its structure tree valid and consistent with the edited content,
/// checked by the accessibility plugin's own checker.
#[test]
#[ignore = "lands with tools-accessibility's checker in M5"]
fn editing_a_tagged_document_leaves_its_structure_tree_valid() {
    unimplemented!("needs core's structure tree and the accessibility checker")
}

/// Guarantee 9, performance: decision 11's budgets - time to first page
/// under 200 ms on the 1000-page corpus file, something visible under
/// 200 ms on any page, memory proportional to viewed pages, 60 fps
/// scroll - run as benches, and a regression past budget fails the build
/// like any other test.
///
/// The budgets are asserted in `crates/core/benches/`, where the session,
/// the render worker and the tile store are; `app` has no document to
/// measure and running them twice would only double the corpus a test
/// needs. What this guarantee owns is the second half of the sentence:
/// that those benches are wired to fail a build. Each one is a `[[bench]]`
/// target with its own `main`, and CI runs them as a gate with the corpus
/// made mandatory. A bench that quietly stopped being run would leave
/// every budget green and unmeasured, and that is the regression this
/// catches and the benches themselves cannot.
#[test]
fn open_and_scroll_stay_within_the_performance_budgets() {
    let workspace = workspace_root();

    let manifest = std::fs::read_to_string(workspace.join("crates/core/Cargo.toml"))
        .expect("core's manifest is readable");
    for bench in ["open", "paint", "scroll"] {
        let path = workspace.join(format!("crates/core/benches/{bench}.rs"));
        let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "{} is unreadable ({error}), so a decision-11 budget is unmeasured",
                path.display()
            )
        });
        assert!(
            manifest.contains(&format!("name = \"{bench}\"")),
            "crates/core/benches/{bench}.rs is not declared, so cargo bench never runs it"
        );
        assert!(
            source.contains("harness::under"),
            "crates/core/benches/{bench}.rs states no budget, so it only reports numbers"
        );
    }
    // Every bench keeps its own main. Under libtest's harness a bench reports
    // timings and passes regardless, and counting against the declarations
    // rather than against three keeps that true of the fourth one.
    let lines = |needle: &str| {
        manifest
            .lines()
            .filter(|line| line.trim() == needle)
            .count()
    };
    assert_eq!(
        lines("harness = false"),
        lines("[[bench]]"),
        "a budget bench that runs under libtest's harness reports numbers instead of failing"
    );
    // The budgets are all reported through one function, so a `println!` where
    // its `assert!` is would turn every one of them green and silent, which is
    // the whole failure mode this package exists to prevent.
    let harness = std::fs::read_to_string(workspace.join("crates/core/benches/harness/mod.rs"))
        .expect("the bench harness is readable");
    assert!(
        harness.contains("BUDGET EXCEEDED") && harness.contains("assert!("),
        "the bench harness no longer fails a run that misses a budget"
    );

    let ci = std::fs::read_to_string(workspace.join(".github/workflows/ci.yml"))
        .expect("the workflow is readable");
    assert!(
        ci.contains("pull_request"),
        "CI does not run on pull requests, so no budget is checked before a merge"
    );
    let bench_job = job(&ci, "bench");
    assert!(
        !bench_job.is_empty(),
        "the workflow has no bench job, so nothing runs the budgets"
    );
    assert!(
        bench_job.contains("cargo bench -p onionskin-core"),
        "the bench job does not run the benches, so a regression past budget fails nothing"
    );
    assert!(
        bench_job.contains("ONIONSKIN_CORPUS_REQUIRED: 1"),
        "the bench job does not require its corpus, so it would skip every budget and pass"
    );
    assert!(
        !bench_job.contains("continue-on-error"),
        "a job allowed to fail is not a gate"
    );
    assert!(
        bench_job.contains("actions/setup-python"),
        "the bench job runs corpus fetch validation but does not install Python"
    );
    assert!(
        bench_job.contains("hashFiles('corpus/fetch.sh', 'corpus/verify-sha256.py', 'corpus/r2.py', 'corpus/checksums/hayro-corpus.sha256')"),
        "the bench corpus cache key does not include the fetch helpers and checksum manifest"
    );
    // A job-level condition sits at four spaces, a step's at eight. A gate that
    // only runs on some events is not a gate on the others.
    assert!(
        !bench_job.lines().any(|line| line.starts_with("    if:")),
        "the bench job is conditional, so there are pushes it does not gate"
    );

    for name in ["test", "shell"] {
        assert!(
            job(&ci, name).contains("actions/setup-python"),
            "the {name} job runs app guarantees but does not install Python"
        );
    }
}

#[test]
fn release_artifacts_build_the_windowed_viewer() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("the CI workflow is readable");
    let release = std::fs::read_to_string(workspace_root().join(".github/workflows/release.yml"))
        .expect("the release workflow is readable");
    let ci_shell = job(&ci, "shell");
    let build = job(&release, "build");
    assert!(
        !build.is_empty(),
        "the release workflow has no build job, so it publishes nothing testable"
    );
    assert!(
        build.contains("CARGO_NET_GIT_FETCH_WITH_CLI: true"),
        "release builds fetch git dependencies differently from CI"
    );
    assert!(
        build.contains("actions/setup-python@e797f83bcb11b83ae66e0230d6156d7c80228e7c"),
        "release builds do not install the reviewed Python setup action even though the packaging version gate uses shell helpers"
    );
    for step in [
        "      - name: Update Linux package index\n        if: runner.os == 'Linux'\n        run: sudo apt-get update",
        "      - name: Install GPUI Linux dependencies\n        if: runner.os == 'Linux'\n        run: sudo apt-get install -y libfontconfig1-dev libvulkan-dev libwayland-dev libx11-xcb-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libxkbcommon-x11-dev",
        "      - name: Install macOS Metal toolchain\n        if: runner.os == 'macOS'\n        run: xcodebuild -downloadComponent MetalToolchain",
    ] {
        assert!(
            ci_shell.contains(step),
            "the CI shell job no longer contains the expected prerequisite step:\n{step}"
        );
        assert!(
            build.contains(step),
            "the release build job does not mirror the CI shell prerequisite step:\n{step}"
        );
    }
    assert!(
        build.contains("cargo build --release -p onionskin-app --features shell"),
        "release artifacts are not built with the windowed shell feature"
    );
}

#[test]
fn accessibility_probe_is_a_required_ci_gate() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("the CI workflow is readable");
    let shell = job(&ci, "shell");
    assert!(!shell.is_empty(), "CI has no shell job");
    assert!(
        shell.contains("run: cargo test -p onionskin-app --features a11y-probe --test a11y_probe"),
        "the shell job does not run the macOS accessibility probe"
    );
    let probe_step = shell
        .split("      - name: Accessibility probe")
        .nth(1)
        .and_then(|rest| rest.split("\n      - ").next())
        .expect("the shell job has no named Accessibility probe step");
    assert!(
        probe_step.contains("if: runner.os == 'macOS'"),
        "the accessibility probe is not scoped to macOS"
    );
    assert!(
        !probe_step.contains("continue-on-error"),
        "the accessibility probe is advisory instead of gating"
    );
}

#[test]
fn supply_chain_policy_is_checked_in_and_gated() {
    let workspace = workspace_root();
    let ci = std::fs::read_to_string(workspace.join(".github/workflows/ci.yml"))
        .expect("the CI workflow is readable");
    let dependabot = std::fs::read_to_string(workspace.join(".github/dependabot.yml"))
        .expect("Dependabot policy is readable");
    let deny = std::fs::read_to_string(workspace.join("deny.toml"))
        .expect("cargo-deny policy is readable");

    assert!(
        dependabot.contains("version: 2"),
        "Dependabot policy does not declare the current schema version"
    );
    for ecosystem in ["cargo", "github-actions"] {
        assert!(
            dependabot.contains(&format!("package-ecosystem: \"{ecosystem}\"")),
            "Dependabot does not monitor the {ecosystem} ecosystem"
        );
    }
    assert!(
        dependabot.contains("directory: \"/\""),
        "Dependabot does not monitor the workspace root"
    );
    assert!(
        dependabot.contains("interval: \"weekly\""),
        "Dependabot has no predictable update cadence"
    );

    for section in ["advisories", "licenses", "bans", "sources", "graph"] {
        assert!(
            deny.contains(&format!("[{section}]")),
            "cargo-deny policy is missing [{section}]"
        );
    }

    // Every setting cargo-deny would have to be told to stop enforcing. Read
    // as values rather than as text: `cargo deny check` reports all four
    // sections green against a policy that ignores every advisory, permits
    // every license and trusts every source, so "the file mentions the right
    // words somewhere" is not evidence of anything.
    for (section, key, required) in [
        ("graph", "all-features", "true"),
        ("advisories", "yanked", "\"deny\""),
        ("licenses", "exceptions", "[]"),
        ("licenses", "confidence-threshold", "0.8"),
        ("bans", "wildcards", "\"deny\""),
        // The wildcard ban only means anything once the workspace's own path
        // dependencies stop reading as wildcards, which is what `publish =
        // false` buys. Without it the check fails on our own crates, and the
        // way out of that is to stop denying wildcards at all.
        ("bans", "allow-wildcard-paths", "true"),
        ("bans", "multiple-versions", "\"warn\""),
        ("bans", "allow", "[]"),
        ("bans", "skip", "[]"),
        ("bans", "skip-tree", "[]"),
        ("sources", "unknown-registry", "\"deny\""),
        ("sources", "unknown-git", "\"deny\""),
        ("sources", "required-git-spec", "\"rev\""),
    ] {
        let setting = policy_setting(&deny, section, key);
        assert_eq!(
            setting.as_deref(),
            Some(required),
            "cargo-deny policy weakened: [{section}] {key} must be {required}"
        );
    }
    // cargo-deny's default reports every unmaintained advisory. Narrowing that
    // to "none" or "workspace" mutes whole classes at once, which is what an
    // exception list exists to avoid.
    assert_eq!(
        policy_setting(&deny, "advisories", "unmaintained"),
        None,
        "the policy narrows which unmaintained advisories are reported; tolerate them one ID at a time instead"
    );
    assert_eq!(
        policy_setting(&deny, "licenses", "unused-allowed-license"),
        None,
        "the policy silences the warning that keeps the license allow-list from outgrowing the tree"
    );
    assert_eq!(
        policy_setting(&deny, "graph", "targets").as_deref(),
        Some(
            "[{ triple = \"aarch64-apple-darwin\" }, { triple = \"x86_64-pc-windows-msvc\" }, \
             { triple = \"x86_64-unknown-linux-gnu\" }, { triple = \"aarch64-unknown-linux-gnu\" }]"
        ),
        "cargo-deny no longer scans exactly the triples release.yml publishes"
    );

    // Allow-lists are pinned whole. Asserting only that the reviewed entries
    // are present would let a fourth git source or a copyleft license be added
    // beside them without failing anything.
    assert_eq!(
        policy_setting(&deny, "sources", "allow-registry").as_deref(),
        Some("[\"https://github.com/rust-lang/crates.io-index\"]"),
        "cargo-deny trusts a registry other than crates.io"
    );
    assert_eq!(
        policy_setting(&deny, "sources", "allow-git").as_deref(),
        Some(
            "[\"https://github.com/IAmJSD/gpui\", \"https://github.com/cristim/hayro\", \
             \"https://github.com/linebender/vello\"]"
        ),
        "the reviewed set of git sources changed"
    );
    assert_eq!(
        policy_setting(&deny, "licenses", "allow").as_deref(),
        Some(
            "[\"Apache-2.0\", \"Apache-2.0 WITH LLVM-exception\", \"BSD-2-Clause\", \
             \"BSD-3-Clause\", \"CC0-1.0\", \"ISC\", \"MIT\", \"MPL-2.0\", \"Unicode-3.0\", \
             \"Zlib\"]"
        ),
        "the reviewed set of permitted licenses changed"
    );

    let manifest = std::fs::read_to_string(workspace.join("Cargo.toml"))
        .expect("the workspace manifest is readable");
    assert_eq!(
        policy_setting(&manifest, "workspace.package", "publish").as_deref(),
        Some("false"),
        "the workspace is not marked unpublished, so allow-wildcard-paths does not apply to it"
    );

    // Exceptions are the part of a policy that rots. Every one has to name a
    // single advisory and say why it is tolerated: a bare string would mute an
    // advisory with no stated reason, and a crate name would mute every future
    // advisory against that crate, which is a blanket allow wearing an
    // exception's clothes.
    let ignored = policy_setting(&deny, "advisories", "ignore")
        .expect("the policy declares an advisory exception list");
    for entry in ignored
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split("}, ")
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        assert!(
            entry.starts_with("{ id = \"RUSTSEC-"),
            "an advisory exception is not a table pinned to one advisory ID: {entry}"
        );
        let reason = entry
            .split_once("reason = \"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(reason, _)| reason)
            .unwrap_or_default();
        assert!(
            reason.len() > 20,
            "an advisory exception carries no reviewed reason: {entry}"
        );
    }

    let supply_chain = job(&ci, "supply-chain");
    assert!(!supply_chain.is_empty(), "CI has no supply-chain job");
    assert!(
        supply_chain.contains("CARGO_NET_GIT_FETCH_WITH_CLI: true"),
        "the supply-chain job fetches git dependencies differently from the rest of CI"
    );
    assert!(
        supply_chain.contains("fetch-depth: 0"),
        "the secret scanner cannot inspect git history from a shallow checkout"
    );
    assert!(
        supply_chain.contains("actions/checkout@1af3b93b6815bc44a9784bd300feb67ff0d1eeb3"),
        "CI does not use the reviewed checkout v6 pin"
    );
    assert!(
        supply_chain.contains("dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c"),
        "CI does not use the reviewed rust-toolchain stable pin"
    );
    assert!(
        supply_chain.contains("Swatinem/rust-cache@e172ef532f714507ca8b9ce7978a442736438fc1"),
        "CI does not use the reviewed rust-cache pin"
    );
    assert!(
        supply_chain
            .contains("EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25"),
        "CI does not run cargo-deny"
    );
    assert!(
        supply_chain.contains("https://github.com/gitleaks/gitleaks/releases/download/v8.28.0/gitleaks_8.28.0_linux_x64.tar.gz"),
        "CI does not install the reviewed Gitleaks release"
    );
    assert!(
        supply_chain.contains(
            "echo \"a65b5253807a68ac0cafa4414031fd740aeb55f54fb7e55f386acb52e6a840eb  gitleaks.tar.gz\" | sha256sum -c -"
        ),
        "CI does not verify the reviewed Gitleaks release checksum against the downloaded tarball"
    );
    assert!(
        supply_chain.contains("./gitleaks detect --redact --source ."),
        "CI does not run Gitleaks"
    );
    assert!(
        !supply_chain.contains("GITLEAKS_LICENSE"),
        "the secret scan must not depend on an organization-only action license secret"
    );
    for line in supply_chain
        .lines()
        .filter(|line| line.trim_start().starts_with("- uses: "))
    {
        let Some((_, reference)) = line.split_once('@') else {
            panic!("action is not pinned to a reviewed ref: {line}");
        };
        let sha = reference.split_whitespace().next().unwrap_or_default();
        assert!(
            sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "supply-chain action is not pinned to a full commit SHA: {line}"
        );
    }
    assert!(
        supply_chain.contains(
            "run: cargo test -p onionskin-app --test guarantees -- --exact supply_chain_policy_is_checked_in_and_gated"
        ),
        "the supply-chain job runs cargo-deny without checking that deny.toml still denies anything, so it stays green against a blanket policy"
    );
    assert!(
        !supply_chain.contains("continue-on-error"),
        "supply-chain policy is advisory instead of gating"
    );
    assert!(
        !supply_chain.lines().any(|line| line.starts_with("    if:")),
        "the supply-chain job is conditional, so some events skip it"
    );
}

#[test]
fn checksum_verifier_accepts_a_complete_matching_corpus_root() {
    let temp = TempTree::new("checksum-valid");
    let root = temp.path().join("root");
    fs::create_dir(&root).expect("test root is created");
    fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);

    let output = run_checksum_verifier(&manifest, &root);
    assert!(
        output.status.success(),
        "valid checksum manifest failed\n{}",
        output_text(&output)
    );
}

#[test]
fn checksum_verifier_rejects_portable_manifest_and_tree_errors() {
    assert_checksum_failure("checksum-duplicate", "duplicate", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(
            manifest,
            &[
                &format!("{ALPHA_SHA256}  alpha.pdf"),
                &format!("{ALPHA_SHA256}  alpha.pdf"),
            ],
        );
    });
    assert_checksum_failure("checksum-case-alias", "duplicate", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(
            manifest,
            &[
                &format!("{ALPHA_SHA256}  alpha.pdf"),
                &format!("{ALPHA_SHA256}  ALPHA.pdf"),
            ],
        );
    });
    assert_checksum_failure("checksum-malformed-hash", "malformed", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(manifest, &["not-a-sha256  alpha.pdf"]);
    });
    assert_checksum_failure("checksum-leading-space", "malformed", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(manifest, &[&format!(" {ALPHA_SHA256}  alpha.pdf")]);
    });
    assert_checksum_failure(
        "checksum-extra-separator",
        "whitespace",
        |manifest, root| {
            fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
            write_manifest(manifest, &[&format!("{ALPHA_SHA256}   alpha.pdf")]);
        },
    );
    assert_checksum_failure("checksum-absolute-path", "absolute", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  /alpha.pdf")]);
    });
    assert_checksum_failure("checksum-traversal", "traversal", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  ../alpha.pdf")]);
    });
    assert_checksum_failure("checksum-dot-alias", "traversal", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  ./alpha.pdf")]);
    });
    assert_checksum_failure("checksum-windows-ads", "colon", |manifest, _root| {
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha:stream.pdf")]);
    });
    assert_checksum_failure(
        "checksum-windows-reserved",
        "reserved",
        |manifest, _root| {
            write_manifest(manifest, &[&format!("{ALPHA_SHA256}  con.pdf")]);
        },
    );
    assert_checksum_failure(
        "checksum-control-character",
        "control",
        |manifest, _root| {
            write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha\t.pdf")]);
        },
    );
    assert_checksum_failure(
        "checksum-empty-manifest",
        "no entries",
        |manifest, _root| {
            fs::write(manifest, b"").expect("empty checksum manifest is written");
        },
    );
    assert_checksum_failure("checksum-missing-file", "missing", |manifest, _root| {
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  missing.pdf")]);
    });
    assert_checksum_failure(
        "checksum-nonregular",
        "not a regular file",
        |manifest, root| {
            fs::create_dir(root.join("alpha.pdf")).expect("non-regular PDF path is created");
            write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
        },
    );
    assert_checksum_failure("checksum-unexpected-pdf", "unexpected", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
        fs::write(root.join("extra.pdf"), b"beta\n").expect("extra PDF is written");
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
    });
    assert_checksum_failure(
        "checksum-unexpected-directory",
        "unexpected directory",
        |manifest, root| {
            fs::write(root.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
            fs::create_dir(root.join("extra.pdf")).expect("unexpected directory is created");
            write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
        },
    );
    assert_checksum_failure("checksum-changed-bytes", "mismatch", |manifest, root| {
        fs::write(root.join("alpha.pdf"), b"beta\n").expect("test PDF is written");
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
    });
}

#[test]
fn checksum_helpers_fail_closed_without_a_secure_backend() {
    let temp = TempTree::new("checksum-unsupported-backend");
    let root = temp.path().join("root");
    fs::create_dir(&root).expect("checksum root is created");
    fs::write(root.join("alpha.pdf"), b"alpha\n").expect("checksum PDF is written");
    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);

    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let external = temp.path().join("external");
    fs::create_dir(&external).expect("destination parent is created");
    let dest = external.join("hayro-corpus");

    let driver = temp.path().join("unsupported_backend_driver.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import pathlib
import sys

verifier_path = pathlib.Path(sys.argv[1])
manifest = pathlib.Path(sys.argv[2])
root = pathlib.Path(sys.argv[3])
staging = pathlib.Path(sys.argv[4])
dest = pathlib.Path(sys.argv[5])

sys.path.insert(0, str(verifier_path.parent))
spec = importlib.util.spec_from_file_location("verify_sha256_under_test", verifier_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

module.r2.can_use_dir_fd = lambda: False
module.r2.can_publish_with_dir_fd = lambda: False
module.r2.has_windows_handles = lambda: False

errors, _ = module.verify(manifest, root)
if not any("unsupported platform" in error for error in errors):
    raise SystemExit(f"expected verifier unsupported-backend failure, got {errors!r}")

try:
    module.r2.publish_staged_set(staging, dest, "test-source", "test-revision", None)
except module.r2.R2Error as error:
    if "unsupported platform" in str(error):
        raise SystemExit(0)
    raise SystemExit(f"unexpected publisher error: {error}")
raise SystemExit("publisher succeeded without a secure backend")
"#,
    )
    .expect("unsupported backend driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/verify-sha256.py"))
        .arg(&manifest)
        .arg(&root)
        .arg(&staging)
        .arg(&dest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("unsupported backend driver is runnable");
    assert!(
        output.status.success(),
        "helpers did not fail closed without a secure backend\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_verifier_rejects_a_root_identity_change_before_dir_fd_open() {
    let temp = TempTree::new("checksum-root-identity-change-dir-fd");
    let root = temp.path().join("root");
    let old_root = temp.path().join("old-root");
    let replacement = temp.path().join("replacement-root");
    fs::create_dir(&root).expect("original root is created");
    fs::create_dir(&replacement).expect("replacement root is created");
    fs::write(root.join("alpha.pdf"), b"original\n").expect("original PDF is written");
    fs::write(replacement.join("alpha.pdf"), b"replacement\n").expect("replacement PDF is written");
    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{REPLACEMENT_SHA256}  alpha.pdf")]);

    let driver = temp.path().join("root_identity_dir_fd_driver.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

verifier_path = pathlib.Path(sys.argv[1])
manifest = pathlib.Path(sys.argv[2])
root = pathlib.Path(sys.argv[3])
old_root = pathlib.Path(sys.argv[4])
replacement = pathlib.Path(sys.argv[5])

sys.path.insert(0, str(verifier_path.parent))
spec = importlib.util.spec_from_file_location("verify_sha256_under_test", verifier_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.r2.can_use_dir_fd():
    raise SystemExit("dir-fd verification is unavailable on this platform")

original_read_manifest = module.read_manifest

def swapping_read_manifest(path):
    result = original_read_manifest(path)
    os.rename(root, old_root)
    os.rename(replacement, root)
    return result

module.read_manifest = swapping_read_manifest
errors, _ = module.verify(manifest, root)
if not any("root directory changed during verification" in error for error in errors):
    raise SystemExit(f"expected dir-fd root identity failure, got {errors!r}")
"#,
    )
    .expect("dir-fd root identity driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/verify-sha256.py"))
        .arg(&manifest)
        .arg(&root)
        .arg(&old_root)
        .arg(&replacement)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("dir-fd root identity driver is runnable");
    assert!(
        output.status.success(),
        "verifier did not reject pre-open root identity substitution\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_verifier_rejects_symlinked_pdf_entries() {
    assert_checksum_failure("checksum-symlink", "symlink", |manifest, root| {
        fs::write(root.join("target.pdf"), b"alpha\n").expect("test PDF is written");
        std::os::unix::fs::symlink(root.join("target.pdf"), root.join("alpha.pdf"))
            .expect("test symlink is created");
        write_manifest(manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
    });
}

#[cfg(unix)]
#[test]
fn checksum_verifier_rejects_a_manifest_symlink_without_leaking_target_contents() {
    const SENTINEL: &str = "private-manifest-target-must-not-be-read";

    let temp = TempTree::new("checksum-manifest-symlink");
    let root = temp.path().join("root");
    fs::create_dir(&root).expect("test root is created");
    let target = temp.path().join("manifest-target");
    fs::write(&target, SENTINEL).expect("manifest target is written");
    let manifest = temp.path().join("manifest.sha256");
    std::os::unix::fs::symlink(&target, &manifest).expect("manifest symlink is created");

    let output = run_checksum_verifier(&manifest, &root);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "manifest symlink unexpectedly passed"
    );
    assert!(text.contains("symlink not allowed"), "{text}");
    assert!(
        !text.contains(SENTINEL),
        "manifest symlink target leaked into diagnostics\n{text}"
    );
}

#[cfg(windows)]
#[test]
fn checksum_verifier_rejects_a_windows_junction_root() {
    let temp = TempTree::new("checksum-root-junction");
    let target = temp.path().join("target");
    fs::create_dir(&target).expect("junction target is created");
    fs::write(target.join("alpha.pdf"), b"alpha\n").expect("test PDF is written");
    let junction = temp.path().join("root-junction");
    let created = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&junction)
        .arg(&target)
        .output()
        .expect("Windows junction command is runnable");
    assert!(
        created.status.success(),
        "Windows junction creation failed\n{}",
        output_text(&created)
    );

    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
    let output = run_checksum_verifier(&manifest, &junction);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "Windows junction root unexpectedly passed"
    );
    assert!(text.contains("reparse point not allowed"), "{text}");
}

#[cfg(windows)]
#[test]
fn checksum_verifier_rejects_a_windows_reparse_pdf_entry() {
    let temp = TempTree::new("checksum-windows-reparse-pdf");
    let root = temp.path().join("root");
    fs::create_dir(&root).expect("test root is created");
    let target = temp.path().join("target.pdf");
    fs::write(&target, b"alpha\n").expect("target PDF is written");
    create_windows_reparse_for_file_role(&root.join("alpha.pdf"), &target);

    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);
    let output = run_checksum_verifier(&manifest, &root);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "Windows reparse PDF entry unexpectedly passed\n{text}"
    );
    assert!(text.contains("reparse point not allowed"), "{text}");
}

#[cfg(windows)]
#[test]
fn checksum_verifier_windows_root_handle_denies_rename_during_verification() {
    let temp = TempTree::new("checksum-windows-root-rename-denial");
    let root = temp.path().join("root");
    let old_root = temp.path().join("old-root");
    fs::create_dir(&root).expect("checksum root is created");
    fs::write(root.join("alpha.pdf"), b"alpha\n").expect("checksum PDF is written");
    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{ALPHA_SHA256}  alpha.pdf")]);

    let driver = temp.path().join("windows_root_rename_denial.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

verifier_path = pathlib.Path(sys.argv[1])
manifest = pathlib.Path(sys.argv[2])
root = pathlib.Path(sys.argv[3])
old_root = pathlib.Path(sys.argv[4])

sys.path.insert(0, str(verifier_path.parent))
spec = importlib.util.spec_from_file_location("verify_sha256_under_test", verifier_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.r2.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

rename_denied = False
original_reader = module.root_pdfs_from_windows

def racing_reader(path):
    global rename_denied
    try:
        os.rename(root, old_root)
    except OSError:
        rename_denied = True
    else:
        os.rename(old_root, root)
        raise SystemExit("root rename succeeded while verifier handle was held")
    return original_reader(path)

module.root_pdfs_from_windows = racing_reader
errors, _ = module.verify(manifest, root)
if errors:
    raise SystemExit(f"verification failed unexpectedly: {errors!r}")
if not rename_denied:
    raise SystemExit("root rename was not attempted")
"#,
    )
    .expect("Windows root rename-denial driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/verify-sha256.py"))
        .arg(&manifest)
        .arg(&root)
        .arg(&old_root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows root rename-denial driver is runnable");
    assert!(
        output.status.success(),
        "Windows verifier root handle did not deny rename\n{}",
        output_text(&output)
    );
}

#[test]
fn checksum_r2_helper_validates_manifest_ids_and_destination_state() {
    let temp = TempTree::new("checksum-r2-helper-validation");

    let valid = temp.path().join("valid.json");
    fs::write(&valid, r#"[{"id":"tiny"}]"#).expect("valid R2 manifest is written");
    let valid_output = run_r2_helper(&[OsStr::new("ids"), valid.as_os_str()]);
    assert!(
        valid_output.status.success(),
        "valid object manifest did not produce ids\n{}",
        output_text(&valid_output)
    );
    assert_eq!(valid_output.stdout, b"tiny\n");

    for (name, contents, diagnostic) in [
        (
            "top-level-object",
            r#"{"id":"tiny"}"#,
            "top-level JSON array",
        ),
        ("non-string-id", r#"[{"id":7}]"#, "id is not a string"),
        ("traversal-id", r#"["../escape"]"#, "traversal"),
        ("slash-id", r#"["dir/file"]"#, "path separators"),
        ("control-id", r#"["line\nfeed"]"#, "control"),
        ("case-duplicate-id", r#"["Tiny","tiny"]"#, "duplicate"),
        ("reserved-id", r#"["CON"]"#, "reserved"),
    ] {
        let manifest = temp.path().join(format!("{name}.json"));
        fs::write(&manifest, contents).expect("invalid R2 manifest is written");
        let output = run_r2_helper(&[OsStr::new("ids"), manifest.as_os_str()]);
        let text = output_text(&output);
        assert!(
            !output.status.success(),
            "invalid R2 manifest {name} unexpectedly passed\n{text}"
        );
        assert!(
            text.contains(diagnostic),
            "invalid R2 manifest {name} did not mention {diagnostic:?}\n{text}"
        );
    }

    let invalid_utf = temp.path().join("invalid-utf.json");
    fs::write(&invalid_utf, b"[\"ok\", \"\xff\"]").expect("invalid UTF-8 manifest is written");
    let invalid_utf_output = run_r2_helper(&[OsStr::new("ids"), invalid_utf.as_os_str()]);
    let invalid_utf_text = output_text(&invalid_utf_output);
    assert!(
        !invalid_utf_output.status.success(),
        "invalid UTF-8 R2 manifest unexpectedly passed\n{invalid_utf_text}"
    );
    assert!(
        invalid_utf_text.contains("manifest is not UTF-8"),
        "{invalid_utf_text}"
    );
    assert!(
        !invalid_utf_text.contains("Traceback"),
        "invalid UTF-8 manifest leaked a Python traceback\n{invalid_utf_text}"
    );

    let nonregular_manifest = temp.path().join("manifest-directory.json");
    fs::create_dir(&nonregular_manifest).expect("non-regular manifest path is created");
    let nonregular_output = run_r2_helper(&[OsStr::new("ids"), nonregular_manifest.as_os_str()]);
    let nonregular_text = output_text(&nonregular_output);
    assert!(
        !nonregular_output.status.success(),
        "non-regular R2 manifest unexpectedly passed\n{nonregular_text}"
    );
    assert!(
        nonregular_text.contains("not a regular file"),
        "{nonregular_text}"
    );

    let absent = temp.path().join("absent-dest");
    let absent_output = run_r2_helper(&[OsStr::new("check-dest"), absent.as_os_str()]);
    assert!(
        absent_output.status.success(),
        "absent destination should be publishable\n{}",
        output_text(&absent_output)
    );
    assert_eq!(absent_output.stdout, b"absent\n");

    let missing_parent = temp.path().join("missing-parent/hayro-corpus");
    let missing_parent_output =
        run_r2_helper(&[OsStr::new("check-dest"), missing_parent.as_os_str()]);
    assert!(
        missing_parent_output.status.success(),
        "destination under a missing parent should remain an absent preflight\n{}",
        output_text(&missing_parent_output)
    );
    assert_eq!(missing_parent_output.stdout, b"absent\n");

    let regular_file = temp.path().join("regular-dest");
    fs::write(&regular_file, b"not a directory").expect("regular destination file is written");
    let regular_output = run_r2_helper(&[OsStr::new("check-dest"), regular_file.as_os_str()]);
    let regular_text = output_text(&regular_output);
    assert!(
        !regular_output.status.success(),
        "regular-file destination unexpectedly passed\n{regular_text}"
    );
    assert!(regular_text.contains("not a directory"), "{regular_text}");

    let unstamped = temp.path().join("unstamped-dest");
    fs::create_dir(&unstamped).expect("unstamped destination is created");
    let unstamped_output = run_r2_helper(&[OsStr::new("check-dest"), unstamped.as_os_str()]);
    let unstamped_text = output_text(&unstamped_output);
    assert!(
        !unstamped_output.status.success(),
        "unstamped destination unexpectedly passed\n{unstamped_text}"
    );
    assert!(
        unstamped_text.contains("no .fetch-stamp"),
        "{unstamped_text}"
    );

    let stamped = temp.path().join("stamped-dest");
    fs::create_dir(&stamped).expect("stamped destination is created");
    write_fetch_stamp(&stamped);
    let stamped_output = run_r2_helper(&[OsStr::new("check-dest"), stamped.as_os_str()]);
    assert!(
        stamped_output.status.success(),
        "stamped destination did not pass\n{}",
        output_text(&stamped_output)
    );
    assert_eq!(stamped_output.stdout, b"stamped\n");
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_rejects_a_manifest_symlink_without_leaking_target_contents() {
    const SENTINEL: &str = "r2-private-manifest-target-must-not-be-read";

    let temp = TempTree::new("checksum-r2-manifest-symlink");
    let target = temp.path().join("manifest-target.json");
    fs::write(&target, SENTINEL).expect("manifest target is written");
    let manifest = temp.path().join("manifest.json");
    std::os::unix::fs::symlink(&target, &manifest).expect("manifest symlink is created");

    let output = run_r2_helper(&[OsStr::new("ids"), manifest.as_os_str()]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "R2 manifest symlink unexpectedly passed\n{text}"
    );
    assert!(text.contains("symlink not allowed"), "{text}");
    assert!(
        !text.contains(SENTINEL),
        "R2 manifest symlink target leaked into diagnostics\n{text}"
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_rejects_a_windows_reparse_manifest() {
    let temp = TempTree::new("checksum-r2-windows-manifest-reparse");
    let target = temp.path().join("manifest-target.json");
    fs::write(&target, r#"["tiny"]"#).expect("manifest target is written");
    let manifest = temp.path().join("manifest-link.json");
    create_windows_reparse_for_file_role(&manifest, &target);

    let output = run_r2_helper(&[OsStr::new("ids"), manifest.as_os_str()]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "Windows R2 manifest reparse point unexpectedly passed\n{text}"
    );
    assert!(text.contains("reparse point not allowed"), "{text}");
}

#[test]
fn checksum_r2_helper_rejects_non_regular_staged_entries() {
    let temp = TempTree::new("checksum-r2-nonregular-staging");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::create_dir(staging.join("tiny.pdf")).expect("non-regular staged entry is created");
    let dest = temp.path().join("dest");

    let output = run_r2_helper(&[
        OsStr::new("publish"),
        staging.as_os_str(),
        dest.as_os_str(),
        OsStr::new("test-source"),
        OsStr::new("test-revision"),
        OsStr::new("--no-checksum"),
    ]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "non-regular staged entry unexpectedly published\n{text}"
    );
    assert!(text.contains("not a regular file"), "{text}");
    assert!(
        !dest.exists(),
        "destination was created after staged-entry validation failed"
    );
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_rejects_a_visible_parent_swap_before_unix_publish_returns() {
    let temp = TempTree::new("checksum-r2-unix-parent-swap");
    let parent = temp.path().join("external");
    let old_parent = temp.path().join("old-external");
    let replacement_parent = temp.path().join("replacement-external");
    fs::create_dir(&parent).expect("destination parent is created");
    fs::create_dir(&replacement_parent).expect("replacement parent is created");

    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");

    let driver = temp.path().join("r2_unix_parent_swap_driver.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
dest = pathlib.Path(sys.argv[3])
old_parent = pathlib.Path(sys.argv[4])
replacement_parent = pathlib.Path(sys.argv[5])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.can_publish_with_dir_fd():
    raise SystemExit("dir-fd publication is unavailable on this platform")

original_write_stamp = module.write_stamp_to_fd

def swapping_write_stamp(descriptor, source, revision):
    original_write_stamp(descriptor, source, revision)
    os.rename(dest.parent, old_parent)
    os.rename(replacement_parent, dest.parent)

module.write_stamp_to_fd = swapping_write_stamp
try:
    module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
except module.R2Error as error:
    if "parent directory changed during publication" in str(error):
        raise SystemExit(0)
    raise SystemExit(f"unexpected R2 error: {error}")
raise SystemExit("detached parent publication reported success")
"#,
    )
    .expect("Unix parent-swap driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(parent.join("hayro-corpus"))
        .arg(&old_parent)
        .arg(&replacement_parent)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Unix parent-swap driver is runnable");
    assert!(
        output.status.success(),
        "R2 publication did not reject visible parent substitution\n{}",
        output_text(&output)
    );
}

#[test]
fn checksum_r2_helper_uses_exclusive_handle_relative_publication() {
    let source = fs::read_to_string(workspace_root().join("corpus/r2.py"))
        .expect("R2 helper source is readable");
    assert!(
        source.contains("renameat2") && source.contains("RENAME_NOREPLACE"),
        "Unix publication no longer names the Linux exclusive finalizer"
    );
    assert!(
        source.contains("renameatx_np") && source.contains("0x4"),
        "Unix publication no longer names the macOS exclusive finalizer"
    );
    assert!(
        source.contains("SetFileInformationByHandle")
            && source.contains("FILE_RENAME_INFO_CLASS")
            && source.contains("RootDirectory"),
        "Windows publication no longer uses handle-based FileRenameInfo"
    );
    assert!(
        !source.contains("os.rename(") && !source.contains("MoveFileEx"),
        "publication must not use path-only rename fallbacks"
    );
    assert!(
        !source.contains("create_windows_directory"),
        "Windows publication must not create the public destination directly"
    );
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_rejects_a_competing_destination_at_unix_finalization() {
    let temp = TempTree::new("checksum-r2-unix-finalizer-competitor");
    let parent = temp.path().join("external");
    fs::create_dir(&parent).expect("destination parent is created");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let dest = parent.join("hayro-corpus");

    let driver = temp.path().join("r2_unix_finalizer_competitor.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
dest = pathlib.Path(sys.argv[3])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.can_publish_with_dir_fd():
    raise SystemExit("dir-fd publication is unavailable on this platform")

original_finalize = module.finalize_payload_directory_unix

def racing_finalize(private_fd, parent_fd, publish_dest):
    os.mkdir(publish_dest)
    (publish_dest / "competitor.txt").write_text("competitor", encoding="utf-8")
    return original_finalize(private_fd, parent_fd, publish_dest)

module.finalize_payload_directory_unix = racing_finalize
try:
    module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
except module.R2Error as error:
    if "destination appeared before publication" not in str(error):
        raise SystemExit(f"unexpected R2 error: {error}")
else:
    raise SystemExit("publication replaced a competing destination")

if (dest / "competitor.txt").read_text(encoding="utf-8") != "competitor":
    raise SystemExit("competing destination contents were replaced")
leaks = [path.name for path in dest.parent.iterdir() if path.name.startswith("r2publish-")]
if leaks:
    raise SystemExit(f"private publication directory leaked after finalizer failure: {leaks}")
"#,
    )
    .expect("Unix finalizer competitor driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&dest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Unix finalizer competitor driver is runnable");
    assert!(
        output.status.success(),
        "R2 publication did not reject finalization-time competitor\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_keeps_the_public_destination_absent_until_finalization() {
    let temp = TempTree::new("checksum-r2-unix-no-public-before-finalize");
    let parent = temp.path().join("external");
    fs::create_dir(&parent).expect("destination parent is created");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let dest = parent.join("hayro-corpus");

    let driver = temp.path().join("r2_unix_no_public_before_finalize.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
dest = pathlib.Path(sys.argv[3])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.can_publish_with_dir_fd():
    raise SystemExit("dir-fd publication is unavailable on this platform")

original_copy = module.copy_file_from_fd_to_fd

def observing_copy(source_fd, source_root, name, payload_fd):
    if dest.exists():
        raise SystemExit("visible final destination existed before finalization")
    return original_copy(source_fd, source_root, name, payload_fd)

module.copy_file_from_fd_to_fd = observing_copy
module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
if (dest / "tiny.pdf").read_bytes() != b"%PDF-1.4\n%tiny\n":
    raise SystemExit("published bytes changed")
leaks = [path.name for path in dest.parent.iterdir() if path.name.startswith("r2publish-")]
if leaks:
    raise SystemExit(f"private publication directory leaked after success: {leaks}")
"#,
    )
    .expect("Unix no-public-before-finalize driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&dest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Unix no-public-before-finalize driver is runnable");
    assert!(
        output.status.success(),
        "R2 publication exposed the final destination before finalization\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_anchors_the_staging_root_during_publication() {
    let temp = TempTree::new("checksum-r2-unix-staging-root-anchor");
    let parent = temp.path().join("external");
    fs::create_dir(&parent).expect("destination parent is created");
    let staging = temp.path().join("staging");
    let old_staging = temp.path().join("old-staging");
    let attacker = temp.path().join("attacker-staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::create_dir(&attacker).expect("attacker staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    fs::write(attacker.join("tiny.pdf"), b"%PDF-1.4\nattacker\n").expect("attacker PDF is written");
    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{TINY_PDF_SHA256}  tiny.pdf")]);
    let dest = parent.join("hayro-corpus");

    let driver = temp.path().join("r2_unix_staging_root_anchor.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
old_staging = pathlib.Path(sys.argv[3])
attacker = pathlib.Path(sys.argv[4])
dest = pathlib.Path(sys.argv[5])
manifest = pathlib.Path(sys.argv[6])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.can_publish_with_dir_fd():
    raise SystemExit("dir-fd publication is unavailable on this platform")

original_staged = module.staged_pdf_names_from_fd
swapped = False

def swapping_staged(path, descriptor):
    global swapped
    names = original_staged(path, descriptor)
    if path == staging and not swapped:
        os.rename(staging, old_staging)
        os.rename(attacker, staging)
        swapped = True
    return names

module.staged_pdf_names_from_fd = swapping_staged
module.publish_staged_set(staging, dest, "test-source", "test-revision", manifest)
if not swapped:
    raise SystemExit("staging root swap was not attempted")
if (dest / "tiny.pdf").read_bytes() != b"%PDF-1.4\n%tiny\n":
    raise SystemExit("publication copied from the swapped visible staging root")
"#,
    )
    .expect("Unix staging-root anchor driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&old_staging)
        .arg(&attacker)
        .arg(&dest)
        .arg(&manifest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Unix staging-root anchor driver is runnable");
    assert!(
        output.status.success(),
        "R2 publication was not anchored to the opened staging root\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_rechecks_private_hashes_after_copy_and_cleans_failure() {
    let temp = TempTree::new("checksum-r2-unix-private-hash-recheck");
    let parent = temp.path().join("external");
    fs::create_dir(&parent).expect("destination parent is created");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let changed = temp.path().join("changed.pdf");
    fs::write(&changed, b"%PDF-1.4\nchanged\n").expect("replacement PDF is written");
    let manifest = temp.path().join("manifest.sha256");
    write_manifest(&manifest, &[&format!("{TINY_PDF_SHA256}  tiny.pdf")]);
    let dest = parent.join("hayro-corpus");

    let driver = temp.path().join("r2_unix_private_hash_recheck.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
changed = pathlib.Path(sys.argv[3])
dest = pathlib.Path(sys.argv[4])
manifest = pathlib.Path(sys.argv[5])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.can_publish_with_dir_fd():
    raise SystemExit("dir-fd publication is unavailable on this platform")

original_copy = module.copy_file_from_fd_to_fd
swapped = False

def swapping_copy(source_fd, source_root, name, payload_fd):
    global swapped
    if not swapped:
        os.replace(changed, source_root / name)
        swapped = True
    return original_copy(source_fd, source_root, name, payload_fd)

module.copy_file_from_fd_to_fd = swapping_copy
try:
    module.publish_staged_set(staging, dest, "test-source", "test-revision", manifest)
except module.R2Error as error:
    if "sha256 mismatch" not in str(error):
        raise SystemExit(f"unexpected R2 error: {error}")
else:
    raise SystemExit("publication succeeded after staged bytes changed")
if not swapped:
    raise SystemExit("staged file replacement was not attempted")
if dest.exists():
    raise SystemExit("destination exists after checksum mismatch")
leaks = [path.name for path in dest.parent.iterdir() if path.name.startswith("r2publish-")]
if leaks:
    raise SystemExit(f"private publication directory leaked after checksum mismatch: {leaks}")
"#,
    )
    .expect("Unix private hash recheck driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&changed)
        .arg(&dest)
        .arg(&manifest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Unix private hash recheck driver is runnable");
    assert!(
        output.status.success(),
        "R2 publication did not recheck copied private bytes\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_cleans_private_publication_failures() {
    for mode in [
        "private-validation",
        "exact-name-recheck",
        "copy",
        "stamp",
        "private-symlink",
        "unsupported-finalizer",
        "private-name-swap",
    ] {
        let temp = TempTree::new(&format!("checksum-r2-unix-cleanup-{mode}"));
        let parent = temp.path().join("external");
        fs::create_dir(&parent).expect("destination parent is created");
        let staging = temp.path().join("staging");
        fs::create_dir(&staging).expect("staging directory is created");
        fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
        let dest = parent.join("hayro-corpus");

        let driver = temp.path().join("r2_unix_cleanup_failure.py");
        fs::write(
            &driver,
            r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
dest = pathlib.Path(sys.argv[3])
mode = sys.argv[4]

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.can_publish_with_dir_fd():
    raise SystemExit("dir-fd publication is unavailable on this platform")

expected = "forced"
if mode == "private-validation":
    def failing_validate(root, root_fd, names, expected_checksums):
        raise module.R2Error(expected + " private validation")
    module.validate_private_contents_from_fd = failing_validate
elif mode == "exact-name-recheck":
    original_validate = module.validate_private_contents_from_fd
    def extra_name(root, root_fd, names, expected_checksums):
        descriptor = os.open("extra.pdf", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644, dir_fd=root_fd)
        os.close(descriptor)
        return original_validate(root, root_fd, names, expected_checksums)
    module.validate_private_contents_from_fd = extra_name
    expected = "unexpected extra.pdf"
elif mode == "copy":
    def failing_copy(source_fd, source_root, name, payload_fd):
        raise module.R2Error(expected + " copy")
    module.copy_file_from_fd_to_fd = failing_copy
elif mode == "stamp":
    def failing_stamp(payload_fd, source, revision):
        raise module.R2Error(expected + " stamp")
    module.write_stamp_to_fd = failing_stamp
elif mode == "private-symlink":
    def symlink_then_fail(payload_fd, source, revision):
        os.symlink("/tmp/onionskin-r2-cleanup-target", "link.pdf", dir_fd=payload_fd)
        raise module.R2Error(expected + " private symlink")
    module.write_stamp_to_fd = symlink_then_fail
    expected = "symlink not allowed"
elif mode == "unsupported-finalizer":
    module.sys.platform = "freebsd13"
    expected = "exclusive finalization unsupported"
elif mode == "private-name-swap":
    original_create_payload = module.create_payload_directory_from_private
    def swapped_private(private_fd, private_path):
        saved = private_path.parent / (private_path.name + "-saved")
        os.rename(private_path, saved)
        os.mkdir(private_path)
        (private_path / "attacker.txt").write_text("attacker", encoding="utf-8")
        raise module.R2Error(expected + " private-name swap")
    module.create_payload_directory_from_private = swapped_private
    expected = "leaving"
else:
    raise SystemExit(f"unknown mode {mode}")

try:
    module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
except module.R2Error as error:
    if expected not in str(error):
        raise SystemExit(f"mode {mode}: unexpected R2 error: {error}")
else:
    raise SystemExit(f"mode {mode}: publication succeeded unexpectedly")

if dest.exists():
    raise SystemExit(f"mode {mode}: destination exists after failed publication")
leaks = [path for path in dest.parent.iterdir() if path.name.startswith("r2publish-")]
if mode == "private-name-swap":
    if not any((path / "attacker.txt").exists() for path in leaks):
        raise SystemExit("swapped private directory was not preserved")
elif mode == "private-symlink":
    if not any((path / "payload" / "link.pdf").is_symlink() for path in leaks):
        raise SystemExit("private payload symlink was not preserved for manual inspection")
else:
    if leaks:
        raise SystemExit(f"mode {mode}: private publication directory leaked: {[path.name for path in leaks]}")
"#,
        )
        .expect("Unix cleanup failure driver is written");

        let output = Command::new(python_interpreter())
            .arg(&driver)
            .arg(workspace_root().join("corpus/r2.py"))
            .arg(&staging)
            .arg(&dest)
            .arg(mode)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .expect("Unix cleanup failure driver is runnable");
        assert!(
            output.status.success(),
            "R2 cleanup proof failed for {mode}\n{}",
            output_text(&output)
        );
    }
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_rejects_a_symlink_destination() {
    let temp = TempTree::new("checksum-r2-symlink-dest");
    let target = temp.path().join("target");
    fs::create_dir(&target).expect("symlink target is created");
    let dest = temp.path().join("dest");
    std::os::unix::fs::symlink(&target, &dest).expect("destination symlink is created");

    let output = run_r2_helper(&[OsStr::new("check-dest"), dest.as_os_str()]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "symlink destination unexpectedly passed\n{text}"
    );
    assert!(text.contains("symlink not allowed"), "{text}");
}

#[cfg(unix)]
#[test]
fn checksum_r2_helper_rejects_a_symlinked_destination_parent() {
    let temp = TempTree::new("checksum-r2-symlink-parent");
    let redirect = temp.path().join("redirect");
    fs::create_dir(&redirect).expect("redirect parent target is created");
    let external = temp.path().join("external");
    std::os::unix::fs::symlink(&redirect, &external).expect("external symlink is created");
    let dest = external.join("hayro-corpus");

    let check = run_r2_helper(&[OsStr::new("check-dest"), dest.as_os_str()]);
    let check_text = output_text(&check);
    assert!(
        !check.status.success(),
        "symlinked destination parent unexpectedly passed check-dest\n{check_text}"
    );
    assert!(check_text.contains("symlink not allowed"), "{check_text}");

    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let publish = run_r2_helper(&[
        OsStr::new("publish"),
        staging.as_os_str(),
        dest.as_os_str(),
        OsStr::new("test-source"),
        OsStr::new("test-revision"),
        OsStr::new("--no-checksum"),
    ]);
    let publish_text = output_text(&publish);
    assert!(
        !publish.status.success(),
        "symlinked destination parent unexpectedly published\n{publish_text}"
    );
    assert!(
        publish_text.contains("symlink not allowed"),
        "{publish_text}"
    );
    assert_eq!(
        fs::read_dir(&redirect)
            .expect("redirect target is readable")
            .count(),
        0,
        "helper publication wrote through the symlinked parent"
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_rejects_a_windows_junction_destination() {
    let temp = TempTree::new("checksum-r2-junction-dest");
    let target = temp.path().join("target");
    fs::create_dir(&target).expect("junction target is created");
    let dest = temp.path().join("dest-junction");
    let created = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&dest)
        .arg(&target)
        .output()
        .expect("Windows junction command is runnable");
    assert!(
        created.status.success(),
        "Windows junction creation failed\n{}",
        output_text(&created)
    );

    let output = run_r2_helper(&[OsStr::new("check-dest"), dest.as_os_str()]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "junction destination unexpectedly passed\n{text}"
    );
    assert!(text.contains("reparse point not allowed"), "{text}");

    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let publish = run_r2_helper(&[
        OsStr::new("publish"),
        staging.as_os_str(),
        dest.as_os_str(),
        OsStr::new("test-source"),
        OsStr::new("test-revision"),
        OsStr::new("--no-checksum"),
    ]);
    let publish_text = output_text(&publish);
    assert!(
        !publish.status.success(),
        "junction destination unexpectedly published\n{publish_text}"
    );
    assert!(
        publish_text.contains("reparse point not allowed"),
        "{publish_text}"
    );
    assert_eq!(
        fs::read_dir(&target)
            .expect("junction target is readable")
            .count(),
        0,
        "helper publication wrote through the junction"
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_publishes_a_valid_set_with_windows_handles() {
    let temp = TempTree::new("checksum-r2-windows-valid-publish");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let external = temp.path().join("external");
    fs::create_dir(&external).expect("destination parent is created");
    let dest = external.join("hayro-corpus");

    let output = run_r2_helper(&[
        OsStr::new("publish"),
        staging.as_os_str(),
        dest.as_os_str(),
        OsStr::new("test-source"),
        OsStr::new("test-revision"),
        OsStr::new("--no-checksum"),
    ]);
    let text = output_text(&output);
    assert!(
        output.status.success(),
        "Windows handle publication failed\n{text}"
    );
    assert_eq!(
        fs::read(dest.join("tiny.pdf")).expect("published PDF is readable"),
        b"%PDF-1.4\n%tiny\n"
    );
    assert!(
        dest.join(".fetch-stamp").is_file(),
        "Windows handle publication did not write a stamp"
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_rejects_a_windows_reparse_staged_pdf() {
    let temp = TempTree::new("checksum-r2-windows-staged-reparse");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    let target = temp.path().join("target.pdf");
    fs::write(&target, b"%PDF-1.4\n%tiny\n").expect("target PDF is written");
    create_windows_reparse_for_file_role(&staging.join("tiny.pdf"), &target);
    let external = temp.path().join("external");
    fs::create_dir(&external).expect("destination parent is created");
    let dest = external.join("hayro-corpus");

    let output = run_r2_helper(&[
        OsStr::new("publish"),
        staging.as_os_str(),
        dest.as_os_str(),
        OsStr::new("test-source"),
        OsStr::new("test-revision"),
        OsStr::new("--no-checksum"),
    ]);
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "Windows reparse staged PDF unexpectedly published\n{text}"
    );
    assert!(text.contains("reparse point not allowed"), "{text}");
    assert!(
        !dest.exists(),
        "destination was created after staged reparse validation failed"
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_windows_parent_handle_denies_rename_during_publication() {
    let temp = TempTree::new("checksum-r2-windows-parent-rename-denial");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let external = temp.path().join("external");
    let old_external = temp.path().join("old-external");
    fs::create_dir(&external).expect("destination parent is created");
    let dest = external.join("hayro-corpus");

    let driver = temp.path().join("windows_parent_rename_denial.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
dest = pathlib.Path(sys.argv[3])
old_parent = pathlib.Path(sys.argv[4])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

rename_denied = False
original_create_private = module.create_windows_private_directory

def racing_create_private(parent, publish_dest):
    global rename_denied
    result = original_create_private(parent, publish_dest)
    try:
        os.rename(dest.parent, old_parent)
    except OSError:
        rename_denied = True
    else:
        os.rename(old_parent, dest.parent)
        raise SystemExit("parent rename succeeded while publisher handle was held")
    return result

module.create_windows_private_directory = racing_create_private
module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
if not rename_denied:
    raise SystemExit("parent rename was not attempted")
if (dest / "tiny.pdf").read_bytes() != b"%PDF-1.4\n%tiny\n":
    raise SystemExit("published bytes changed during rename-denial proof")
if not (dest / ".fetch-stamp").is_file():
    raise SystemExit("publication stamp missing")
"#,
    )
    .expect("Windows parent rename-denial driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&dest)
        .arg(&old_external)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows parent rename-denial driver is runnable");
    assert!(
        output.status.success(),
        "Windows publisher parent handle did not deny rename\n{}",
        output_text(&output)
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_windows_rename_info_abi_and_handle_transfer_are_strict() {
    let temp = TempTree::new("checksum-r2-windows-rename-abi");
    let driver = temp.path().join("windows_rename_info_abi.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

module.validate_windows_rename_info_abi()
module.validate_windows_disposition_info_abi()
if module.FILE_RENAME_INFO.RootDirectory.offset != 8:
    raise SystemExit("RootDirectory offset changed")
if module.FILE_RENAME_INFO.FileNameLength.offset != 16:
    raise SystemExit("FileNameLength offset changed")
if module.FILE_RENAME_INFO.FileName.offset != 20:
    raise SystemExit("FileName offset changed")
if module.ctypes.sizeof(module.FILE_RENAME_INFO) != 24:
    raise SystemExit("FileRenameInfo size changed")
if module.FILE_DISPOSITION_INFO.DeleteFile.offset != 0:
    raise SystemExit("DeleteFile offset changed")
if module.ctypes.sizeof(module.FILE_DISPOSITION_INFO) != 1:
    raise SystemExit("FileDispositionInfo size changed")

closed = []
real_close = module.kernel32.CloseHandle
real_open = module.msvcrt.open_osfhandle

def fake_close(handle):
    closed.append(handle)
    return 1

def fake_open(handle, flags):
    if not flags & getattr(os, "O_NOINHERIT", 0):
        raise SystemExit("O_NOINHERIT was not passed to open_osfhandle")
    raise OSError("forced open_osfhandle failure")

module.kernel32.CloseHandle = fake_close
module.msvcrt.open_osfhandle = fake_open
try:
    try:
        module.windows_handle_to_descriptor(module.WindowsHandle(12345), os.O_RDONLY, pathlib.Path("x.pdf"))
    except module.R2Error as error:
        if "cannot wrap file handle" not in str(error):
            raise SystemExit(f"unexpected handle-transfer error: {error}")
    else:
        raise SystemExit("open_osfhandle failure unexpectedly succeeded")
finally:
    module.kernel32.CloseHandle = real_close
    module.msvcrt.open_osfhandle = real_open

if closed != [12345]:
    raise SystemExit(f"raw handle was not closed exactly once: {closed!r}")
"#,
    )
    .expect("Windows rename ABI driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows rename ABI driver is runnable");
    assert!(
        output.status.success(),
        "Windows FileRenameInfo ABI or handle transfer changed\n{}",
        output_text(&output)
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_windows_cleanup_keeps_identity_handles_open_until_delete() {
    let temp = TempTree::new("checksum-r2-windows-cleanup-handle-lifetime");
    let parent = temp.path().join("external");
    fs::create_dir(&parent).expect("destination parent is created");
    let private = parent.join("r2publish-hayro-corpus-test");
    let payload = private.join("payload");
    fs::create_dir(&private).expect("private publication directory is created");
    fs::create_dir(&payload).expect("private payload directory is created");
    fs::write(payload.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n")
        .expect("private payload file is written");

    let driver = temp.path().join("windows_cleanup_handle_lifetime.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
parent = pathlib.Path(sys.argv[2])
private = pathlib.Path(sys.argv[3])
payload = private / "payload"

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

private_handle, private_identity = module.open_windows_private_directory(private)
private_handle.close()
payload_handle, payload_identity = module.open_windows_private_directory(payload)
payload_handle.close()

real_mark = module.mark_windows_directory_for_delete
marked = []

def recording_mark(path, handle, identity, expected):
    if handle.handle is None:
        raise SystemExit(f"{path}: cleanup closed identity handle before delete")
    marked.append(path.name)
    return real_mark(path, handle, identity, expected)

module.mark_windows_directory_for_delete = recording_mark
message = module.cleanup_private_publication_windows(
    parent, private.name, private_identity, payload_identity
)
if message is not None:
    raise SystemExit(f"cleanup failed unexpectedly: {message}")
if marked != ["payload", private.name]:
    raise SystemExit(f"cleanup did not delete payload then private: {marked!r}")
if private.exists():
    raise SystemExit("private publication directory still exists")
"#,
    )
    .expect("Windows cleanup handle-lifetime driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&parent)
        .arg(&private)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows cleanup handle-lifetime driver is runnable");
    assert!(
        output.status.success(),
        "Windows cleanup did not retain identity handles until delete\n{}",
        output_text(&output)
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_windows_cleanup_preserves_substituted_private_directory() {
    let temp = TempTree::new("checksum-r2-windows-cleanup-private-substitution");
    let parent = temp.path().join("external");
    fs::create_dir(&parent).expect("destination parent is created");
    let private = parent.join("r2publish-hayro-corpus-test");
    let saved_private = parent.join("saved-private-publication");
    let payload = private.join("payload");
    fs::create_dir(&private).expect("private publication directory is created");
    fs::create_dir(&payload).expect("private payload directory is created");
    fs::write(payload.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n")
        .expect("private payload file is written");

    let driver = temp.path().join("windows_cleanup_private_substitution.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
parent = pathlib.Path(sys.argv[2])
private = pathlib.Path(sys.argv[3])
saved_private = pathlib.Path(sys.argv[4])
payload = private / "payload"

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

private_handle, private_identity = module.open_windows_private_directory(private)
private_handle.close()
payload_handle, payload_identity = module.open_windows_private_directory(payload)
payload_handle.close()

os.rename(private, saved_private)
os.mkdir(private)
(private / "attacker.txt").write_text("attacker", encoding="utf-8")

message = module.cleanup_private_publication_windows(
    parent, private.name, private_identity, payload_identity
)
if message is None:
    raise SystemExit("cleanup deleted or accepted a substituted private directory")
if "private publication directory changed" not in message:
    raise SystemExit(f"unexpected cleanup diagnostic: {message}")
if (private / "attacker.txt").read_text(encoding="utf-8") != "attacker":
    raise SystemExit("substituted private directory contents were removed")
if not (saved_private / "payload" / "tiny.pdf").is_file():
    raise SystemExit("original private payload was not preserved")
"#,
    )
    .expect("Windows cleanup private-substitution driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&parent)
        .arg(&private)
        .arg(&saved_private)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows cleanup private-substitution driver is runnable");
    assert!(
        output.status.success(),
        "Windows cleanup did not preserve substituted private directory\n{}",
        output_text(&output)
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_rejects_a_competing_destination_at_windows_finalization() {
    let temp = TempTree::new("checksum-r2-windows-finalizer-competitor");
    let staging = temp.path().join("staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let external = temp.path().join("external");
    fs::create_dir(&external).expect("destination parent is created");
    let dest = external.join("hayro-corpus");

    let driver = temp.path().join("windows_finalizer_competitor.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
dest = pathlib.Path(sys.argv[3])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

original_finalize = module.finalize_payload_directory_windows

def racing_finalize(payload_handle, parent_handle, publish_dest):
    os.mkdir(publish_dest)
    (publish_dest / "competitor.txt").write_text("competitor", encoding="utf-8")
    return original_finalize(payload_handle, parent_handle, publish_dest)

module.finalize_payload_directory_windows = racing_finalize
try:
    module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
except module.R2Error as error:
    if "destination appeared before publication" not in str(error):
        raise SystemExit(f"unexpected R2 error: {error}")
else:
    raise SystemExit("publication replaced a competing destination")

if (dest / "competitor.txt").read_text(encoding="utf-8") != "competitor":
    raise SystemExit("competing destination contents were replaced")
leaks = [path.name for path in dest.parent.iterdir() if path.name.startswith("r2publish-")]
if leaks:
    raise SystemExit(f"private publication directory leaked after finalizer failure: {leaks}")
"#,
    )
    .expect("Windows finalizer competitor driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&dest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows finalizer competitor driver is runnable");
    assert!(
        output.status.success(),
        "Windows publication did not reject finalization-time competitor\n{}",
        output_text(&output)
    );
}

#[cfg(windows)]
#[test]
fn checksum_r2_helper_windows_staging_handle_denies_rename_during_publication() {
    let temp = TempTree::new("checksum-r2-windows-staging-rename-denial");
    let staging = temp.path().join("staging");
    let old_staging = temp.path().join("old-staging");
    fs::create_dir(&staging).expect("staging directory is created");
    fs::write(staging.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("staged PDF is written");
    let external = temp.path().join("external");
    fs::create_dir(&external).expect("destination parent is created");
    let dest = external.join("hayro-corpus");

    let driver = temp.path().join("windows_staging_rename_denial.py");
    fs::write(
        &driver,
        r#"
import importlib.util
import os
import pathlib
import sys

r2_path = pathlib.Path(sys.argv[1])
staging = pathlib.Path(sys.argv[2])
old_staging = pathlib.Path(sys.argv[3])
dest = pathlib.Path(sys.argv[4])

spec = importlib.util.spec_from_file_location("r2_under_test", r2_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

if not module.has_windows_handles():
    raise SystemExit("Windows handle backend is unavailable")

rename_denied = False
original_copy = module.copy_file_to_windows_path

def racing_copy(source, target):
    global rename_denied
    try:
        os.rename(staging, old_staging)
    except OSError:
        rename_denied = True
    else:
        os.rename(old_staging, staging)
        raise SystemExit("staging rename succeeded while publisher handle was held")
    return original_copy(source, target)

module.copy_file_to_windows_path = racing_copy
module.publish_staged_set(staging, dest, "test-source", "test-revision", None)
if not rename_denied:
    raise SystemExit("staging rename was not attempted")
if (dest / "tiny.pdf").read_bytes() != b"%PDF-1.4\n%tiny\n":
    raise SystemExit("published bytes changed during staging rename-denial proof")
"#,
    )
    .expect("Windows staging rename-denial driver is written");

    let output = Command::new(python_interpreter())
        .arg(&driver)
        .arg(workspace_root().join("corpus/r2.py"))
        .arg(&staging)
        .arg(&old_staging)
        .arg(&dest)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Windows staging rename-denial driver is runnable");
    assert!(
        output.status.success(),
        "Windows publisher staging handle did not deny rename\n{}",
        output_text(&output)
    );
}

#[cfg(unix)]
#[test]
fn checksum_fetch_rejects_malformed_r2_manifest_ids_before_invoking_curl() {
    for (name, contents, diagnostic) in [
        (
            "top-level-object",
            r#"{"id":"tiny"}"#,
            "top-level JSON array",
        ),
        ("non-string-id", r#"[{"id":7}]"#, "id is not a string"),
        ("traversal-id", r#"["../escape"]"#, "traversal"),
        ("slash-id", r#"["dir/file"]"#, "path separators"),
        ("control-id", r#"["line\nfeed"]"#, "control"),
        ("case-duplicate-id", r#"["Tiny","tiny"]"#, "duplicate"),
        ("reserved-id", r#"["CON"]"#, "reserved"),
    ] {
        let temp = TempTree::new(&format!("checksum-fetch-bad-manifest-{name}"));
        let corpus = temp.path().join("corpus");
        copy_corpus_tools(&corpus);
        let checksums = corpus.join("checksums");
        fs::create_dir_all(&checksums).expect("checksum directory is created");
        write_manifest(
            &checksums.join("hayro-corpus.sha256"),
            &[&format!("{TINY_PDF_SHA256}  tiny.pdf")],
        );
        write_hayro_manifest(&corpus, "corpus", contents);

        let bin = temp.path().join("bin");
        fs::create_dir(&bin).expect("fake bin directory is created");
        write_failing_fake_curl(&bin.join("curl"));
        let path = path_with_fake_bin(&bin);

        let output = run_fetch(&corpus, &path, "hayro-corpus");
        let text = output_text(&output);
        assert!(
            !output.status.success(),
            "invalid R2 manifest {name} unexpectedly passed\n{text}"
        );
        assert!(
            text.contains(diagnostic),
            "invalid R2 manifest {name} did not mention {diagnostic:?}\n{text}"
        );
        assert!(
            !text.contains("fake curl must not be invoked"),
            "invalid R2 manifest {name} reached curl\n{text}"
        );
    }
}

#[cfg(unix)]
#[test]
fn checksum_fetch_rejects_preexisting_r2_destination_symlink_before_invoking_curl() {
    let temp = TempTree::new("checksum-fetch-preexisting-symlink");
    let corpus = temp.path().join("corpus");
    copy_corpus_tools(&corpus);
    let checksums = corpus.join("checksums");
    fs::create_dir_all(&checksums).expect("checksum directory is created");
    write_manifest(
        &checksums.join("hayro-corpus.sha256"),
        &[&format!("{TINY_PDF_SHA256}  tiny.pdf")],
    );
    write_hayro_manifest(&corpus, "corpus", r#"[{"id":"tiny"}]"#);
    let redirect = temp.path().join("redirect");
    fs::create_dir(&redirect).expect("redirect target is created");
    let dest = corpus.join("external/hayro-corpus");
    std::os::unix::fs::symlink(&redirect, &dest).expect("destination symlink is created");

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    write_failing_fake_curl(&bin.join("curl"));
    let path = path_with_fake_bin(&bin);

    let output = run_fetch(&corpus, &path, "hayro-corpus");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "pre-existing symlink destination unexpectedly passed\n{text}"
    );
    assert!(text.contains("symlink not allowed"), "{text}");
    assert!(
        !text.contains("fake curl must not be invoked"),
        "pre-existing symlink destination reached curl\n{text}"
    );
}

#[cfg(unix)]
#[test]
fn checksum_fetch_rejects_symlinked_external_parent_before_invoking_curl() {
    let temp = TempTree::new("checksum-fetch-symlink-parent");
    let corpus = temp.path().join("corpus");
    copy_corpus_tools(&corpus);
    let redirect = temp.path().join("redirect");
    fs::create_dir(&redirect).expect("redirect parent target is created");
    std::os::unix::fs::symlink(&redirect, corpus.join("external"))
        .expect("external symlink is created");

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    write_failing_fake_curl(&bin.join("curl"));
    let path = path_with_fake_bin(&bin);

    let output = run_fetch(&corpus, &path, "hayro-corpus");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "symlinked external parent unexpectedly passed\n{text}"
    );
    assert!(text.contains("symlink not allowed"), "{text}");
    assert!(
        !text.contains("fake curl must not be invoked"),
        "symlinked external parent reached curl\n{text}"
    );
    assert_eq!(
        fs::read_dir(&redirect)
            .expect("redirect target is readable")
            .count(),
        0,
        "fetch wrote through the symlinked external parent"
    );
}

#[cfg(unix)]
#[test]
fn checksum_fetch_rejects_a_destination_swap_after_download_before_publication() {
    let temp = TempTree::new("checksum-fetch-publication-swap");
    let corpus = temp.path().join("corpus");
    copy_corpus_tools(&corpus);
    let checksums = corpus.join("checksums");
    fs::create_dir_all(&checksums).expect("checksum directory is created");
    write_manifest(
        &checksums.join("hayro-corpus.sha256"),
        &[&format!("{TINY_PDF_SHA256}  tiny.pdf")],
    );
    write_hayro_manifest(&corpus, "corpus", r#"[{"id":"tiny"}]"#);

    let redirect = temp.path().join("redirect");
    fs::create_dir(&redirect).expect("redirect target is created");
    fs::write(redirect.join("tiny.pdf"), b"unchanged\n").expect("redirect sentinel is written");
    let dest = corpus.join("external/hayro-corpus");

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    write_destination_swap_curl(&bin.join("curl"));
    let path = path_with_fake_bin(&bin);

    let output = Command::new("bash")
        .arg(corpus.join("fetch.sh"))
        .arg("hayro-corpus")
        .env("PATH", &path)
        .env("PYTHON", python_interpreter())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("ONIONSKIN_DEST_TO_SWAP", &dest)
        .env("ONIONSKIN_SWAP_TARGET", &redirect)
        .output()
        .expect("copied fetch.sh is runnable for the destination-swap proof");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "post-download destination swap unexpectedly passed\n{text}"
    );
    assert!(text.contains("symlink not allowed"), "{text}");
    assert_eq!(
        fs::read(redirect.join("tiny.pdf")).expect("redirect sentinel is readable"),
        b"unchanged\n",
        "publication followed the swapped destination symlink"
    );
}

#[cfg(unix)]
#[test]
fn checksum_fetch_downloads_checked_hayro_corpus_through_staging() {
    let temp = TempTree::new("checksum-fetch-checked-r2");
    let corpus = temp.path().join("corpus");
    copy_corpus_tools(&corpus);
    let checksums = corpus.join("checksums");
    fs::create_dir_all(&checksums).expect("checksum directory is created");
    write_manifest(
        &checksums.join("hayro-corpus.sha256"),
        &[&format!("{TINY_PDF_SHA256}  tiny.pdf")],
    );
    write_hayro_manifest(&corpus, "corpus", r#"[{"id":"tiny"}]"#);

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    write_pdf_curl(&bin.join("curl"));
    let path = path_with_fake_bin(&bin);

    let output = run_fetch(&corpus, &path, "hayro-corpus");
    let text = output_text(&output);
    assert!(
        output.status.success(),
        "checked R2 corpus fetch failed\n{text}"
    );
    assert!(
        text.contains("verified 1 files"),
        "checked R2 corpus fetch did not verify staged bytes\n{text}"
    );
    let dest = corpus.join("external/hayro-corpus");
    assert_eq!(
        fs::read(dest.join("tiny.pdf")).expect("published checked PDF is readable"),
        b"%PDF-1.4\n%tiny\n"
    );
    assert!(
        dest.join(".fetch-stamp").is_file(),
        "checked R2 corpus fetch did not write a completion stamp"
    );
}

#[cfg(unix)]
#[test]
fn checksum_fetch_downloads_unchecked_optional_sets_through_staging() {
    let temp = TempTree::new("checksum-fetch-unchecked-r2");
    let corpus = temp.path().join("corpus");
    copy_corpus_tools(&corpus);
    write_hayro_manifest(&corpus, "pdfjs", r#"[{"id":"tiny"}]"#);

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    write_pdf_curl(&bin.join("curl"));
    let path = path_with_fake_bin(&bin);

    let output = run_fetch(&corpus, &path, "hayro-pdfjs");
    let text = output_text(&output);
    assert!(
        output.status.success(),
        "unchecked optional R2 fetch failed\n{text}"
    );
    assert!(
        text.contains("fetched without checksum enforcement"),
        "unchecked optional R2 fetch did not report checksum absence\n{text}"
    );
    let dest = corpus.join("external/hayro-pdfjs");
    assert_eq!(
        fs::read(dest.join("tiny.pdf")).expect("published unchecked PDF is readable"),
        b"%PDF-1.4\n%tiny\n"
    );
    assert!(
        dest.join(".fetch-stamp").is_file(),
        "unchecked optional R2 fetch did not write a completion stamp"
    );
}

#[cfg(unix)]
#[test]
fn checksum_fetch_validates_a_stamped_cached_hayro_corpus_without_invoking_curl() {
    let temp = TempTree::new("checksum-fetch-cache");
    let corpus = temp.path().join("corpus");
    let checksums = corpus.join("checksums");
    let root = corpus.join("external/hayro-corpus");
    copy_corpus_tools(&corpus);
    fs::create_dir_all(&checksums).expect("checksum directory is created");
    fs::create_dir_all(&root).expect("cached corpus directory is created");

    fs::write(root.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("cached PDF is written");
    write_fetch_stamp(&root);
    write_manifest(
        &checksums.join("hayro-corpus.sha256"),
        &[&format!("{TINY_PDF_SHA256}  tiny.pdf")],
    );

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    let fake_curl = bin.join("curl");
    fs::write(
        &fake_curl,
        "#!/bin/sh\nprintf '%s\\n' 'fake curl must not be invoked' >&2\nexit 42\n",
    )
    .expect("fake curl is written");
    make_executable(&fake_curl);

    let path = path_with_fake_bin(&bin);

    let workspace_corpus = workspace_root().join("corpus");
    assert_ne!(
        corpus, workspace_corpus,
        "the cached-fetch proof must use a copied corpus tree"
    );
    let output = Command::new("bash")
        .arg(corpus.join("fetch.sh"))
        .arg("hayro-corpus")
        .env("PATH", &path)
        .env("PYTHON", python_interpreter())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("copied fetch.sh is runnable through bash");
    let text = output_text(&output);
    assert!(
        output.status.success(),
        "stamped valid cache should be verified and skipped without curl\n{}",
        text
    );
    assert!(
        text.contains("verified 1 files"),
        "stamped cache did not report checksum verification\n{text}"
    );
    assert!(
        !text.contains(&workspace_corpus.display().to_string()),
        "copied fetch.sh output unexpectedly referenced the retained corpus\n{text}"
    );

    fs::write(root.join("tiny.pdf"), b"changed after stamp\n")
        .expect("cached PDF is corrupted after the valid proof");
    let corrupt = Command::new("bash")
        .arg(corpus.join("fetch.sh"))
        .arg("hayro-corpus")
        .env("PATH", &path)
        .env("PYTHON", python_interpreter())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("copied fetch.sh is runnable for the corrupted-cache proof");
    let corrupt_text = output_text(&corrupt);
    assert!(
        !corrupt.status.success(),
        "stamped corrupted cache unexpectedly passed\n{corrupt_text}"
    );
    assert!(
        corrupt_text.contains("sha256 mismatch"),
        "stamped corrupted cache did not report its checksum mismatch\n{corrupt_text}"
    );
    assert!(
        !corrupt_text.contains("fake curl must not be invoked"),
        "corrupted cached validation reached the network path\n{corrupt_text}"
    );

    fs::write(root.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n")
        .expect("cached PDF is restored for the interpreter proof");
    let fake_python = bin.join("python-noop");
    fs::write(&fake_python, "#!/bin/sh\nexit 0\n").expect("fake Python is written");
    make_executable(&fake_python);
    let no_python = Command::new("bash")
        .arg(corpus.join("fetch.sh"))
        .arg("hayro-corpus")
        .env("PATH", &path)
        .env("PYTHON", &fake_python)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("copied fetch.sh is runnable for the fake-interpreter proof");
    let no_python_text = output_text(&no_python);
    assert!(
        !no_python.status.success(),
        "no-op executable bypassed Python validation\n{no_python_text}"
    );
    assert!(
        no_python_text.contains("not a runnable Python 3.10+ interpreter"),
        "fake interpreter failure was not explicit\n{no_python_text}"
    );
}

#[test]
fn acrobat_parity_headline_matches_every_inventory_row() {
    let parity = std::fs::read_to_string(workspace_root().join("ACROBAT-PARITY.md"))
        .expect("the Acrobat parity matrix is readable");
    let mut statuses = std::collections::BTreeMap::new();
    let mut milestones = std::collections::BTreeMap::new();
    let mut in_inventory = false;
    let mut rows = 0;

    for line in parity.lines() {
        if line == "| Item | Status | Milestone | Notes |" {
            in_inventory = true;
            continue;
        }
        if !in_inventory {
            continue;
        }
        if line == "|---|---|---|---|" {
            continue;
        }
        let Some(row) = line
            .strip_prefix('|')
            .and_then(|line| line.strip_suffix('|'))
        else {
            in_inventory = false;
            continue;
        };
        let columns = row.split('|').map(str::trim).collect::<Vec<_>>();
        assert_eq!(
            columns.len(),
            4,
            "Acrobat inventory row has {} columns instead of 4: {line}",
            columns.len()
        );
        let status = columns[1];
        assert!(
            matches!(
                status,
                "implemented" | "planned" | "partial" | "out-of-scope"
            ),
            "unknown Acrobat parity status {status:?} in row: {line}"
        );
        let milestone = columns[2];
        assert!(
            matches!(
                milestone,
                "M0" | "M1" | "M2" | "M3" | "M4" | "M5" | "M6" | "post-1.0" | "-"
            ),
            "unknown Acrobat parity milestone {milestone:?} in row: {line}"
        );
        if status == "out-of-scope" {
            assert_eq!(
                milestone, "-",
                "out-of-scope row must not claim a milestone: {line}"
            );
        } else {
            assert_ne!(
                milestone, "-",
                "parity-target row needs a milestone: {line}"
            );
            *milestones.entry(milestone).or_insert(0usize) += 1;
        }
        *statuses.entry(status).or_insert(0usize) += 1;
        rows += 1;
    }

    assert_eq!(
        rows, 403,
        "the Acrobat inventory denominator changed; reconcile it against a dated reference before updating this contract"
    );
    let count = |status| statuses.get(status).copied().unwrap_or_default();
    let headline = format!(
        "**{rows} rows: {} planned / {} partial / {} out-of-scope. {} implemented.**",
        count("planned"),
        count("partial"),
        count("out-of-scope"),
        count("implemented")
    );
    assert!(
        parity.contains(&headline),
        "Acrobat parity headline does not match its rows; expected {headline:?}"
    );

    let target_rows = rows - count("out-of-scope");
    let target =
        format!("{target_rows} rows (implemented plus planned and partial) are the parity target.");
    assert!(
        parity.contains(&target),
        "Acrobat parity target total does not match its rows; expected {target:?}"
    );

    let milestone_summary = ["M0", "M1", "M2", "M3", "M4", "M5", "M6", "post-1.0"]
        .into_iter()
        .filter_map(|milestone| {
            milestones
                .get(milestone)
                .map(|count| format!("{milestone} {count}"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        parity.contains(&format!("By milestone: {milestone_summary}.")),
        "Acrobat parity milestone totals do not match its rows: {milestone_summary}"
    );
}

fn workspace_root() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/app sits two levels under the workspace root")
}

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "onionskin-guarantee-{name}-{}-{nanos}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap_or_else(|error| {
            panic!(
                "failed to create test temp directory {}: {error}",
                path.display()
            )
        });
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let Some(name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if name.starts_with("onionskin-guarantee-") {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn assert_checksum_failure(
    name: &str,
    expected_diagnostic: &str,
    build: impl FnOnce(&Path, &Path),
) {
    let temp = TempTree::new(name);
    let root = temp.path().join("root");
    fs::create_dir(&root).expect("test root is created");
    let manifest = temp.path().join("manifest.sha256");
    build(&manifest, &root);

    let output = run_checksum_verifier(&manifest, &root);
    assert!(
        !output.status.success(),
        "invalid checksum case {name} unexpectedly succeeded"
    );
    let text = output_text(&output);
    assert!(
        text.contains(expected_diagnostic),
        "invalid checksum case {name} did not mention {expected_diagnostic:?}\n{text}"
    );
}

fn copy_corpus_tools(corpus: &Path) {
    fs::create_dir_all(corpus).expect("copied corpus directory is created");
    for tool in ["fetch.sh", "verify-sha256.py", "r2.py"] {
        let source = workspace_root().join("corpus").join(tool);
        assert!(
            source.is_file(),
            "corpus helper missing from workspace: {}",
            source.display()
        );
        fs::copy(&source, corpus.join(tool))
            .unwrap_or_else(|error| panic!("failed to copy {}: {error}", source.display()));
    }
}

fn write_fetch_stamp(root: &Path) {
    fs::write(root.join(".fetch-stamp"), "source=test\nrevision=test\n")
        .expect("fetch stamp is written");
}

fn write_hayro_manifest(corpus: &Path, kind: &str, contents: &str) {
    let hayro = corpus.join("external/hayro");
    fs::create_dir_all(&hayro).expect("hayro manifest directory is created");
    write_fetch_stamp(&hayro);
    fs::write(hayro.join(format!("manifest_{kind}.json")), contents)
        .expect("hayro R2 manifest is written");
}

fn write_manifest(path: &Path, lines: &[&str]) {
    fs::write(path, format!("{}\n", lines.join("\n"))).expect("checksum manifest is written");
}

fn run_checksum_verifier(manifest: &Path, root: &Path) -> Output {
    let verifier = workspace_root().join("corpus/verify-sha256.py");
    assert!(
        verifier.is_file(),
        "checksum verifier missing: {}",
        verifier.display()
    );
    Command::new(python_interpreter())
        .arg(&verifier)
        .arg(manifest)
        .arg(root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("checksum verifier process is spawned")
}

fn run_r2_helper(args: &[&OsStr]) -> Output {
    let helper = workspace_root().join("corpus/r2.py");
    assert!(helper.is_file(), "R2 helper missing: {}", helper.display());
    Command::new(python_interpreter())
        .arg(&helper)
        .args(args)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("R2 helper process is spawned")
}

#[cfg(unix)]
fn run_fetch(corpus: &Path, path: &OsString, set: &str) -> Output {
    Command::new("bash")
        .arg(corpus.join("fetch.sh"))
        .arg(set)
        .env("PATH", path)
        .env("PYTHON", python_interpreter())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("copied fetch.sh is runnable through bash")
}

#[cfg(unix)]
fn path_with_fake_bin(bin: &Path) -> OsString {
    let old_path = env::var_os("PATH").unwrap_or_default();
    let mut path = bin.as_os_str().to_os_string();
    path.push(":");
    path.push(old_path);
    path
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata is readable")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(path, permissions).expect("fake executable is made executable");
}

#[cfg(unix)]
fn write_failing_fake_curl(path: &Path) {
    fs::write(
        path,
        "#!/bin/sh\nprintf '%s\\n' 'fake curl must not be invoked' >&2\nexit 42\n",
    )
    .expect("failing fake curl is written");
    make_executable(path);
}

#[cfg(unix)]
fn write_pdf_curl(path: &Path) {
    fs::write(
        path,
        r#"#!/bin/sh
output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            shift
            output="$1"
            ;;
    esac
    shift || exit 64
done
if [ -z "$output" ]; then
    printf '%s\n' 'missing --output' >&2
    exit 64
fi
printf '%s' '%PDF-1.4
%tiny
' > "$output"
"#,
    )
    .expect("PDF fake curl is written");
    make_executable(path);
}

#[cfg(unix)]
fn write_destination_swap_curl(path: &Path) {
    fs::write(
        path,
        r#"#!/bin/sh
output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            shift
            output="$1"
            ;;
    esac
    shift || exit 64
done
if [ -z "$output" ]; then
    printf '%s\n' 'missing --output' >&2
    exit 64
fi
ln -s "$ONIONSKIN_SWAP_TARGET" "$ONIONSKIN_DEST_TO_SWAP" 2>/dev/null || true
printf '%s' '%PDF-1.4
%tiny
' > "$output"
"#,
    )
    .expect("destination-swap fake curl is written");
    make_executable(path);
}

#[cfg(windows)]
fn create_windows_reparse_for_file_role(link: &Path, target_file: &Path) {
    let file_link = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg(link)
        .arg(target_file)
        .output()
        .expect("Windows file symlink command is runnable");
    if file_link.status.success() {
        return;
    }

    let link_name = link
        .file_name()
        .expect("Windows reparse test link has a file name")
        .to_string_lossy();
    let target_dir = target_file
        .parent()
        .expect("Windows reparse test target has a parent")
        .join(format!("{link_name}.junction-target"));
    fs::create_dir(&target_dir).expect("junction target is created");
    create_windows_junction(link, &target_dir);
}

#[cfg(windows)]
fn create_windows_junction(link: &Path, target: &Path) {
    let created = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .expect("Windows junction command is runnable");
    assert!(
        created.status.success(),
        "Windows junction creation failed\n{}",
        output_text(&created)
    );
}

fn python_interpreter() -> OsString {
    if let Some(candidate) = env::var_os("PYTHON") {
        if interpreter_runs(&candidate) {
            return candidate;
        }
        panic!(
            "PYTHON is not a runnable Python 3.10+ interpreter: {}",
            candidate.to_string_lossy()
        );
    }

    for candidate in ["python3", "python"] {
        let candidate = OsString::from(candidate);
        if interpreter_runs(&candidate) {
            return candidate;
        }
    }

    panic!("required Python 3.10+ interpreter not found (tried PYTHON, python3, python)");
}

fn interpreter_runs(candidate: &OsString) -> bool {
    Command::new(candidate)
        .args([
            "-c",
            "import sys; sys.stdout.write('onionskin-python') if sys.version_info >= (3, 10) else sys.exit(1)",
        ])
        .output()
        .is_ok_and(|output| output.status.success() && output.stdout == b"onionskin-python")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// One top-level job of a workflow: its header line and everything indented
/// under it. Scoped, so that what another job is allowed to do stays that
/// job's business.
fn job(workflow: &str, name: &str) -> String {
    let header = format!("  {name}:");
    workflow
        .lines()
        .skip_while(|line| *line != header)
        .take_while(|line| *line == header || line.starts_with("   ") || line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The source of the `crates/cos` test file that enforces guarantee `number`,
/// having proved the file still claims that guarantee and still runs each
/// named test as a plain `#[test]`. An `#[ignore]` or a rename between the
/// attribute and the signature breaks the match, which is the point: a pointer
/// guarantee is worth exactly as much as the pointer staying true.
fn enforcing_suite(file: &str, number: u8, tests: &[&str]) -> String {
    let path = workspace_root().join("crates/cos/tests").join(file);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is unreadable ({error}), so guarantee {number} is unchecked",
            path.display()
        )
    });
    assert!(
        source.starts_with(&format!("//! Guarantee test {number}:")),
        "{} no longer claims guarantee {number}",
        path.display()
    );
    for name in tests {
        assert!(
            source.contains(&format!("\n#[test]\nfn {name}(")),
            "{} no longer runs {name} as a plain #[test], so guarantee {number} is unchecked",
            path.display()
        );
    }
    source
}

/// Guarantees 1, 2 and 6 are enforced in `crates/cos`, which only helps if CI
/// runs that crate's tests. It reaches them through the workspace suite, so
/// both the command and the membership have to hold.
fn assert_ci_reaches_the_cos_suite() {
    let workspace = workspace_root();
    let ci = std::fs::read_to_string(workspace.join(".github/workflows/ci.yml"))
        .expect("the CI workflow is readable");
    assert!(
        job(&ci, "test").contains("- run: cargo test --workspace"),
        "CI no longer runs the workspace test suite, so the cos guarantees run nowhere"
    );
    let manifest = std::fs::read_to_string(workspace.join("Cargo.toml"))
        .expect("the workspace manifest is readable");
    assert!(
        manifest.contains("\"crates/cos\","),
        "crates/cos is not a workspace member, so `cargo test --workspace` does not reach it"
    );
}

/// One section of a TOML document with its comments removed. Stripping them is
/// what makes an assertion about this text an assertion about the setting: a
/// commented-out `yanked = "deny"` reads identically to the real thing, and a
/// policy that keeps the reviewed settings in comments and permissive ones in
/// force passes `cargo deny check` with all four sections green.
fn toml_section(document: &str, name: &str) -> String {
    let header = format!("[{name}]");
    document
        .lines()
        .skip_while(|line| line.trim() != header)
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .map(without_comment)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A TOML line up to its first comment marker, leaving `#` inside a string
/// alone.
fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, byte) in line.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b'#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

/// The value of `key` in `section`, as cargo-deny would read it: comments
/// gone, and a value spread over several lines joined onto one so an array can
/// be compared whole. `None` when the key is absent, which is a distinct
/// answer from an empty value.
fn policy_setting(document: &str, section: &str, key: &str) -> Option<String> {
    let body = toml_section(document, section);
    let mut lines = body.lines().map(str::trim);
    let first = lines.find_map(|line| line.strip_prefix(key)?.trim_start().strip_prefix('='))?;

    let mut value = first.trim().to_owned();
    while unbalanced(&value) {
        let next = lines.next()?.trim();
        if next.is_empty() {
            continue;
        }
        if next.starts_with(']') {
            // TOML allows a trailing comma before the bracket; a joined value
            // that keeps it would not compare equal to the array it means.
            while value.ends_with(',') {
                value.pop();
            }
        } else if !value.ends_with('[') {
            value.push(' ');
        }
        value.push_str(next);
    }
    Some(value)
}

/// Whether a value has an array or table still open, so the next line belongs
/// to it.
fn unbalanced(value: &str) -> bool {
    let opens = value.chars().filter(|c| *c == '[' || *c == '{').count();
    let closes = value.chars().filter(|c| *c == ']' || *c == '}').count();
    opens > closes
}
