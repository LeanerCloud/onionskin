// Each integration test uses a different part of this harness.
#![allow(dead_code)]

//! Corpus discovery and tallying shared by the guarantee tests.
//!
//! `corpus/external/` and `corpus/malformed/` are gitignored, so they are
//! absent from a fresh clone and from CI. A test that cannot find its corpus
//! says so loudly and returns; it never reports a pass it did not earn.

pub(crate) mod fixtures;
#[allow(unused_imports)]
pub(crate) use fixtures::skeleton;

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
    classic_pdf_covering(bodies, offset_lies, bodies.len() as u32)
}

/// As `classic_pdf`, but the table covers only objects 1 through `covers`.
/// Bodies above that are in the file and absent from the cross-reference, the
/// way objects left out of a section that was never written would be: a scan
/// finds them, the table does not know them.
pub fn classic_pdf_covering(bodies: &[&[u8]], offset_lies: &[(u32, u64)], covers: u32) -> Vec<u8> {
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
    offsets.truncate(covers as usize);

    let xref_at = bytes.len();
    let size = offsets.len() + 1;
    bytes.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in &offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// A PDF 1.5 file whose cross-reference is a stream, object 4, rather than a
/// table. `declared_length` overrides the `/Length` that stream writes about
/// its own data, which is how a test produces a document whose entire table
/// rests on a boundary the parser had to guess.
pub fn xref_stream_pdf(declared_length: Option<i64>) -> Vec<u8> {
    let header = b"%PDF-1.5\n";
    let catalog = b"1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n";
    let pages = b"2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj\n";
    let page =
        b"3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<<>>>>\nendobj\n";

    let catalog_at = header.len();
    let pages_at = catalog_at + catalog.len();
    let page_at = pages_at + pages.len();
    let xref_at = page_at + page.len();

    // /W [1 2 1]: type, a two-byte offset, then the generation.
    let row = |kind: u8, field: usize, last: u8| [kind, (field >> 8) as u8, field as u8, last];
    let mut rows = Vec::new();
    rows.extend_from_slice(&row(0, 0, 255));
    rows.extend_from_slice(&row(1, catalog_at, 0));
    rows.extend_from_slice(&row(1, pages_at, 0));
    rows.extend_from_slice(&row(1, page_at, 0));
    rows.extend_from_slice(&row(1, xref_at, 0));

    let mut bytes = Vec::new();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(catalog);
    bytes.extend_from_slice(pages);
    bytes.extend_from_slice(page);
    bytes.extend_from_slice(
        format!(
            "4 0 obj\n<</Type/XRef/Size 5/W[1 2 1]/Index[0 5]/Root 1 0 R/Length {}>>\nstream\n",
            declared_length.unwrap_or(rows.len() as i64)
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&rows);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    bytes
}

/// One row of a classic cross-reference table, as read back out of bytes this
/// crate wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XrefRow {
    pub number: u32,
    /// A byte offset for an in-use entry, the next free object number for a
    /// free one (ISO 32000-1 7.5.4).
    pub field: u64,
    pub generation: u16,
    pub free: bool,
}

/// The classic table of the last cross-reference section in `bytes`: what the
/// save just appended, checked rather than taken on trust.
pub fn last_xref_table(bytes: &[u8]) -> Vec<XrefRow> {
    let at = *table_offsets(bytes)
        .last()
        .expect("the file has a classic cross-reference table");
    parse_xref_table(&bytes[at..])
}

/// The table a reader ends up with after following the whole chain: later
/// sections override earlier ones, entry by entry.
pub fn effective_xref(bytes: &[u8]) -> BTreeMap<u32, XrefRow> {
    let mut merged = BTreeMap::new();
    for at in table_offsets(bytes) {
        for row in parse_xref_table(&bytes[at..]) {
            merged.insert(row.number, row);
        }
    }
    merged
}

/// Where each `xref` keyword in the file starts, oldest section first.
fn table_offsets(bytes: &[u8]) -> Vec<usize> {
    let needle = b"\nxref\n";
    bytes
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at + 1)
        .collect()
}

fn parse_xref_table(from: &[u8]) -> Vec<XrefRow> {
    let text = String::from_utf8_lossy(from);
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
