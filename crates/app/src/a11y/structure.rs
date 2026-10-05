//! What a tagged page tells a screen reader: its structure elements as nodes
//! with the roles a reader navigates by, instead of a flat list of text runs.
//!
//! [`outline`] turns one page's share of a document's reading-order blocks
//! into a tree. Headings carry their level, lists their items, tables their
//! rows and cells, a figure its alternate text as its label, and a node whose
//! language differs from its parent's names it.
//!
//! The nodes are in page space ([`Outline::bounds`]); the shell maps them into
//! the view, which is the half this module has no viewport for.

use std::collections::HashMap;

use accesskit::Role;
use onionskin_core::Block;

/// One node of a page's structure, before it is placed in the view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Outline {
    /// Names the node among the page's: the structure element it describes, and
    /// for the text under it the place in that element. Stable while the
    /// element is, which a position in the walk is not.
    pub(crate) key: String,
    pub(crate) role: Role,
    /// A heading's level.
    pub(crate) level: Option<usize>,
    /// What the node says: a text node's words, a figure's alternate text.
    /// Empty on a container, whose children say it.
    pub(crate) label: String,
    /// Set only where it differs from the parent's, so a screen reader changes
    /// voice at the passage that changes language and not at every node.
    pub(crate) language: Option<String>,
    /// The content the node and its children cover, `[x0, y0, x1, y1]`.
    pub(crate) bounds: Option<[f64; 4]>,
    pub(crate) children: Vec<Outline>,
}

/// The role a standard structure type is announced as, and a heading's level.
///
/// A type no role map resolves has no meaning to give a screen reader, so it is
/// a generic container: its text is still read, under no particular name.
fn role_of(standard_type: Option<&[u8]>) -> (Role, Option<usize>) {
    let Some(name) = standard_type else {
        return (Role::GenericContainer, None);
    };
    if let [b'H', digits @ ..] = name {
        if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) {
            let level = std::str::from_utf8(digits)
                .ok()
                .and_then(|n| n.parse().ok());
            return (Role::Heading, level);
        }
    }
    let role = match name {
        b"Document" => Role::Document,
        b"Art" => Role::Article,
        b"Part" | b"Sect" => Role::Section,
        b"Aside" => Role::Complementary,
        b"BlockQuote" => Role::Blockquote,
        b"Caption" => Role::Caption,
        b"H" => Role::Heading,
        b"Title" => return (Role::Heading, Some(1)),
        b"P" => Role::Paragraph,
        b"L" | b"TOC" => Role::List,
        b"LI" | b"TOCI" => Role::ListItem,
        b"Lbl" => Role::ListMarker,
        b"Table" => Role::Table,
        b"TR" => Role::Row,
        b"TH" => Role::ColumnHeader,
        b"TD" => Role::Cell,
        b"THead" | b"TBody" | b"TFoot" => Role::RowGroup,
        b"Link" => Role::Link,
        b"Note" | b"FENote" => Role::Note,
        b"Figure" => Role::Image,
        b"Formula" => Role::Math,
        b"Form" => Role::Form,
        _ => Role::GenericContainer,
    };
    (role, None)
}

/// A node under construction: its children are indexes into the arena, so a
/// continuation block can add to an element whose subtree has already been
/// walked.
struct Pending {
    outline: Outline,
    /// How many text children the node has, for their keys.
    leaves: usize,
    /// The element has content on this page, as opposed to being here only
    /// because something below it is.
    present: bool,
    children: Vec<usize>,
}

/// The nodes of `page`, from `blocks` in reading order.
///
/// An element with nothing on the page, and nothing below it with anything,
/// is left out; one that is only an ancestor of content here stays, so the
/// nesting a screen reader navigates is the document's.
pub(crate) fn outline(blocks: &[Block], page: usize) -> Vec<Outline> {
    let mut arena: Vec<Pending> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut node_of: HashMap<u32, usize> = HashMap::new();

    for block in blocks {
        let index = if block.continuation {
            let Some(&index) = node_of.get(&block.element) else {
                continue;
            };
            index
        } else {
            let (role, level) = role_of(block.standard_type.as_ref().map(|name| name.as_bytes()));
            let index = arena.len();
            arena.push(Pending {
                leaves: 0,
                outline: Outline {
                    key: format!("e{}", block.element),
                    role,
                    level,
                    label: String::new(),
                    language: block.lang.clone(),
                    bounds: None,
                    children: Vec::new(),
                },
                present: false,
                children: Vec::new(),
            });
            open.truncate(block.depth);
            match open.last() {
                Some(&parent) => arena[parent].children.push(index),
                None => roots.push(index),
            }
            open.push(index);
            node_of.insert(block.element, index);
            index
        };
        say(&mut arena, index, block, page);
    }

    roots
        .into_iter()
        .filter_map(|root| finish(&arena, root, None))
        .collect()
}

/// Put what `block` says on `page` into the node: a figure's label, or a text
/// child for words, in order after whatever the node already holds.
fn say(arena: &mut Vec<Pending>, index: usize, block: &Block, page: usize) {
    // What a replaced element's subtree holds is said by the replacement.
    if block.excluded {
        return;
    }
    let bounds = block.bounds_on(page);
    let words = match &block.replacement {
        Some(replacement) if replacement.page == page => replacement.text.clone(),
        Some(_) => return,
        None => block.text_on(page),
    };
    if bounds.is_none() && words.is_empty() {
        return;
    }
    arena[index].present = true;
    if words.is_empty() {
        return;
    }
    if arena[index].outline.role == Role::Image {
        let outline = &mut arena[index].outline;
        if !outline.label.is_empty() {
            outline.label.push(' ');
        }
        outline.label.push_str(&words);
        outline.bounds = union(outline.bounds, bounds);
        return;
    }
    let leaf = arena.len();
    let key = format!("{}t{}", arena[index].outline.key, arena[index].leaves);
    arena[index].leaves += 1;
    arena.push(Pending {
        leaves: 0,
        outline: Outline {
            key,
            role: Role::Label,
            level: None,
            label: words,
            language: None,
            bounds,
            children: Vec::new(),
        },
        present: true,
        children: Vec::new(),
    });
    arena[index].children.push(leaf);
}

fn finish(arena: &[Pending], index: usize, parent_language: Option<&str>) -> Option<Outline> {
    let pending = &arena[index];
    let mut outline = pending.outline.clone();
    let own_language = outline.language.clone();
    outline.language = own_language
        .clone()
        .filter(|language| Some(language.as_str()) != parent_language);
    let language = own_language.as_deref().or(parent_language);
    outline.children = pending
        .children
        .iter()
        .filter_map(|&child| finish(arena, child, language))
        .collect();
    if !pending.present && outline.children.is_empty() {
        return None;
    }
    for child in &outline.children {
        outline.bounds = union(outline.bounds, child.bounds);
    }
    // A node whose children are only its own words is named by them. A screen
    // reader announces a node by its name, and a Heading, Link or Cell with
    // an empty one is announced as the bare role, so the words are the label
    // and the node is one stop. Structural containers are not named by their
    // words: a Document or a List that read as one string would leave nothing
    // to navigate.
    let is_text = |node: &Outline| {
        node.role == Role::Label && node.children.is_empty() && node.language.is_none()
    };
    if names_by_words(outline.role)
        && !outline.children.is_empty()
        && outline.children.iter().all(is_text)
    {
        outline.label = join_words(outline.children.iter().map(|c| c.label.as_str()));
        outline.children.clear();
        // Platform trees drop a generic container and promote its children, but
        // not its name, so words moved into one would not be read at all. A
        // node that is only words is a text node.
        if outline.role == Role::GenericContainer {
            outline.role = Role::Label;
        }
    } else if matches!(
        outline.role,
        Role::Heading | Role::ColumnHeader | Role::RowHeader
    ) && outline.children.iter().any(is_text)
    {
        // A heading is one stop with one name, so a link inside it must not
        // leave it nameless: its own words are its name, and the link, figure
        // or table inside it stay children a reader can reach and activate.
        outline.label = join_words(
            outline
                .children
                .iter()
                .filter(|child| is_text(child))
                .map(|child| child.label.as_str()),
        );
        outline.children.retain(|child| !is_text(child));
    }
    Some(outline)
}

/// Whether a node of this role is named by the words under it. Not the roles
/// whose children are the thing to navigate, and not an image, which is named
/// by its alternate text.
fn names_by_words(role: Role) -> bool {
    !matches!(
        role,
        Role::Document
            | Role::Article
            | Role::Section
            | Role::Complementary
            | Role::List
            | Role::ListItem
            | Role::Table
            | Role::Row
            | Role::RowGroup
            | Role::Form
            | Role::Image
    )
}

/// The words one after another, a space between two unless one is there.
fn join_words<'a>(words: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for word in words {
        if !out.is_empty() && !out.ends_with(' ') && !word.starts_with(' ') {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.trim().to_owned()
}

fn union(a: Option<[f64; 4]>, b: Option<[f64; 4]>) -> Option<[f64; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use onionskin_core::{ContentItem, ItemKind};
    use onionskin_cos::Name;

    use super::*;

    fn text(page: usize, words: &str, bounds: [f64; 4]) -> ContentItem {
        ContentItem {
            page,
            kind: ItemKind::Text,
            bounds,
            text: Some(words.to_owned()),
            artifact: false,
        }
    }

    fn block(element: u32, depth: usize, kind: &str, lang: Option<&str>) -> Block {
        Block {
            element,
            continuation: false,
            depth,
            struct_type: Some(Name::new(kind)),
            standard_type: Some(Name::new(kind)),
            lang: lang.map(str::to_owned),
            alt: None,
            actual_text: None,
            title: None,
            page: None,
            text: String::new(),
            items: Vec::new(),
            replacement: None,
            excluded: false,
            objects: Vec::new(),
            unplaced: Vec::new(),
        }
    }

    fn says(mut block: Block, items: Vec<ContentItem>) -> Block {
        block.page = items.first().map(|item| item.page);
        block.items = items;
        block
    }

    fn shape(nodes: &[Outline]) -> Vec<String> {
        fn walk(node: &Outline, depth: usize, out: &mut Vec<String>) {
            let level = node.level.map(|l| format!(" h{l}")).unwrap_or_default();
            let lang = node
                .language
                .as_deref()
                .map(|l| format!(" [{l}]"))
                .unwrap_or_default();
            out.push(format!(
                "{}{:?}{level}{lang} {:?}",
                "  ".repeat(depth),
                node.role,
                node.label
            ));
            for child in &node.children {
                walk(child, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        for node in nodes {
            walk(node, 0, &mut out);
        }
        out
    }

    #[test]
    fn headings_paragraphs_and_lists_become_the_roles_a_reader_navigates_by() {
        let blocks = [
            block(1, 0, "Document", Some("en")),
            says(
                block(2, 1, "H2", Some("en")),
                vec![text(0, "Intro", [0.0, 80.0, 40.0, 92.0])],
            ),
            says(
                block(3, 1, "P", Some("en")),
                vec![text(0, "Hello", [0.0, 60.0, 30.0, 72.0])],
            ),
            block(4, 1, "L", Some("en")),
            block(5, 2, "LI", Some("en")),
            says(
                block(6, 3, "Lbl", Some("en")),
                vec![text(0, "1.", [0.0, 40.0, 8.0, 52.0])],
            ),
            says(
                block(7, 3, "LBody", Some("en")),
                vec![text(0, "one", [12.0, 40.0, 30.0, 52.0])],
            ),
        ];
        assert_eq!(
            shape(&outline(&blocks, 0)),
            [
                r#"Document [en] """#,
                r#"  Heading h2 "Intro""#,
                r#"  Paragraph "Hello""#,
                r#"  List """#,
                r#"    ListItem """#,
                r#"      ListMarker "1.""#,
                r#"      Label "one""#,
            ]
        );
    }

    #[test]
    fn a_figure_is_an_image_labelled_with_its_alternate_text() {
        let mut figure = says(
            block(2, 1, "Figure", None),
            vec![ContentItem {
                page: 0,
                kind: ItemKind::Image,
                bounds: [0.0, 0.0, 50.0, 50.0],
                text: None,
                artifact: false,
            }],
        );
        figure.alt = Some("A cat".to_owned());
        figure.replacement = Some(onionskin_core::Replacement {
            text: "A cat".to_owned(),
            page: 0,
        });
        let nodes = outline(&[block(1, 0, "Document", None), figure], 0);
        let image = &nodes[0].children[0];
        assert_eq!((image.role, image.label.as_str()), (Role::Image, "A cat"));
        assert_eq!(image.bounds, Some([0.0, 0.0, 50.0, 50.0]));
        assert!(image.children.is_empty());
    }

    #[test]
    fn a_figure_without_alt_is_still_an_image_a_reader_can_land_on() {
        let figure = says(
            block(2, 0, "Figure", None),
            vec![ContentItem {
                page: 0,
                kind: ItemKind::Image,
                bounds: [0.0, 0.0, 5.0, 5.0],
                text: None,
                artifact: false,
            }],
        );
        let nodes = outline(&[figure], 0);
        assert_eq!((nodes[0].role, nodes[0].label.as_str()), (Role::Image, ""));
    }

    #[test]
    fn only_a_changed_language_is_named_and_a_new_one_names_itself_once() {
        let blocks = [
            block(1, 0, "Document", Some("en")),
            says(
                block(2, 1, "P", Some("en")),
                vec![text(0, "a", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(3, 1, "P", Some("fr")),
                vec![text(0, "b", [0.0, 10.0, 5.0, 15.0])],
            ),
            says(
                block(4, 2, "Link", Some("fr")),
                vec![text(0, "c", [6.0, 10.0, 9.0, 15.0])],
            ),
        ];
        let nodes = outline(&blocks, 0);
        let languages: Vec<_> = nodes[0]
            .children
            .iter()
            .map(|c| c.language.clone())
            .collect();
        assert_eq!(nodes[0].language.as_deref(), Some("en"));
        assert_eq!(languages, [None, Some("fr".to_owned())]);
        assert_eq!(
            nodes[0].children[1].children[1].language, None,
            "the Link inherits fr"
        );
    }

    #[test]
    fn words_in_a_generic_container_become_a_text_node_because_platforms_drop_the_container() {
        let blocks = [
            block(1, 0, "P", None),
            says(
                block(2, 1, "Span", None),
                vec![text(0, "inside", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(3, 1, "Span", None),
                vec![text(0, "more", [6.0, 0.0, 9.0, 5.0])],
            ),
        ];
        assert_eq!(
            shape(&outline(&blocks, 0)),
            [r#"Paragraph "inside more""#],
            "the Spans became text and the paragraph is named by them"
        );
    }

    #[test]
    fn a_heading_is_named_by_its_own_words_and_keeps_a_link_inside_it() {
        let blocks = [
            says(
                block(1, 0, "H1", None),
                vec![text(0, "See ", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(2, 1, "Link", None),
                vec![text(0, "here", [6.0, 0.0, 9.0, 5.0])],
            ),
        ];
        assert_eq!(
            shape(&outline(&blocks, 0)),
            [r#"Heading h1 "See""#, r#"  Link "here""#],
            "the heading is named by its own words and the link stays reachable"
        );
    }

    #[test]
    fn a_document_or_list_is_not_collapsed_to_one_string() {
        let blocks = [
            says(
                block(1, 0, "Document", None),
                vec![text(0, "all of it", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(2, 0, "L", None),
                vec![text(0, "list words", [0.0, 10.0, 5.0, 15.0])],
            ),
        ];
        assert_eq!(
            shape(&outline(&blocks, 0)),
            [
                r#"Document """#,
                r#"  Label "all of it""#,
                r#"List """#,
                r#"  Label "list words""#
            ]
        );
    }

    #[test]
    fn a_passage_in_another_language_keeps_its_own_node_when_the_rest_collapses() {
        let blocks = [
            says(
                block(1, 0, "P", Some("en")),
                vec![text(0, "hello", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(2, 1, "Span", Some("fr")),
                vec![text(0, "bonjour", [6.0, 0.0, 9.0, 5.0])],
            ),
        ];
        let nodes = outline(&blocks, 0);
        assert_eq!(nodes[0].label, "", "the paragraph is not named by both");
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].children[1].label, "bonjour");
        assert_eq!(nodes[0].children[1].language.as_deref(), Some("fr"));
    }

    #[test]
    fn words_that_already_end_in_a_space_are_not_given_another() {
        let blocks = [
            says(
                block(1, 0, "P", None),
                vec![text(0, "See ", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(2, 1, "Span", None),
                vec![text(0, "here", [6.0, 0.0, 9.0, 5.0])],
            ),
        ];
        assert_eq!(shape(&outline(&blocks, 0)), [r#"Paragraph "See here""#]);
    }

    #[test]
    fn the_text_nodes_of_one_element_have_keys_of_their_own() {
        let mut after = says(
            block(2, 1, "P", None),
            vec![text(0, "now", [30.0, 0.0, 45.0, 10.0])],
        );
        after.continuation = true;
        let blocks = [
            block(1, 0, "Document", None),
            says(
                block(2, 1, "P", None),
                vec![text(0, "See", [0.0, 0.0, 12.0, 10.0])],
            ),
            says(
                block(3, 2, "Link", None),
                vec![text(0, "here", [14.0, 0.0, 28.0, 10.0])],
            ),
            after,
        ];
        let paragraph = &outline(&blocks, 0)[0].children[0];
        let keys: Vec<&str> = paragraph.children.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["e2t0", "e3", "e2t1"]);
    }

    #[test]
    fn content_after_a_child_returns_to_its_element_in_order() {
        let mut after = says(
            block(2, 1, "P", None),
            vec![text(0, "now", [30.0, 0.0, 45.0, 10.0])],
        );
        after.continuation = true;
        let blocks = [
            block(1, 0, "Document", None),
            says(
                block(2, 1, "P", None),
                vec![text(0, "See", [0.0, 0.0, 12.0, 10.0])],
            ),
            says(
                block(3, 2, "Link", None),
                vec![text(0, "here", [14.0, 0.0, 28.0, 10.0])],
            ),
            after,
        ];
        assert_eq!(
            shape(&outline(&blocks, 0)),
            [
                r#"Document """#,
                r#"  Paragraph """#,
                r#"    Label "See""#,
                r#"    Link "here""#,
                r#"    Label "now""#,
            ]
        );
    }

    #[test]
    fn each_standard_type_is_announced_as_the_role_a_reader_navigates_by() {
        let expected: &[(&[u8], Role)] = &[
            (b"Document", Role::Document),
            (b"Art", Role::Article),
            (b"Part", Role::Section),
            (b"Sect", Role::Section),
            (b"Aside", Role::Complementary),
            (b"BlockQuote", Role::Blockquote),
            (b"Caption", Role::Caption),
            (b"P", Role::Paragraph),
            (b"L", Role::List),
            (b"TOC", Role::List),
            (b"LI", Role::ListItem),
            (b"TOCI", Role::ListItem),
            (b"Lbl", Role::ListMarker),
            (b"Table", Role::Table),
            (b"TR", Role::Row),
            (b"TH", Role::ColumnHeader),
            (b"TD", Role::Cell),
            (b"THead", Role::RowGroup),
            (b"TBody", Role::RowGroup),
            (b"TFoot", Role::RowGroup),
            (b"Link", Role::Link),
            (b"Note", Role::Note),
            (b"FENote", Role::Note),
            (b"Figure", Role::Image),
            (b"Formula", Role::Math),
            (b"Form", Role::Form),
            (b"Span", Role::GenericContainer),
            (b"Em", Role::GenericContainer),
            (b"Strong", Role::GenericContainer),
            (b"Code", Role::GenericContainer),
            (b"Div", Role::GenericContainer),
        ];
        for (name, role) in expected {
            assert_eq!(
                role_of(Some(name)).0,
                *role,
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn a_node_keeps_its_key_when_something_before_it_appears() {
        let later = says(
            block(3, 1, "P", None),
            vec![text(0, "b", [0.0, 0.0, 5.0, 5.0])],
        );
        let key_of = |blocks: &[Block]| outline(blocks, 0)[0].children.last().unwrap().key.clone();
        let alone = key_of(&[block(1, 0, "Document", None), later.clone()]);
        let with_earlier = key_of(&[
            block(1, 0, "Document", None),
            says(
                block(2, 1, "P", None),
                vec![text(0, "a", [0.0, 10.0, 5.0, 15.0])],
            ),
            later,
        ]);
        assert_eq!(alone, with_earlier);
        assert_eq!(alone, "e3");
    }

    #[test]
    fn a_replacement_on_one_page_is_not_said_on_another_that_the_element_also_marks() {
        let mut whole = says(
            block(2, 0, "P", None),
            vec![
                text(0, "own", [0.0, 0.0, 9.0, 5.0]),
                text(1, "more", [0.0, 0.0, 9.0, 5.0]),
            ],
        );
        whole.replacement = Some(onionskin_core::Replacement {
            text: "WHOLE".to_owned(),
            page: 0,
        });
        assert_eq!(
            shape(&outline(&[whole.clone()], 0)),
            [r#"Paragraph "WHOLE""#]
        );
        assert!(outline(&[whole], 1).is_empty());
    }

    #[test]
    fn a_figure_continuation_adds_to_its_label() {
        let mut figure = says(
            block(2, 0, "Figure", None),
            vec![text(0, "one", [0.0, 0.0, 5.0, 5.0])],
        );
        figure.replacement = None;
        let mut more = says(
            block(2, 0, "Figure", None),
            vec![text(0, "two", [6.0, 0.0, 9.0, 5.0])],
        );
        more.continuation = true;
        let nodes = outline(&[figure, more], 0);
        assert_eq!(nodes[0].label, "one two");
        assert_eq!(nodes[0].bounds, Some([0.0, 0.0, 9.0, 5.0]));
    }

    #[test]
    fn a_page_keeps_the_ancestors_of_its_content_and_drops_what_is_elsewhere() {
        let blocks = [
            block(1, 0, "Document", None),
            says(
                block(2, 1, "P", None),
                vec![text(0, "page one", [0.0, 0.0, 5.0, 5.0])],
            ),
            block(3, 1, "Sect", None),
            says(
                block(4, 2, "P", None),
                vec![text(1, "page two", [0.0, 0.0, 5.0, 5.0])],
            ),
        ];
        assert_eq!(
            shape(&outline(&blocks, 1)),
            [
                r#"Document """#,
                r#"  Section """#,
                r#"    Paragraph "page two""#,
            ]
        );
        assert!(outline(&blocks, 2).is_empty());
    }

    #[test]
    fn a_container_covers_what_is_inside_it() {
        let blocks = [
            block(1, 0, "Document", None),
            says(
                block(2, 1, "P", None),
                vec![text(0, "a", [0.0, 0.0, 5.0, 5.0])],
            ),
            says(
                block(3, 1, "P", None),
                vec![text(0, "b", [10.0, 20.0, 15.0, 25.0])],
            ),
        ];
        assert_eq!(outline(&blocks, 0)[0].bounds, Some([0.0, 0.0, 15.0, 25.0]));
    }

    #[test]
    fn a_replaced_subtree_is_said_once_by_its_element() {
        let mut whole = says(
            block(2, 1, "P", None),
            vec![text(0, "own", [0.0, 0.0, 9.0, 5.0])],
        );
        whole.replacement = Some(onionskin_core::Replacement {
            text: "WHOLE".to_owned(),
            page: 0,
        });
        let mut inside = says(
            block(3, 2, "Span", None),
            vec![text(0, "kid", [10.0, 0.0, 19.0, 5.0])],
        );
        inside.excluded = true;
        let nodes = outline(&[block(1, 0, "Document", None), whole, inside], 0);
        assert_eq!(
            shape(&nodes),
            [r#"Document """#, r#"  Paragraph "WHOLE""#],
            "the Span under the replaced paragraph has nothing left to say"
        );
    }

    #[test]
    fn a_replacement_belongs_to_one_page_and_the_others_do_not_repeat_it() {
        let mut whole = says(
            block(2, 0, "P", None),
            vec![text(0, "own", [0.0, 0.0, 9.0, 5.0])],
        );
        whole.replacement = Some(onionskin_core::Replacement {
            text: "WHOLE".to_owned(),
            page: 0,
        });
        assert!(outline(&[whole], 1).is_empty());
    }

    #[test]
    fn element_actual_text_replaces_the_words_and_artifacts_are_not_said() {
        let mut spelled = says(
            block(2, 1, "P", None),
            vec![text(0, "f i", [0.0, 0.0, 9.0, 5.0])],
        );
        spelled.replacement = Some(onionskin_core::Replacement {
            text: "fi".to_owned(),
            page: 0,
        });
        let mut art = text(0, "page 3", [0.0, 90.0, 20.0, 99.0]);
        art.artifact = true;
        let blocks = [
            block(1, 0, "Document", None),
            spelled,
            says(block(3, 1, "P", None), vec![art]),
        ];
        assert_eq!(
            shape(&outline(&blocks, 0)),
            [r#"Document """#, r#"  Paragraph "fi""#]
        );
    }

    #[test]
    fn a_table_is_rows_of_headers_and_cells() {
        let blocks = [
            block(1, 0, "Table", None),
            block(2, 1, "TR", None),
            says(
                block(3, 2, "TH", None),
                vec![text(0, "Name", [0.0, 20.0, 20.0, 30.0])],
            ),
            says(
                block(4, 2, "TH", None),
                vec![text(0, "Qty", [30.0, 20.0, 45.0, 30.0])],
            ),
            block(5, 1, "TR", None),
            says(
                block(6, 2, "TD", None),
                vec![text(0, "nut", [0.0, 0.0, 15.0, 10.0])],
            ),
            says(
                block(7, 2, "TD", None),
                vec![text(0, "4", [30.0, 0.0, 35.0, 10.0])],
            ),
        ];
        let roles: Vec<Role> = {
            fn walk(node: &Outline, out: &mut Vec<Role>) {
                out.push(node.role);
                node.children.iter().for_each(|c| walk(c, out));
            }
            let mut out = Vec::new();
            outline(&blocks, 0).iter().for_each(|n| walk(n, &mut out));
            out
        };
        assert_eq!(
            roles,
            [
                Role::Table,
                Role::Row,
                Role::ColumnHeader,
                Role::ColumnHeader,
                Role::Row,
                Role::Cell,
                Role::Cell,
            ]
        );
    }

    #[test]
    fn an_unresolved_type_is_read_as_a_generic_container_and_an_hn_beyond_six_is_a_heading() {
        assert_eq!(role_of(None), (Role::GenericContainer, None));
        assert_eq!(role_of(Some(b"Weird")), (Role::GenericContainer, None));
        assert_eq!(role_of(Some(b"H7")), (Role::Heading, Some(7)));
        assert_eq!(role_of(Some(b"H")), (Role::Heading, None));
        assert_eq!(role_of(Some(b"Title")), (Role::Heading, Some(1)));
    }
}
