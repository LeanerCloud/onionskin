//! What a print job asks for: the choices Acrobat's Print dialog offers,
//! as plain data.

use onionskin_core::AnnotationFilter;

/// A sheet of paper, in points, portrait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaperSize {
    pub name: &'static str,
    pub width: f64,
    pub height: f64,
}

impl PaperSize {
    pub const LETTER: PaperSize = PaperSize {
        name: "Letter",
        width: 612.0,
        height: 792.0,
    };
    pub const LEGAL: PaperSize = PaperSize {
        name: "Legal",
        width: 612.0,
        height: 1008.0,
    };
    pub const A4: PaperSize = PaperSize {
        name: "A4",
        width: 595.276,
        height: 841.89,
    };
    pub const ALL: [PaperSize; 3] = [Self::LETTER, Self::LEGAL, Self::A4];
}

/// Portrait, landscape, or whichever suits the pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    Portrait,
    Landscape,
    #[default]
    Auto,
}

/// Page Sizing & Handling's "Size" choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sizing {
    /// Scale every page to fill its space, up or down.
    #[default]
    Fit,
    /// 100%, whatever the paper.
    ActualSize,
    /// 100%, except a page too big for its space, which is scaled down.
    ShrinkOversized,
    /// A percentage.
    Custom(u16),
}

/// Pages to Print's "Odd or Even Pages".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Subset {
    #[default]
    All,
    Odd,
    Even,
}

/// Which pages, and in which order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageSelection {
    /// First and last page of each range, zero-based and inclusive, in the
    /// order they print. Empty is every page.
    pub ranges: Vec<(usize, usize)>,
    pub subset: Subset,
    /// Reverse Pages.
    pub reverse: bool,
}

impl PageSelection {
    /// Every page.
    pub fn all() -> Self {
        PageSelection::default()
    }

    /// One page: the dialog's "Current page".
    pub fn page(page: usize) -> Self {
        PageSelection {
            ranges: vec![(page, page)],
            ..PageSelection::default()
        }
    }

    /// The pages this selects from a document of `count` pages, in print
    /// order. Odd and even count from one, as the page numbers a user sees.
    /// A range running past the end stops at the last page.
    pub fn pages(&self, count: usize) -> Vec<usize> {
        if count == 0 {
            return Vec::new();
        }
        let whole = [(0, count - 1)];
        let ranges = if self.ranges.is_empty() {
            &whole[..]
        } else {
            &self.ranges[..]
        };
        let mut pages: Vec<usize> = ranges
            .iter()
            .flat_map(|(first, last)| *first..=(*last).min(count - 1))
            .filter(|page| match self.subset {
                Subset::All => true,
                Subset::Odd => (page + 1) % 2 == 1,
                Subset::Even => (page + 1) % 2 == 0,
            })
            .collect();
        if self.reverse {
            pages.reverse();
        }
        pages
    }
}

/// Multiple's "Page Order".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NUpOrder {
    /// Left to right, then down.
    #[default]
    Horizontal,
    /// Right to left, then down.
    HorizontalReversed,
    /// Top to bottom, then right.
    Vertical,
    /// Top to bottom, then left.
    VerticalReversed,
}

/// Multiple pages per sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NUp {
    /// 1, 2, 4, 6, 9 or 16, Acrobat's choices.
    pub per_sheet: u8,
    pub order: NUpOrder,
    /// Print Page Border.
    pub borders: bool,
}

impl Default for NUp {
    fn default() -> Self {
        NUp {
            per_sheet: 1,
            order: NUpOrder::Horizontal,
            borders: false,
        }
    }
}

impl NUp {
    /// Acrobat's pages-per-sheet choices.
    pub const CHOICES: [u8; 6] = [1, 2, 4, 6, 9, 16];

    /// Columns and rows on a portrait sheet; a landscape sheet swaps them.
    pub fn grid(self) -> (usize, usize) {
        match self.per_sheet {
            2 => (1, 2),
            4 => (2, 2),
            6 => (2, 3),
            9 => (3, 3),
            16 => (4, 4),
            _ => (1, 1),
        }
    }
}

/// Print on Both Sides of Paper, and which edge the sheet turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Duplex {
    #[default]
    Off,
    /// Flip on long edge: the usual choice for portrait sheets.
    LongEdge,
    /// Flip on short edge: the usual choice for landscape sheets.
    ShortEdge,
}

impl Duplex {
    pub fn is_on(self) -> bool {
        self != Duplex::Off
    }
}

/// Page Sizing & Handling's other two buttons: the pages in a grid (Size and
/// Multiple, which `sizing` and `n_up` describe), a booklet, or a poster.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Handling {
    #[default]
    Pages,
    Booklet(Booklet),
    Poster(Poster),
}

/// Booklet: pages two to a side, in the order that folds into a booklet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Booklet {
    pub sides: BookletSides,
    pub binding: Binding,
    /// An inclusive zero-based physical-sheet interval. `None` prints all
    /// composed sheets. An interval with `start > end`, or either endpoint
    /// outside the composed sheet count, produces an empty imposition.
    pub sheets: Option<(usize, usize)>,
}

/// Acrobat's "Booklet subset".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookletSides {
    #[default]
    BothSides,
    FrontSideOnly,
    BackSideOnly,
}

/// Which edge the booklet opens from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Binding {
    #[default]
    Left,
    Right,
}

/// Poster: each page enlarged and split into tiles, one to a sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Poster {
    /// Tile Scale, a percentage of the page's size.
    pub scale: f64,
    /// How much neighbouring tiles repeat, in points, for gluing.
    pub overlap: f64,
    /// Cut marks around each tile, in a margin they print in.
    pub cut_marks: bool,
}

impl Default for Poster {
    fn default() -> Self {
        Poster {
            scale: 200.0,
            overlap: 18.0,
            cut_marks: true,
        }
    }
}

/// Everything a print asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintJob {
    pub paper: PaperSize,
    pub orientation: Orientation,
    pub selection: PageSelection,
    pub sizing: Sizing,
    pub n_up: NUp,
    /// Print on Both Sides of Paper. The sheet count is kept even, so the
    /// last page's back is blank rather than the next job's front.
    pub duplex: Duplex,
    /// How many copies, at least one.
    pub copies: u16,
    /// Collate: each copy printed whole before the next starts.
    pub collate: bool,
    /// The printer by name; `None` for the system's default. The file
    /// backend has no printer and ignores it.
    pub printer: Option<String>,
    /// Comments & Forms.
    pub comments: AnnotationFilter,
    /// Print as Image: every sheet becomes pixels.
    pub print_as_image: bool,
    /// The resolution Print as Image renders at.
    pub image_dpi: f32,
    /// Size and Multiple, Booklet, or Poster.
    pub handling: Handling,
}

impl Default for PrintJob {
    fn default() -> Self {
        PrintJob {
            paper: PaperSize::LETTER,
            orientation: Orientation::Auto,
            selection: PageSelection::default(),
            sizing: Sizing::Fit,
            n_up: NUp::default(),
            duplex: Duplex::Off,
            copies: 1,
            collate: true,
            printer: None,
            comments: AnnotationFilter::DocumentAndMarkups,
            print_as_image: false,
            image_dpi: 150.0,
            handling: Handling::Pages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_even_and_odd_subset_of_a_five_page_document_selects_one_three_five() {
        let odd = PageSelection {
            subset: Subset::Odd,
            ..PageSelection::default()
        };
        assert_eq!(odd.pages(5), [0, 2, 4], "pages 1, 3 and 5");
        let even = PageSelection {
            subset: Subset::Even,
            ..PageSelection::default()
        };
        assert_eq!(even.pages(5), [1, 3], "pages 2 and 4");
    }

    #[test]
    fn a_range_is_inclusive_clamped_and_can_run_backwards() {
        let selection = PageSelection {
            ranges: vec![(1, 9)],
            reverse: true,
            ..PageSelection::default()
        };
        assert_eq!(selection.pages(4), [3, 2, 1]);
        assert!(PageSelection::all().pages(0).is_empty());
    }

    #[test]
    fn several_ranges_print_in_the_order_given_and_one_page_is_one_range() {
        let selection = PageSelection {
            ranges: vec![(6, 6), (1, 3)],
            ..PageSelection::default()
        };
        assert_eq!(selection.pages(10), [6, 1, 2, 3]);
        assert_eq!(PageSelection::page(2).pages(5), [2]);
    }

    #[test]
    fn the_grids_are_acrobats() {
        let grid = |per_sheet| {
            NUp {
                per_sheet,
                ..NUp::default()
            }
            .grid()
        };
        assert_eq!(
            NUp::CHOICES.map(grid),
            [(1, 1), (1, 2), (2, 2), (2, 3), (3, 3), (4, 4)]
        );
    }
}
