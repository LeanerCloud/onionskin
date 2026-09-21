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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageSelection {
    /// First and last page, zero-based and inclusive. `None` is every page.
    pub range: Option<(usize, usize)>,
    pub subset: Subset,
    /// Reverse Pages.
    pub reverse: bool,
}

impl PageSelection {
    /// The pages this selects from a document of `count` pages, in print
    /// order. Odd and even count from one, as the page numbers a user sees.
    pub fn pages(&self, count: usize) -> Vec<usize> {
        if count == 0 {
            return Vec::new();
        }
        let (first, last) = self.range.unwrap_or((0, count - 1));
        let last = last.min(count - 1);
        let mut pages: Vec<usize> = (first..=last)
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
    pub duplex: bool,
    /// Comments & Forms.
    pub comments: AnnotationFilter,
    /// Print as Image: every sheet becomes pixels.
    pub print_as_image: bool,
    /// The resolution Print as Image renders at.
    pub image_dpi: f32,
}

impl Default for PrintJob {
    fn default() -> Self {
        PrintJob {
            paper: PaperSize::LETTER,
            orientation: Orientation::Auto,
            selection: PageSelection::default(),
            sizing: Sizing::Fit,
            n_up: NUp::default(),
            duplex: false,
            comments: AnnotationFilter::DocumentAndMarkups,
            print_as_image: false,
            image_dpi: 150.0,
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
            range: Some((1, 9)),
            reverse: true,
            ..PageSelection::default()
        };
        assert_eq!(selection.pages(4), [3, 2, 1]);
        assert!(PageSelection::default().pages(0).is_empty());
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
