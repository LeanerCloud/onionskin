//! The recents list behind File > Open Recent and the Home view.
//!
//! Local only, as the parity rows say: no account, no cloud, no starred
//! list (that is M3). The file is `~/.config/onionskin/recents.json` and is
//! written owner-only, because a list of document paths says what a user has
//! been reading and a home directory is not always theirs alone.
//!
//! Paths are absolute. A recents entry has to survive the working directory
//! changing between sessions, so there is no relative form that would work;
//! what the file permission buys is that only the account that opened the
//! documents can read the list.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// One document the user opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentDocument {
    pub path: PathBuf,
    /// Seconds since the epoch, so the file stays readable and the ordering
    /// survives a machine whose clock moved.
    pub opened_at: u64,
}

impl RecentDocument {
    /// What a menu row calls it: the file name, or the whole path when there
    /// is no file name to show.
    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[derive(Debug)]
pub enum RecentsError {
    Unreadable {
        path: PathBuf,
        source: io::Error,
    },
    Malformed {
        path: PathBuf,
        message: String,
    },
    Unwritable {
        path: PathBuf,
        source: io::Error,
    },
    /// A document path that is not valid UTF-8, which JSON cannot carry.
    /// Rare, and a real filename on every platform that allows one.
    Unrepresentable {
        path: PathBuf,
    },
}

impl fmt::Display for RecentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => {
                write!(f, "{} could not be read: {source}", path.display())
            }
            Self::Unwritable { path, source } => {
                write!(f, "{} could not be written: {source}", path.display())
            }
            Self::Malformed { path, message } => write!(
                f,
                "{} is not a recent-documents list: {message}",
                path.display()
            ),
            Self::Unrepresentable { path } => write!(
                f,
                "{} cannot be added to the recents list: its name is not valid UTF-8",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RecentsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } | Self::Unwritable { source, .. } => Some(source),
            Self::Malformed { .. } | Self::Unrepresentable { .. } => None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentsFile {
    documents: Vec<RecentDocument>,
}

/// The list, most recently opened first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recents {
    documents: Vec<RecentDocument>,
}

impl Recents {
    pub fn load(path: Option<&Path>) -> (Self, Vec<RecentsError>) {
        let Some(path) = path else {
            return (Self::default(), Vec::new());
        };
        match crate::config::read(path) {
            Ok(None) => (Self::default(), Vec::new()),
            Ok(Some(source)) => match serde_json::from_str::<RecentsFile>(&source) {
                Ok(file) => (
                    Self {
                        documents: file.documents,
                    },
                    Vec::new(),
                ),
                Err(error) => (
                    Self::default(),
                    vec![RecentsError::Malformed {
                        path: path.to_path_buf(),
                        message: error.to_string(),
                    }],
                ),
            },
            Err(source) => (
                Self::default(),
                vec![RecentsError::Unreadable {
                    path: path.to_path_buf(),
                    source,
                }],
            ),
        }
    }

    pub fn documents(&self) -> &[RecentDocument] {
        &self.documents
    }

    pub fn get(&self, index: usize) -> Option<&RecentDocument> {
        self.documents.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Record a document as just opened, and return whether the list changed.
    ///
    /// Re-opening a document moves it to the front rather than adding a
    /// second row: the list is of documents, not of openings.
    pub fn record(&mut self, path: &Path, opened_at: SystemTime, limit: usize) -> bool {
        // Compared against a copy rather than reasoned about: "did this
        // change" has to be right for the limit-of-zero and
        // already-at-the-front cases, and this is a handful of paths on a
        // user-driven open.
        let before = self.documents.clone();
        self.documents.retain(|recent| recent.path != path);
        self.documents.insert(
            0,
            RecentDocument {
                path: path.to_path_buf(),
                opened_at: opened_at
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs()),
            },
        );
        self.truncate(limit);
        before != self.documents
    }

    /// Drop everything past `limit`, which the Documents preference sets.
    pub fn truncate(&mut self, limit: usize) {
        self.documents.truncate(limit);
    }

    /// Write the list.
    ///
    /// A document whose name is not valid UTF-8 has no JSON form, and
    /// serde's `Path` refuses it rather than inventing one. Reported as
    /// itself: this used to be an `expect`, which made an unusual filename a
    /// panic inside the window update that opened it.
    pub fn save(&self, path: &Path) -> Result<(), RecentsError> {
        if let Some(unrepresentable) = self
            .documents
            .iter()
            .find(|recent| recent.path.to_str().is_none())
        {
            return Err(RecentsError::Unrepresentable {
                path: unrepresentable.path.clone(),
            });
        }
        let json = serde_json::to_string_pretty(&RecentsFile {
            documents: self.documents.clone(),
        })
        .expect("every path is UTF-8 by the check above, and a timestamp is a number");
        crate::config::write_private(path, &json).map_err(|source| RecentsError::Unwritable {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn the_most_recently_opened_document_is_first() {
        let mut recents = Recents::default();

        recents.record(Path::new("/docs/a.pdf"), at(10), 10);
        recents.record(Path::new("/docs/b.pdf"), at(20), 10);

        assert_eq!(
            recents
                .documents()
                .iter()
                .map(RecentDocument::title)
                .collect::<Vec<_>>(),
            vec!["b.pdf", "a.pdf"]
        );
        assert_eq!(recents.get(0).unwrap().opened_at, 20);
    }

    #[test]
    fn re_opening_a_document_moves_it_rather_than_repeating_it() {
        let mut recents = Recents::default();
        recents.record(Path::new("/docs/a.pdf"), at(10), 10);
        recents.record(Path::new("/docs/b.pdf"), at(20), 10);

        let changed = recents.record(Path::new("/docs/a.pdf"), at(30), 10);

        assert!(changed);
        assert_eq!(recents.documents().len(), 2);
        assert_eq!(recents.get(0).unwrap().path, Path::new("/docs/a.pdf"));
        assert_eq!(recents.get(0).unwrap().opened_at, 30);
    }

    /// The Documents preference is the limit, and lowering it takes effect
    /// on the next open rather than waiting for a restart.
    #[test]
    fn the_list_never_grows_past_the_limit_it_is_given() {
        let mut recents = Recents::default();
        for index in 0..5 {
            recents.record(&PathBuf::from(format!("/docs/{index}.pdf")), at(index), 3);
        }

        assert_eq!(recents.documents().len(), 3);
        assert_eq!(recents.get(0).unwrap().title(), "4.pdf");

        recents.truncate(1);
        assert_eq!(recents.documents().len(), 1);
    }

    /// Zero is the Documents preference's "keep no recents": recording still
    /// answers honestly rather than leaving an entry the user turned off.
    #[test]
    fn a_limit_of_zero_keeps_nothing() {
        let mut recents = Recents::default();

        let changed = recents.record(Path::new("/docs/a.pdf"), at(10), 0);

        assert!(!changed);
        assert!(recents.is_empty());
    }

    /// A filename that is not UTF-8 is still a filename.
    #[cfg(unix)]
    #[test]
    fn a_document_whose_name_is_not_utf8_is_reported_rather_than_fatal() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let path = crate::config::test_dir("recents-non-utf8").join("recents.json");
        let mut recents = Recents::default();
        recents.record(Path::new(OsStr::from_bytes(b"/docs/\xff.pdf")), at(10), 10);

        let error = recents.save(&path).expect_err("the path has no JSON form");

        assert!(error.to_string().contains("not valid UTF-8"), "{error}");
    }

    #[test]
    fn the_list_round_trips_through_its_file() {
        let path = crate::config::test_dir("recents-round-trip").join("recents.json");
        let mut recents = Recents::default();
        recents.record(Path::new("/docs/a.pdf"), at(10), 10);
        recents.record(Path::new("/docs/b.pdf"), at(20), 10);

        recents.save(&path).expect("the list saves");
        let (loaded, errors) = Recents::load(Some(&path));

        assert!(errors.is_empty());
        assert_eq!(loaded, recents);
    }

    #[test]
    fn a_malformed_list_is_reported_and_the_app_starts_with_none() {
        let path = crate::config::test_dir("recents-malformed").join("recents.json");
        std::fs::write(&path, "not json").expect("the test writes its file");

        let (recents, errors) = Recents::load(Some(&path));

        assert!(recents.is_empty());
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0]
                .to_string()
                .contains("is not a recent-documents list"),
            "{errors:?}"
        );
    }

    /// The list names documents the user opened, so it is written the way a
    /// private file is written. Pinned here as well as in `config` because
    /// this is the list that made the rule.
    #[cfg(unix)]
    #[test]
    fn the_saved_list_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = crate::config::test_dir("recents-mode").join("recents.json");
        let _ = std::fs::remove_file(&path);
        let mut recents = Recents::default();
        recents.record(Path::new("/docs/private.pdf"), at(10), 10);

        recents.save(&path).expect("the list saves");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "group and other must have no access");
    }
}
