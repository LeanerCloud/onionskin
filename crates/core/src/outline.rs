//! The document outline, as the bookmarks pane reads it.
//!
//! Structural only. A bookmark is a title and, when the file gives one that
//! resolves, the page it goes to. A bookmark whose destination is missing,
//! broken or points outside this document keeps `page: None` rather than
//! being dropped or defaulted to page one: the pane shows it and says it has
//! nowhere to go, which is what the file says.

use std::collections::{BTreeMap, BTreeSet};

use onionskin_cos::{Dict, Document as CosDocument, Object};

use crate::{Error, PageIndex, Result};

/// A hostile `/Outlines` can chain siblings without ever repeating a node on
/// one path, so the cycle set alone does not bound the walk. Both caps are
/// generous against real files: the largest outline in the corpus is under
/// 3000 entries, and nesting past 32 is a producer bug rather than a
/// structure a reader has to serve.
const MAX_ITEMS: usize = 20_000;
const MAX_DEPTH: usize = 32;

/// One bookmark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineItem {
    pub title: String,
    /// The page this bookmark goes to, when the file names one this document
    /// can resolve.
    pub page: Option<PageIndex>,
    pub children: Vec<OutlineItem>,
}

impl OutlineItem {
    /// This item and its descendants, depth first, which is the order the
    /// pane draws rows in.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(OutlineItem::count).sum::<usize>()
    }
}

/// Read `/Outlines` in document order.
///
/// An absent `/Outlines` is an empty outline, not an error: most PDFs have
/// none. A present one that is not a dictionary is an error, because the file
/// claims an outline the reader cannot produce and silently showing an empty
/// pane would read as "this document has no bookmarks".
pub(crate) fn read(doc: &CosDocument, page_count: usize) -> Result<Vec<OutlineItem>> {
    let catalog = doc.catalog()?;
    let Some(outlines) = catalog.get(b"Outlines") else {
        return Ok(Vec::new());
    };
    let outlines = doc.resolve(outlines)?;
    if matches!(outlines, Object::Null) {
        return Ok(Vec::new());
    }
    let outlines = outlines.as_dict().cloned().ok_or_else(|| {
        Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: "/Outlines does not resolve to a dictionary".into(),
        })
    })?;

    let mut reader = Reader {
        doc,
        page_count,
        pages: None,
        seen: BTreeSet::new(),
        budget: MAX_ITEMS,
    };
    reader.siblings(&outlines, 0)
}

struct Reader<'a> {
    doc: &'a CosDocument,
    page_count: usize,
    /// Object number of each page, to its index in document order. Built at
    /// most once, and only when a destination actually names a page by
    /// reference, because building it walks the page tree once per page.
    pages: Option<BTreeMap<u32, PageIndex>>,
    /// Object numbers already turned into an item. A node reached twice is a
    /// cycle or a shared subtree; either way, following it again would not
    /// terminate.
    seen: BTreeSet<u32>,
    budget: usize,
}

impl Reader<'_> {
    /// The chain hanging off `parent`'s `/First`, following `/Next`.
    fn siblings(&mut self, parent: &Dict, depth: usize) -> Result<Vec<OutlineItem>> {
        if depth >= MAX_DEPTH {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        let mut next = parent.get(b"First").and_then(Object::as_reference);
        while let Some(node) = next {
            if self.budget == 0 || !self.seen.insert(node.number) {
                break;
            }
            self.budget -= 1;
            let Some(dict) = self.doc.get(node.number)?.object.as_dict().cloned() else {
                break;
            };
            items.push(OutlineItem {
                title: title(&dict),
                page: self.destination(&dict)?,
                children: self.siblings(&dict, depth + 1)?,
            });
            next = dict.get(b"Next").and_then(Object::as_reference);
        }
        Ok(items)
    }

    /// The page an item goes to: its own `/Dest`, else the `/D` of a `/GoTo`
    /// action on `/A`. Anything else, including a `/GoToR` into another file,
    /// is no destination in this document.
    fn destination(&mut self, item: &Dict) -> Result<Option<PageIndex>> {
        let target = match item.get(b"Dest") {
            Some(dest) => Some(self.doc.resolve(dest)?),
            None => self.action_destination(item)?,
        };
        match target {
            Some(target) => self.resolve_destination(target, 0),
            None => Ok(None),
        }
    }

    fn action_destination(&mut self, item: &Dict) -> Result<Option<Object>> {
        let Some(action) = item.get(b"A") else {
            return Ok(None);
        };
        let action = self.doc.resolve(action)?;
        let Some(action) = action.as_dict() else {
            return Ok(None);
        };
        let is_goto = action
            .get(b"S")
            .and_then(Object::as_name)
            .is_some_and(|name| name.as_bytes() == b"GoTo");
        if !is_goto {
            return Ok(None);
        }
        match action.get(b"D") {
            Some(dest) => Ok(Some(self.doc.resolve(dest)?)),
            None => Ok(None),
        }
    }

    /// A destination is an array whose first element names the page, or a
    /// name or string standing for one of those in the document's name tree.
    /// `depth` bounds the one indirection a named destination is allowed:
    /// a name tree entry that is itself a name would otherwise loop.
    fn resolve_destination(&mut self, target: Object, depth: usize) -> Result<Option<PageIndex>> {
        if depth > 1 {
            return Ok(None);
        }
        match target {
            Object::Array(entries) => {
                let Some(first) = entries.first() else {
                    return Ok(None);
                };
                match first {
                    // The common form: the page itself, by reference.
                    Object::Ref(page) => Ok(self.page_index(page.number)),
                    // A page number rather than a reference. Legal in a
                    // remote destination and written by some producers for
                    // local ones too; taken only when it is a page this
                    // document actually has.
                    Object::Integer(index) => Ok(usize::try_from(*index)
                        .ok()
                        .filter(|index| *index < self.page_count)),
                    _ => Ok(None),
                }
            }
            Object::Dict(dict) => match dict.get(b"D") {
                Some(inner) => {
                    let inner = self.doc.resolve(inner)?;
                    self.resolve_destination(inner, depth + 1)
                }
                None => Ok(None),
            },
            Object::Name(name) => {
                let named = self.named_destination(name.as_bytes())?;
                match named {
                    Some(named) => self.resolve_destination(named, depth + 1),
                    None => Ok(None),
                }
            }
            Object::String(key) => {
                let named = self.named_destination(&key)?;
                match named {
                    Some(named) => self.resolve_destination(named, depth + 1),
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    /// Look `key` up in the catalog's `/Dests` dictionary (PDF 1.1) and then
    /// in the `/Names /Dests` name tree (PDF 1.2 on). Both are searched
    /// because a file may carry either, and neither is a default for the
    /// other.
    fn named_destination(&mut self, key: &[u8]) -> Result<Option<Object>> {
        let catalog = self.doc.catalog()?;
        if let Some(dests) = catalog.get(b"Dests") {
            let dests = self.doc.resolve(dests)?;
            if let Some(hit) = dests.as_dict().and_then(|dests| dests.get(key)) {
                return Ok(Some(self.doc.resolve(hit)?));
            }
        }
        let Some(names) = catalog.get(b"Names") else {
            return Ok(None);
        };
        let names = self.doc.resolve(names)?;
        let Some(dests) = names
            .as_dict()
            .and_then(|names| names.get(b"Dests"))
            .cloned()
        else {
            return Ok(None);
        };
        let root = self.doc.resolve(&dests)?;
        self.name_tree_lookup(root, key, 0)
    }

    /// Walk a name tree for `key`. `/Limits` prunes subtrees, but is trusted
    /// only to skip: a node whose limits exclude the key is skipped, and a
    /// node without limits is searched, so a file that lies about its limits
    /// costs time rather than a missed entry.
    fn name_tree_lookup(
        &mut self,
        node: Object,
        key: &[u8],
        depth: usize,
    ) -> Result<Option<Object>> {
        if depth >= MAX_DEPTH {
            return Ok(None);
        }
        let Some(node) = node.as_dict().cloned() else {
            return Ok(None);
        };
        if let Some(names) = node.get(b"Names") {
            let names = self.doc.resolve(names)?;
            if let Some(entries) = names.as_array() {
                for pair in entries.as_chunks::<2>().0 {
                    let Object::String(name) = &pair[0] else {
                        continue;
                    };
                    if name == key {
                        return Ok(Some(self.doc.resolve(&pair[1])?));
                    }
                }
            }
        }
        let Some(kids) = node.get(b"Kids") else {
            return Ok(None);
        };
        let kids = self.doc.resolve(kids)?;
        let Some(kids) = kids.as_array().map(<[Object]>::to_vec) else {
            return Ok(None);
        };
        for kid in kids {
            let kid = self.doc.resolve(&kid)?;
            if !self.limits_may_contain(&kid, key)? {
                continue;
            }
            if let Some(hit) = self.name_tree_lookup(kid, key, depth + 1)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    fn limits_may_contain(&self, node: &Object, key: &[u8]) -> Result<bool> {
        let Some(limits) = node.as_dict().and_then(|node| node.get(b"Limits")) else {
            return Ok(true);
        };
        let limits = self.doc.resolve(limits)?;
        let Some([Object::String(low), Object::String(high)]) = limits
            .as_array()
            .and_then(|limits| limits.first_chunk::<2>())
        else {
            return Ok(true);
        };
        Ok(low.as_slice() <= key && key <= high.as_slice())
    }

    /// The index of the page living in object `number`.
    ///
    /// The map comes from `cos::Document::page`, one call per page, rather
    /// than from a page-tree walk written here: a second descent is a second
    /// set of rules about what counts as a page, and the two disagreeing
    /// would misnumber destinations only on the trees where it matters. The
    /// cost is one walk per page, paid once per document and only when a
    /// bookmark actually names a page by reference.
    fn page_index(&mut self, number: u32) -> Option<PageIndex> {
        if self.pages.is_none() {
            let mut map = BTreeMap::new();
            for index in 0..self.page_count {
                if let Ok(page) = self.doc.page(index) {
                    map.entry(page.objref.number).or_insert(index);
                }
            }
            self.pages = Some(map);
        }
        self.pages
            .as_ref()
            .expect("the map was just built")
            .get(&number)
            .copied()
    }
}

/// `/Title`, decoded from whichever of the two PDF text-string encodings the
/// file used. An item with no title is an empty string rather than a
/// substituted one: the pane draws a blank row, which is what the file says.
fn title(item: &Dict) -> String {
    match item.get(b"Title") {
        Some(Object::String(bytes)) => onionskin_content::pdf_text_string(bytes),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testpdf::{dict, pages, pdf};

    fn read_outline(bytes: Vec<u8>, page_count: usize) -> Vec<OutlineItem> {
        let (doc, _) = onionskin_cos::Document::open_repairing(Box::new(
            onionskin_cos::BytesSource::new(bytes),
        ))
        .expect("the fixture opens");
        read(&doc, page_count).expect("the outline reads")
    }

    /// Objects 1 and 2 are the catalog and the page tree root, 3.. are the
    /// pages, and the outline objects follow them.
    fn document(catalog: &str, page_count: usize, outline: &[&str]) -> Vec<u8> {
        let (tree, page_bodies) = pages(3, page_count);
        let mut objects = vec![dict(catalog), tree];
        objects.extend(page_bodies);
        objects.extend(outline.iter().map(|body| dict(body)));
        pdf(&objects)
    }

    #[test]
    fn a_nested_outline_reads_in_document_order_with_its_destinations() {
        // Pages are objects 3 and 4; the outline root is 5, its two top-level
        // items 6 and 8, and 6's child is 7.
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R >>",
                2,
                &[
                    "<< /Type /Outlines /First 6 0 R /Last 8 0 R /Count 3 >>",
                    "<< /Title (Chapter one) /Parent 5 0 R /First 7 0 R /Last 7 0 R \
                      /Next 8 0 R /Dest [3 0 R /XYZ 0 100 0] >>",
                    "<< /Title (Section one point one) /Parent 6 0 R \
                      /Dest [4 0 R /Fit] >>",
                    "<< /Title (Chapter two) /Parent 5 0 R /Prev 6 0 R \
                      /A << /S /GoTo /D [4 0 R /XYZ 0 50 0] >> >>",
                ],
            ),
            2,
        );

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Chapter one");
        assert_eq!(items[0].page, Some(0));
        assert_eq!(items[0].children.len(), 1);
        assert_eq!(items[0].children[0].title, "Section one point one");
        assert_eq!(items[0].children[0].page, Some(1));
        assert!(items[0].children[0].children.is_empty());
        // The second top-level item reaches its page through /A /GoTo, which
        // is the other half of the destination rule.
        assert_eq!(items[1].title, "Chapter two");
        assert_eq!(items[1].page, Some(1));
        assert_eq!(items.iter().map(OutlineItem::count).sum::<usize>(), 3);
    }

    /// The classic hostile document: `/Next` points back at an earlier
    /// sibling. The walk has to stop, and stop with the items it had.
    #[test]
    fn a_cyclic_sibling_chain_terminates_with_the_items_it_reached() {
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
                1,
                &[
                    "<< /Type /Outlines /First 5 0 R >>",
                    "<< /Title (first) /Next 6 0 R /Dest [3 0 R /Fit] >>",
                    "<< /Title (second) /Next 5 0 R >>",
                ],
            ),
            1,
        );

        assert_eq!(
            items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(items[0].page, Some(0));
        assert_eq!(items[1].page, None);
    }

    /// A `/First` chain that descends into itself terminates too, and does
    /// not repeat the node it came from as a child.
    #[test]
    fn a_cyclic_child_chain_terminates() {
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
                1,
                &[
                    "<< /Type /Outlines /First 5 0 R >>",
                    "<< /Title (self parent) /First 5 0 R >>",
                ],
            ),
            1,
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "self parent");
        assert!(items[0].children.is_empty());
    }

    #[test]
    fn a_named_destination_resolves_through_the_name_tree() {
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R /Names << /Dests 8 0 R >> >>",
                3,
                &[
                    "<< /Type /Outlines /First 7 0 R >>",
                    "<< /Title (by name) /Dest (chapter.two) >>",
                    "<< /Kids [9 0 R 10 0 R] >>",
                    "<< /Limits [(a) (b)] /Names [(alpha) [3 0 R /Fit]] >>",
                    "<< /Limits [(c) (d)] /Names [(chapter.two) [5 0 R /Fit]] >>",
                ],
            ),
            3,
        );

        assert_eq!(items.len(), 1);
        // Object 5 is the third page, so the name tree, not the array
        // position, decided this.
        assert_eq!(items[0].page, Some(2));
    }

    /// `/Limits` may skip a subtree, so a lookup whose key sits outside every
    /// kid's range finds nothing rather than falling through to a linear scan
    /// that would have found it. Pinned because the pruning is the part that
    /// can silently return the wrong page.
    #[test]
    fn a_name_tree_lookup_outside_every_limit_finds_nothing() {
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /Names << /Dests 7 0 R >> >>",
                2,
                &[
                    "<< /Type /Outlines /First 6 0 R >>",
                    "<< /Title (missing) /Dest (zeta) >>",
                    "<< /Kids [8 0 R] >>",
                    "<< /Limits [(a) (b)] /Names [(alpha) [4 0 R /Fit]] >>",
                ],
            ),
            2,
        );

        assert_eq!(items[0].page, None);
    }

    #[test]
    fn a_destination_outside_the_document_is_no_destination() {
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
                1,
                &[
                    "<< /Type /Outlines /First 5 0 R /Last 7 0 R >>",
                    // A page index past the end, written as a number.
                    "<< /Title (past the end) /Dest [9 /Fit] /Next 6 0 R >>",
                    // A reference to an object that is not a page.
                    "<< /Title (not a page) /Dest [4 0 R /Fit] /Next 7 0 R >>",
                    // A remote go-to, which is a destination in another file.
                    "<< /Title (remote) /A << /S /GoToR /D [0 /Fit] >> >>",
                ],
            ),
            1,
        );

        assert_eq!(items.len(), 3);
        assert!(
            items.iter().all(|item| item.page.is_none()),
            "no unresolvable destination may be turned into a page"
        );
        // Still listed: a bookmark that goes nowhere is the file's own state.
        assert_eq!(items[2].title, "remote");
    }

    #[test]
    fn a_utf16_title_decodes_and_an_absent_one_is_empty() {
        let items = read_outline(
            document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
                1,
                &[
                    "<< /Type /Outlines /First 5 0 R >>",
                    "<< /Title <FEFF00480069> /Next 6 0 R >>",
                    "<< /Dest [3 0 R /Fit] >>",
                ],
            ),
            1,
        );

        assert_eq!(items[0].title, "Hi");
        assert_eq!(items[1].title, "");
        assert_eq!(items[1].page, Some(0));
    }

    #[test]
    fn a_document_without_an_outline_reads_as_empty() {
        assert!(read_outline(document("<< /Type /Catalog /Pages 2 0 R >>", 1, &[]), 1).is_empty());
    }

    /// A file that claims an outline the reader cannot produce says so.
    /// Showing an empty pane instead would read as "no bookmarks".
    #[test]
    fn an_outline_that_is_not_a_dictionary_fails_loudly() {
        let (doc, _) = onionskin_cos::Document::open_repairing(Box::new(
            onionskin_cos::BytesSource::new(document(
                "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>",
                1,
                &["[1 2 3]"],
            )),
        ))
        .expect("the fixture opens");

        assert!(read(&doc, 1).is_err());
    }
}
