//! PLAN.md's guarantee tests, the spec as executable checks. Guarantee 5
//! runs today (see `kernel_emptiness.rs`); the rest are named here so the
//! suite has its final shape from day one and each one lands by deleting
//! an `#[ignore]`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, fs, process};

const ALPHA_SHA256: &str = "b6a98d9ce9a2d9149288fa3df42d377c3e42737afdcdaf714e33c0a100b51060";
const TINY_PDF_SHA256: &str = "98704aee8801c3738f9b38577f4c7917b82da5d770a8f4c1c162293e49c0d172";

/// Guarantee 1, round-trip: for every corpus file, open then save with no
/// edit produces byte-identical output. A no-op save appends nothing.
#[test]
#[ignore = "lands with cos parse/save in M1"]
fn a_save_with_no_edit_is_byte_identical_to_the_original() {
    unimplemented!("needs cos open/save and the corpus fetch")
}

/// Guarantee 2, onionskin: open, edit, save produces the original bytes
/// followed by exactly one incremental section, and truncating that
/// section yields the byte-exact original back.
#[test]
#[ignore = "lands with the incremental writer in M1"]
fn an_edit_appends_one_incremental_section_that_truncates_away() {
    unimplemented!("needs cos incremental save and core's edit graph")
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
#[test]
#[ignore = "lands with the cos scan-and-rebuild path in M1"]
fn every_malformed_file_opens_and_repairs_into_a_new_section() {
    unimplemented!("needs the cos repair path and the malformed corpus")
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
        bench_job.contains("hashFiles('corpus/fetch.sh', 'corpus/checksums/hayro-corpus.sha256')"),
        "the bench corpus cache key does not include the checksum manifest"
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

#[cfg(unix)]
#[test]
fn checksum_fetch_validates_a_stamped_cached_hayro_corpus_without_invoking_curl() {
    let temp = TempTree::new("checksum-fetch-cache");
    let corpus = temp.path().join("corpus");
    let checksums = corpus.join("checksums");
    let root = corpus.join("external/hayro-corpus");
    fs::create_dir_all(&checksums).expect("checksum directory is created");
    fs::create_dir_all(&root).expect("cached corpus directory is created");

    fs::copy(
        workspace_root().join("corpus/fetch.sh"),
        corpus.join("fetch.sh"),
    )
    .expect("fetch.sh is copied into the test corpus tree");
    let verifier = workspace_root().join("corpus/verify-sha256.py");
    assert!(
        verifier.is_file(),
        "checksum verifier missing: {}",
        verifier.display()
    );
    fs::copy(&verifier, corpus.join("verify-sha256.py"))
        .expect("checksum verifier is copied into the test corpus tree");

    fs::write(root.join("tiny.pdf"), b"%PDF-1.4\n%tiny\n").expect("cached PDF is written");
    fs::write(root.join(".fetch-stamp"), "source=test\nrevision=test\n")
        .expect("cached stamp is written");
    write_manifest(
        &checksums.join("hayro-corpus.sha256"),
        &[&format!("{TINY_PDF_SHA256}  tiny.pdf")],
    );

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake bin directory is created");
    let make_executable = |path: &Path| {
        let mut permissions = fs::metadata(path)
            .expect("fake executable metadata is readable")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(path, permissions).expect("fake executable is made executable");
    };
    let fake_curl = bin.join("curl");
    fs::write(
        &fake_curl,
        "#!/bin/sh\nprintf '%s\\n' 'fake curl must not be invoked' >&2\nexit 42\n",
    )
    .expect("fake curl is written");
    make_executable(&fake_curl);

    let old_path = env::var_os("PATH").unwrap_or_default();
    let mut path = OsString::from(&bin);
    path.push(":");
    path.push(old_path);

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
        .output()
        .expect("checksum verifier process is spawned")
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
