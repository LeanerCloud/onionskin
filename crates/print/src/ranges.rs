//! The Pages box: "2-4, 7" as the print dialog's user types it, into the
//! zero-based ranges a [`PageSelection`](crate::PageSelection) holds.
//!
//! A backwards range is refused rather than reversed: "7-2" is more often a
//! typo than a wish, and Reverse Pages is its own checkbox.

use std::fmt;

/// Why a Pages entry was not accepted, in words the dialog shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeError {
    Empty,
    /// A piece that is not a page number or a range of them.
    NotAPage(String),
    /// Page 0, or past the last page.
    OutOfRange {
        page: usize,
        count: usize,
    },
    /// A range whose first page comes after its last.
    Backwards {
        first: usize,
        last: usize,
    },
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Type the pages to print, like 2-4, 7"),
            Self::NotAPage(piece) => write!(f, "\"{piece}\" is not a page or a range of pages"),
            Self::OutOfRange { page, count } => {
                write!(f, "There is no page {page}: the document has {count}")
            }
            Self::Backwards { first, last } => write!(
                f,
                "{first}-{last} runs backwards; type {last}-{first}, and use Reverse Pages to print it backwards"
            ),
        }
    }
}

impl std::error::Error for RangeError {}

/// Parse `text` for a document of `count` pages. Pieces are separated by
/// commas; each is a page number or two joined by a hyphen, spaces allowed.
pub fn parse_page_ranges(text: &str, count: usize) -> Result<Vec<(usize, usize)>, RangeError> {
    let pieces: Vec<&str> = text
        .split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect();
    if pieces.is_empty() {
        return Err(RangeError::Empty);
    }
    pieces
        .into_iter()
        .map(|piece| parse_piece(piece, count))
        .collect()
}

fn parse_piece(piece: &str, count: usize) -> Result<(usize, usize), RangeError> {
    let (first, last) = match piece.split_once('-') {
        Some((first, last)) => (page(first, piece, count)?, page(last, piece, count)?),
        None => {
            let only = page(piece, piece, count)?;
            (only, only)
        }
    };
    if first > last {
        return Err(RangeError::Backwards { first, last });
    }
    Ok((first - 1, last - 1))
}

/// A one-based page number in `1..=count`.
fn page(text: &str, piece: &str, count: usize) -> Result<usize, RangeError> {
    let page: usize = text
        .trim()
        .parse()
        .map_err(|_| RangeError::NotAPage(piece.to_owned()))?;
    if page == 0 || page > count {
        return Err(RangeError::OutOfRange { page, count });
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_of_ranges_and_pages_parses_to_those_pages() {
        assert_eq!(parse_page_ranges("2-4,7", 10), Ok(vec![(1, 3), (6, 6)]));
        assert_eq!(
            parse_page_ranges(" 1 - 2 , , 5 ", 5),
            Ok(vec![(0, 1), (4, 4)])
        );
    }

    #[test]
    fn a_backwards_range_is_refused_with_a_message_not_reversed() {
        let error = parse_page_ranges("7-2", 10).unwrap_err();
        assert_eq!(error, RangeError::Backwards { first: 7, last: 2 });
        assert!(error.to_string().contains("Reverse Pages"));
    }

    #[test]
    fn nothing_junk_zero_and_past_the_end_are_each_refused() {
        assert_eq!(parse_page_ranges(" , ", 3), Err(RangeError::Empty));
        assert_eq!(
            parse_page_ranges("2-x", 3),
            Err(RangeError::NotAPage("2-x".into()))
        );
        assert_eq!(
            parse_page_ranges("0", 3),
            Err(RangeError::OutOfRange { page: 0, count: 3 })
        );
        assert_eq!(
            parse_page_ranges("1-4", 3),
            Err(RangeError::OutOfRange { page: 4, count: 3 })
        );
        for error in [
            RangeError::Empty,
            RangeError::NotAPage("a".into()),
            RangeError::OutOfRange { page: 4, count: 3 },
        ] {
            assert!(!error.to_string().is_empty());
        }
    }
}
