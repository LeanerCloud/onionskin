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

/// Builds a small, valid, classic-cross-reference PDF out of the given object
/// bodies: `bodies[0]` becomes object 1, and so on. Object 1 must be the
/// catalog, because the trailer names it as `/Root`.
///
/// `offset_lies` replaces the offset recorded for an object with one the
/// object is not at, which is how a test produces a file whose xref is wrong
/// about exactly one object while still opening clean.
pub fn classic_pdf(bodies: &[&[u8]], offset_lies: &[(u32, u64)]) -> Vec<u8> {
    let mut bytes = Vec::from(&b"%PDF-1.7\n"[..]);
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(bytes.len() as u64);
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    for (number, offset) in offset_lies {
        offsets[*number as usize - 1] = *offset;
    }

    let xref_at = bytes.len();
    let size = bodies.len() + 1;
    bytes.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// The three-object skeleton every fixture here needs: a catalog, a page tree
/// and one page. Further bodies become objects 4 and up.
pub fn skeleton() -> Vec<&'static [u8]> {
    vec![
        b"<</Type/Catalog/Pages 2 0 R>>",
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<<>>>>",
    ]
}

/// One row of a classic cross-reference table, as read back out of bytes this
/// crate wrote.
#[derive(Debug, PartialEq, Eq)]
pub struct XrefRow {
    pub number: u32,
    /// A byte offset for an in-use entry, the next free object number for a
    /// free one (ISO 32000-1 7.5.4).
    pub field: u64,
    pub generation: u16,
    pub free: bool,
}

/// Parses the classic table of the last cross-reference section in `bytes`,
/// so a test can check the section this crate wrote rather than trust it.
pub fn last_xref_table(bytes: &[u8]) -> Vec<XrefRow> {
    let at = rfind(bytes, b"\nxref\n").expect("the section has a classic xref table") + 1;
    let text = String::from_utf8_lossy(&bytes[at..]);
    let mut words = text.split_ascii_whitespace();
    assert_eq!(words.next(), Some("xref"));

    let mut rows = Vec::new();
    while let Some(first) = words.next() {
        if first == "trailer" {
            break;
        }
        let start: u32 = first.parse().expect("subsection start");
        let count: u32 = words
            .next()
            .expect("subsection count")
            .parse()
            .expect("count");
        for i in 0..count {
            let field: u64 = words.next().expect("entry field").parse().expect("field");
            let generation: u16 = words
                .next()
                .expect("entry generation")
                .parse()
                .expect("gen");
            let kind = words.next().expect("entry kind");
            rows.push(XrefRow {
                number: start + i,
                field,
                generation,
                free: match kind {
                    "n" => false,
                    "f" => true,
                    other => panic!("unknown cross-reference entry kind {other}"),
                },
            });
        }
    }
    rows
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}
