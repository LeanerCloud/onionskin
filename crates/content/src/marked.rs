//! Which marked-content sequence each text run, image and path was drawn in,
//! which is how a structure element's `/MCID` finds its content.
//!
//! Only [`crate::page_marked`] fills these in. The ordinary extractors leave
//! `marked` as `None` and keep no sequence stack, so tagging costs nothing for
//! a caller that does not ask.

use std::collections::BTreeSet;

use onionskin_cos::Name;

use crate::placements::ImagePlacement;
use crate::run::PageText;
use crate::shapes::Shape;

/// The marked-content sequences open around one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkedRef {
    /// The `/MCID` of the innermost enclosing sequence that carries one and was
    /// opened by the page's own content. A form's own ids belong to the form
    /// (they are numbered separately and reached through `/Stm`), so a form's
    /// content takes the page's id around the `Do`, as `page_mcids` does.
    ///
    /// So content inside a form that the page does not enclose in a sequence of
    /// its own has `None` here whatever ids the form opens: it is reachable only
    /// by an `/MCR` with a `/Stm`, which nothing in this crate resolves. Text
    /// drawn by a soft-mask group (`gs`) takes the sequence around the `gs`.
    pub mcid: Option<i64>,
    /// The tag of the sequence that supplied `mcid`, else of the innermost
    /// sequence. `None` when that sequence's first operand is not a name, which
    /// is a malformed `BDC`; it still counts as a sequence, so its `EMC` closes
    /// it and nothing else.
    pub tag: Option<Name>,
    /// Whether any enclosing sequence is an `/Artifact`: content that is not
    /// part of the logical structure whatever else encloses it.
    pub artifact: bool,
    /// How many sequences are open.
    pub depth: usize,
}

/// One page's runs, images and paths, each with its [`MarkedRef`].
#[derive(Clone, Debug, Default)]
pub struct MarkedPage {
    /// Every `/MCID` the page's own content opens, including a sequence that
    /// draws nothing, which has no item to carry it.
    pub mcids: BTreeSet<i64>,
    pub text: PageText,
    pub images: Vec<ImagePlacement>,
    pub shapes: Vec<Shape>,
}
