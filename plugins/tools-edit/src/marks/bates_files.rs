//! Bates numbering across files: each numbered after the last, written as a
//! new file named from the original, all of them or none.

use std::path::{Path, PathBuf};

use onionskin_core::{AnnotationFilter, Document};
use onionskin_plugin_api::CommandError;

use super::header_footer::{add_bates, Bates};

/// The label a failure is reported under.
const LABEL: &str = "Bates Numbering";

/// How a numbered copy is named: Acrobat's "Add to original file names".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Naming {
    /// Put before the original name.
    pub before: String,
    /// Put after it, before `.pdf`.
    pub after: String,
    /// Add the file's first and last Bates numbers after that.
    pub numbers: bool,
    /// Where the copies go; `None` beside each original.
    pub folder: Option<PathBuf>,
}

/// One file numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbered {
    pub output: PathBuf,
    pub first: String,
    pub last: String,
}

/// Number every page of `inputs`, in order, the numbers running on from one
/// file to the next, and write each as a new file named by `naming`. Nothing
/// is written unless every file numbers, and no file is overwritten.
pub fn number_files(
    inputs: &[PathBuf],
    bates: &Bates,
    naming: &Naming,
) -> Result<Vec<Numbered>, CommandError> {
    let mut start = bates.start;
    let mut outputs = Vec::with_capacity(inputs.len());
    for input in inputs {
        let mut doc = Document::open_path(input).map_err(|source| failed(input, &source))?;
        let pages: Vec<usize> = (0..doc.page_count()).collect();
        let this = Bates {
            start,
            ..bates.clone()
        };
        let (first, last) = add_bates(&mut doc, &pages, &this, "")?;
        start += pages.len() as u64;
        let bytes = doc
            .preview_bytes(AnnotationFilter::default())
            .map_err(|source| failed(input, &source))?;
        let output = output_name(input, naming, &first, &last);
        outputs.push((
            Numbered {
                output,
                first,
                last,
            },
            bytes.to_vec(),
        ));
    }
    write_all(&outputs)?;
    Ok(outputs.into_iter().map(|(numbered, _)| numbered).collect())
}

fn failed(input: &Path, source: &dyn std::fmt::Display) -> CommandError {
    CommandError::Failed {
        label: LABEL,
        reason: format!("{}: {source}", input.display()),
    }
}

/// `before` + the original stem + `after` (+ `_first-last`) + `.pdf`, in
/// the naming's folder or beside the original.
pub fn output_name(input: &Path, naming: &Naming, first: &str, last: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .map_or_else(|| "Document".into(), |stem| stem.to_string_lossy());
    let numbers = if naming.numbers {
        format!("_{first}-{last}")
    } else {
        String::new()
    };
    let name = format!("{}{stem}{}{numbers}.pdf", naming.before, naming.after);
    let folder = naming
        .folder
        .clone()
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    folder.join(name)
}

/// Write every file or none: refused if any exists already, or would be
/// written twice; each written beside its place and then linked into it,
/// which fails rather than overwrite; the ones written taken back if a later
/// one fails.
fn write_all(outputs: &[(Numbered, Vec<u8>)]) -> Result<(), CommandError> {
    let refuse = |reason: String| CommandError::Failed {
        label: LABEL,
        reason,
    };
    let mut seen = std::collections::BTreeSet::new();
    for (numbered, _) in outputs {
        let path = &numbered.output;
        if path.exists() {
            return Err(refuse(format!("{} already exists", path.display())));
        }
        if !seen.insert(path.clone()) {
            return Err(refuse(format!(
                "two files would be named {}",
                path.display()
            )));
        }
    }
    let mut placed: Vec<&Path> = Vec::new();
    for (numbered, bytes) in outputs {
        if let Err(error) = place(&numbered.output, bytes) {
            for done in placed {
                // Ours, and the error being reported is the one that failed.
                let _ = std::fs::remove_file(done);
            }
            return Err(refuse(format!(
                "could not write {}: {error}",
                numbered.output.display()
            )));
        }
        placed.push(&numbered.output);
    }
    Ok(())
}

/// Write `bytes` to a temporary beside `path`, then link it in without
/// overwriting.
fn place(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("pdf.part");
    std::fs::write(&partial, bytes)?;
    let linked = std::fs::hard_link(&partial, path);
    std::fs::remove_file(&partial)?;
    linked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_takes_its_text_and_numbers_around_the_stem() {
        let naming = Naming {
            before: "Numbered ".into(),
            after: "-final".into(),
            numbers: true,
            folder: None,
        };
        assert_eq!(
            output_name(Path::new("/cases/brief.pdf"), &naming, "A1", "A9"),
            PathBuf::from("/cases/Numbered brief-final_A1-A9.pdf")
        );
        let elsewhere = Naming {
            folder: Some("/out".into()),
            ..Naming::default()
        };
        assert_eq!(
            output_name(Path::new("brief.pdf"), &elsewhere, "", ""),
            PathBuf::from("/out/brief.pdf")
        );
    }
}
