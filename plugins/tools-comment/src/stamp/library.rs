//! Custom stamps: the user's own, kept as one-page PDFs on disk.
//!
//! `<library>/<category>/<name>.pdf`. The folder is the category and the file
//! name is the stamp's name, so there is no index to fall out of step with
//! the files, and a user who copies a stamp PDF into the folder has added a
//! stamp. Built-in stamps are not here and cannot be: they are compiled in,
//! so nothing a user does in this folder can remove one.
//!
//! A custom stamp is always a page. One made from an image is the page the
//! image importer makes of it (P14a); one made from a PDF is the chosen page,
//! extracted through `core::pages`, which copies what the page needs and
//! refuses an encrypted source.

use std::fmt;
use std::path::{Path, PathBuf};

use onionskin_core::pages::extract_pages;
use onionskin_cos::{BytesSource, Document as CosDocument};

/// One custom stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomStamp {
    pub category: String,
    pub name: String,
    pub path: PathBuf,
}

impl CustomStamp {
    /// The id the stamp tool knows it by.
    pub fn id(&self) -> String {
        format!("{CUSTOM_PREFIX}{}/{}", self.category, self.name)
    }
}

/// What a custom stamp's id starts with.
pub(crate) const CUSTOM_PREFIX: &str = "custom:";

#[derive(Debug)]
pub enum LibraryError {
    /// A category or name that is empty or would name another directory.
    InvalidName(String),
    /// A stamp by that name is already in that category.
    Exists(String),
    /// The page could not be taken out of the source: encrypted, or not a PDF.
    Source(onionskin_core::Error),
    Io(std::io::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(
                f,
                "{name:?} cannot be a stamp's name or category: it is empty or names a folder"
            ),
            Self::Exists(name) => write!(f, "there is already a stamp called {name:?} there"),
            Self::Source(error) => write!(f, "the stamp's page could not be read: {error}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LibraryError {}

impl From<std::io::Error> for LibraryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The folder of custom stamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampLibrary {
    dir: PathBuf,
}

impl StampLibrary {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every custom stamp, by category and then name. A missing folder is an
    /// empty library, which is what a first run looks like.
    pub fn list(&self) -> Vec<CustomStamp> {
        let mut found = Vec::new();
        for category in entries(&self.dir).filter(|path| path.is_dir()) {
            let Some(category_name) = file_name(&category) else {
                continue;
            };
            for file in entries(&category).filter(|path| is_pdf(path)) {
                if let Some(name) = file.file_stem().and_then(|stem| stem.to_str()) {
                    found.push(CustomStamp {
                        category: category_name.clone(),
                        name: name.to_owned(),
                        path: file.clone(),
                    });
                }
            }
        }
        found.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
        found
    }

    /// The stamp with this id, if the library has it.
    pub fn find(&self, id: &str) -> Option<CustomStamp> {
        self.list().into_iter().find(|stamp| stamp.id() == id)
    }

    /// Page `page` of the PDF `source` as a new stamp. Refused if the name is
    /// taken, unless `replace`.
    pub fn add(
        &self,
        category: &str,
        name: &str,
        source: &[u8],
        page: usize,
        replace: bool,
    ) -> Result<CustomStamp, LibraryError> {
        check(category)?;
        check(name)?;
        let folder = self.dir.join(category);
        let path = folder.join(format!("{name}.pdf"));
        if path.exists() && !replace {
            return Err(LibraryError::Exists(name.to_owned()));
        }
        let (document, _) =
            CosDocument::open_repairing(Box::new(BytesSource::new(source.to_vec())))
                .map_err(|error| LibraryError::Source(error.into()))?;
        let bytes = extract_pages(&document, &[page]).map_err(LibraryError::Source)?;
        std::fs::create_dir_all(&folder)?;
        write_whole(&path, &bytes)?;
        Ok(CustomStamp {
            category: category.to_owned(),
            name: name.to_owned(),
            path,
        })
    }

    /// Delete a custom stamp, and its category folder once that is empty.
    pub fn remove(&self, stamp: &CustomStamp) -> Result<(), LibraryError> {
        // Only a stamp this library lists: an id cannot be made to point a
        // delete anywhere outside the folder.
        let listed = self
            .list()
            .into_iter()
            .find(|candidate| candidate == stamp)
            .ok_or_else(|| LibraryError::InvalidName(stamp.name.clone()))?;
        std::fs::remove_file(&listed.path)?;
        let folder = self.dir.join(&listed.category);
        if entries(&folder).next().is_none() {
            let _ = std::fs::remove_dir(&folder);
        }
        Ok(())
    }
}

/// A category or a name: one path component, and a real one.
fn check(name: &str) -> Result<(), LibraryError> {
    let refused = name.trim().is_empty()
        || name.contains(['/', '\\', '\0'])
        || name.trim_matches('.').is_empty();
    if refused {
        return Err(LibraryError::InvalidName(name.to_owned()));
    }
    Ok(())
}

fn entries(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
}

fn file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

fn is_pdf(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// Into a sibling first, then renamed into place: a replaced stamp is never
/// half a file.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let staged = path.with_extension("pdf.part");
    std::fs::write(&staged, bytes)?;
    std::fs::rename(&staged, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_a_path_is_refused() {
        for bad in ["", " ", "..", "a/b", "a\\b", "nul\0"] {
            assert!(
                matches!(check(bad), Err(LibraryError::InvalidName(_))),
                "{bad:?}"
            );
        }
        assert!(check("Receipts").is_ok());
    }

    #[test]
    fn a_missing_folder_is_an_empty_library() {
        let library = StampLibrary::new("/nonexistent/onionskin/stamps");
        assert!(library.list().is_empty());
        assert_eq!(library.find("custom:A/b"), None);
    }
}
