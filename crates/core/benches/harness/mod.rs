//! Corpus discovery and budget assertions shared by the three benches.
//!
//! These are benches only in the cargo sense. Nothing here prints a number and
//! calls it a result: every measurement is compared against a budget from
//! PLAN.md decision 11 and a breach ends the process, so `cargo bench` fails
//! the build the way a test does.

// Each bench uses a different part of this module.
#![allow(dead_code)]

mod composite_tally;

#[allow(unused_imports)]
pub use composite_tally::CompositeTally;

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Root of the shared corpus: `$ONIONSKIN_CORPUS`, else `<workspace>/corpus`.
///
/// The same rule the test harnesses use (`crates/cos/tests/common/mod.rs`,
/// `crates/core/tests/search.rs`). Bench targets cannot import either without
/// one crate publishing its test helpers, so the rule is restated and the
/// environment variables are kept identical.
fn corpus_root() -> PathBuf {
    match std::env::var_os("ONIONSKIN_CORPUS") {
        Some(from_env) => PathBuf::from(from_env),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/core sits two levels under the workspace root")
            .join("corpus"),
    }
}

/// A corpus file, or `None` after saying loudly why it is absent.
///
/// `corpus/bench/` and `corpus/external/` are gitignored, so both are absent
/// from a fresh clone. Set `ONIONSKIN_CORPUS_REQUIRED=1` (the CI bench job
/// does) to turn absence into a failure: a bench job that skips every budget
/// and reports success is the outcome that flag exists to prevent.
pub fn corpus_file(relative: &str, how_to_get_it: &str) -> Option<PathBuf> {
    let path = corpus_root().join(relative);
    if path.is_file() {
        return Some(path);
    }
    let why = format!("{} is absent; {how_to_get_it}", path.display());
    if std::env::var_os("ONIONSKIN_CORPUS_REQUIRED").is_some() {
        panic!("corpus required but {why}");
    }
    eprintln!("SKIPPED: {why}");
    None
}

/// The thousand-page bench file decision 11 states its open budget against.
pub fn bench_document() -> Option<PathBuf> {
    corpus_file(
        "bench/pages-1000.pdf",
        "generate it with corpus/make-bench.py",
    )
}

/// The transparency-heavy page the first-paint budget exists for: the one the
/// M1 spike found costliest to rasterize, which is what makes "something
/// visible in 200 ms" a claim about the placeholder rather than about hayro.
/// What it costs today is measured and printed by `benches/paint.rs`, and
/// stated in one place rather than two so the two cannot drift apart.
pub fn heavy_document() -> Option<PathBuf> {
    corpus_file(
        "external/hayro-corpus/0041790.pdf",
        "fetch it with corpus/fetch.sh hayro-corpus",
    )
}

/// Report a measurement against its budget, and end the run if it is over.
///
/// Every budget goes through here so a breach reads the same way whichever
/// bench found it, and so tightening a constant is the whole of a mutation
/// test.
fn under(what: &str, measured: f64, budget: f64, unit: &str, decimals: usize) {
    let percent = measured / budget * 100.0;
    println!(
        "  {what}: {measured:.decimals$} {unit} of {budget:.decimals$} {unit} budget \
         ({percent:.1}%)"
    );
    assert!(
        measured <= budget,
        "BUDGET EXCEEDED: {what} measured {measured:.decimals$} {unit} against a budget of \
         {budget:.decimals$} {unit}"
    );
}

pub fn under_time(what: &str, measured: Duration, budget: Duration) {
    under(
        what,
        measured.as_secs_f64() * 1000.0,
        budget.as_secs_f64() * 1000.0,
        "ms",
        3,
    );
}

pub fn under_bytes(what: &str, measured: u64, budget: u64) {
    under(what, measured as f64, budget as f64, "bytes", 0);
}

pub fn under_count(what: &str, measured: usize, budget: usize, unit: &str) {
    under(what, measured as f64, budget as f64, unit, 0);
}

/// Announce which budget is running, so a failure in CI names itself.
pub fn heading(what: &str) {
    println!("\n== {what}");
}
