//! From a structure element to the page content it marks.
//!
//! An element names its content by `/MCID`, on its own `/Pg` or on the page
//! its `/MCR` names. [`ContentMap`] interprets each page once, with
//! [`onionskin_content::page_marked`], and answers for any element from that.
//!
//! **A reference that cannot be followed is reported, not dropped.** An id with
//! no page, an `/MCR` into a form or appearance stream (its ids number that
//! stream's own content, which this does not interpret), and an id the page
//! never opens each come back in [`ElementContent::unplaced`], so a consumer
//! cannot mistake "marks nothing" for "marks something I could not find". A
//! sequence that is opened and draws nothing is a legitimate empty element.
//!
//! `/OBJR` kids (annotations and XObjects) are not content items here.
//!
//! Items come out as text runs, then images, then paths, each in the order
//! the page drew them; the order across kinds is not preserved.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_content::{page_marked, MarkedPage, PageIndex};
use onionskin_cos::{Document as CosDocument, ObjRef};

use super::read::{Element, Kid};
use crate::Result;

/// What kind of content an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Image,
    Path,
}

/// One run, image or path an element marks.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentItem {
    pub page: PageIndex,
    pub kind: ItemKind,
    /// `[x0, y0, x1, y1]` in the space `PageQuad` uses: the page's default user
    /// space with the media box's lower left as the origin.
    pub bounds: [f64; 4],
    /// A run's text: its marked-content `/ActualText` where it has one, said
    /// once for the whole occurrence (the later runs of it read as empty),
    /// else the decoded glyphs. `None` for an image or path, which carry no
    /// text of their own; their words are the element's `/Alt`.
    pub text: Option<String>,
    /// Drawn inside an `/Artifact` sequence as well, so not logical content
    /// whatever else names it.
    pub artifact: bool,
}

/// A kid whose content could not be located.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unplaced {
    /// An id on an element, or an `/MCR`, that names no page.
    NoPage { mcid: i64 },
    /// An `/MCR` into a form or appearance stream.
    InStream { mcid: i64, stream: ObjRef },
    /// An `/MCR` or element `/Pg` that is not a page of this document.
    NotAPage { page: ObjRef, mcid: i64 },
    /// The page never opens a sequence with this id.
    Dangling { page: PageIndex, mcid: i64 },
    /// Another element already took this id, and the map was asked to deliver
    /// each once. A valid file has one parent per id; a hostile one can name a
    /// large sequence from every element.
    Claimed { page: PageIndex, mcid: i64 },
}

/// The content one element marks directly, not its descendants'.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElementContent {
    pub items: Vec<ContentItem>,
    /// The `/Obj` of each `/OBJR` kid: an annotation or XObject that belongs to
    /// the element and has no page content to place here. An element with only
    /// these (a `Link`, a `Form`) marks no content and is not empty.
    pub objects: Vec<ObjRef>,
    pub unplaced: Vec<Unplaced>,
}

/// One interpreted page, indexed by the ids its sequences open.
struct PageContent {
    opened: BTreeSet<i64>,
    by_mcid: BTreeMap<i64, Vec<ContentItem>>,
}

/// Page content, interpreted on first use and kept.
pub struct ContentMap<'a> {
    doc: &'a CosDocument,
    pages: BTreeMap<u32, PageIndex>,
    interpreted: BTreeMap<PageIndex, PageContent>,
    /// `Some` once the map delivers each `(page, id)` to one element only.
    claimed: Option<BTreeSet<(PageIndex, i64)>>,
}

impl<'a> ContentMap<'a> {
    pub fn new(doc: &'a CosDocument) -> Result<Self> {
        let count = usize::try_from(doc.page_count()?).unwrap_or(0);
        let mut pages = BTreeMap::new();
        for index in 0..count {
            pages.insert(doc.page(index)?.objref.number, index);
        }
        Ok(Self {
            doc,
            pages,
            interpreted: BTreeMap::new(),
            claimed: None,
        })
    }

    /// Deliver each `(page, id)` to the first element that names it and report
    /// a later claim as [`Unplaced::Claimed`]. For a walk over the whole tree,
    /// where a sequence named by every element would otherwise be copied into
    /// each of them.
    pub fn each_claim_once(mut self) -> Self {
        self.claimed = Some(BTreeSet::new());
        self
    }

    /// The content `element` marks directly.
    pub fn content_of(&mut self, element: &Element) -> Result<ElementContent> {
        self.content_of_kids(element, &element.kids)
    }

    /// The content named by `kids`, a stretch of `element`'s own `/K`, which
    /// gives a bare id its page. Kids that are elements are skipped.
    pub(crate) fn content_of_kids(
        &mut self,
        element: &Element,
        kids: &[Kid],
    ) -> Result<ElementContent> {
        let mut out = ElementContent::default();
        for kid in kids {
            let (page, mcid) = match kid {
                Kid::Mcid(mcid) => (element.page, *mcid),
                Kid::MarkedContent {
                    stream: Some(stream),
                    mcid,
                    ..
                } => {
                    out.unplaced.push(Unplaced::InStream {
                        mcid: *mcid,
                        stream: *stream,
                    });
                    continue;
                }
                Kid::MarkedContent { page, mcid, .. } => (page.or(element.page), *mcid),
                Kid::Object { object, .. } => {
                    out.objects.push(*object);
                    continue;
                }
                Kid::Element(_) => continue,
            };
            let Some(page) = page else {
                out.unplaced.push(Unplaced::NoPage { mcid });
                continue;
            };
            let Some(&index) = self.pages.get(&page.number) else {
                out.unplaced.push(Unplaced::NotAPage { page, mcid });
                continue;
            };
            let content = self.page(index)?;
            if !content.opened.contains(&mcid) {
                out.unplaced.push(Unplaced::Dangling { page: index, mcid });
                continue;
            }
            if let Some(claimed) = self.claimed.as_mut() {
                if !claimed.insert((index, mcid)) {
                    out.unplaced.push(Unplaced::Claimed { page: index, mcid });
                    continue;
                }
            }
            if let Some(items) = self.interpreted[&index].by_mcid.get(&mcid) {
                out.items.extend(items.iter().cloned());
            }
        }
        Ok(out)
    }

    fn page(&mut self, index: PageIndex) -> Result<&PageContent> {
        if !self.interpreted.contains_key(&index) {
            let marked = page_marked(self.doc, index)?;
            self.interpreted.insert(index, index_page(index, marked));
        }
        Ok(&self.interpreted[&index])
    }
}

fn index_page(page: PageIndex, marked: MarkedPage) -> PageContent {
    let mut by_mcid: BTreeMap<i64, Vec<ContentItem>> = BTreeMap::new();
    let mut add = |reference: &Option<onionskin_content::MarkedRef>, item: ContentItem| {
        if let Some(mcid) = reference.as_ref().and_then(|reference| reference.mcid) {
            by_mcid.entry(mcid).or_default().push(ContentItem {
                artifact: reference
                    .as_ref()
                    .is_some_and(|reference| reference.artifact),
                ..item
            });
        }
    };
    let mut spoken: Option<&onionskin_content::ActualText> = None;
    for run in &marked.text.runs {
        let corners = run.glyphs.iter().flat_map(|glyph| glyph.quad.corners);
        let text = match &run.actual_text {
            Some(actual) if spoken == Some(actual) => String::new(),
            Some(actual) => {
                spoken = Some(actual);
                actual.as_str().to_owned()
            }
            None => run.decoded_text.clone(),
        };
        add(
            &run.marked,
            ContentItem {
                page,
                kind: ItemKind::Text,
                bounds: bounds_of(corners),
                text: Some(text),
                artifact: false,
            },
        );
    }
    for image in &marked.images {
        add(
            &image.marked,
            ContentItem {
                page,
                kind: ItemKind::Image,
                bounds: image.bounds(),
                text: None,
                artifact: false,
            },
        );
    }
    for shape in &marked.shapes {
        add(
            &shape.marked,
            ContentItem {
                page,
                kind: ItemKind::Path,
                bounds: shape.bounds(),
                text: None,
                artifact: false,
            },
        );
    }
    PageContent {
        opened: marked.mcids,
        by_mcid,
    }
}

fn bounds_of(points: impl Iterator<Item = (f64, f64)>) -> [f64; 4] {
    points.fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |out, (x, y)| [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)],
    )
}
