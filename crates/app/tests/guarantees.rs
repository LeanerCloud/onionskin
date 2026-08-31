//! PLAN.md's guarantee tests, the spec as executable checks. Guarantee 5
//! runs today (see `kernel_emptiness.rs`); the rest are named here so the
//! suite has its final shape from day one and each one lands by deleting
//! an `#[ignore]`.

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
    // A job-level condition sits at four spaces, a step's at eight. A gate that
    // only runs on some events is not a gate on the others.
    assert!(
        !bench_job.lines().any(|line| line.starts_with("    if:")),
        "the bench job is conditional, so there are pushes it does not gate"
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
