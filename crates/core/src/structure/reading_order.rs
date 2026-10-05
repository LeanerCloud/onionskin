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

use std::collections::{BTreeSet, HashMap};

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
    /// What stands for this element's whole subtree: its `/ActualText`, or its
    /// `/Alt` where the element is an illustration or has no text below it,
    /// with the page of the first content in the subtree. Set on the element's
    /// first block only; its descendants are then [`excluded`](Self::excluded).
    pub replacement: Option<Replacement>,
    /// This block's content is not read: it is in the subtree of an element
    /// whose `replacement` says it, or in a PDF 2.0 `Artifact` element, which is
    /// not content. A consumer that reads blocks skips it; one that describes
    /// structure may still show the element.
    pub excluded: bool,
    /// The annotations and XObjects the block's `/OBJR` kids name; see
    /// [`ElementContent::objects`](super::content_map::ElementContent::objects).
    pub objects: Vec<ObjRef>,
    pub unplaced: Vec<Unplaced>,
}

/// What stands for a subtree, and the page it is said on.
#[derive(Clone, Debug, PartialEq)]
pub struct Replacement {
    pub text: String,
    pub page: PageIndex,
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
    let mut blocks = walk.out;
    resolve_replacements(&mut blocks);
    Ok(blocks)
}

/// One past the last block of the subtree whose first block is `blocks[start]`:
/// the element's later continuation blocks and everything deeper.
fn subtree_end(blocks: &[Block], start: usize) -> usize {
    let (depth, element) = (blocks[start].depth, blocks[start].element);
    let mut end = start + 1;
    while end < blocks.len()
        && (blocks[end].depth > depth
            || (blocks[end].depth == depth
                && blocks[end].continuation
                && blocks[end].element == element))
    {
        end += 1;
    }
    end
}

/// Whether an element of this type is replaced by its `/Alt` whatever it
/// contains: the illustration elements of ISO 32000-1 14.8.4.5.
fn is_illustration(standard_type: Option<&Name>) -> bool {
    standard_type.is_some_and(|name| matches!(name.as_bytes(), b"Figure" | b"Formula" | b"Form"))
}

fn is_caption(block: &Block) -> bool {
    [&block.struct_type, &block.standard_type]
        .into_iter()
        .flatten()
        .any(|name| name.as_bytes() == b"Caption")
}

/// Settle which elements are said by a replacement, and exclude what each one
/// replaces, so no consumer says an element's text and its substitute, or the
/// text of a PDF 2.0 `Artifact`.
fn resolve_replacements(blocks: &mut [Block]) {
    let mut index = 0;
    while index < blocks.len() {
        if blocks[index].excluded || blocks[index].continuation {
            index += 1;
            continue;
        }
        let end = subtree_end(blocks, index);
        // By the type as written: `Artifact` is a PDF 2.0 type, so a role map
        // that does not reach the 2.0 namespace leaves it with no standard type.
        if blocks[index]
            .struct_type
            .as_ref()
            .is_some_and(|name| name.as_bytes() == b"Artifact")
        {
            blocks[index..end]
                .iter_mut()
                .for_each(|b| b.excluded = true);
            index = end;
            continue;
        }
        let spoken = |item: &&ContentItem| !item.artifact;
        let subtree = &blocks[index..end];
        let has_text = subtree
            .iter()
            .flat_map(|b| &b.items)
            .filter(spoken)
            .any(|item| item.text.as_deref().is_some_and(|text| !text.is_empty()));
        // An empty string says nothing, so it replaces nothing: a producer that
        // writes `/ActualText ()` on a Document has not asked for it to be mute.
        let first = &blocks[index];
        let actual = first.actual_text.clone().filter(|text| !text.is_empty());
        let alt = first
            .alt
            .clone()
            .filter(|text| !text.is_empty())
            .filter(|_| is_illustration(first.standard_type.as_ref()) || !has_text);
        let from_alt = actual.is_none() && alt.is_some();
        let text = actual.or(alt);
        let page = subtree
            .iter()
            .flat_map(|b| &b.items)
            .find(spoken)
            .map(|item| item.page);
        if let (Some(text), Some(page)) = (text, page) {
            blocks[index].replacement = Some(Replacement { text, page });
            // An `/Alt` describes the illustration, not a caption inside it,
            // which is text of its own (ISO 32000-2 14.8.4.5); an
            // `/ActualText` stands for everything below it.
            let mut below = index + 1;
            while below < end {
                if from_alt && !blocks[below].continuation && is_caption(&blocks[below]) {
                    below = subtree_end(blocks, below);
                    continue;
                }
                blocks[below].excluded = true;
                below += 1;
            }
        }
        index += 1;
    }
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
            replacement: None,
            excluded: false,
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
    append_runs(&mut out, items.iter(), &mut None);
    out
}

/// Add the text of `items` to `out`, a space between two where geometry says
/// one belongs. `previous` is the last text item written, which carries the
/// geometry across a call.
fn append_runs<'a>(
    out: &mut String,
    items: impl Iterator<Item = &'a ContentItem>,
    previous: &mut Option<&'a ContentItem>,
) {
    for item in items {
        let Some(text) = item.text.as_deref().filter(|text| !text.is_empty()) else {
            continue;
        };
        if let Some(last) = *previous {
            if needs_space(last, item) && !out.ends_with(' ') && !text.starts_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(text);
        *previous = Some(item);
    }
}

/// Whether text of this structure type runs on from what precedes it instead
/// of starting a line of its own: the inline types of ISO 32000 14.8.4, and a
/// list item's label and body, which read as one line. A type no role map
/// resolves is taken as a block, which errs toward a line break.
fn is_inline(standard_type: Option<&Name>) -> bool {
    const INLINE: &[&[u8]] = &[
        b"Span",
        b"Link",
        b"Em",
        b"Strong",
        b"Sub",
        b"Code",
        b"Quote",
        b"Reference",
        b"Note",
        b"FENote",
        b"BibEntry",
        b"Formula",
        b"Form",
        b"Lbl",
        b"LBody",
        b"Annot",
        b"Ruby",
        b"RB",
        b"RT",
        b"RP",
        b"Warichu",
        b"WT",
        b"WP",
    ];
    standard_type.is_some_and(|name| INLINE.contains(&name.as_bytes()))
}

/// What `blocks` say on `page`, in reading order, as text: one line for each
/// block-level element that has any, inline elements and the content after a
/// child continuing the line they are in.
///
/// An element's `/ActualText` replaces its whole subtree, and so does the
/// `/Alt` of an illustration or of an element with no text below it, each said
/// on the page of the first content in the subtree; a caption inside an
/// illustration is still read. An empty string replaces nothing. Text drawn in
/// an `/Artifact`, marked or as a PDF 2.0 structure element, is left out: it is
/// not part of the structure.
pub fn reading_text(blocks: &[Block], page: PageIndex) -> String {
    let mut out = String::new();
    let mut previous: Option<&ContentItem> = None;
    let mut break_pending = false;
    // Whether the most recent block at each depth, below the one being read, is
    // inline: what a continuation block looks at to know whether the child it
    // follows ended the line.
    let mut inline_at: HashMap<usize, bool> = HashMap::new();
    for block in blocks.iter().filter(|block| !block.excluded) {
        let inline = is_inline(block.standard_type.as_ref());
        if block.continuation {
            // Taken: the next continuation follows a later child, or none.
            if inline_at.remove(&(block.depth + 1)) == Some(false) {
                break_pending = true;
            }
        } else {
            inline_at.insert(block.depth, inline);
            break_pending |= !inline;
        }

        let mut said = String::new();
        let gap_before;
        if let Some(replacement) = &block.replacement {
            if replacement.page != page {
                continue;
            }
            said.push_str(&replacement.text);
            gap_before = true;
            previous = None;
        } else {
            let items = block
                .items
                .iter()
                .filter(|item| item.page == page && !item.artifact);
            let mut local = previous;
            append_runs(&mut said, items, &mut local);
            gap_before = previous.is_none();
            if !said.is_empty() {
                previous = local;
            }
        }
        if said.is_empty() {
            continue;
        }
        if break_pending && !out.is_empty() {
            out.push('\n');
            said = said.trim_start_matches(' ').to_owned();
        } else if out.ends_with(' ') {
            said = said.trim_start_matches(' ').to_owned();
        } else if !out.is_empty() && !said.starts_with(' ') && gap_before {
            out.push(' ');
        }
        out.push_str(&said);
        break_pending = false;
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
/// and a gap of a fifth of it on the line is a word space.
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
    off_the_line || gap > size * 0.2
}
