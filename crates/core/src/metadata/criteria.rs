//! Advanced Search's "additional criteria": tests on what a document says
//! about itself, over both `/Info` and the XMP packet.
//!
//! A reader that trusts only one of the two misses documents whose other copy
//! carries the value, so a text field matches when either copy does, and a
//! date is read from `/Info` first and the packet second. Text tests ignore
//! case, as Acrobat's do.

use super::{Info, XmpFields};

/// What a criterion looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyField {
    Title,
    Author,
    Subject,
    Keywords,
    /// `/Creator` or `xmp:CreatorTool`.
    Creator,
    Producer,
    Created,
    Modified,
}

impl PropertyField {
    pub const ALL: [Self; 8] = [
        Self::Title,
        Self::Author,
        Self::Subject,
        Self::Keywords,
        Self::Creator,
        Self::Producer,
        Self::Created,
        Self::Modified,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Author => "Author",
            Self::Subject => "Subject",
            Self::Keywords => "Keywords",
            Self::Creator => "Creator",
            Self::Producer => "Producer",
            Self::Created => "Date Created",
            Self::Modified => "Date Modified",
        }
    }

    pub fn is_date(self) -> bool {
        matches!(self, Self::Created | Self::Modified)
    }

    /// The tests this field takes, in the order a chooser offers them.
    pub fn tests(self) -> &'static [PropertyTest] {
        if self.is_date() {
            &[PropertyTest::Before, PropertyTest::After, PropertyTest::On]
        } else {
            &[PropertyTest::Contains, PropertyTest::DoesNotContain]
        }
    }
}

/// How a criterion compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyTest {
    Contains,
    DoesNotContain,
    Before,
    After,
    On,
}

impl PropertyTest {
    pub fn label(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::DoesNotContain => "does not contain",
            Self::Before => "is before",
            Self::After => "is after",
            Self::On => "is on",
        }
    }
}

/// One criterion, with the value as the user typed it. A date is written
/// `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyCriterion {
    pub field: PropertyField,
    pub test: PropertyTest,
    pub value: String,
}

/// Why a criterion cannot be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriterionError {
    /// A date field with a value that is not `YYYY-MM-DD`.
    NotADate(String),
    /// A test the field does not take, such as "is before" on Author.
    WrongTest {
        field: PropertyField,
        test: PropertyTest,
    },
}

impl std::fmt::Display for CriterionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotADate(value) => write!(f, "{value:?} is not a date; write it as YYYY-MM-DD"),
            Self::WrongTest { field, test } => {
                write!(
                    f,
                    "{} cannot be tested with \"{}\"",
                    field.label(),
                    test.label()
                )
            }
        }
    }
}

impl std::error::Error for CriterionError {}

/// A calendar day, comparable.
type Day = (u32, u32, u32);

impl PropertyCriterion {
    /// Whether the document's description passes. Every criterion must pass
    /// for the document to match.
    pub fn matches(&self, info: &Info, xmp: Option<&XmpFields>) -> Result<bool, CriterionError> {
        if !self.field.tests().contains(&self.test) {
            return Err(CriterionError::WrongTest {
                field: self.field,
                test: self.test,
            });
        }
        if self.field.is_date() {
            let wanted = parse_day(&self.value)
                .ok_or_else(|| CriterionError::NotADate(self.value.clone()))?;
            return Ok(
                date_of(self.field, info, xmp).is_some_and(|day| match self.test {
                    PropertyTest::Before => day < wanted,
                    PropertyTest::After => day > wanted,
                    _ => day == wanted,
                }),
            );
        }
        let needle = self.value.to_lowercase();
        let found = texts_of(self.field, info, xmp)
            .iter()
            .any(|text| text.to_lowercase().contains(&needle));
        Ok(match self.test {
            PropertyTest::DoesNotContain => !found,
            _ => found,
        })
    }
}

/// Whether every criterion passes; the first one that cannot be applied is
/// the error.
pub fn matches_all(
    criteria: &[PropertyCriterion],
    info: &Info,
    xmp: Option<&XmpFields>,
) -> Result<bool, CriterionError> {
    for criterion in criteria {
        if !criterion.matches(info, xmp)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Every value either copy gives the field.
fn texts_of(field: PropertyField, info: &Info, xmp: Option<&XmpFields>) -> Vec<String> {
    let description = &info.description;
    let mut texts: Vec<String> = match field {
        PropertyField::Title => description.title.clone().into_iter().collect(),
        PropertyField::Author => description.author.clone().into_iter().collect(),
        PropertyField::Subject => description.subject.clone().into_iter().collect(),
        PropertyField::Keywords => description.keywords.clone().into_iter().collect(),
        PropertyField::Creator => info.creator.clone().into_iter().collect(),
        PropertyField::Producer => info.producer.clone().into_iter().collect(),
        PropertyField::Created | PropertyField::Modified => Vec::new(),
    };
    if let Some(xmp) = xmp {
        match field {
            PropertyField::Title => texts.extend(xmp.title.clone()),
            PropertyField::Author => texts.extend(xmp.authors.iter().cloned()),
            PropertyField::Subject => texts.extend(xmp.subject.clone()),
            PropertyField::Keywords => texts.extend(xmp.keywords.clone()),
            PropertyField::Creator => texts.extend(xmp.creator_tool.clone()),
            PropertyField::Producer => texts.extend(xmp.producer.clone()),
            PropertyField::Created | PropertyField::Modified => {}
        }
    }
    texts
}

/// The day a date field names: `/Info` first, then the packet.
fn date_of(field: PropertyField, info: &Info, xmp: Option<&XmpFields>) -> Option<Day> {
    let (info_date, xmp_date) = match field {
        PropertyField::Created => (&info.created, xmp.and_then(|xmp| xmp.created.as_ref())),
        _ => (&info.modified, xmp.and_then(|xmp| xmp.modified.as_ref())),
    };
    info_date
        .as_deref()
        .and_then(parse_pdf_day)
        .or_else(|| xmp_date.and_then(|date| parse_day(date)))
}

/// `D:YYYYMMDD...` as a PDF writes it (ISO 32000-1 7.9.4). The time and zone
/// are ignored: criteria compare days.
fn parse_pdf_day(date: &str) -> Option<Day> {
    let digits = date.strip_prefix("D:").unwrap_or(date);
    let number = |range: std::ops::Range<usize>| digits.get(range)?.parse::<u32>().ok();
    let year = number(0..4)?;
    let month = number(4..6).unwrap_or(1);
    let day = number(6..8).unwrap_or(1);
    valid((year, month, day))
}

/// `YYYY-MM-DD`, with anything after the day (an XMP time) ignored.
fn parse_day(date: &str) -> Option<Day> {
    let mut parts = date.trim().get(..10.min(date.trim().len()))?.splitn(3, '-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    valid((year, month, day))
}

fn valid(day: Day) -> Option<Day> {
    ((1..=12).contains(&day.1) && (1..=31).contains(&day.2)).then_some(day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Description;

    fn info() -> Info {
        Info {
            description: Description {
                title: Some("Annual Report".into()),
                author: Some("Ana Pop".into()),
                subject: None,
                keywords: Some("finance, 2025".into()),
            },
            creator: Some("Writer".into()),
            producer: Some("Onionskin".into()),
            created: Some("D:20250301120000Z".into()),
            modified: None,
            custom: Vec::new(),
        }
    }

    fn xmp() -> XmpFields {
        XmpFields {
            subject: Some("Quarterly figures".into()),
            authors: vec!["Radu Ion".into()],
            modified: Some("2025-06-10T08:00:00Z".into()),
            ..XmpFields::default()
        }
    }

    fn criterion(field: PropertyField, test: PropertyTest, value: &str) -> PropertyCriterion {
        PropertyCriterion {
            field,
            test,
            value: value.into(),
        }
    }

    fn check(field: PropertyField, test: PropertyTest, value: &str) -> bool {
        criterion(field, test, value)
            .matches(&info(), Some(&xmp()))
            .expect("applies")
    }

    #[test]
    fn a_text_field_matches_either_copy_ignoring_case() {
        use PropertyField::*;
        use PropertyTest::*;
        assert!(check(Author, Contains, "ana"));
        assert!(check(Author, Contains, "RADU"), "the XMP author");
        assert!(check(Subject, Contains, "quarterly"), "only XMP has it");
        assert!(!check(Author, Contains, "Maria"));
        assert!(check(Author, DoesNotContain, "Maria"));
        assert!(!check(Keywords, DoesNotContain, "finance"));
        assert!(check(Creator, Contains, "writer"));
        assert!(check(Producer, Contains, "onion"));
        assert!(check(Title, Contains, "report"));
    }

    #[test]
    fn a_date_is_read_from_info_then_xmp_and_compared_by_day() {
        use PropertyField::*;
        use PropertyTest::*;
        assert!(check(Created, On, "2025-03-01"));
        assert!(check(Created, After, "2025-02-28"));
        assert!(!check(Created, Before, "2025-03-01"));
        assert!(check(Modified, On, "2025-06-10"), "only XMP has it");
        assert!(check(Modified, Before, "2026-01-01"));
        // No date at all passes no date test.
        assert!(!criterion(Modified, After, "1900-01-01")
            .matches(&info(), None)
            .unwrap());
    }

    #[test]
    fn a_criterion_that_cannot_apply_says_why() {
        use PropertyField::*;
        use PropertyTest::*;
        let error = criterion(Created, Before, "March").matches(&info(), None);
        assert_eq!(error, Err(CriterionError::NotADate("March".into())));
        assert!(error.unwrap_err().to_string().contains("YYYY-MM-DD"));
        let error = criterion(Author, Before, "x").matches(&info(), None);
        assert_eq!(
            error.unwrap_err().to_string(),
            "Author cannot be tested with \"is before\""
        );
        assert_eq!(parse_day("2025-13-01"), None);
        assert_eq!(parse_pdf_day("D:2025"), Some((2025, 1, 1)));
        assert_eq!(parse_pdf_day("D:20xx"), None);
    }

    #[test]
    fn every_criterion_must_pass() {
        use PropertyField::*;
        use PropertyTest::*;
        let both = [
            criterion(Author, Contains, "ana"),
            criterion(Title, Contains, "report"),
        ];
        assert_eq!(matches_all(&both, &info(), None), Ok(true));
        let one_fails = [
            criterion(Author, Contains, "ana"),
            criterion(Title, Contains, "memo"),
        ];
        assert_eq!(matches_all(&one_fails, &info(), None), Ok(false));
        assert_eq!(matches_all(&[], &info(), None), Ok(true));
        for field in PropertyField::ALL {
            assert!(!field.tests().is_empty(), "{}", field.label());
        }
    }
}
