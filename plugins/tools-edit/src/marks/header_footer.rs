//! Headers and footers, and Bates numbers, which Acrobat writes the same
//! way: up to six lines of text, left, centre and right at the top and the
//! bottom of each page, with page numbers and the date filled in per page.
//!
//! Text may name `[page]` (the page's number), `[pages]` (the last page's
//! number), `[date]` and, for Bates numbering, `[bates]`.

use onionskin_core::pages::{Margins, MarkKind, PageMark};
use onionskin_core::{Document, PageIndex};
use onionskin_cos::Dict;
use onionskin_plugin_api::CommandError;

use super::text::TextStyle;
use super::{shown_sizes, write};

/// The six places a line can go, in the order [`HeaderFooter::text`] holds
/// them.
pub const POSITIONS: [&str; 6] = [
    "Left Header",
    "Center Header",
    "Right Header",
    "Left Footer",
    "Center Footer",
    "Right Footer",
];

/// A header and footer, as Acrobat's dialog sets one.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooter {
    /// One line per place in [`POSITIONS`]; an empty one draws nothing.
    pub text: [String; 6],
    pub style: TextStyle,
    /// From the page's edges as shown, in points.
    pub margins: Margins,
    pub numbering: Numbering,
}

/// What `[page]`, `[pages]` and `[date]` become.
#[derive(Debug, Clone, PartialEq)]
pub struct Numbering {
    /// The number the document's first page is given: Acrobat's Start Page
    /// Number.
    pub start: usize,
    pub date: String,
}

impl Default for Numbering {
    fn default() -> Self {
        Self {
            start: 1,
            date: String::new(),
        }
    }
}

/// Bates numbering: a number that runs on across the pages, with a prefix
/// and a suffix, in one of the six places.
#[derive(Debug, Clone, PartialEq)]
pub struct Bates {
    pub prefix: String,
    pub suffix: String,
    /// At least this many digits, zero-padded, as Acrobat's default of six.
    pub digits: usize,
    /// The first page's number.
    pub start: u64,
    /// An index into [`POSITIONS`].
    pub position: usize,
    pub style: TextStyle,
    pub margins: Margins,
}

impl Bates {
    /// The Bates number of the `offset`th page numbered.
    pub fn number(&self, offset: u64) -> String {
        format!(
            "{}{:0width$}{}",
            self.prefix,
            self.start + offset,
            self.suffix,
            width = self.digits
        )
    }
}

/// Draw `header` on `pages`, replacing the header and footer they have when
/// `replace` is set: Acrobat's Update. `settings` is kept with it for
/// [`super::saved_settings`].
pub fn add_header_footer(
    doc: &mut Document,
    pages: &[PageIndex],
    header: &HeaderFooter,
    replace: bool,
    settings: &str,
) -> Result<(), CommandError> {
    let last = doc.page_count() + header.numbering.start - 1;
    let marks: Vec<(PageIndex, PageMark)> = shown_sizes(doc, pages)?
        .into_iter()
        .map(|(page, size)| {
            let fill = |text: &str| {
                text.replace("[page]", &(page + header.numbering.start).to_string())
                    .replace("[pages]", &last.to_string())
                    .replace("[date]", &header.numbering.date)
            };
            let lines = header.text.clone().map(|text| fill(&text));
            (
                page,
                mark(&lines, &header.style, header.margins, size, settings),
            )
        })
        .collect();
    write(doc, MarkKind::HeaderFooter, replace, |_| Ok(marks))
}

/// Number `pages` in order, from `bates.start`, replacing any Bates numbers
/// they have, and keeping `settings` with them. The numbers the first and
/// last pages got.
pub fn add_bates(
    doc: &mut Document,
    pages: &[PageIndex],
    bates: &Bates,
    settings: &str,
) -> Result<(String, String), CommandError> {
    let marks: Vec<(PageIndex, PageMark)> = shown_sizes(doc, pages)?
        .into_iter()
        .enumerate()
        .map(|(offset, (page, size))| {
            let mut lines: [String; 6] = Default::default();
            lines[bates.position.min(5)] = bates.number(offset as u64);
            (
                page,
                mark(&lines, &bates.style, bates.margins, size, settings),
            )
        })
        .collect();
    write(doc, MarkKind::Bates, true, |_| Ok(marks))?;
    let last = pages.len().saturating_sub(1) as u64;
    Ok((bates.number(0), bates.number(last)))
}

/// One page's lines, placed in a shown page `width` by `height`.
fn mark(
    lines: &[String; 6],
    style: &TextStyle,
    margins: Margins,
    size: (f64, f64),
    settings: &str,
) -> PageMark {
    let mut content = b"q\n".to_vec();
    for (index, text) in lines.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let origin = place(index, style.width(text), style, margins, size);
        style.line(&mut content, text, origin);
    }
    content.extend_from_slice(b"Q\n");
    let mut resources = Dict::new();
    style.resources(&mut resources);
    PageMark {
        content,
        resources,
        behind: false,
        settings: settings.as_bytes().to_vec(),
    }
}

/// Where the line in place `index` of [`POSITIONS`], `width` wide, starts
/// its baseline: a header's top at the top margin, a footer's baseline at
/// the bottom one.
fn place(
    index: usize,
    width: f64,
    style: &TextStyle,
    margins: Margins,
    (page_width, page_height): (f64, f64),
) -> (f64, f64) {
    let x = match index % 3 {
        0 => margins.left,
        1 => (page_width - width) / 2.0,
        _ => page_width - margins.right - width,
    };
    let y = if index < 3 {
        page_height - margins.top - style.ascent()
    } else {
        margins.bottom
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marks::text::Font;

    fn style() -> TextStyle {
        TextStyle {
            font: Font::Courier,
            size: 10.0,
            color: [0.0; 3],
        }
    }

    const MARGINS: Margins = Margins {
        top: 20.0,
        bottom: 30.0,
        left: 40.0,
        right: 50.0,
    };

    #[test]
    fn each_place_is_at_its_edge() {
        let page = (600.0, 800.0);
        // Courier at 10 points: 6 points a character, so "abcd" is 24 wide.
        let at = |index| place(index, 24.0, &style(), MARGINS, page);
        assert_eq!(at(0), (40.0, 800.0 - 20.0 - 7.2));
        assert_eq!(at(1), (288.0, 772.8));
        assert_eq!(at(2), (526.0, 772.8));
        assert_eq!(at(3), (40.0, 30.0));
        assert_eq!(at(4).0, 288.0);
        assert_eq!(at(5), (526.0, 30.0));
    }

    #[test]
    fn an_empty_place_draws_nothing() {
        let mut lines: [String; 6] = Default::default();
        lines[4] = "x".to_owned();
        let drawn = mark(&lines, &style(), MARGINS, (100.0, 100.0), "kept");
        assert_eq!(drawn.settings, b"kept");
        let text = String::from_utf8_lossy(&drawn.content);
        assert_eq!(text.matches("Tj").count(), 1);
        assert!(drawn.resources.get(b"Font").is_some());
        assert!(!drawn.behind);
    }

    #[test]
    fn a_bates_number_is_padded_between_its_prefix_and_suffix() {
        let bates = Bates {
            prefix: "ACME-".into(),
            suffix: "-C".into(),
            digits: 6,
            start: 41,
            position: 5,
            style: style(),
            margins: MARGINS,
        };
        assert_eq!(bates.number(0), "ACME-000041-C");
        assert_eq!(bates.number(1_000_000), "ACME-1000041-C", "never truncated");
        assert_eq!(POSITIONS[bates.position], "Right Footer");
    }
}
