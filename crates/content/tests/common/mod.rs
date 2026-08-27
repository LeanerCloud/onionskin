// Each integration test uses a different part of this harness.
#![allow(dead_code)]

//! Corpus discovery and the oracle tally shared by the extraction tests.
//!
//! `corpus/external/` is gitignored, so it is absent from a fresh clone and
//! from CI. A test that cannot find its corpus says so loudly and returns; it
//! never reports a pass it did not earn. This mirrors the contract
//! `crates/cos/tests/common` sets for the structural guarantee tests; the
//! tallies differ because a text run is scored by how much of it matched, not
//! by whether it threw.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---- synthetic documents ----------------------------------------------------

/// Assembles numbered object bodies into a one-page PDF with a correct classic
/// xref table, the same shape `corpus/make-seeds.py` writes.
///
/// This is how the regression tests get a document that exhibits one specific
/// malformation. The corpus proves extraction works on real files; these prove
/// it behaves on the file that broke it, which no corpus file may contain.
pub fn build_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// A stream object body with a correct `/Length`.
pub fn stream(dict_body: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict_body} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// One page, 200x200, with `content` as its only content stream.
///
/// Objects 1 to 4 are the catalog, page tree, page and content stream, so
/// `extra` starts at object 5 and `resources` refers to it by number.
pub fn one_page(content: &str, resources: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
             /Resources {resources} /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
    ];
    objects.extend_from_slice(extra);
    build_pdf(&objects)
}

/// Opens synthetic bytes, refusing anything that needs repair so a malformed
/// fixture cannot pass for a well-formed one.
pub fn open_bytes(bytes: Vec<u8>) -> onionskin_cos::Document {
    onionskin_cos::Document::open(Box::new(onionskin_cos::BytesSource::new(bytes)))
        .expect("synthetic fixture is well formed")
}

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

pub fn short(path: &Path) -> String {
    match corpus_root() {
        Some(root) => path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string(),
        None => path.display().to_string(),
    }
}

/// Opens a document the way the viewer will: repairing when it has to, and
/// treating an encrypted file as skipped rather than failed.
pub fn open(path: &Path) -> Result<onionskin_cos::Document, String> {
    match onionskin_cos::Document::open_path_repairing(path) {
        Ok((doc, _)) => Ok(doc),
        Err(e) => Err(e.category().to_string()),
    }
}

// ---- oracle scoring ---------------------------------------------------------

/// Text normalisation applied to both sides of a `pdftotext` comparison.
///
/// Every rule here exists because the difference it erases is not a
/// correctness difference:
///
/// * **Whitespace collapses to single spaces and all line structure is
///   dropped.** Line and column layout is `pdftotext`'s job and is explicitly
///   not this milestone's; comparing it would score layout analysis rather
///   than extraction.
/// * **Ligatures decompose.** `ﬁ` and `fi` are the same two characters to a
///   reader and to a search; which one comes out depends on whether the font's
///   `/ToUnicode` decomposed them, and both are defensible.
/// * **Unicode punctuation folds to ASCII.** Soft hyphens, non-breaking
///   spaces, the several dashes and the directional quotes are producer
///   choices, not extraction differences.
///
/// Nothing here erases a wrong character, a missing character, or a character
/// in the wrong order. Those are what the comparison is measuring.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        let expanded: &str = match ch {
            '\u{FB00}' => "ff",
            '\u{FB01}' => "fi",
            '\u{FB02}' => "fl",
            '\u{FB03}' => "ffi",
            '\u{FB04}' => "ffl",
            '\u{FB05}' | '\u{FB06}' => "st",
            '\u{2018}' | '\u{2019}' | '\u{201B}' | '\u{2032}' => "'",
            '\u{201C}' | '\u{201D}' | '\u{201F}' | '\u{2033}' => "\"",
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => "-",
            '\u{2026}' => "...",
            '\u{00A0}' | '\u{2007}' | '\u{202F}' | '\u{2009}' | '\u{200A}' => " ",
            // Soft hyphen and the zero-width formatting characters are
            // invisible; neither side is wrong to keep or drop them.
            '\u{00AD}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' => "",
            _ => {
                if ch.is_whitespace() || ch.is_control() {
                    pending_space = !out.is_empty();
                    continue;
                }
                if pending_space {
                    out.push(' ');
                    pending_space = false;
                }
                out.push(ch);
                continue;
            }
        };
        if expanded.is_empty() {
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push_str(expanded);
    }
    out
}

/// Longest common subsequence length over characters, capped so a pathological
/// pair cannot run the suite out of time. The cap is reported rather than
/// silently applied.
pub fn similarity(a: &str, b: &str) -> Option<f64> {
    const MAX_CELLS: usize = 24_000_000;
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return Some(1.0);
    }
    if a.is_empty() || b.is_empty() {
        return Some(0.0);
    }
    if a.len().saturating_mul(b.len()) > MAX_CELLS {
        return None;
    }
    let mut previous = vec![0u32; b.len() + 1];
    let mut current = vec![0u32; b.len() + 1];
    for x in &a {
        for (j, y) in b.iter().enumerate() {
            current[j + 1] = if x == y {
                previous[j] + 1
            } else {
                current[j].max(previous[j + 1])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let lcs = f64::from(previous[b.len()]);
    Some(2.0 * lcs / (a.len() + b.len()) as f64)
}

/// Order-independent overlap: how much of the character content matched,
/// ignoring where it landed.
///
/// Reported beside [`similarity`] because the two answer different questions.
/// A page whose text is entirely right but drawn in a different order than
/// poppler sorts it scores badly on sequence similarity and perfectly here,
/// and that gap is exactly the milestone's "document order, no layout
/// analysis" boundary rather than an extraction error.
pub fn char_overlap(a: &str, b: &str) -> f64 {
    let count = |t: &str| {
        let mut map: BTreeMap<char, usize> = BTreeMap::new();
        for ch in t.chars().filter(|c| !c.is_whitespace()) {
            *map.entry(ch).or_default() += 1;
        }
        map
    };
    let (a, b) = (count(a), count(b));
    let total: usize = a.values().sum::<usize>() + b.values().sum::<usize>();
    if total == 0 {
        return 1.0;
    }
    let shared: usize = a
        .iter()
        .map(|(ch, n)| *n.min(b.get(ch).unwrap_or(&0)))
        .sum();
    2.0 * shared as f64 / total as f64
}

/// Per-corpus scoring for an oracle run. Reports honestly: how many files
/// matched exactly, how the rest are distributed, and what the failures were.
#[derive(Default)]
pub struct Oracle {
    pub name: String,
    pub scores: Vec<(String, f64)>,
    pub overlaps: Vec<f64>,
    pub skipped: BTreeMap<String, Vec<String>>,
    pub failed: BTreeMap<String, Vec<String>>,
}

impl Oracle {
    pub fn new(name: &str) -> Oracle {
        Oracle {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn score(&mut self, file: &Path, similarity: f64, overlap: f64) {
        self.scores.push((short(file), similarity));
        self.overlaps.push(overlap);
    }

    pub fn mean_overlap(&self) -> f64 {
        if self.overlaps.is_empty() {
            return 0.0;
        }
        self.overlaps.iter().sum::<f64>() / self.overlaps.len() as f64
    }

    pub fn skip(&mut self, file: &Path, category: &str) {
        self.skipped
            .entry(category.to_string())
            .or_default()
            .push(short(file));
    }

    pub fn fail(&mut self, file: &Path, category: &str, detail: &str) {
        self.failed
            .entry(category.to_string())
            .or_default()
            .push(format!("{}: {detail}", short(file)));
    }

    pub fn bucket(&self, low: f64, high: f64) -> usize {
        self.scores
            .iter()
            .filter(|(_, s)| *s >= low && *s < high)
            .count()
    }

    pub fn mean(&self) -> f64 {
        if self.scores.is_empty() {
            return 0.0;
        }
        self.scores.iter().map(|(_, s)| s).sum::<f64>() / self.scores.len() as f64
    }

    pub fn report(&self) {
        let total = self.scores.len()
            + self.skipped.values().map(Vec::len).sum::<usize>()
            + self.failed.values().map(Vec::len).sum::<usize>();
        println!(
            "\n== {} == {total} files, {} compared, mean sequence similarity {:.4}, mean character overlap {:.4}",
            self.name,
            self.scores.len(),
            self.mean(),
            self.mean_overlap()
        );
        println!(
            "   exact 1.00: {:>5}   >=0.99: {:>5}   >=0.95: {:>5}   >=0.80: {:>5}   <0.80: {:>5}",
            self.bucket(1.0, f64::INFINITY),
            self.bucket(0.99, 1.0),
            self.bucket(0.95, 0.99),
            self.bucket(0.80, 0.95),
            self.bucket(f64::NEG_INFINITY, 0.80),
        );
        for (category, files) in &self.skipped {
            println!("   skipped [{category}]: {}", files.len());
        }
        for (category, files) in &self.failed {
            println!("   FAILED  [{category}]: {}", files.len());
            for file in files.iter().take(5) {
                println!("       {file}");
            }
            if files.len() > 5 {
                println!("       ... and {} more", files.len() - 5);
            }
        }
        let mut worst: Vec<&(String, f64)> =
            self.scores.iter().filter(|(_, s)| *s < 0.80).collect();
        worst.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (file, score) in worst.iter().take(15) {
            println!("   worst {score:.3}  {file}");
        }
    }
}

/// Runs poppler's `pdftotext` over a file. `None` when the tool is absent,
/// which the caller must report as a skip rather than a pass.
pub fn pdftotext(path: &Path, first: usize, last: usize) -> Option<Result<String, String>> {
    let output = std::process::Command::new("pdftotext")
        .arg("-q")
        .arg("-f")
        .arg(first.to_string())
        .arg("-l")
        .arg(last.to_string())
        .arg("-enc")
        .arg("UTF-8")
        .arg(path)
        .arg("-")
        .output();
    match output {
        Ok(out) => Some(Ok(String::from_utf8_lossy(&out.stdout).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(e.to_string())),
    }
}

pub fn have_pdftotext() -> bool {
    std::process::Command::new("pdftotext")
        .arg("-v")
        .output()
        .is_ok()
}
