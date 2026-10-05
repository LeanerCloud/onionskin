//! New Bookmarks From Structure: a bookmark for each heading of a tagged
//! document, nested by heading level.
//!
//! [`plan`] decides what to make from the reading-order blocks, and
//! [`super::write::add_bookmark_tree`] makes it. Splitting them keeps the
//! decision, which is about headings and levels, testable without a file, and
//! keeps the write, which is about the outline's chain and counts, free of any
//! knowledge of structure.

use onionskin_content::PageIndex;

use crate::structure::Block;

/// One bookmark to make, with the bookmarks nested under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedBookmark {
    pub title: String,
    pub page: PageIndex,
    pub children: Vec<PlannedBookmark>,
}

impl PlannedBookmark {
    /// This bookmark and everything under it.
    pub fn count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(PlannedBookmark::count)
            .sum::<usize>()
    }
}

/// The deepest the planned tree nests: what the outline reader follows, so a
/// deeper bookmark would be written and then not read back. A file can write
/// `H5000`, and the writer walks the tree recursively.
const MAX_NEST: usize = super::MAX_DEPTH;

/// The longest a title is, cut at a word. A mis-tagged document can mark a whole
/// page as one heading, and a bookmark is a line in a narrow pane.
const MAX_TITLE_CHARS: usize = 200;

/// A heading's level: `H1` to `Hn` are their number, a bare `H` and `Title` are
/// the top, and anything else is not a heading. A bare `H` takes its level from
/// how deep its section nests in ISO 32000-1, which is not read here: a document
/// that uses only `H` gets a flat outline. `H7` and beyond are headings only
/// where the file is in the PDF 2.0 namespace, which is what makes them a
/// standard type.
fn level_of(block: &Block) -> Option<usize> {
    let name = block.standard_type.as_ref()?.as_bytes();
    let level = match name {
        b"H" | b"Title" => 1,
        [b'H', digits @ ..] if digits.iter().all(u8::is_ascii_digit) => {
            // Digits only, so a number too long for a `usize` is the one way this
            // fails: a heading, then, and as deep as any.
            std::str::from_utf8(digits)
                .ok()?
                .parse()
                .unwrap_or(MAX_NEST)
        }
        _ => return None,
    };
    Some(level.min(MAX_NEST))
}

/// The blocks that are the heading at `blocks[at]`: its own, its later
/// continuations and what is below it, up to a heading nested inside it, which
/// is a heading of its own and not part of this one's words.
fn heading_blocks(blocks: &[Block], at: usize) -> impl Iterator<Item = &Block> {
    let heading = &blocks[at];
    blocks[at..]
        .iter()
        .enumerate()
        .take_while(move |(offset, block)| {
            let own = block.element == heading.element && block.continuation;
            *offset == 0 || own || (block.depth > heading.depth && level_of(block).is_none())
        })
        .map(|(_, block)| block)
}

/// The words of the heading at `blocks[at]`: its blocks' replacements and text
/// runs joined the way reading joins them, artifacts left out, runs of white
/// space made one, and cut at [`MAX_TITLE_CHARS`] on a word.
fn title_of(blocks: &[Block], at: usize) -> String {
    squeeze(&crate::structure::spoken_words(heading_blocks(blocks, at)))
}

fn squeeze(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= MAX_TITLE_CHARS {
        return joined;
    }
    let cut: String = joined.chars().take(MAX_TITLE_CHARS).collect();
    let word_ends_at_cut = joined.chars().nth(MAX_TITLE_CHARS) == Some(' ');
    let at_word = if word_ends_at_cut {
        cut.len()
    } else {
        cut.rfind(' ').filter(|at| *at > 0).unwrap_or(cut.len())
    };
    format!("{}…", &cut[..at_word])
}

/// The page of the first content in the heading's blocks.
fn page_of(blocks: &[Block], at: usize) -> Option<PageIndex> {
    heading_blocks(blocks, at)
        .flat_map(|block| &block.items)
        .find(|item| !item.artifact)
        .map(|item| item.page)
}

/// The bookmarks a document's headings make, in reading order.
///
/// A heading nests under the nearest heading above it of a lower level, so an
/// `H3` that follows an `H1` with no `H2` between them hangs from the `H1`.
/// A heading with no words, or with no content to go to, makes no bookmark: a
/// bookmark must have a title a person can read and somewhere to go. A heading
/// an ancestor's replacement already stands for makes none either.
pub fn plan(blocks: &[Block]) -> Vec<PlannedBookmark> {
    // The open chain of headings, each with its level: a stack of the path of
    // indexes from the top to the heading last added.
    let mut roots: Vec<PlannedBookmark> = Vec::new();
    let mut open: Vec<(usize, Vec<usize>)> = Vec::new();
    for (at, block) in blocks.iter().enumerate() {
        if block.continuation || block.excluded {
            continue;
        }
        let Some(level) = level_of(block) else {
            continue;
        };
        let title = title_of(blocks, at);
        let Some(page) = page_of(blocks, at) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        while open.last().is_some_and(|(held, _)| *held >= level) {
            open.pop();
        }
        let planned = PlannedBookmark {
            title,
            page,
            children: Vec::new(),
        };
        let path = match open.last() {
            None => {
                roots.push(planned);
                vec![roots.len() - 1]
            }
            Some((_, parent)) => {
                let mut node = &mut roots[parent[0]];
                for &step in &parent[1..] {
                    node = &mut node.children[step];
                }
                node.children.push(planned);
                let mut path = parent.clone();
                path.push(node.children.len() - 1);
                path
            }
        };
        open.push((level, path));
    }
    roots
}

#[cfg(test)]
mod tests {
    use onionskin_content::PageIndex;
    use onionskin_cos::Name;

    use super::*;
    use crate::structure::ContentItem;
    use crate::ItemKind;

    fn heading(element: u32, depth: usize, kind: &str, words: &str, page: PageIndex) -> Block {
        Block {
            element,
            continuation: false,
            depth,
            struct_type: Some(Name::new(kind)),
            standard_type: Some(Name::new(kind)),
            lang: None,
            alt: None,
            actual_text: None,
            title: None,
            page: Some(page),
            text: words.to_owned(),
            items: vec![ContentItem {
                page,
                kind: ItemKind::Text,
                bounds: [0.0; 4],
                text: Some(words.to_owned()),
                artifact: false,
            }],
            replacement: None,
            excluded: false,
            objects: Vec::new(),
            unplaced: Vec::new(),
        }
    }

    fn shape(items: &[PlannedBookmark]) -> Vec<String> {
        fn walk(item: &PlannedBookmark, depth: usize, out: &mut Vec<String>) {
            out.push(format!(
                "{}{} p{}",
                "  ".repeat(depth),
                item.title,
                item.page
            ));
            for child in &item.children {
                walk(child, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        items.iter().for_each(|item| walk(item, 0, &mut out));
        out
    }

    #[test]
    fn headings_nest_by_level_in_reading_order() {
        let blocks = [
            heading(1, 1, "H1", "One", 0),
            heading(2, 1, "H2", "One.A", 0),
            heading(3, 1, "H2", "One.B", 1),
            heading(4, 1, "H1", "Two", 2),
        ];
        assert_eq!(
            shape(&plan(&blocks)),
            ["One p0", "  One.A p0", "  One.B p1", "Two p2"]
        );
    }

    #[test]
    fn a_skipped_level_hangs_from_the_nearest_lower_one_and_a_return_pops_back() {
        let blocks = [
            heading(1, 1, "H1", "One", 0),
            heading(2, 1, "H3", "Deep", 0),
            heading(3, 1, "H2", "Mid", 0),
            heading(4, 1, "H3", "Deeper", 0),
        ];
        assert_eq!(
            shape(&plan(&blocks)),
            ["One p0", "  Deep p0", "  Mid p0", "    Deeper p0"]
        );
    }

    #[test]
    fn a_heading_before_any_higher_one_is_top_level_and_a_title_is_level_one() {
        let blocks = [
            heading(1, 1, "H2", "Starts at two", 0),
            heading(2, 1, "Title", "The title", 0),
            heading(3, 1, "H", "Unnumbered", 0),
        ];
        assert_eq!(
            shape(&plan(&blocks)),
            ["Starts at two p0", "The title p0", "Unnumbered p0"],
            "a level-one heading closes the level-two one before it"
        );
    }

    #[test]
    fn only_headings_make_bookmarks() {
        let mut paragraph = heading(2, 1, "P", "Body text", 0);
        paragraph.standard_type = Some(Name::new("P"));
        let blocks = [heading(1, 1, "H1", "Heading", 0), paragraph];
        assert_eq!(shape(&plan(&blocks)), ["Heading p0"]);
    }

    #[test]
    fn a_heading_with_no_words_or_nowhere_to_go_makes_no_bookmark() {
        let mut empty = heading(1, 1, "H1", "   ", 0);
        empty.text = "   ".to_owned();
        let mut nowhere = heading(2, 1, "H1", "Floating", 0);
        nowhere.items.clear();
        let blocks = [empty, nowhere, heading(3, 1, "H1", "Kept", 4)];
        assert_eq!(shape(&plan(&blocks)), ["Kept p4"]);
    }

    #[test]
    fn a_heading_nested_inside_another_is_its_own_bookmark_and_not_its_words() {
        let blocks = [
            heading(1, 1, "H1", "Outer", 0),
            heading(2, 2, "H2", "Inner", 1),
        ];
        assert_eq!(shape(&plan(&blocks)), ["Outer p0", "  Inner p1"]);
    }

    #[test]
    fn a_title_is_the_words_of_the_whole_heading_with_white_space_made_one() {
        let mut blocks = vec![heading(1, 1, "H1", "See\n  the", 0)];
        blocks.push(heading(2, 2, "Link", "  link ", 0));
        let mut after = heading(1, 1, "H1", "now", 0);
        after.continuation = true;
        blocks.push(after);
        assert_eq!(shape(&plan(&blocks)), ["See the link now p0"]);
    }

    #[test]
    fn a_replacement_is_the_title_and_the_page_is_the_first_real_content() {
        let mut first = heading(1, 1, "H1", "glyphs", 3);
        first.replacement = Some(crate::structure::Replacement {
            text: "Chapter  1".to_owned(),
            page: 3,
        });
        first.items.insert(
            0,
            ContentItem {
                page: 0,
                kind: ItemKind::Text,
                bounds: [0.0; 4],
                text: Some("page 1".to_owned()),
                artifact: true,
            },
        );
        assert_eq!(shape(&plan(&[first])), ["Chapter 1 p3"]);
    }

    #[test]
    fn a_file_that_writes_h5000_does_not_nest_a_bookmark_that_deep() {
        let blocks: Vec<Block> = (1..=100)
            .map(|n| heading(n, 1, &format!("H{n}"), &format!("level {n}"), 0))
            .collect();
        fn depth(items: &[PlannedBookmark]) -> usize {
            items
                .iter()
                .map(|item| 1 + depth(&item.children))
                .max()
                .unwrap_or(0)
        }
        assert!(depth(&plan(&blocks)) <= MAX_NEST);
    }

    #[test]
    fn title_and_a_bare_h_are_level_one_and_not_below_an_h1() {
        let blocks = [
            heading(1, 1, "H1", "A", 0),
            heading(2, 1, "Title", "T", 0),
            heading(3, 1, "H", "Bare", 0),
            heading(4, 1, "H2", "Under bare", 0),
        ];
        assert_eq!(
            shape(&plan(&blocks)),
            ["A p0", "T p0", "Bare p0", "  Under bare p0"]
        );
    }

    #[test]
    fn artifact_text_inside_a_heading_is_not_in_its_title() {
        let mut footer = heading(2, 2, "Span", "99", 0);
        footer.items[0].artifact = true;
        let blocks = [heading(1, 1, "H1", "Real", 0), footer];
        assert_eq!(shape(&plan(&blocks)), ["Real p0"]);
    }

    #[test]
    fn a_word_split_across_inline_elements_is_joined_by_geometry_not_by_a_space() {
        let mut first = heading(1, 1, "H1", "Chap", 0);
        first.items[0].bounds = [0.0, 0.0, 24.0, 12.0];
        let mut touching = heading(2, 2, "Span", "ter", 0);
        touching.items[0].bounds = [24.0, 0.0, 36.0, 12.0];
        let mut apart = heading(3, 2, "Span", "ter", 0);
        apart.items[0].bounds = [60.0, 0.0, 72.0, 12.0];
        assert_eq!(shape(&plan(&[first.clone(), touching])), ["Chapter p0"]);
        assert_eq!(shape(&plan(&[first, apart])), ["Chap ter p0"]);
    }

    #[test]
    fn a_heading_that_is_only_a_figure_is_titled_by_its_alt_and_goes_to_its_page() {
        let mut empty = heading(1, 1, "H1", "", 0);
        empty.items.clear();
        empty.text.clear();
        empty.page = None;
        let mut figure = heading(2, 2, "Figure", "", 3);
        figure.replacement = Some(crate::structure::Replacement {
            text: "Logo".to_owned(),
            page: 3,
        });
        assert_eq!(shape(&plan(&[empty, figure])), ["Logo p3"]);
    }

    #[test]
    fn a_title_longer_than_a_line_is_cut_at_a_word_with_an_ellipsis() {
        let long = "word ".repeat(100);
        let planned = plan(&[heading(1, 1, "H1", &long, 0)]);
        let title = &planned[0].title;
        assert!(title.ends_with("word…"), "{title}");
        assert!(title.chars().count() <= MAX_TITLE_CHARS + 1);
        let short = plan(&[heading(1, 1, "H1", "A short one", 0)]);
        assert_eq!(short[0].title, "A short one");
    }

    #[test]
    fn a_title_whose_last_whole_word_ends_at_the_cut_keeps_it() {
        let words = format!("{} tail", "a".repeat(MAX_TITLE_CHARS));
        let planned = plan(&[heading(1, 1, "H1", &words, 0)]);
        assert_eq!(
            planned[0].title,
            format!("{}…", "a".repeat(MAX_TITLE_CHARS))
        );
    }

    #[test]
    fn text_beside_a_replacement_is_spaced_from_it_on_both_sides() {
        let replaced = |n, alt: &str| {
            let mut figure = heading(n, 2, "Figure", "", 0);
            figure.replacement = Some(crate::structure::Replacement {
                text: alt.to_owned(),
                page: 0,
            });
            figure
        };
        let before = [heading(1, 1, "H1", "Chapter", 0), replaced(2, "Logo")];
        assert_eq!(shape(&plan(&before)), ["Chapter Logo p0"]);
        let mut after = heading(3, 2, "Span", "end", 0);
        after.items[0].bounds = [500.0, 0.0, 530.0, 12.0];
        let first = replaced(2, "Logo");
        let mut top = heading(1, 1, "H1", "", 0);
        top.items.clear();
        top.text.clear();
        assert_eq!(shape(&plan(&[top, first, after])), ["Logo end p0"]);
    }

    #[test]
    fn a_level_too_long_for_a_number_is_as_deep_as_any_not_a_top_level() {
        let blocks = [
            heading(1, 1, "H1", "A", 0),
            heading(2, 1, "H99999999999999999999", "Deep", 0),
            heading(3, 1, "H2", "Mid", 0),
        ];
        assert_eq!(shape(&plan(&blocks)), ["A p0", "  Deep p0", "  Mid p0"]);
    }

    #[test]
    fn a_level_too_long_for_a_number_is_a_heading_as_deep_as_any() {
        let blocks = [heading(1, 1, "H99999999999999999999", "Deep", 0)];
        assert_eq!(shape(&plan(&blocks)), ["Deep p0"]);
    }

    #[test]
    fn a_heading_inside_an_excluded_subtree_makes_no_bookmark() {
        let mut inside = heading(2, 2, "H2", "Hidden", 0);
        inside.excluded = true;
        assert_eq!(
            shape(&plan(&[heading(1, 1, "H1", "Shown", 0), inside])),
            ["Shown p0"]
        );
    }
}
