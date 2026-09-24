//! Spell checking, for Edit > Check Spelling: the text of the document's
//! comments and form fields, as Acrobat checks it.
//!
//! The dictionary is SCOWL's en_US in Hunspell form, read by `spellbook`,
//! with the words the user added kept beside it (see [`user`]). What is
//! checked and changed is in [`passages`].

pub mod passages;
pub mod user;

use std::collections::BTreeSet;
use std::ops::Range;

use spellbook::Dictionary;

const AFFIX: &str = include_str!("../dictionary/en_US.aff");
const WORDS: &str = include_str!("../dictionary/en_US.dic");

/// The most suggestions offered for one word.
pub const MAX_SUGGESTIONS: usize = 8;

/// A dictionary, and the words the user added to it.
pub struct Checker {
    dictionary: Dictionary,
    added: BTreeSet<String>,
}

impl Checker {
    /// US English.
    pub fn english() -> Checker {
        Checker {
            dictionary: Dictionary::new(AFFIX, WORDS).expect("the bundled dictionary parses"),
            added: BTreeSet::new(),
        }
    }

    /// Accept `word` from now on.
    pub fn add(&mut self, word: &str) {
        self.added.insert(normalized(word));
    }

    /// Accept each of `words`.
    pub fn with_words(mut self, words: impl IntoIterator<Item = String>) -> Checker {
        for word in words {
            self.add(&word);
        }
        self
    }

    pub fn is_correct(&self, word: &str) -> bool {
        let word = normalized(word);
        self.added.contains(&word) || self.dictionary.check(&word)
    }

    /// What `word` may have meant, likeliest first. A capitalized word is
    /// looked up as written in lower case too, and its suggestions
    /// capitalized, since the dictionary suggests best for lower case.
    /// Suggestions that split the word in two are left out.
    pub fn suggestions(&self, word: &str) -> Vec<String> {
        let word = normalized(word);
        let mut found = Vec::new();
        let lower = word.to_lowercase();
        if lower != word && capitalized(&word) {
            let mut lowered = Vec::new();
            self.dictionary.suggest(&lower, &mut lowered);
            found.extend(lowered.iter().map(|suggestion| capitalize(suggestion)));
        }
        // `suggest` fills its list afresh, so each lookup gets its own.
        let mut as_written = Vec::new();
        self.dictionary.suggest(&word, &mut as_written);
        found.extend(as_written);
        let mut seen = BTreeSet::new();
        found.retain(|suggestion| !suggestion.contains(' ') && seen.insert(suggestion.clone()));
        found.truncate(MAX_SUGGESTIONS);
        found
    }

    /// Where `text` has a word this checker does not know.
    pub fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        words(text)
            .into_iter()
            .filter(|range| !self.is_correct(&text[range.clone()]))
            .collect()
    }
}

/// Whether `word` is an initial capital and nothing else in capitals.
fn capitalized(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(char::is_uppercase) && !chars.any(char::is_uppercase)
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The typographic apostrophe read as the plain one, as the dictionary
/// spells them.
fn normalized(word: &str) -> String {
    word.replace('\u{2019}', "'")
}

/// The words of `text` worth checking: letters, with apostrophes inside
/// them. A token with a digit, an address or a path is not a word, and a
/// single letter, an acronym or a mixed-case name is left alone, as
/// Acrobat leaves them.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (index, &(at, ch)) in chars.iter().enumerate() {
        let apostrophe = matches!(ch, '\'' | '\u{2019}')
            && start.is_some()
            && chars
                .get(index + 1)
                .is_some_and(|(_, next)| next.is_alphabetic());
        match (ch.is_alphabetic() || apostrophe, start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                found.push(from..at);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        found.push(from..text.len());
    }
    found
        .into_iter()
        .filter(|range| worth_checking(text, range))
        .collect()
}

fn worth_checking(text: &str, range: &Range<usize>) -> bool {
    let word = &text[range.clone()];
    let joined_to = |at: Option<char>| {
        at.is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, '@' | '/' | '\\' | '_' | '.'))
    };
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    let after_joined = after == Some('.')
        && text[range.end + 1..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric);
    word.chars().count() > 1
        && !word.chars().skip(1).any(char::is_uppercase)
        && !joined_to(before)
        && !(joined_to(after) && (after != Some('.') || after_joined))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spelled(text: &str) -> Vec<&str> {
        words(text).into_iter().map(|range| &text[range]).collect()
    }

    #[test]
    fn words_are_letters_with_apostrophes_inside_them() {
        assert_eq!(
            spelled("It's the teams’ turn, isn't it? 'Quoted'"),
            ["It's", "the", "teams", "turn", "isn't", "it", "Quoted"]
        );
        assert_eq!(spelled("Café crème"), ["Café", "crème"]);
    }

    #[test]
    fn what_is_not_a_word_is_left_alone() {
        assert_eq!(
            spelled("a NASA iPhone A4 x2 mail@example.com src/main.rs end."),
            ["end"]
        );
    }

    #[test]
    fn the_dictionary_knows_english_and_learns_more() {
        let mut checker = Checker::english();
        assert!(checker.is_correct("Receive"));
        assert!(checker.is_correct("isn’t"));
        assert!(!checker.is_correct("recieve"));
        assert!(checker
            .suggestions("recieve")
            .contains(&"receive".to_owned()));
        assert!(checker.suggestions("recieve").len() <= MAX_SUGGESTIONS);
        assert_eq!(checker.suggestions("Teh")[0], "The", "as for lower case");
        assert!(checker
            .suggestions("Teh")
            .iter()
            .all(|word| !word.contains(' ')));
        assert_eq!(checker.suggestions("teh")[0], "the");
        let text = "Zorblax recieves teh text";
        let wrong: Vec<_> = checker
            .misspelled(text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(wrong, ["Zorblax", "recieves", "teh"]);
        checker.add("Zorblax");
        assert!(checker.is_correct("Zorblax"));
        let checker = checker.with_words(["teh".to_owned()]);
        assert_eq!(checker.misspelled(text).len(), 1);
    }
}
