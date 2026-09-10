//! Corpus discovery for test targets, in one place.
//!
//! Most of `corpus/` is gitignored: `external/` is fetched, `malformed/` and
//! `bench/` are generated, and only `seeds/` is committed. A test that cannot
//! find what it needs has two honest answers and no third one: say so and
//! return, or fail. Which one it gives is the caller's environment, not the
//! caller's code - [`missing`] fails when `ONIONSKIN_CORPUS_REQUIRED` is set,
//! and CI sets it on the re-run step that exists to turn a skip into a
//! failure.
//!
//! This crate exists because that rule was implemented four times before it
//! was implemented anywhere the app and plugin test targets could reach it,
//! and their fixtures are the ones CI has never seen.

use std::path::{Path, PathBuf};

/// Root of the shared corpus: `$ONIONSKIN_CORPUS`, else `<workspace>/corpus`.
///
/// The fallback is this crate's own manifest directory two levels up, which is
/// the workspace root for as long as this crate lives under `crates/`. Nothing
/// derives it from the *caller's* manifest, which is how a helper copied into
/// a plugin target ends up pointing one directory too high.
pub fn corpus_root() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("ONIONSKIN_CORPUS") {
        let path = PathBuf::from(from_env);
        return path.is_dir().then_some(path);
    }
    let workspace = workspace_root();
    let corpus = workspace.join("corpus");
    corpus.is_dir().then_some(corpus)
}

/// Returns the corpus subdirectory, or `None` after [`missing`] has had its
/// say.
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

/// Reports an absent corpus: a loud skip normally, a failure under
/// `ONIONSKIN_CORPUS_REQUIRED`.
///
/// Generic in the return type so a caller producing anything other than a path
/// can end a lookup with it rather than reimplementing the rule.
pub fn missing<T>(why: &str) -> Option<T> {
    if std::env::var_os("ONIONSKIN_CORPUS_REQUIRED").is_some() {
        panic!("corpus required but {why}");
    }
    eprintln!("SKIPPED: {why}");
    None
}

/// Every PDF under `dir`, recursively, sorted by path so a run that samples a
/// prefix samples the same files everywhere.
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

/// One of the committed seeds under `corpus/seeds`.
///
/// Not a lookup: the seeds are in the repository, so a caller that cannot find
/// one has a broken checkout rather than an unfetched corpus, and should fail
/// on the read rather than skip. It goes through [`corpus_root`] so that
/// `ONIONSKIN_CORPUS` moves the seeds along with everything else.
pub fn seed(name: &str) -> PathBuf {
    let root = corpus_root().unwrap_or_else(|| workspace_root().join("corpus"));
    root.join("seeds").join(name)
}

fn workspace_root() -> &'static Path {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels under the workspace root");
    assert!(
        root.join("Cargo.toml").is_file(),
        "{} is not the workspace root, so every corpus lookup would resolve one directory off",
        root.display()
    );
    root
}
