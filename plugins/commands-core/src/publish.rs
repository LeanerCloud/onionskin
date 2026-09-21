//! Writing a command's output files: all of them or none, and never over a
//! file that is already there.
//!
//! The same failure mode export closed in B4.4: a crash or a full disk
//! halfway through must not leave a truncated PDF under the name the user
//! asked for, and a split that fails on its fourth part must not leave three.
//! Each file is written to a temporary beside its destination, and only once
//! every one is complete are they moved into place; if a move fails, the ones
//! already moved are taken back.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Why the outputs could not be published. Nothing is left behind either way.
#[derive(Debug)]
pub enum PublishError {
    /// A destination already exists. Checked before anything is written.
    Exists(PathBuf),
    /// Writing or moving `path` failed.
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists(path) => write!(f, "{} already exists", path.display()),
            Self::Write { path, source } => {
                write!(f, "could not write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PublishError {}

/// Write every `(path, bytes)`, or none of them.
pub fn publish(outputs: &[(PathBuf, Vec<u8>)]) -> Result<(), PublishError> {
    if let Some((path, _)) = outputs.iter().find(|(path, _)| path.exists()) {
        return Err(PublishError::Exists(path.clone()));
    }
    let staged = outputs
        .iter()
        .map(|(path, bytes)| stage(path, bytes))
        .collect::<Result<Vec<_>, _>>()?;

    let mut placed: Vec<&Path> = Vec::with_capacity(staged.len());
    for (file, (path, _)) in staged.into_iter().zip(outputs) {
        if let Err(failure) = file.persist_noclobber(path) {
            for done in placed {
                // Best effort: the error being reported is the move that
                // failed, and a file we just created is ours to remove.
                let _ = std::fs::remove_file(done);
            }
            return Err(match failure.error.kind() {
                std::io::ErrorKind::AlreadyExists => PublishError::Exists(path.clone()),
                _ => PublishError::Write {
                    path: path.clone(),
                    source: failure.error,
                },
            });
        }
        placed.push(path);
    }
    Ok(())
}

/// `bytes` in a temporary file in `path`'s directory, flushed to disk.
fn stage(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile, PublishError> {
    let failed = |source| PublishError::Write {
        path: path.to_path_buf(),
        source,
    };
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(failed)?;
    file.write_all(bytes).map_err(failed)?;
    file.as_file().sync_all().map_err(failed)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_output_is_written_when_nothing_is_in_the_way() {
        let dir = tempfile::tempdir().expect("dir");
        let outputs = vec![
            (dir.path().join("a.pdf"), b"one".to_vec()),
            (dir.path().join("b.pdf"), b"two".to_vec()),
        ];
        publish(&outputs).expect("publishes");
        for (path, bytes) in &outputs {
            assert_eq!(&std::fs::read(path).expect("written"), bytes);
        }
    }

    /// Refused before anything is written: the first output is not there
    /// afterwards either, and the file in the way is untouched.
    #[test]
    fn an_existing_destination_refuses_the_whole_set() {
        let dir = tempfile::tempdir().expect("dir");
        let taken = dir.path().join("b.pdf");
        std::fs::write(&taken, b"mine").expect("write");
        let outputs = vec![
            (dir.path().join("a.pdf"), b"one".to_vec()),
            (taken.clone(), b"two".to_vec()),
        ];
        assert!(matches!(publish(&outputs), Err(PublishError::Exists(path)) if path == taken));
        assert!(!dir.path().join("a.pdf").exists());
        assert_eq!(std::fs::read(&taken).expect("read"), b"mine");
        let leftovers = std::fs::read_dir(dir.path()).expect("list").count();
        assert_eq!(leftovers, 1, "no temporary file is left behind");
    }
}
