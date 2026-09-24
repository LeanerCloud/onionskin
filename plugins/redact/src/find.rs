//! Find Text & Redact: words, phrases or patterns found on every page, each
//! with the quads a mark over it covers.

use std::sync::OnceLock;

use onionskin_content::{search, Flattened, PageText, SearchOptions};
use onionskin_core::{Document, PageIndex, PageQuad};
use onionskin_plugin_api::CommandError;
use regex::Regex;

/// Acrobat's search-and-redact patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    PhoneNumbers,
    EmailAddresses,
    CreditCards,
    SocialSecurityNumbers,
    Dates,
}

impl Pattern {
    pub const ALL: [Pattern; 5] = [
        Pattern::PhoneNumbers,
        Pattern::EmailAddresses,
        Pattern::CreditCards,
        Pattern::SocialSecurityNumbers,
        Pattern::Dates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Pattern::PhoneNumbers => "Phone Numbers",
            Pattern::EmailAddresses => "Email Addresses",
            Pattern::CreditCards => "Credit Card Numbers",
            Pattern::SocialSecurityNumbers => "Social Security Numbers",
            Pattern::Dates => "Dates",
        }
    }

    fn source(self) -> &'static str {
        match self {
            // (555) 123-4567, 555-123-4567, 555.123.4567, +1 555 123 4567.
            Pattern::PhoneNumbers => {
                r"(?:\+?1[\s.-]?)?(?:\(\d{3}\)\s?|\b\d{3}[\s.-])\d{3}[\s.-]\d{4}\b"
            }
            Pattern::EmailAddresses => r"\b[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b",
            // Thirteen to sixteen digits in groups, checked by Luhn below.
            Pattern::CreditCards => r"\b(?:\d[ -]?){12,15}\d\b",
            Pattern::SocialSecurityNumbers => r"\b\d{3}-\d{2}-\d{4}\b",
            // 12/31/2025, 2025-12-31, 31.12.2025, December 31, 2025.
            Pattern::Dates => concat!(
                r"\b(?:\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}|\d{4}-\d{2}-\d{2}|",
                r"(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec)[a-z]*\.? \d{1,2},? \d{4})\b"
            ),
        }
    }

    fn regex(self) -> &'static Regex {
        static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();
        let all = COMPILED.get_or_init(|| {
            Pattern::ALL
                .iter()
                .map(|pattern| Regex::new(pattern.source()).expect("a valid pattern"))
                .collect()
        });
        let at = Pattern::ALL
            .iter()
            .position(|pattern| *pattern == self)
            .unwrap_or(0);
        &all[at]
    }

    /// Whether a match really is one: a card number passes the Luhn check.
    fn accepts(self, text: &str) -> bool {
        match self {
            Pattern::CreditCards => luhn(text),
            _ => true,
        }
    }
}

/// What to look for.
#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    /// A word, a phrase or several words, as the find bar's options say.
    Text(String, SearchOptions),
    Pattern(Pattern),
}

/// One thing found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub page: PageIndex,
    pub text: String,
    pub quads: Vec<PageQuad>,
}

/// Everything `query` finds, in page order.
pub fn find(doc: &mut Document, query: &Query) -> Result<Vec<Found>, CommandError> {
    let mut out = Vec::new();
    for page in 0..doc.page_count() {
        let text = doc
            .page_text(page)
            .map_err(|source| CommandError::Page { page, source })?;
        out.extend(find_on(text, query));
    }
    Ok(out)
}

/// What `query` finds on one page.
pub fn find_on(text: &PageText, query: &Query) -> Vec<Found> {
    match query {
        Query::Text(needle, options) => search(text, needle, *options)
            .into_iter()
            .filter(|hit| !hit.quads.is_empty())
            .map(|hit| Found {
                page: text.page,
                text: hit.text,
                quads: hit.quads,
            })
            .collect(),
        Query::Pattern(pattern) => {
            let flat = text.flatten();
            pattern
                .regex()
                .find_iter(&flat.text)
                .filter(|hit| pattern.accepts(hit.as_str()))
                .filter_map(|hit| found(text, &flat, hit.range()))
                .collect()
        }
    }
}

fn found(text: &PageText, flat: &Flattened, range: std::ops::Range<usize>) -> Option<Found> {
    let quads: Vec<PageQuad> = flat
        .runs_for(text, range.clone())
        .into_iter()
        .flat_map(|(run, local)| run.quads_for(local))
        .collect();
    (!quads.is_empty()).then(|| Found {
        page: text.page,
        text: flat.text[range].to_owned(),
        quads,
    })
}

fn luhn(text: &str) -> bool {
    let digits: Vec<u32> = text.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=16).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            if index % 2 == 1 {
                let doubled = digit * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                *digit
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: Pattern, text: &str) -> Vec<String> {
        pattern
            .regex()
            .find_iter(text)
            .filter(|hit| pattern.accepts(hit.as_str()))
            .map(|hit| hit.as_str().to_owned())
            .collect()
    }

    #[test]
    fn each_pattern_finds_its_kind_and_not_the_others() {
        assert_eq!(
            matches(
                Pattern::PhoneNumbers,
                "call (555) 123-4567 or 555.987.6543, not 12345"
            ),
            ["(555) 123-4567", "555.987.6543"]
        );
        assert_eq!(
            matches(
                Pattern::EmailAddresses,
                "write to ada.l+x@example.co.uk today"
            ),
            ["ada.l+x@example.co.uk"]
        );
        assert_eq!(
            matches(
                Pattern::CreditCards,
                "card 4111 1111 1111 1111 and 4111 1111 1111 1112"
            ),
            ["4111 1111 1111 1111"]
        );
        assert_eq!(
            matches(
                Pattern::SocialSecurityNumbers,
                "SSN 078-05-1120 phone 555-123-4567"
            ),
            ["078-05-1120"]
        );
        assert_eq!(
            matches(
                Pattern::Dates,
                "on 12/31/2025, 2025-12-31 and December 31, 2025"
            ),
            ["12/31/2025", "2025-12-31", "December 31, 2025"]
        );
        assert_eq!(Pattern::ALL.map(Pattern::label)[2], "Credit Card Numbers");
    }

    #[test]
    fn luhn_needs_thirteen_to_sixteen_digits() {
        assert!(luhn("4111-1111-1111-1111"));
        assert!(!luhn("4111"));
        assert!(!luhn("41111111111111111111"));
    }
}
