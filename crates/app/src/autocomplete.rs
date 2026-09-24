//! Forms Auto-Complete: what was typed into form fields, kept on this
//! machine to be offered again as the same letters are typed.
//!
//! Local only, and inspectable: Preferences > Forms lists every entry, and
//! each can be removed, or all of them. The file is `autocomplete.json`
//! beside the other settings, written owner-only, since what someone typed
//! into forms says a great deal about them. Numbers are kept only when
//! Preferences > Forms says to, so an account or card number is not
//! remembered by default.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The most entries kept; the oldest go first.
pub const MAX_ENTRIES: usize = 500;

/// How many suggestions a field offers at once.
pub const SUGGESTIONS: usize = 5;

/// The remembered entries, most recent first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryList {
    entries: Vec<String>,
}

/// Whether `text` is a number, as Acrobat's "Remember numerical data"
/// means it: digits with the usual separators, signs and symbols.
pub fn is_number(text: &str) -> bool {
    let digits: String = text
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '$' | '%' | '-' | '+' | '(' | ')' | '/'))
        .collect();
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
}

impl EntryList {
    /// The list at `path`, and why it could not be read, when it could
    /// not. A missing file is an empty list.
    pub fn load(path: Option<&Path>) -> (Self, Option<String>) {
        let Some(path) = path else {
            return (Self::default(), None);
        };
        match crate::config::read(path) {
            Ok(None) => (Self::default(), None),
            Ok(Some(source)) => match serde_json::from_str::<EntryList>(&source) {
                Ok(mut list) => {
                    list.entries.truncate(MAX_ENTRIES);
                    (list, None)
                }
                Err(error) => (
                    Self::default(),
                    Some(format!(
                        "{} could not be read: {error}{}",
                        path.display(),
                        crate::config::keep_unreadable(path)
                    )),
                ),
            },
            Err(error) => (
                Self::default(),
                Some(format!("{} could not be read: {error}", path.display())),
            ),
        }
    }

    /// Write the list to `path`, owner-only.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).expect("a list of strings serializes");
        crate::config::write_private(path, &json)
            .map_err(|error| format!("{} could not be saved: {error}", path.display()))
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Keep `text` as the most recent entry. `numbers` is whether numbers
    /// are kept. Whether the list changed.
    pub fn remember(&mut self, text: &str, numbers: bool) -> bool {
        let text = text.trim();
        if text.is_empty() || (!numbers && is_number(text)) {
            return false;
        }
        if self.entries.first().is_some_and(|first| first == text) {
            return false;
        }
        self.entries.retain(|entry| entry != text);
        self.entries.insert(0, text.to_owned());
        self.entries.truncate(MAX_ENTRIES);
        true
    }

    /// The entries that start with what was typed, ignoring case, most
    /// recent first, but not one that is what was typed.
    pub fn suggest(&self, typed: &str) -> Vec<String> {
        let typed = typed.trim_start().to_lowercase();
        if typed.is_empty() {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|entry| {
                let entry = entry.to_lowercase();
                entry.starts_with(&typed) && entry != typed
            })
            .take(SUGGESTIONS)
            .cloned()
            .collect()
    }

    /// Remove entry `index`. Whether there was one.
    pub fn forget(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_kept_most_recent_first_without_repeats() {
        let mut list = EntryList::default();
        assert!(list.remember(" Ada ", false));
        assert!(list.remember("Alan", false));
        assert!(!list.remember("Alan", false), "already first");
        assert!(list.remember("Ada", false));
        assert_eq!(list.entries(), ["Ada", "Alan"]);
        assert!(!list.remember("  ", false));
        for number in 0..MAX_ENTRIES + 3 {
            list.remember(&format!("name {number}"), false);
        }
        assert_eq!(list.entries().len(), MAX_ENTRIES);
        assert_eq!(list.entries()[0], format!("name {}", MAX_ENTRIES + 2));
    }

    #[test]
    fn numbers_are_kept_only_when_asked() {
        let mut list = EntryList::default();
        for number in ["1,234.50", "$12", "078-05-1120", "(555) 123 4567", "50%"] {
            assert!(is_number(number), "{number}");
            assert!(!list.remember(number, false));
        }
        assert!(!is_number("12 Main St"));
        assert!(!is_number("-"));
        assert!(list.remember("42", true));
        assert_eq!(list.entries(), ["42"]);
    }

    #[test]
    fn suggestions_match_the_start_ignoring_case() {
        let mut list = EntryList::default();
        for entry in ["alpha", "Albert", "beta", "al", "ALFA", "alp", "alt", "all"] {
            list.remember(entry, false);
        }
        assert_eq!(list.suggest(""), Vec::<String>::new());
        assert_eq!(list.suggest("B"), ["beta"]);
        let many = list.suggest("al");
        assert_eq!(many.len(), SUGGESTIONS);
        assert_eq!(many[0], "all", "most recent first");
        assert!(!many.contains(&"al".to_owned()), "not what was typed");
        assert!(list.suggest("zeta").is_empty());
    }

    #[test]
    fn entries_are_forgotten_one_by_one_or_all_at_once() {
        let mut list = EntryList::default();
        list.remember("a", false);
        list.remember("b", false);
        assert!(list.forget(1));
        assert!(!list.forget(5));
        assert_eq!(list.entries(), ["b"]);
        list.clear();
        assert!(list.entries().is_empty());
    }

    #[test]
    fn the_list_is_saved_and_read_back_and_a_bad_file_is_said() {
        let dir = crate::config::test_dir("autocomplete");
        let path = dir.join("autocomplete.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(EntryList::load(Some(&path)), (EntryList::default(), None));
        assert_eq!(EntryList::load(None), (EntryList::default(), None));
        let mut list = EntryList::default();
        list.remember("Ada", false);
        list.save(&path).expect("saves");
        assert_eq!(EntryList::load(Some(&path)), (list, None));
        std::fs::write(&path, "not json").expect("writes");
        let (empty, error) = EntryList::load(Some(&path));
        assert!(empty.entries().is_empty());
        assert!(error.is_some_and(|error| error.contains("could not be read")));
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "a file, not a folder").expect("writes");
        assert!(EntryList::default().save(&blocker.join("a.json")).is_err());
    }
}
