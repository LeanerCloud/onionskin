/// Zero-based index into the document's page tree.
pub type PageIndex = usize;

/// Four corners in a page's user space, in Acrobat `/QuadPoints` order:
/// upper-left, upper-right, lower-left, lower-right.
///
/// The Z order is deliberate: Acrobat writes this order, and consumers read it
/// this way even though ISO 32000-1 describes the points counterclockwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageQuad {
    pub page: PageIndex,
    pub corners: [(f64, f64); 4],
}
