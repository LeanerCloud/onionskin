//! A tagged document as a flat sequence of blocks in logical reading order.
//!
//! A depth-first walk of the structure tree, in each element's `/K` order,
//! with every element's own content attached. It is what the screen-reader
//! tree, the accessible text export and New Bookmarks From Structure read
//! instead of each walking the tree itself.
//!
//! **Language is inherited here, not read.** [`Element::lang`] holds what the
//! element states; a block's `lang` is the nearest stated one on the way down,
//! because that is what a consumer needs and the reader stores what the file
//! says. The walk starts from the catalog's `/Lang` (ISO 32000-2 14.9.2.2), and
//! an empty `/Lang` states that the language is unknown, so it ends the
//! inheritance: the element and what is below it have no language.
//!
//! **Mixed content keeps its order.** An element's `/K` can interleave content
//! with child elements ("see ", a `Link`, " for details"). The element's first
//! block holds the content before its first child; each stretch of content that
//! follows a child subtree is a further block with `continuation` set, so
//! reading the blocks in order reads the paragraph in order.
//!
//! An element reached twice (a shared subtree) is emitted once, where it is
//! first reached, so a cycle ends the walk instead of repeating it.

use std::collections::BTreeSet;

use onionskin_content::{pdf_text_string, PageIndex};
use onionskin_cos::{Document as CosDocument, Name, ObjRef, Object};

use super::content_map::{ContentItem, ContentMap, Unplaced};
use super::read::{Element, Kid, StructureTree};
use crate::Result;

/// One stretch of a structure element's content.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The element's object number, the key into [`StructureTree::elements`].
    pub element: u32,
    /// Content that follows a child element of `element`. The fields that
    /// describe the element itself (`alt`, `actual_text`, `title`) are set only
    /// on its first block, so a consumer does not say them twice.
    pub continuation: bool,
    /// Distance below the root's `/K`: top-level elements are 0.
    pub depth: usize,
    /// `/S` as written.
    pub struct_type: Option<Name>,
    /// `/S` through the role map; see [`Element::standard_type`].
    pub standard_type: Option<Name>,
    /// The nearest `/Lang` stated on this element or an ancestor; `None` when
    /// none is, or the nearest one is empty (unknown).
    pub lang: Option<String>,
    pub alt: Option<String>,
    pub actual_text: Option<String>,
    pub title: Option<String>,
    /// The page of the first content item; `None` when the block has none.
    pub page: Option<PageIndex>,
    /// The words of this block's text runs, in drawing order. Not replaced by
    /// `actual_text`, which a consumer weighs itself.
    pub text: String,
    pub items: Vec<ContentItem>,
    /// The annotations and XObjects the block's `/OBJR` kids name; see
    /// [`ElementContent::objects`](super::content_map::ElementContent::objects).
    pub objects: Vec<ObjRef>,
    pub unplaced: Vec<Unplaced>,
}

/// Every element of `tree` as [`Block`]s, in reading order.
pub fn reading_order(doc: &CosDocument, tree: &StructureTree) -> Result<Vec<Block>> {
    let mut walk = Walk {
        tree,
        map: ContentMap::new(doc)?.each_claim_once(),
        seen: BTreeSet::new(),
        out: Vec::new(),
    };
    let catalog_lang = catalog_lang(doc)?;
    for kid in &tree.roots {
        if let Kid::Element(number) = kid {
            walk.element(*number, 0, catalog_lang.as_deref())?;
        }
    }
    Ok(walk.out)
}

/// The catalog's `/Lang`, which applies to everything no element or sequence
/// overrides. Empty reads as none.
fn catalog_lang(doc: &CosDocument) -> Result<Option<String>> {
    let catalog = doc.catalog()?;
    let Some(value) = catalog.get(b"Lang") else {
        return Ok(None);
    };
    Ok(match doc.resolve(value) {
        Ok(Object::String(bytes)) => Some(pdf_text_string(&bytes)).filter(|lang| !lang.is_empty()),
        _ => None,
    })
}

struct Walk<'a> {
    tree: &'a StructureTree,
    map: ContentMap<'a>,
    seen: BTreeSet<u32>,
    out: Vec<Block>,
}

impl Walk<'_> {
    fn element(&mut self, number: u32, depth: usize, inherited: Option<&str>) -> Result<()> {
        // The reader bounds `/K` nesting, so recursion here is bounded too.
        if !self.seen.insert(number) {
            return Ok(());
        }
        let Some(element) = self.tree.elements.get(&number) else {
            return Ok(());
        };
        let lang = match element.lang.as_deref() {
            Some("") => None,
            Some(stated) => Some(stated.to_owned()),
            None => inherited.map(str::to_owned),
        };
        let mut run: Vec<Kid> = Vec::new();
        let mut emitted = false;
        for kid in &element.kids {
            let Kid::Element(child) = kid else {
                run.push(kid.clone());
                continue;
            };
            if !emitted || !run.is_empty() {
                self.emit(number, depth, &lang, &run, emitted)?;
                emitted = true;
                run.clear();
            }
            self.element(*child, depth + 1, lang.as_deref())?;
        }
        if !emitted || !run.is_empty() {
            self.emit(number, depth, &lang, &run, emitted)?;
        }
        Ok(())
    }

    fn emit(
        &mut self,
        number: u32,
        depth: usize,
        lang: &Option<String>,
        run: &[Kid],
        continuation: bool,
    ) -> Result<()> {
        let element: &Element = &self.tree.elements[&number];
        let content = self.map.content_of_kids(element, run)?;
        let own = |value: &Option<String>| value.clone().filter(|_| !continuation);
        self.out.push(Block {
            element: number,
            continuation,
            depth,
            struct_type: element.struct_type.clone(),
            standard_type: element.standard_type.clone(),
            lang: lang.clone(),
            alt: own(&element.alt),
            actual_text: own(&element.actual_text),
            title: own(&element.title),
            page: content.items.first().map(|item| item.page),
            text: join_text(&content.items),
            items: content.items,
            objects: content.objects,
            unplaced: content.unplaced,
        });
        Ok(())
    }
}

/// The text of `items` in order. Runs are joined with nothing between them
/// unless geometry says a space belongs there: a producer that splits a word
/// across operators must not be given a space inside it.
fn join_text(items: &[ContentItem]) -> String {
    let mut out = String::new();
    let mut previous: Option<&ContentItem> = None;
    for item in items {
        let Some(text) = item.text.as_deref().filter(|text| !text.is_empty()) else {
            continue;
        };
        if let Some(last) = previous {
            if needs_space(last, item) && !out.ends_with(' ') && !text.starts_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(text);
        previous = Some(item);
    }
    out
}

/// Whether a space stands between two text runs: the next run is on another
/// line, or on the same line and not touching.
///
/// The boxes are page-space rectangles. The short side of the previous one is
/// the size of its text and lines run along its long side, so a run on a page
/// turned a quarter is handled by swapping the axes. A run more than three
/// quarters of that size off the line is another line (a superscript is not),
/// and a gap of a quarter of it on the line is a word space.
fn needs_space(last: &ContentItem, next: &ContentItem) -> bool {
    let side = |b: &[f64; 4], axis: usize| b[axis + 2] - b[axis];
    let vertical = side(&last.bounds, 1) > side(&last.bounds, 0)
        && side(&next.bounds, 1) > side(&next.bounds, 0);
    let (along, across) = if vertical { (1, 0) } else { (0, 1) };
    let size = side(&last.bounds, across).max(f64::EPSILON);
    let centre = |b: &[f64; 4]| (b[across] + b[across + 2]) / 2.0;
    let off_the_line = (centre(&next.bounds) - centre(&last.bounds)).abs() > size * 0.75;
    let gap = (next.bounds[along] - last.bounds[along + 2])
        .max(last.bounds[along] - next.bounds[along + 2]);
    off_the_line || gap > size * 0.25
}
