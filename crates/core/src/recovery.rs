//! Autosave's recovery file.
//!
//! **This is the user's document content, and it is treated that way.** The
//! section a recovery file holds is made of objects read out of the user's PDF
//! and the edits they are about to write, so a recovery file for a contract
//! under review contains that contract. It is not a preference, and it does not
//! go where preferences go.
//!
//! Four rules, each asserted by reading the result back rather than assuming
//! it from the call that was meant to produce it:
//!
//! 1. Recovery files live in **their own directory**, never the config one.
//! 2. That directory's mode is **verified** to be `0o700` after it is opened or
//!    created, and the store refuses to exist if it is not. A directory that
//!    predates this rule at `0o755` is exactly the case `known-issues.md`
//!    records for the config directory, and inheriting that here would put
//!    document bytes somewhere world-readable.
//! 3. Each file is **verified** to be `0o600`.
//! 4. A recovery file is **deleted** as soon as its document is saved or its tab
//!    closed cleanly, so the window in which document bytes exist outside the
//!    document is as short as the feature allows.
//!
//! **The format is the incremental section itself**, behind a short header.
//! That reuses the one serializer this crate already trusts rather than
//! inventing a second one for the overlay, and it makes replay a matter of
//! appending bytes. The header records the length and a checksum of the
//! original the section was built against, which is what stops a replay from
//! double-applying an edit: once the document has been saved, the file on disk
//! is the original *plus* the section, its length and checksum no longer match,
//! and the recovery is reported stale rather than applied a second time.
//!
//! Mode checks are Unix-only. Elsewhere the directory and files are created
//! with the platform's defaults, and the evidence says so rather than claiming
//! a guarantee that is not enforced.

use std::io::Write as _;
use std::path::{Path, PathBuf};

const MAGIC: &[u8] = b"ONIONSKIN-RECOVERY 1\n";
const EXTENSION: &str = "recovery";

/// Why the recovery store refused to do something.
#[derive(Debug)]
pub enum RecoveryError {
    /// The directory exists with a mode other than owner-only. Nothing is
    /// written into it.
    DirectoryNotPrivate {
        path: PathBuf,
        mode: u32,
    },
    /// A file was written with a mode other than owner-only and was removed.
    FileNotPrivate {
        path: PathBuf,
        mode: u32,
    },
    /// The file is not a recovery file this version wrote.
    Unrecognized {
        path: PathBuf,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoveryError::DirectoryNotPrivate { path, mode } => write!(
                f,
                "the recovery directory {} is mode {mode:o}, not 700; recovery is off \
                 until it is owner-only, because recovery files contain document content",
                path.display()
            ),
            RecoveryError::FileNotPrivate { path, mode } => write!(
                f,
                "{} was created mode {mode:o}, not 600, and was removed",
                path.display()
            ),
            RecoveryError::Unrecognized { path } => {
                write!(f, "{} is not a recovery file", path.display())
            }
            RecoveryError::Io(error) => write!(f, "recovery: {error}"),
        }
    }
}

impl std::error::Error for RecoveryError {}

impl From<std::io::Error> for RecoveryError {
    fn from(error: std::io::Error) -> Self {
        RecoveryError::Io(error)
    }
}

/// What reading a recovery file found.
#[derive(Debug, PartialEq, Eq)]
pub enum Recovered {
    /// No recovery file exists for this document.
    Nothing,
    /// The recovery applies: these are the bytes the session had.
    Bytes(Vec<u8>),
    /// The document on disk is not the one the recovery was built against,
    /// which is what a save after the last autosave looks like. Applying it
    /// would double-apply the edits, so it is reported and not applied.
    Stale,
}

/// The directory recovery files live in, verified private.
#[derive(Clone, Debug)]
pub struct RecoveryStore {
    dir: PathBuf,
}

impl RecoveryStore {
    /// Open or create `dir`, and refuse unless it is owner-only.
    ///
    /// An existing directory is **not** narrowed silently: a directory someone
    /// widened on purpose is their decision, but a document-content store is
    /// not allowed to live in it, so the answer is a visible refusal rather
    /// than a quiet chmod.
    pub fn open(dir: &Path) -> Result<Self, RecoveryError> {
        create_private_dir(dir)?;
        verify_mode(dir, 0o700).map_err(|mode| RecoveryError::DirectoryNotPrivate {
            path: dir.to_path_buf(),
            mode,
        })?;
        Ok(RecoveryStore {
            dir: dir.to_path_buf(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file a document's recovery lives in.
    ///
    /// Named by an opaque checksum of the document's path, never by the path
    /// or its file name: the names of the files in a directory are themselves
    /// a record of which documents someone had open.
    pub fn path_for(&self, document: &Path) -> PathBuf {
        let key = fnv1a64(document.as_os_str().as_encoded_bytes());
        self.dir.join(format!("{key:016x}.{EXTENSION}"))
    }

    /// Write one recovery file: the header, then the section.
    ///
    /// Written to a temporary file created owner-only, verified, then renamed
    /// into place, so a crash mid-write leaves the previous recovery rather than
    /// a truncated one.
    pub fn write(
        &self,
        document: &Path,
        original: &[u8],
        section: &[u8],
    ) -> Result<PathBuf, RecoveryError> {
        let path = self.path_for(document);
        let temporary = path.with_extension(format!("{EXTENSION}.tmp"));
        let _ = std::fs::remove_file(&temporary);

        let mut file = create_private_file(&temporary)?;
        if let Err(mode) = verify_mode(&temporary, 0o600) {
            let _ = std::fs::remove_file(&temporary);
            return Err(RecoveryError::FileNotPrivate {
                path: temporary,
                mode,
            });
        }
        file.write_all(MAGIC)?;
        file.write_all(format!("{}\n", original.len()).as_bytes())?;
        file.write_all(format!("{:016x}\n", fnv1a64(original)).as_bytes())?;
        file.write_all(section)?;
        file.sync_all()?;
        drop(file);

        std::fs::rename(&temporary, &path)?;
        if let Err(mode) = verify_mode(&path, 0o600) {
            let _ = std::fs::remove_file(&path);
            return Err(RecoveryError::FileNotPrivate { path, mode });
        }
        Ok(path)
    }

    /// Read a document's recovery back against the original it has to apply to.
    pub fn recover(&self, document: &Path, original: &[u8]) -> Result<Recovered, RecoveryError> {
        let path = self.path_for(document);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Recovered::Nothing)
            }
            Err(error) => return Err(error.into()),
        };
        let Some((length, checksum, section)) = parse(&bytes) else {
            return Err(RecoveryError::Unrecognized { path });
        };
        if length != original.len() || checksum != fnv1a64(original) {
            return Ok(Recovered::Stale);
        }
        let mut recovered = Vec::with_capacity(original.len() + section.len());
        recovered.extend_from_slice(original);
        recovered.extend_from_slice(section);
        Ok(Recovered::Bytes(recovered))
    }

    /// Delete a document's recovery. Absent is not an error: a document saved
    /// before any autosave never had one.
    pub fn discard(&self, document: &Path) -> Result<(), RecoveryError> {
        match std::fs::remove_file(self.path_for(document)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn parse(bytes: &[u8]) -> Option<(usize, u64, &[u8])> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let (length, rest) = line(rest)?;
    let (checksum, rest) = line(rest)?;
    Some((
        std::str::from_utf8(length).ok()?.parse().ok()?,
        u64::from_str_radix(std::str::from_utf8(checksum).ok()?, 16).ok()?,
        rest,
    ))
}

fn line(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let end = bytes.iter().position(|byte| *byte == b'\n')?;
    Some((&bytes[..end], &bytes[end + 1..]))
}

/// FNV-1a, 64 bits.
///
/// A staleness check, not an integrity guarantee: it answers "is this the
/// document the recovery was built against", which an accident can get wrong
/// and FNV-1a will catch. It is not a defence against someone crafting a
/// collision, and nothing here needs one; that is why this is five lines
/// rather than a dependency.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// The mode actually on disk, compared against what was asked for. Returns the
/// observed mode on a mismatch so the error can name it.
#[cfg(unix)]
fn verify_mode(path: &Path, expected: u32) -> Result<(), u32> {
    use std::os::unix::fs::PermissionsExt as _;
    // An unreadable path reports mode 0, which never equals what was asked
    // for, so it refuses rather than passing.
    let mode = std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0);
    if mode == expected {
        Ok(())
    } else {
        Err(mode)
    }
}

#[cfg(not(unix))]
fn verify_mode(_path: &Path, _expected: u32) -> Result<(), u32> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_distinguishes_documents_that_differ_by_one_byte() {
        assert_ne!(fnv1a64(b"%PDF-1.7 a"), fnv1a64(b"%PDF-1.7 b"));
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn a_header_round_trips() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(b"42\n00000000000000ff\nSECTION");
        let (length, checksum, section) = parse(&bytes).expect("parses");
        assert_eq!((length, checksum, section), (42, 0xff, &b"SECTION"[..]));
    }

    #[test]
    fn something_that_is_not_a_recovery_file_does_not_parse() {
        assert!(parse(b"%PDF-1.7\n").is_none());
        assert!(parse(MAGIC).is_none());
    }
}
