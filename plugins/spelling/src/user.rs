//! The words the user added to the dictionary: one to a line, in a file in
//! the folder the shell keeps its data in, so every document is checked
//! with them.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

/// The file the added words are kept in, inside the data folder.
pub const FILE_NAME: &str = "dictionary.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDictionary {
    path: PathBuf,
}

impl UserDictionary {
    pub fn in_dir(data: &Path) -> UserDictionary {
        UserDictionary {
            path: data.join(FILE_NAME),
        }
    }

    /// The words, or none when nothing was added yet.
    pub fn words(&self) -> io::Result<BTreeSet<String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text
                .lines()
                .map(str::trim)
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(error) => Err(error),
        }
    }

    /// Add `word`, keeping the list sorted and each word once.
    pub fn add(&self, word: &str) -> io::Result<()> {
        let mut words = self.words()?;
        if !words.insert(word.trim().to_owned()) {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut text = words.into_iter().collect::<Vec<_>>().join("\n");
        text.push('\n');
        std::fs::write(&self.path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_words_are_kept_once_each_across_readings() {
        let dir = tempfile::tempdir().expect("dir");
        let user = UserDictionary::in_dir(&dir.path().join("data"));
        assert!(user.words().expect("reads").is_empty(), "nothing yet");
        user.add("Onionskin").expect("adds");
        user.add(" Onionskin ").expect("adds");
        user.add("Acrobat").expect("adds");
        let words: Vec<_> = user.words().expect("reads").into_iter().collect();
        assert_eq!(words, ["Acrobat", "Onionskin"]);
        let text = std::fs::read_to_string(dir.path().join("data").join(FILE_NAME)).expect("file");
        assert_eq!(text, "Acrobat\nOnionskin\n");

        let unreadable = UserDictionary::in_dir(dir.path());
        std::fs::create_dir(dir.path().join(FILE_NAME)).expect("a folder in the way");
        assert!(unreadable.words().is_err());
    }
}
