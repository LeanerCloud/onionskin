//! The codec contract: turning pages of an open document into some other
//! format.
//!
//! M2 has three consumers, all in `codecs-common` and all exports: plain
//! text, PNG and SVG. Import is deliberately absent - reading a format *into*
//! a `Document` needs an edit graph to build one, which is M3.

use std::fmt;
use std::ops::RangeInclusive;

use onionskin_core::{Document, PageIndex};

/// An inclusive run of pages an export covers.
///
/// Constructed rather than built literally so a caller cannot hand a codec a
/// range the document does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    first: PageIndex,
    last: PageIndex,
}

impl PageRange {
    pub fn new(first: PageIndex, last: PageIndex, page_count: usize) -> Result<Self, ExportError> {
        if page_count == 0 {
            return Err(ExportError::EmptyDocument);
        }
        if first > last {
            return Err(ExportError::InvalidRange { first, last });
        }
        if last >= page_count {
            return Err(ExportError::NoSuchPage {
                page: last,
                count: page_count,
            });
        }
        Ok(Self { first, last })
    }

    /// Every page of the document.
    pub fn whole(page_count: usize) -> Result<Self, ExportError> {
        Self::new(0, page_count.saturating_sub(1), page_count)
    }

    pub fn pages(self) -> RangeInclusive<PageIndex> {
        self.first..=self.last
    }
}

/// What the user asked an exporter for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportRequest {
    pub pages: PageRange,
    /// Raster resolution. 72 renders one page point to one pixel; the text
    /// and SVG codecs have no pixels and ignore it.
    pub dpi: f32,
}

impl ExportRequest {
    /// The render zoom `dpi` asks for, validated. Rasterizing codecs call
    /// this before rendering so a bad resolution is reported as itself rather
    /// than as a failure of the requested page.
    pub fn zoom(&self) -> Result<f32, ExportError> {
        if !self.dpi.is_finite() || self.dpi <= 0.0 {
            return Err(ExportError::InvalidDpi(self.dpi));
        }
        Ok(self.dpi / 72.0)
    }
}

/// How page chunks are published at the chosen destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportOutputKind {
    /// Concatenate each request-relative page chunk into one destination.
    Single,
    /// Publish each absolute page chunk at its own derived destination.
    PerPage,
}

#[derive(Debug)]
pub enum ExportError {
    EmptyDocument,
    NoSuchPage {
        page: PageIndex,
        count: usize,
    },
    InvalidRange {
        first: PageIndex,
        last: PageIndex,
    },
    InvalidDpi(f32),
    /// A page the export needed could not be read or rendered. Named by page,
    /// because "the export failed" is not something a user can act on.
    Page {
        page: PageIndex,
        source: onionskin_core::Error,
    },
    Encode {
        page: PageIndex,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDocument => write!(f, "the document has no pages to export"),
            Self::NoSuchPage { page, count } => {
                write!(f, "page {} is outside a {count}-page document", page + 1)
            }
            Self::InvalidRange { first, last } => write!(
                f,
                "export range starts at page {} and ends at page {}",
                first + 1,
                last + 1
            ),
            Self::InvalidDpi(dpi) => {
                write!(
                    f,
                    "export resolution must be positive and finite, got {dpi}"
                )
            }
            Self::Page { page, source } => write!(f, "page {}: {source}", page + 1),
            Self::Encode { page, source } => {
                write!(f, "encoding page {}: {source}", page + 1)
            }
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Page { source, .. } => Some(source),
            Self::Encode { source, .. } => Some(source.as_ref()),
            Self::EmptyDocument
            | Self::NoSuchPage { .. }
            | Self::InvalidRange { .. }
            | Self::InvalidDpi(_) => None,
        }
    }
}

/// A format an open document can be written out to.
pub trait CodecPlugin: Send + Sync {
    /// Stable identifier, e.g. "png". Namespaced by the plugin that registers
    /// it only if it needs to be; these are file formats, not commands.
    fn id(&self) -> &'static str;
    /// What a menu entry calls it, e.g. "PNG Image".
    fn name(&self) -> &'static str;
    /// Filename extension, without the dot.
    fn extension(&self) -> &'static str;
    fn output_kind(&self) -> ExportOutputKind;

    /// Export exactly one absolute page from a validated request.
    /// `first_in_request` is relative to the requested range, not page zero.
    fn export_page(
        &self,
        doc: &mut Document,
        request: &ExportRequest,
        page: PageIndex,
        first_in_request: bool,
    ) -> Result<Vec<u8>, ExportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_cannot_leave_the_document() {
        assert!(matches!(
            PageRange::new(0, 9, 4),
            Err(ExportError::NoSuchPage { page: 9, count: 4 })
        ));
        assert!(matches!(
            PageRange::new(3, 1, 4),
            Err(ExportError::InvalidRange { first: 3, last: 1 })
        ));
        assert!(matches!(
            PageRange::whole(0),
            Err(ExportError::EmptyDocument)
        ));
    }

    #[test]
    fn a_whole_document_range_covers_every_page_once() {
        let range = PageRange::whole(3).expect("three pages is a range");

        assert_eq!(range.pages().collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(
            PageRange::new(2, 2, 3).unwrap().pages().collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn resolution_converts_to_a_render_zoom_and_refuses_nonsense() {
        let request = |dpi| ExportRequest {
            pages: PageRange::whole(1).expect("one page"),
            dpi,
        };

        assert_eq!(request(144.0).zoom().unwrap(), 2.0);
        assert!(matches!(
            request(0.0).zoom(),
            Err(ExportError::InvalidDpi(dpi)) if dpi == 0.0
        ));
        assert!(matches!(
            request(f32::NAN).zoom(),
            Err(ExportError::InvalidDpi(_))
        ));
    }

    /// Every message a user could see names a one-based page, because the
    /// page numbers in the UI are one-based.
    #[test]
    fn export_failures_read_as_prose_about_a_page_the_user_can_find() {
        assert_eq!(
            ExportError::NoSuchPage { page: 9, count: 4 }.to_string(),
            "page 10 is outside a 4-page document"
        );
        assert_eq!(
            ExportError::InvalidRange { first: 3, last: 1 }.to_string(),
            "export range starts at page 4 and ends at page 2"
        );
    }
}
