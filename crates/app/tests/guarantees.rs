//! PLAN.md's guarantee tests, the spec as executable checks. A guarantee whose
//! capability has landed either runs here or names the test that enforces it
//! (guarantee 5 in `kernel_emptiness.rs`, 9 in `crates/core/benches/`, 1, 2 and
//! 6 in `crates/cos/tests/`). One whose capability has not landed stays
//! `#[ignore]`d, and its reason names the milestone PLAN.md gives it.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, fs, process};

use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, Token};
use yaml_rust2::{Yaml, YamlLoader};

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
    // Every test in roundtrip.rs, so the sentence's "for every corpus file" is
    // not carried by the seeds alone: the external-corpus walks have pass
    // floors of their own and could otherwise be deleted with this green.
    let suite = enforcing_suite(
        "roundtrip.rs",
        1,
        &[
            "seeds_round_trip_exactly",
            "pdf20examples_corpus_round_trips",
            "pdf_association_corpus_round_trips",
            "verapdf_corpus_round_trips",
            "hayro_custom_corpus_round_trips",
            "hayro_other_corpus_round_trips",
            "hayro_regression_corpus_round_trips",
        ],
    );
    suite.invokes(
        "document.incremental_section",
        "a no-op save is no longer asked what it appended",
    );
    suite.asserts(
        "non-empty-noop-save",
        "a no-op save that appended bytes no longer fails",
    );
    suite.invokes(
        ".save_to_vec",
        "nothing is saved, so byte-identity is no longer compared",
    );
    // The floors are what stops an external corpus walk reporting a pass for
    // a set it silently stopped covering.
    suite.asserts(
        "below the {floor_percent}% floor",
        "the external corpus walks no longer hold a floor on their pass counts",
    );
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
    let suite = enforcing_suite(
        "incremental.rs",
        2,
        &[
            "editing_a_seed_appends_exactly_one_section",
            "a_second_edit_appends_a_second_section",
        ],
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
            "truncating the section must undo the edit",
            "the roll-back half of the sentence is no longer checked",
        ),
    ] {
        suite.asserts(marker, missing);
    }
    suite.invokes(
        "document.original_len",
        "the truncation point is no longer the one the document reports",
    );
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
    // "Opens" and "repairs" are two clauses and two tests. Naming only the
    // repair walk would let the refusal case be deleted with this green.
    let suite = enforcing_suite(
        "repair.rs",
        6,
        &[
            "open_refuses_a_damaged_file_instead_of_opening_it_quietly",
            "every_malformed_file_repairs_and_saves_over_intact_original_bytes",
            "damaged_files_in_the_external_corpora_repair_the_same_way",
        ],
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
        suite.asserts(marker, missing);
    }
    assert_ci_reaches_the_cos_suite();

    // repair.rs returns early when corpus/malformed is absent, and the set is
    // gitignored, so the enforcing test is only worth anything on a runner
    // that generated it. Generating it is not enough either: with
    // ONIONSKIN_CORPUS_REQUIRED the rerun turns a skip into a failure, which
    // is what makes disabling the generation step above visible.
    let ci = workflow("ci.yml");
    let steps = job_steps(&ci, "test");
    let one = |command: &str, absent: &str| {
        let found = steps
            .iter()
            .filter(|step| field(step, "run").as_deref().map(str::trim) == Some(command))
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 1, "{absent}");
        assert_reviewed_keys(
            found[0],
            &["name", "if", "run", "env"],
            &format!("the `{command}` step"),
        );
        // Both are pinned to Linux, and the guarantee says so: the repair path
        // is byte manipulation with no platform dimension, and the generator
        // wants bash and perl. Pinned to the value rather than merely allowed,
        // so `if: false` is not a way to switch either one off.
        assert_eq!(
            field(found[0], "if").as_deref(),
            Some("runner.os == 'Linux'"),
            "the `{command}` step runs on a different set of runners than the other half of this gate"
        );
        found[0]
    };
    one(
        "./corpus/make-malformed.sh",
        "CI never generates corpus/malformed, so repair.rs skips it and guarantee 6 passes unmeasured",
    );
    let prove = one(
        "cargo test -p onionskin-cos --test repair",
        "CI never reruns the repair suite with the corpus made mandatory, so a skipped guarantee 6 still reports a pass",
    );
    assert_eq!(
        field(&prove["env"], "ONIONSKIN_CORPUS_REQUIRED").as_deref(),
        Some("1"),
        "the repair rerun still lets an absent corpus report a pass"
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

    let ci_text = std::fs::read_to_string(workspace.join(".github/workflows/ci.yml"))
        .expect("the workflow is readable");
    assert!(
        ci_text.contains("pull_request"),
        "CI does not run on pull requests, so no budget is checked before a merge"
    );
    let ci = workflow("ci.yml");
    let bench = job_of(&ci, "bench");
    let bench_steps = job_steps(&ci, "bench");
    // The whole script, so `cargo bench -p onionskin-core || true` is not the
    // gate it reads as.
    gate_step(&bench_steps, "cargo bench -p onionskin-core");
    assert_eq!(
        field(&bench["env"], "ONIONSKIN_CORPUS_REQUIRED").as_deref(),
        Some("1"),
        "the bench job does not require its corpus, so it would skip every budget and pass"
    );
    assert_eq!(
        field(bench, "continue-on-error"),
        None,
        "a job allowed to fail is not a gate"
    );
    assert_eq!(
        field(bench, "if"),
        None,
        "the bench job is conditional, so there are pushes it does not gate"
    );
    let cache = bench_steps
        .iter()
        .find(|step| action_of(step).is_some_and(|action| action.starts_with("actions/cache@")))
        .expect("the bench job does not cache the corpus");
    assert!(
        field(&cache["with"], "key").is_some_and(|key| key.contains(
            "hashFiles('corpus/fetch.sh', 'corpus/verify-sha256.py', 'corpus/r2.py', 'corpus/checksums/hayro-corpus.sha256')"
        )),
        "the bench corpus cache key does not include the fetch helpers and checksum manifest"
    );

    for name in ["bench", "test", "shell"] {
        assert!(
            job_steps(&ci, name).iter().any(|step| action_of(step)
                .is_some_and(|action| action.starts_with("actions/setup-python@"))),
            "the {name} job runs corpus or guarantee work but does not install Python"
        );
    }
}

#[test]
fn release_artifacts_build_the_windowed_viewer() {
    let ci = workflow("ci.yml");
    let release = workflow("release.yml");
    let build = job_of(&release, "build");
    assert_eq!(
        field(&build["env"], "CARGO_NET_GIT_FETCH_WITH_CLI").as_deref(),
        Some("true"),
        "release builds fetch git dependencies differently from CI"
    );

    // Every platform the release publishes for. Deleting a leg stops shipping
    // it silently, while deny.toml goes on scanning four triples.
    let matrix = build["strategy"]["matrix"]["include"]
        .as_vec()
        .expect("the release build job declares no matrix");
    let legs = matrix
        .iter()
        .map(|leg| {
            (
                field(leg, "os").unwrap_or_default(),
                field(leg, "artifact").unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        legs,
        vec![
            (
                "ubuntu-latest".to_owned(),
                "onionskin-linux-x86_64".to_owned()
            ),
            (
                "ubuntu-24.04-arm".to_owned(),
                "onionskin-linux-aarch64".to_owned()
            ),
            (
                "macos-latest".to_owned(),
                "onionskin-macos-arm64".to_owned()
            ),
            (
                "windows-latest".to_owned(),
                "onionskin-windows-x86_64".to_owned()
            ),
        ],
        "the set of platforms the release publishes for changed"
    );

    let build_steps = job_steps(&release, "build");
    let shell_steps = job_steps(&ci, "shell");

    // The one step that makes the artifact a viewer. `gate_step` refuses a
    // conditioned, wrapped or redirected version of it: the script has to be
    // that command and nothing else, on every leg of the matrix, or some
    // platform publishes an artifact it never built.
    gate_step(
        &build_steps,
        "cargo build --release -p onionskin-app --features shell",
    );

    // ...and no other step may build the app without the feature. Both
    // packaging scripts ship whatever sits at target/release/onionskin, so a
    // second build anywhere in this job replaces the viewer with a binary that
    // opens no window while the step above still reads correctly. Word-wise,
    // because `cargo b` and `--bin onionskin` reach the same path.
    for step in &build_steps {
        let Some(run) = field(step, "run") else {
            continue;
        };
        for command in script_lines(&run) {
            assert!(
                !builds_the_app(&command) || command.contains("--features shell"),
                "a release step builds the app without the shell feature, overwriting the binary the packaging scripts ship: {command}"
            );
        }
    }

    assert!(
        build_steps.iter().any(|step| action_of(step).as_deref()
            == Some("actions/setup-python@e797f83bcb11b83ae66e0230d6156d7c80228e7c")),
        "release builds do not install the reviewed Python setup action even though the packaging version gate uses shell helpers"
    );

    // Every prerequisite the shell build needs, mirrored from the job known to
    // build it. Compared as whole steps, so a mirrored step that grew an `if:`
    // or lost half its command no longer counts as mirrored.
    for name in [
        "Update Linux package index",
        "Install GPUI Linux dependencies",
        "Install macOS Metal toolchain",
    ] {
        let named = |steps: &[&Yaml], job: &str| {
            let found = steps
                .iter()
                .filter(|step| field(step, "name").as_deref() == Some(name))
                .collect::<Vec<_>>();
            assert_eq!(
                found.len(),
                1,
                "the {job} job does not have exactly one {name} step"
            );
            (
                field(found[0], "run"),
                field(found[0], "if"),
                field(found[0], "continue-on-error"),
            )
        };
        assert_eq!(
            named(&shell_steps, "CI shell"),
            named(&build_steps, "release build"),
            "the release build job does not mirror the CI shell prerequisite step {name}"
        );
    }
}

#[test]
fn accessibility_probe_is_a_required_ci_gate() {
    let ci = workflow("ci.yml");
    let shell = job_steps(&ci, "shell");
    let probe = shell
        .iter()
        .filter(|step| {
            field(step, "run").as_deref().map(str::trim)
                == Some("cargo test -p onionskin-app --features a11y-probe --test a11y_probe")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        probe.len(),
        1,
        "the shell job does not run the macOS accessibility probe as a step of its own"
    );
    // Scoped to macOS on purpose: it opens a real window and reads the tree
    // back off an NSView. That is the one `if:` a gate here is allowed, so it
    // is pinned to the value rather than merely permitted.
    assert_eq!(
        field(probe[0], "if").as_deref(),
        Some("runner.os == 'macOS'"),
        "the accessibility probe is not scoped to macOS"
    );
    assert_reviewed_keys(probe[0], &["name", "if", "run"], "the accessibility probe");
}

#[test]
fn supply_chain_policy_is_checked_in_and_gated() {
    let workspace = workspace_root();
    let ci_text = std::fs::read_to_string(workspace.join(".github/workflows/ci.yml"))
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

    // The exact set of tables the policy declares. `toml_section` stops at the
    // next `[`, so a sub-table such as `[[licenses.clarify]]` would be
    // invisible to the key-set pins below while cargo-deny obeys it, which is
    // enough to relabel a crate's license past the allow-list with the
    // guarantee and cargo-deny both green.
    assert_eq!(
        policy_sections(&deny),
        vec![
            "[advisories]",
            "[bans]",
            "[graph]",
            "[licenses]",
            "[sources]"
        ],
        "the set of tables the cargo-deny policy declares changed"
    );

    // The exact set of keys each section declares. Pinning values alone leaves
    // every knob nobody named invisible, and cargo-deny has plenty: a
    // `[graph] exclude` mutes a crate out of the scan so its advisory can be
    // deleted with nothing failing, `disable-yank-checking` sits beside an
    // untouched `yanked = "deny"`, `[sources] allow-org` trusts a whole GitHub
    // organisation, and a quoted `"unmaintained"` key defeats an
    // absent-key assertion while deserializing normally. cargo-deny rejects
    // keys it does not know, so pinning the set closes all of them at once.
    for (section, keys) in [
        ("graph", &["all-features", "targets"][..]),
        ("advisories", &["ignore", "version", "yanked"][..]),
        (
            "licenses",
            &["allow", "confidence-threshold", "exceptions"][..],
        ),
        (
            "bans",
            &[
                "allow",
                "allow-wildcard-paths",
                "deny",
                "highlight",
                "multiple-versions",
                "skip",
                "skip-tree",
                "wildcards",
            ][..],
        ),
        (
            "sources",
            &[
                "allow-git",
                "allow-registry",
                "required-git-spec",
                "unknown-git",
                "unknown-registry",
            ][..],
        ),
    ] {
        assert_eq!(
            policy_keys(&deny, section),
            keys,
            "the set of settings [{section}] declares changed"
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
        ("bans", "wildcards", "\"deny\""),
        // The wildcard ban only means anything once the workspace's own path
        // dependencies stop reading as wildcards, which is what `publish =
        // false` buys. Without it the check fails on our own crates, and the
        // way out of that is to stop denying wildcards at all.
        ("bans", "allow-wildcard-paths", "true"),
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
    // Floors rather than equalities, so tightening the policy later is not a
    // test failure. Duplicate versions are a warning today because gpui brings
    // 66 of them; the day they are cleaned up, "deny" must not fail here.
    assert!(
        matches!(
            policy_setting(&deny, "bans", "multiple-versions").as_deref(),
            Some("\"warn\"") | Some("\"deny\"")
        ),
        "duplicate crate versions are not reported at all"
    );
    let confidence = policy_setting(&deny, "licenses", "confidence-threshold")
        .and_then(|value| value.parse::<f64>().ok())
        .expect("the policy states a license confidence threshold");
    assert!(
        confidence >= 0.8,
        "license detection confidence is below the reviewed floor: {confidence}"
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

    let ci = workflow("ci.yml");
    let supply_chain = job_of(&ci, "supply-chain");
    let steps = job_steps(&ci, "supply-chain");
    assert_eq!(
        field(&supply_chain["env"], "CARGO_NET_GIT_FETCH_WITH_CLI").as_deref(),
        Some("true"),
        "the supply-chain job fetches git dependencies differently from the rest of CI"
    );
    assert_eq!(
        field(supply_chain, "if"),
        None,
        "the supply-chain job is conditional, so some events skip it"
    );
    assert_eq!(
        field(supply_chain, "continue-on-error"),
        None,
        "supply-chain policy is advisory instead of gating"
    );

    // The job's steps, pinned whole and in order. Pinning only the steps that
    // must be present leaves room for ones that must not: a step that rewrites
    // deny.toml before cargo-deny reads it and restores it after, and nothing
    // named here would notice.
    assert_eq!(
        steps
            .iter()
            .map(|step| action_of(step)
                .or_else(|| field(step, "run"))
                .unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "actions/checkout@1af3b93b6815bc44a9784bd300feb67ff0d1eeb3".to_owned(),
            "dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c".to_owned(),
            "Swatinem/rust-cache@e172ef532f714507ca8b9ce7978a442736438fc1".to_owned(),
            "EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25".to_owned(),
            "cargo test -p onionskin-app --test guarantees -- --exact supply_chain_policy_is_checked_in_and_gated".to_owned(),
            GITLEAKS_INSTALL.to_owned(),
            "./gitleaks detect --redact --source .".to_owned(),
        ],
        "the supply-chain job's steps are no longer exactly the reviewed ones, in the reviewed order"
    );
    for step in &steps {
        let Some(action) = action_of(step) else {
            continue;
        };
        let sha = action
            .split_once('@')
            .map(|(_, pin)| pin)
            .unwrap_or_default();
        assert!(
            sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "supply-chain action is not pinned to a full commit SHA: {action}"
        );
    }

    // The cargo-deny invocation itself. The action builds its command line as
    // `cargo-deny --manifest-path X <arguments> <command> <command-arguments>`,
    // so `arguments: --config other.toml` points it at a policy this test never
    // reads and `command: check bans` narrows it to one section of four. Both
    // leave the action pin untouched and both stay green. Pinning the whole
    // `with:` block is the only way to say "this policy, all four sections".
    let deny_action = steps
        .iter()
        .find(|step| {
            action_of(step)
                .is_some_and(|action| action.starts_with("EmbarkStudios/cargo-deny-action@"))
        })
        .expect("CI does not run cargo-deny");
    assert_eq!(
        keys_of(&deny_action["with"]),
        vec!["command"],
        "the cargo-deny step passes more than `command`, which can point it at another policy file or narrow it to one section"
    );
    assert_eq!(
        field(&deny_action["with"], "command"),
        Some("check".to_owned()),
        "cargo-deny does not check all four sections"
    );
    for (key, why) in [
        ("if", "is conditional, so some event skips the whole policy"),
        ("continue-on-error", "cannot fail the job"),
        ("working-directory", "reads another tree"),
    ] {
        assert_eq!(field(deny_action, key), None, "the cargo-deny step {why}");
    }

    let checkout = steps
        .iter()
        .find(|step| action_of(step).is_some_and(|action| action.starts_with("actions/checkout@")))
        .expect("the supply-chain job does not check the repository out");
    assert_eq!(
        field(&checkout["with"], "fetch-depth").as_deref(),
        Some("0"),
        "the secret scanner cannot inspect git history from a shallow checkout"
    );

    gate_step(&steps, "./gitleaks detect --redact --source .");
    gate_step(
        &steps,
        "cargo test -p onionskin-app --test guarantees -- --exact supply_chain_policy_is_checked_in_and_gated",
    );
    assert!(
        !ci_text.contains("GITLEAKS_LICENSE"),
        "the secret scan must not depend on an organization-only action license secret"
    );
}

/// The reviewed Gitleaks install script, checksum and all. Pinned as one
/// scalar because the download and the hash that vouches for it only mean
/// anything together.
const GITLEAKS_INSTALL: &str = "curl -fsSLo gitleaks.tar.gz https://github.com/gitleaks/gitleaks/releases/download/v8.28.0/gitleaks_8.28.0_linux_x64.tar.gz\necho \"a65b5253807a68ac0cafa4414031fd740aeb55f54fb7e55f386acb52e6a840eb  gitleaks.tar.gz\" | sha256sum -c -\ntar -xzf gitleaks.tar.gz gitleaks\n./gitleaks version\n";

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

/// The parsed workflow, proved to declare no `defaults`.
fn workflow(file: &str) -> Yaml {
    let path = workspace_root().join(".github/workflows").join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is unreadable ({error})", path.display()));
    parse_workflow(&text, file)
}

/// `defaults` is checked here because it is the one key that reaches every
/// gate in a file at once: a workflow-level `defaults.run.shell` stops every
/// `run:` step being the command it reads as, and a job-level
/// `defaults.run.working-directory` points them all at another tree. Neither
/// file needs the key, so the reviewed answer is that it is absent everywhere.
fn parse_workflow(text: &str, file: &str) -> Yaml {
    let mut documents = YamlLoader::load_from_str(text)
        .unwrap_or_else(|error| panic!("{file} is not valid YAML ({error})"));
    assert_eq!(
        documents.len(),
        1,
        "{file} holds more than one YAML document"
    );
    let document = documents.remove(0);
    assert!(
        document["defaults"].is_badvalue(),
        "{file} declares workflow-level defaults, which rewrite how every run: step in it is executed"
    );
    let jobs = document["jobs"]
        .as_hash()
        .unwrap_or_else(|| panic!("{file} declares no jobs"));
    for (name, job) in jobs {
        assert!(
            job["defaults"].is_badvalue(),
            "{file}'s {} job declares defaults, which rewrite how its run: steps are executed",
            scalar(name).unwrap_or_default()
        );
    }
    document
}

/// A scalar rendered as the text the workflow means. YAML types `1` as an
/// integer and `true` as a boolean, and a gate cares about the value rather
/// than which of those the parser chose.
fn scalar(node: &Yaml) -> Option<String> {
    match node {
        Yaml::String(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Real(number) => Some(number.clone()),
        Yaml::Boolean(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn field(node: &Yaml, key: &str) -> Option<String> {
    scalar(&node[key])
}

/// The keys of a mapping, sorted, so a step or a `with:` block can be pinned
/// whole rather than one key at a time.
fn keys_of(node: &Yaml) -> Vec<String> {
    let mut keys = node
        .as_hash()
        .map(|map| map.keys().filter_map(scalar).collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// A step's keys, proved to be drawn only from the reviewed set. Listing the
/// keys that must *not* appear is how `shell:` went unnoticed: `shell: cat`
/// leaves the command byte-exact and stops it being a command at all, and
/// `shell: python` reinterprets it just as thoroughly. Every step this suite
/// calls a gate goes through here, conditional ones included.
fn assert_reviewed_keys(step: &Yaml, allowed: &[&str], what: &str) {
    for key in keys_of(step) {
        assert!(
            allowed.contains(&key.as_str()),
            "{what} declares `{key}:`, which is not one of the reviewed keys {allowed:?}"
        );
    }
}

fn job_of<'a>(workflow: &'a Yaml, name: &str) -> &'a Yaml {
    let job = &workflow["jobs"][name];
    assert!(!job.is_badvalue(), "the workflow has no {name} job");
    job
}

fn job_steps<'a>(workflow: &'a Yaml, name: &str) -> Vec<&'a Yaml> {
    job_of(workflow, name)["steps"]
        .as_vec()
        .unwrap_or_else(|| panic!("the {name} job declares no steps"))
        .iter()
        .collect()
}

/// The action a step runs, without the `# vN` note a commit pin carries.
fn action_of(step: &Yaml) -> Option<String> {
    Some(field(step, "uses")?.split_whitespace().next()?.to_owned())
}

/// The one step whose whole script is `command`, declaring nothing beyond a
/// name and that script. Not "a step one of whose lines is `command`": a
/// script of `set +e`, the gate, `exit 0` runs the gate and still succeeds
/// with every line reading correctly.
fn gate_step<'a>(steps: &[&'a Yaml], command: &str) -> &'a Yaml {
    let matching = steps
        .iter()
        .filter(|step| field(step, "run").as_deref().map(str::trim) == Some(command))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one step whose whole script is `{command}`, found {}",
        matching.len()
    );
    let step = matching[0];
    assert_reviewed_keys(
        step,
        &["name", "run"],
        &format!("the step running `{command}`"),
    );
    step
}

/// Whether a command builds the app binary. The word `cargo` anywhere, then
/// `build` or `b`: anchoring on the first word caught `cargo +stable build`
/// but let `RUSTFLAGS=... cargo build` and `cd . && cargo build` through, and
/// either overwrites the `target/release/onionskin` the packaging scripts
/// ship.
fn builds_the_app(command: &str) -> bool {
    let words = command
        .split_whitespace()
        .map(|word| word.trim_matches(['"', '\'', '`']))
        .collect::<Vec<_>>();
    // `sh -c "cargo build ..."` makes the word `"cargo`, and a `$CARGO` or an
    // absolute path reaches the same binary.
    let cargo =
        |word: &&str| matches!(*word, "cargo" | "$CARGO" | "${CARGO}") || word.ends_with("/cargo");
    words
        .iter()
        .position(cargo)
        .is_some_and(|at| words[at + 1..].iter().any(|w| matches!(*w, "build" | "b")))
}

/// A `run:` scalar's commands, with shell line continuations joined. Scanning
/// the scalar a line at a time reads a continued `cargo \` and its arguments as
/// two commands that each build nothing.
fn script_lines(run: &str) -> Vec<String> {
    let mut commands: Vec<String> = Vec::new();
    for line in run.lines() {
        let line = line.trim();
        match commands.last_mut() {
            Some(last) if last.ends_with('\\') => {
                last.pop();
                last.push(' ');
                last.push_str(line);
            }
            _ => commands.push(line.to_owned()),
        }
    }
    commands
}

/// Every string literal reachable from an expression.
#[derive(Default)]
struct Strings(Vec<String>);

impl<'ast> Visit<'ast> for Strings {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.0.push(literal.value());
    }
}

fn strings_in<'ast>(args: impl IntoIterator<Item = &'ast Expr>) -> Vec<String> {
    let mut found = Strings::default();
    for arg in args {
        found.visit_expr(arg);
    }
    found.0
}

/// What one function's body says.
#[derive(Default)]
struct Function {
    /// The attribute names the function carries, sorted.
    attributes: Vec<String>,
    /// String literals inside an assertion or a failure report.
    messages: Vec<String>,
    /// Method calls, as `.method` and as `receiver.method`.
    invocations: Vec<String>,
    /// The free functions this one calls. Method names stay out: reachability
    /// resolves names against this file's top-level functions, so a
    /// `tally.report()` in the live walk would otherwise reach a never-called
    /// free `fn report()` and let its assertion answer for a deleted one.
    calls: Vec<String>,
}

impl<'ast> Visit<'ast> for Function {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // syn leaves a macro's tokens unparsed, so its arguments are walked
        // here or not at all.
        if let Ok(args) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            let name = mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            if matches!(
                name.as_str(),
                "assert" | "assert_eq" | "assert_ne" | "panic"
            ) {
                self.messages.extend(strings_in(&args));
            }
            for arg in &args {
                self.visit_expr(arg);
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        self.invocations.push(format!(".{method}"));
        if let Expr::Path(path) = &*call.receiver {
            if let Some(receiver) = path.path.get_ident() {
                self.invocations.push(format!("{receiver}.{method}"));
            }
        }
        // The corpus walks fail a file through their tally rather than through
        // an assert; the pass-count floor is then asserted against that tally,
        // so it is the same thing said in the harness's own terms.
        if matches!(method.as_str(), "fail" | "record") {
            self.messages.extend(strings_in(&call.args));
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*call.func {
            if let Some(last) = path.path.segments.last() {
                if last.ident == "Err" {
                    self.messages.extend(strings_in(&call.args));
                }
                self.calls.push(last.ident.to_string());
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// What a `crates/cos` test file says, read from its syntax tree. Four
/// hand-written readers in a row were each defeated by something they did not
/// model - a trailing `//`, an unbalanced quote, a `/* */` wrapper, then a
/// `#[cfg]` - so a comment is not a token here and an attribute is not an
/// ident lookup. Bodies are kept per function because a file-global bag of
/// assertions lets one parked in a dead test answer for one deleted from a
/// live one.
#[derive(Default)]
struct Suite {
    file: String,
    functions: BTreeMap<String, Function>,
    /// The named enforcing tests, and everything reachable from them.
    reachable: BTreeSet<String>,
}

impl Suite {
    fn asserts(&self, marker: &str, missing: &str) {
        assert!(
            self.reached()
                .any(|body| body.messages.iter().any(|line| line.contains(marker))),
            "{} no longer proves its guarantee: {missing}",
            self.file
        );
    }

    fn invokes(&self, call: &str, missing: &str) {
        assert!(
            self.reached()
                .any(|body| body.invocations.iter().any(|found| found == call)),
            "{} no longer proves its guarantee: {missing}",
            self.file
        );
    }

    fn reached(&self) -> impl Iterator<Item = &Function> {
        self.reachable
            .iter()
            .filter_map(|name| self.functions.get(name))
    }
}

/// What an attribute is called. The first path segment, not the last: a tool
/// attribute such as `#[rustfmt::skip]` is named by its tool, and reading the
/// last segment calls it `skip`, which no reviewed set would contain.
fn attribute_name(attribute: &syn::Attribute) -> String {
    attribute
        .path()
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

/// The inner doc comment of a parsed file, which is an attribute rather than a
/// comment and so survives parsing.
fn module_doc(file: &syn::File) -> String {
    file.attrs
        .iter()
        .filter_map(|attr| {
            let syn::Meta::NameValue(pair) = &attr.meta else {
                return None;
            };
            if !attr.path().is_ident("doc") {
                return None;
            }
            let Expr::Lit(literal) = &pair.value else {
                return None;
            };
            let syn::Lit::Str(text) = &literal.lit else {
                return None;
            };
            Some(text.value())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `crates/cos` test file that enforces guarantee `number`.
fn enforcing_suite(file: &str, number: u8, tests: &[&str]) -> Suite {
    let path = workspace_root().join("crates/cos/tests").join(file);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is unreadable ({error}), so guarantee {number} is unchecked",
            path.display()
        )
    });
    read_suite(&source, file, number, tests)
}

/// Reads one suite and proves each named test is a plain, live `#[test]`. The
/// attribute set is pinned exactly, the same allowlist move as a step's keys:
/// `#[cfg_attr(all(), ignore)]` and `#[cfg(any())]` both leave `#[test]`
/// present while the test is respectively ignored and compiled away.
fn read_suite(source: &str, file: &str, number: u8, tests: &[&str]) -> Suite {
    let parsed = syn::parse_file(source).unwrap_or_else(|error| {
        panic!("{file} does not parse ({error}), so guarantee {number} cannot be read")
    });
    assert!(
        module_doc(&parsed)
            .trim_start()
            .starts_with(&format!("Guarantee test {number}:")),
        "{file} no longer claims guarantee {number}"
    );

    // One scope up from the per-function pin: `#![cfg(any())]` under the `//!`
    // header takes out every test in the file at once, and each of them still
    // reads as a live `#[test]`.
    for attribute in &parsed.attrs {
        let name = attribute_name(attribute);
        assert_eq!(
            name, "doc",
            "{file} carries a file-level `#![{name}]`, which can compile the whole suite away while every test in it still reads as live"
        );
    }

    let mut suite = Suite {
        file: file.to_owned(),
        ..Suite::default()
    };
    for item in &parsed.items {
        let syn::Item::Fn(item) = item else {
            continue;
        };
        let mut body = Function {
            attributes: {
                let mut names = item
                    .attrs
                    .iter()
                    .map(attribute_name)
                    // A doc comment is an attribute too, and the one kind that
                    // cannot switch a test off.
                    .filter(|name| name != "doc")
                    .collect::<Vec<_>>();
                names.sort();
                names
            },
            ..Function::default()
        };
        body.visit_block(&item.block);
        suite.functions.insert(item.sig.ident.to_string(), body);
    }

    for name in tests {
        let body = suite.functions.get(*name).unwrap_or_else(|| {
            panic!("{file} no longer defines {name}, so guarantee {number} is unchecked")
        });
        assert!(
            body.attributes.iter().any(|name| name == "test"),
            "{file}'s {name} is no longer a #[test]"
        );
        for attribute in &body.attributes {
            assert!(
                attribute == "test" || INERT_ATTRIBUTES.contains(&attribute.as_str()),
                "{file}'s {name} carries `#[{attribute}]`, which is not one of the attributes reviewed as unable to change whether the test runs"
            );
        }
    }

    // Everything the named tests reach. The assertions themselves live in the
    // walk helpers those tests call, so a marker found anywhere else in the
    // file - a parked test, a dead helper - does not count.
    let mut frontier = tests
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    while let Some(name) = frontier.pop() {
        if !suite.reachable.insert(name.clone()) {
            continue;
        }
        if let Some(body) = suite.functions.get(&name) {
            frontier.extend(body.calls.iter().cloned());
        }
    }
    suite
}

/// Guarantees 1, 2 and 6 are enforced in `crates/cos`, which only helps if CI
/// runs that crate's tests. It reaches them through the workspace suite, so
/// both the command and the membership have to hold.
fn assert_ci_reaches_the_cos_suite() {
    let ci = workflow("ci.yml");
    gate_step(&job_steps(&ci, "test"), "cargo test --workspace");
    let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml"))
        .expect("the workspace manifest is readable");
    assert!(
        manifest.contains("\"crates/cos\","),
        "crates/cos is not a workspace member, so `cargo test --workspace` does not reach it"
    );
}

/// One section of a TOML document, verbatim apart from its whole-line
/// comments. Those go because cargo-deny does not read them and a reviewed
/// setting parked in one reads exactly like the setting itself; nothing else
/// is stripped, so a value that grew a trailing comment stops comparing equal
/// rather than being quietly trimmed back into shape.
fn toml_section(document: &str, name: &str) -> Vec<String> {
    let header = format!("[{name}]");
    document
        .lines()
        .skip_while(|line| line.trim() != header)
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter(|line| !line.trim_start().starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Every table header the document declares, sorted. `toml_section` stops at
/// the next `[`, so a sub-table such as `[[licenses.clarify]]` would otherwise
/// stay invisible to the key-set pins while cargo-deny obeys it, which is
/// enough to relabel a crate's license past the allow-list with everything
/// green.
fn policy_sections(document: &str) -> Vec<String> {
    let mut headers = document
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('['))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    headers.sort();
    headers
}

/// Every key `section` declares, sorted, with a quoted key read as the key it
/// is. Pinning the set is what stops a knob nobody thought to name - a
/// `[graph] exclude` that mutes a crate out of the scan, or a
/// `disable-yank-checking` beside an untouched `yanked = "deny"` - from
/// disarming the policy while every pinned value still reads correctly.
fn policy_keys(document: &str, section: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0usize;
    for line in toml_section(document, section) {
        let trimmed = line.trim();
        if depth == 0 {
            if let Some((key, _)) = trimmed.split_once('=') {
                keys.push(key.trim().trim_matches('"').trim_matches('\'').to_owned());
            }
        }
        depth = (depth + opens(trimmed)).saturating_sub(closes(trimmed));
    }
    keys.sort();
    keys
}

/// The value of `key` in `section`, as cargo-deny would read it: a value
/// spread over several lines is joined onto one so an array can be compared
/// whole. `None` when the key is absent, which is a distinct answer from an
/// empty value.
fn policy_setting(document: &str, section: &str, key: &str) -> Option<String> {
    let body = toml_section(document, section);
    let mut lines = body.iter().map(|line| line.trim());
    let mut value = lines
        .find_map(|line| {
            Some(
                line.strip_prefix(key)?
                    .trim_start()
                    .strip_prefix('=')?
                    .trim(),
            )
        })?
        .to_owned();

    while opens(&value) > closes(&value) {
        let next = lines.next()?;
        if next.is_empty() {
            continue;
        }
        if next.starts_with(']') {
            // TOML allows a trailing comma before the bracket; a joined value
            // that kept it would not compare equal to the array it means.
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

fn opens(value: &str) -> usize {
    value.chars().filter(|c| *c == '[' || *c == '{').count()
}

fn closes(value: &str) -> usize {
    value.chars().filter(|c| *c == ']' || *c == '}').count()
}

// Every decoy below reached the tree at some point during this package's
// review, and each was found by a person rather than by a test. Four are the
// same class returning - "the check reads what is written, not what runs" - so
// the class is guarded here now. A note recording a past failure is not a
// seed; this is.

/// Runs a reader against a seeded decoy and proves it refuses.
fn rejects(what: &str, check: impl FnOnce()) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(check));
    std::panic::set_hook(previous);
    assert!(
        outcome.is_err(),
        "the reader accepted a decoy it must refuse: {what}"
    );
}

/// A stand-in for `crates/cos/tests/repair.rs`, shaped like the real one.
const DECOY_SUITE: &str = r#"//! Guarantee test 6: every file in the malformed corpus opens, saving appends
//! an incremental section holding the repaired structures, and the corrupt
//! original bytes survive beneath it byte-intact.

#[test]
fn every_malformed_file_repairs_and_saves_over_intact_original_bytes() {
    let tally = walk();
    tally.report();
    assert_eq!(
        tally.passed.len(),
        files.len(),
        "guarantee test 6 requires the whole malformed set"
    );
}
"#;

/// Attributes reviewed as unable to change whether a test runs. `#[ignore]`,
/// `#[cfg]`, `#[cfg_attr]` and `#[should_panic]` are all absent on purpose.
const INERT_ATTRIBUTES: &[&str] = &["allow", "rustfmt"];

const WHOLE_SET: &str = "guarantee test 6 requires the whole malformed set";

/// The assertion as it appears in the decoy, so a seed can replace it whole.
const DECOY_ASSERTION: &str = "    assert_eq!(\n        tally.passed.len(),\n        files.len(),\n        \"guarantee test 6 requires the whole malformed set\"\n    );";

fn read_decoy(source: &str) {
    let suite = read_suite(
        source,
        "decoy.rs",
        6,
        &["every_malformed_file_repairs_and_saves_over_intact_original_bytes"],
    );
    suite.asserts(WHOLE_SET, "the whole-set clause is gone");
}

#[test]
fn an_assertion_that_only_looks_present_is_refused() {
    // The baseline has to pass, or the refusals below prove nothing.
    read_decoy(DECOY_SUITE);
    assert!(
        DECOY_SUITE.contains(DECOY_ASSERTION),
        "the decoy no longer states the clause the seeds replace"
    );

    // Commented out three ways, each of which defeated a hand-written reader
    // in this package: a whole-line `//`, a trailing `//` on live code, and a
    // `/* { ... } */` wrapper whose opening line ends in `{` so a statement
    // scan starts inside it and finds the assertion's own text.
    for (what, replacement) in [
        (
            "the assertion left as whole-line comments",
            DECOY_ASSERTION
                .lines()
                .map(|line| format!("    // {}", line.trim()))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        (
            "the assertion left as a trailing comment",
            format!("    let _ = &tally; // \"{WHOLE_SET}\""),
        ),
        (
            "the assertion wrapped in a block comment",
            format!("    /* {{\n{DECOY_ASSERTION}\n    }} */"),
        ),
    ] {
        let seeded = DECOY_SUITE.replace(DECOY_ASSERTION, &replacement);
        assert!(seeded != DECOY_SUITE, "the decoy for {what} did not apply");
        rejects(what, || read_decoy(&seeded));
    }

    // The assertion deleted from the live walk and parked in an ignored test.
    // A file-global bag of assertions answers this one; a per-function one
    // reachable from the named tests does not.
    let parked = format!(
        "{}\n#[test]\n#[ignore = \"parked\"]\nfn parked_expectation() {{\n    assert_eq!(1, 1, \"{WHOLE_SET}\");\n}}\n",
        DECOY_SUITE.replace(DECOY_ASSERTION, "    let _ = &tally;")
    );
    rejects("the assertion parked in an ignored test", || {
        read_decoy(&parked)
    });

    // The live walk calls `tally.report()`. A free `fn report()` that nothing
    // calls shares that name, so a call graph fed method names as well as
    // function names reaches it and lets its assertion answer for the deleted
    // one. The control below, identical but named so nothing collides, is
    // refused either way; this pair isolates name resolution as the hole.
    for name in ["report", "never_named_anywhere"] {
        let colliding = format!(
            "{}\nfn {name}() {{\n    assert_eq!(1, 1, \"{WHOLE_SET}\");\n}}\n",
            DECOY_SUITE.replace(DECOY_ASSERTION, "    let _ = &tally;")
        );
        rejects(
            &format!("the assertion moved to an uncalled fn {name}()"),
            || read_decoy(&colliding),
        );
    }
}

#[test]
fn an_enforcing_test_switched_off_by_an_attribute_is_refused() {
    // `#[test]` is present in all three. Only an exact attribute set can tell
    // a live test from one that is ignored or compiled away.
    for (what, attribute) in [
        ("the test ignored outright", "#[ignore]"),
        (
            "the test ignored through cfg_attr",
            "#[cfg_attr(all(), ignore)]",
        ),
        ("the test compiled away by cfg", "#[cfg(any())]"),
    ] {
        let seeded = DECOY_SUITE.replace("#[test]\n", &format!("#[test]\n{attribute}\n"));
        assert!(seeded != DECOY_SUITE, "the decoy for {what} did not apply");
        rejects(what, || read_decoy(&seeded));
    }

    // One scope up, and stronger: this compiles every test in the file away at
    // once, and each of them still reads as a live `#[test]`.
    let seeded = DECOY_SUITE.replace("\n#[test]", "\n#![cfg(any())]\n\n#[test]");
    assert!(
        seeded != DECOY_SUITE,
        "the file-level cfg decoy did not apply"
    );
    rejects("the whole suite compiled away by a file-level cfg", || {
        read_decoy(&seeded)
    });

    // Inert attributes are reviewed rather than merely tolerated, so a
    // maintainer adding one does not hit a false failure.
    for inert in ["#[allow(clippy::needless_range_loop)]", "#[rustfmt::skip]"] {
        read_decoy(&DECOY_SUITE.replace("#[test]\n", &format!("#[test]\n{inert}\n")));
    }
}

/// A stand-in workflow with one gate step, shaped like the real jobs.
fn decoy_workflow(step: &str) -> String {
    format!("name: CI\non: [push]\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n{step}")
}

fn read_gate(text: &str) {
    let parsed = parse_workflow(text, "decoy.yml");
    gate_step(&job_steps(&parsed, "check"), "cargo deny check");
}

#[test]
fn a_step_that_would_not_run_the_command_is_refused() {
    read_gate(&decoy_workflow("      - run: cargo deny check\n"));

    for (what, step) in [
        (
            "a shell that never executes the command",
            "      - run: cargo deny check\n        shell: cat\n",
        ),
        (
            "a shell that reinterprets the command",
            "      - run: cargo deny check\n        shell: python\n",
        ),
        (
            "the command wrapped so its failure is swallowed",
            "      - run: |\n          set +e\n          cargo deny check\n          exit 0\n",
        ),
        (
            "a folded scalar splicing a suffix onto the command",
            "      - run: >\n          cargo deny check\n\n          || true\n",
        ),
        (
            "the step conditioned away",
            "      - run: cargo deny check\n        if: false\n",
        ),
        (
            "the step allowed to fail",
            "      - run: cargo deny check\n        continue-on-error: true\n",
        ),
        (
            "the step pointed at another tree",
            "      - run: cargo deny check\n        working-directory: ./stale-copy\n",
        ),
    ] {
        rejects(what, || read_gate(&decoy_workflow(step)));
    }
}

#[test]
fn a_conditional_gate_is_held_to_the_same_key_allowlist() {
    // The accessibility probe and guarantee 6's two steps carry the one
    // permitted `if:`, so they are not `gate_step`s and reach the allowlist
    // through a separate call. `shell:` passed on REPO-002's required gate
    // until that call existed.
    let live = "      - name: Accessibility probe\n        if: runner.os == 'macOS'\n        run: cargo test --features a11y-probe\n";
    let parsed = parse_workflow(&decoy_workflow(live), "decoy.yml");
    let steps = job_steps(&parsed, "check");
    assert_reviewed_keys(steps[0], &["name", "if", "run"], "the decoy probe");

    let seeded = "      - name: Accessibility probe\n        if: runner.os == 'macOS'\n        shell: cat\n        run: cargo test --features a11y-probe\n";
    let parsed = parse_workflow(&decoy_workflow(seeded), "decoy.yml");
    let steps = job_steps(&parsed, "check");
    rejects("a shell: on a conditional gate", || {
        assert_reviewed_keys(steps[0], &["name", "if", "run"], "the decoy probe")
    });
}

#[test]
fn a_workflow_that_rewrites_how_every_step_runs_is_refused() {
    // One key, three lines above `jobs:`, disarms every `run:` gate in the
    // file at once while each command still reads correctly.
    rejects("workflow-level defaults", || {
        read_gate("name: CI\non: [push]\ndefaults:\n  run:\n    shell: cat\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo deny check\n")
    });
    rejects("job-level defaults", || {
        read_gate("name: CI\non: [push]\njobs:\n  check:\n    runs-on: ubuntu-latest\n    defaults:\n      run:\n        working-directory: ./stale-copy\n    steps:\n      - run: cargo deny check\n")
    });
}

#[test]
fn every_way_of_reaching_cargo_build_counts_as_building_the_app() {
    // Each of these overwrites target/release/onionskin, which both packaging
    // scripts ship. Three were live bypasses of earlier versions of this
    // predicate: one anchored on adjacent words, one on the first word.
    for command in [
        "cargo build --release -p onionskin-app",
        "cargo b --release -p onionskin-app",
        "cargo +stable build --release -p onionskin-app",
        "cargo --offline build --release -p onionskin-app",
        "RUSTFLAGS=-Awarnings cargo build --release -p onionskin-app",
        "cd . && cargo build --release -p onionskin-app",
        "cargo build --release --bin onionskin",
        "sh -c \"cargo build --release -p onionskin-app\"",
        "bash -lc 'cargo build --release -p onionskin-app'",
        "$CARGO build --release -p onionskin-app",
        "/usr/local/bin/cargo build --release -p onionskin-app",
    ] {
        assert!(
            builds_the_app(command),
            "a featureless rebuild would go unnoticed: {command}"
        );
    }
    for command in [
        "cargo test --workspace",
        "cargo deny check",
        "./packaging/linux/package.sh",
    ] {
        assert!(
            !builds_the_app(command),
            "a command that builds nothing is treated as a rebuild: {command}"
        );
    }

    // A continued command is one command. Scanned a line at a time, neither
    // half builds anything and the rebuild goes unnoticed.
    let continued = "cargo \\\n  build --release -p onionskin-app";
    assert!(
        script_lines(continued)
            .iter()
            .any(|command| builds_the_app(command)),
        "a rebuild split across a line continuation would go unnoticed"
    );
}
