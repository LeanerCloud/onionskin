// Each integration test uses a different part of this harness.
#![allow(dead_code)]

//! Corpus discovery and tallying shared by the guarantee tests.
//!
//! `corpus/external/` and `corpus/malformed/` are gitignored, so they are
//! absent from a fresh clone and from CI. A test that cannot find its corpus
//! says so loudly and returns; it never reports a pass it did not earn.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use onionskin_cos::Error;

/// Root of the shared corpus: `$ONIONSKIN_CORPUS`, else `<workspace>/corpus`.
pub fn corpus_root() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("ONIONSKIN_CORPUS") {
        let path = PathBuf::from(from_env);
        return path.is_dir().then_some(path);
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    let corpus = workspace.join("corpus");
    corpus.is_dir().then_some(corpus)
}

/// Returns the corpus subdirectory, or `None` after printing why it is absent.
///
/// Set `ONIONSKIN_CORPUS_REQUIRED=1` (CI should) to make a missing corpus a
/// failure: without it these tests would report a pass for work they never did.
pub fn corpus_dir(relative: &str) -> Option<PathBuf> {
    let Some(root) = corpus_root() else {
        return missing("no corpus found; set ONIONSKIN_CORPUS to the corpus directory");
    };
    let dir = root.join(relative);
    if !dir.is_dir() {
        return missing(&format!("{} is absent (it is gitignored)", dir.display()));
    }
    Some(dir)
}

pub fn missing(why: &str) -> Option<PathBuf> {
    if std::env::var_os("ONIONSKIN_CORPUS_REQUIRED").is_some() {
        panic!("corpus required but {why}");
    }
    eprintln!("SKIPPED: {why}");
    None
}

pub fn pdfs_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

/// Pass/fail/skip counts with one bucket per failure category, so a corpus run
/// reports what broke rather than just how much.
#[derive(Default)]
pub struct Tally {
    pub name: String,
    pub passed: Vec<String>,
    pub failed: BTreeMap<String, Vec<String>>,
    pub skipped: BTreeMap<String, Vec<String>>,
}

impl Tally {
    pub fn new(name: &str) -> Self {
        Tally {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn pass(&mut self, file: &Path) {
        self.passed.push(short(file));
    }

    pub fn fail(&mut self, file: &Path, category: &str, detail: &str) {
        self.failed
            .entry(category.to_string())
            .or_default()
            .push(format!("{}: {detail}", short(file)));
    }

    /// Encrypted files are counted, never dropped on the floor.
    pub fn skip(&mut self, file: &Path, category: &str) {
        self.skipped
            .entry(category.to_string())
            .or_default()
            .push(short(file));
    }

    /// Routes an error into the failed or skipped bucket by category.
    pub fn record(&mut self, file: &Path, error: &Error) {
        match error {
            Error::Encrypted => self.skip(file, "encrypted"),
            other => self.fail(file, other.category(), &other.to_string()),
        }
    }

    pub fn total(&self) -> usize {
        self.passed.len() + self.failure_count() + self.skip_count()
    }

    pub fn failure_count(&self) -> usize {
        self.failed.values().map(Vec::len).sum()
    }

    pub fn skip_count(&self) -> usize {
        self.skipped.values().map(Vec::len).sum()
    }

    pub fn report(&self) {
        println!(
            "\n== {} == {} files: {} passed, {} failed, {} skipped",
            self.name,
            self.total(),
            self.passed.len(),
            self.failure_count(),
            self.skip_count()
        );
        for (category, files) in &self.skipped {
            println!("  skipped [{}]: {}", category, files.len());
        }
        for (category, files) in &self.failed {
            println!("  FAILED  [{}]: {}", category, files.len());
            for file in files.iter().take(5) {
                println!("      {file}");
            }
            if files.len() > 5 {
                println!("      ... and {} more", files.len() - 5);
            }
        }
    }
}

fn short(path: &Path) -> String {
    match corpus_root() {
        Some(root) => path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string(),
        None => path.display().to_string(),
    }
}
