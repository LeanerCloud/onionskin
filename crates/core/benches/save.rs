//! What rebuilding the preview costs after an edit.
//!
//! The preview is `original ++ section`, so its cost is O(file size) no matter
//! how small the edit is: every commit that reaches a render copies the file.
//! That is the number this bench exists to pin down, because it is the one that
//! decides whether editing a large document feels instant or not.
//!
//! Three documents, because the cost has three shapes:
//!
//! - the **thousand-page bench file**, clean, which is the steady case;
//! - a **repaired** document, where `needs_full_table()` forces a full
//!   cross-reference table into the section and the section stops being small;
//! - the **largest file in `external/`**, because the synthetic bench file is
//!   not the worst case a user has and the cost measured here scales with size.
//!
//! First and second commit are reported separately. The first pays for the
//! preview from nothing; the second is where reusing a buffer would show up,
//! and reporting them apart is what makes it visible whether it does.
//!
//! Like the other benches, it asserts rather than prints.

mod harness;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use onionskin_core::{AnnotationFilter, Document, DocumentEdit};
use onionskin_cos::{Name, Object};

/// One preview rebuild after one small edit, on a clean document. Chosen above
/// the thousand-page file's measurement with room for a slower machine; the
/// point of the number is to fail when the rebuild stops being proportional to
/// the file and starts being proportional to something worse.
const CLEAN_BUDGET: Duration = Duration::from_millis(250);

/// The repaired and largest-file cases scale with size, so their budget is per
/// megabyte of file rather than absolute.
const PER_MEGABYTE: Duration = Duration::from_millis(60);

fn main() {
    if let Some(path) = harness::bench_document() {
        let (first, second) = two_commits(&path);
        harness::heading("preview rebuild, thousand-page file");
        println!("  first commit {first:?}, second commit {second:?}");
        harness::under_time(
            "first preview rebuild, clean 1000 pages",
            first,
            CLEAN_BUDGET,
        );
        harness::under_time(
            "second preview rebuild, clean 1000 pages",
            second,
            CLEAN_BUDGET,
        );
    }

    if let Some(path) = repaired_document() {
        let (first, second) = two_commits(&path);
        let budget = scaled(&path);
        harness::heading("preview rebuild, repaired document");
        println!(
            "  {}: first {first:?}, second {second:?}, budget {budget:?}",
            name(&path)
        );
        harness::under_time("first preview rebuild, repaired", first, budget);
        harness::under_time("second preview rebuild, repaired", second, budget);
    }

    if let Some(path) = largest_external() {
        let (first, second) = two_commits(&path);
        let budget = scaled(&path);
        harness::heading("preview rebuild, largest external file");
        println!(
            "  {}: first {first:?}, second {second:?}, budget {budget:?}",
            name(&path)
        );
        harness::under_time("first preview rebuild, largest external", first, budget);
        harness::under_time("second preview rebuild, largest external", second, budget);
    }
}

/// Time the preview rebuild after each of two successive edits.
fn two_commits(path: &Path) -> (Duration, Duration) {
    let mut document = Document::open_path(path).expect("the document opens");
    let first = one_commit(&mut document, "first");
    let second = one_commit(&mut document, "second");
    (first, second)
}

fn one_commit(document: &mut Document, text: &str) -> Duration {
    let (edit, base) = document.edit_mut();
    edit.apply(
        base,
        DocumentEdit::SetInfoField {
            key: Name::new("Description"),
            value: Some(Object::String(text.as_bytes().to_vec())),
        },
    )
    .expect("the edit commits");

    let started = Instant::now();
    let bytes = document
        .preview_bytes(AnnotationFilter::DocumentAndMarkups)
        .expect("the preview builds");
    let elapsed = started.elapsed();
    assert!(!bytes.is_empty());
    elapsed
}

fn scaled(path: &Path) -> Duration {
    let megabytes = std::fs::metadata(path).map_or(1, |m| m.len() / 1_000_000 + 1);
    CLEAN_BUDGET + PER_MEGABYTE * megabytes as u32
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The **largest** document the repairing opener had to repair, found by
/// provenance rather than by a filename somebody has to keep up to date.
///
/// Largest, because the first repaired file in sorted order is a few hundred
/// bytes with a damaged header, and timing that measures nothing about what a
/// forced full cross-reference table costs.
fn repaired_document() -> Option<PathBuf> {
    let root = external_root()?;
    let mut found = Vec::new();
    collect(&root, &mut found);
    let mut repaired: Vec<(u64, PathBuf)> = found
        .into_iter()
        .filter(|path| {
            onionskin_cos::Document::open_path_repairing(path)
                .map(|(_, provenance)| !provenance.is_clean())
                .unwrap_or(false)
        })
        .map(|path| (std::fs::metadata(&path).map_or(0, |m| m.len()), path))
        .collect();
    repaired.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    repaired
        .into_iter()
        .map(|(_, path)| path)
        .find(|path| Document::open_path(path).is_ok())
}

fn largest_external() -> Option<PathBuf> {
    let root = external_root()?;
    let mut found = Vec::new();
    collect(&root, &mut found);
    found
        .into_iter()
        .filter(|path| Document::open_path(path).is_ok())
        .max_by_key(|path| std::fs::metadata(path).map_or(0, |m| m.len()))
}

fn external_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)?
        .join("corpus")
        .join("external");
    if root.is_dir() {
        Some(root)
    } else {
        eprintln!("SKIPPED: corpus/external is absent; fetch it with corpus/fetch.sh");
        None
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|kind| kind == "pdf") {
            out.push(path);
        }
    }
}
