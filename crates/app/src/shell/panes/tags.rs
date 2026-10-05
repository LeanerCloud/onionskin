//! The tags pane: a tagged document's structure tree, and the content each
//! element marks.
//!
//! A row is a structure element, named by its type as the file wrote it (and
//! what a role map makes of it, when that differs), with the words it marks
//! after it. Rows start folded to the top level; choosing one opens it and
//! boxes its content on the page, so the tree can be checked against what it
//! describes. Nothing here edits the document.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{Block, PageIndex};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{ShellFrame, ThemeTokens};
use super::{empty_message, error_message, list, PaneAction, ROW_HEIGHT};
use crate::a11y::State as A11yState;

const INDENT: f32 = 12.0;
/// Deeper than this and the indent would leave no room for the name; the
/// reader already caps the tree's own depth.
const MAX_INDENT: usize = 8;
/// How much of an element's text follows its name.
const DETAIL_CHARS: usize = 40;
const NO_TAGS: &str = "This document has no tags.";

/// What the pane remembers between frames.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::shell) struct TagsState {
    expanded: BTreeSet<u32>,
    pub(super) selected: Option<u32>,
}

/// What a row asks of the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum TagAction {
    /// Make this element the chosen one: box its content and go to its page.
    Select(u32),
    /// Open or fold an element, which is the disclosure triangle's and not the
    /// row's: choosing an open element again to box it must not fold it.
    Toggle(u32),
}

/// One structure element, as a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TagRow {
    pub(super) element: u32,
    pub(super) depth: usize,
    /// `/S` as written, and what the role map makes of it when that is another
    /// name: "Section (as Sect)".
    pub(super) name: String,
    /// The element's title, else its alternate text, else the start of its
    /// own text.
    pub(super) detail: String,
    pub(super) page: Option<PageIndex>,
    pub(super) has_children: bool,
    pub(super) expanded: bool,
    pub(super) selected: bool,
}

impl TagRow {
    fn text(&self) -> String {
        if self.detail.is_empty() {
            self.name.clone()
        } else {
            format!("{}  {}", self.name, self.detail)
        }
    }

    /// What is heard after the name. The macOS adapter reads neither a tree
    /// item's level nor whether it is open from the node, so they are words too.
    fn announcement(&self) -> String {
        let mut parts = vec![format!("Level {}", self.depth + 1)];
        if self.has_children {
            parts.push(
                if self.expanded {
                    "expanded"
                } else {
                    "collapsed"
                }
                .to_owned(),
            );
        }
        if let Some(page) = self.page {
            parts.push(format!("page {}", page + 1));
        }
        parts.join(", ")
    }
}

fn name_of(block: &Block) -> String {
    let written = block
        .struct_type
        .as_ref()
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
        .unwrap_or_else(|| "(no type)".to_owned());
    match &block.standard_type {
        Some(standard) if Some(standard) != block.struct_type.as_ref() => {
            format!(
                "{written} (as {})",
                String::from_utf8_lossy(standard.as_bytes())
            )
        }
        _ => written,
    }
}

fn detail_of(block: &Block) -> String {
    let text = block
        .title
        .clone()
        .filter(|title| !title.is_empty())
        .or_else(|| block.alt.clone().filter(|alt| !alt.is_empty()))
        .unwrap_or_else(|| block.text.clone());
    let mut cut: String = text.chars().take(DETAIL_CHARS).collect();
    if text.chars().count() > DETAIL_CHARS {
        cut.push('…');
    }
    cut
}

/// The elements as rows, parents before their children, the children of a
/// folded element left out.
#[cfg(test)]
pub(super) fn rows(blocks: &[Block], state: &TagsState) -> Vec<TagRow> {
    rows_upto(blocks, state, usize::MAX).0
}

/// The first `cap` rows, and how many more there are. Rows past the cap are only
/// counted: building a name and a detail for each would cost what drawing them
/// does.
fn rows_upto(blocks: &[Block], state: &TagsState, cap: usize) -> (Vec<TagRow>, usize) {
    let firsts: Vec<&Block> = blocks.iter().filter(|block| !block.continuation).collect();
    let mut out = Vec::new();
    let mut hidden = 0;
    let mut folded_at: Option<usize> = None;
    for (index, block) in firsts.iter().enumerate() {
        if let Some(depth) = folded_at {
            if block.depth > depth {
                continue;
            }
            folded_at = None;
        }
        let has_children = firsts
            .get(index + 1)
            .is_some_and(|next| next.depth > block.depth);
        let expanded = state.expanded.contains(&block.element);
        if has_children && !expanded {
            folded_at = Some(block.depth);
        }
        if out.len() >= cap {
            hidden += 1;
            continue;
        }
        out.push(TagRow {
            element: block.element,
            depth: block.depth,
            name: name_of(block),
            detail: detail_of(block),
            page: block.page,
            has_children,
            expanded: has_children && expanded,
            selected: state.selected == Some(block.element),
        });
    }
    (out, hidden)
}

/// More rows than this are neither drawn nor described: every row is a view and
/// a node, and opening the root of a book with 200k paragraphs would make the
/// shell unusable.
const MAX_ROWS: usize = 1000;

/// The rows to show, cut at [`MAX_ROWS`], and how many were left out.
pub(super) fn capped(blocks: &[Block], state: &TagsState) -> (Vec<TagRow>, usize) {
    rows_upto(blocks, state, MAX_ROWS)
}

/// The boxes to draw for an element: its own content and everything below it,
/// one box per page it appears on, in page space. The element's content is
/// boxed even where a replacement or a PDF 2.0 `Artifact` element keeps it out
/// of the reading: this pane shows what the tree marks, not what is read.
pub(super) fn content_boxes(blocks: &[Block], element: u32) -> Vec<(PageIndex, [f64; 4])> {
    let Some(start) = blocks
        .iter()
        .position(|block| block.element == element && !block.continuation)
    else {
        return Vec::new();
    };
    let depth = blocks[start].depth;
    let mut boxes: BTreeMap<PageIndex, [f64; 4]> = BTreeMap::new();
    for (offset, block) in blocks[start..].iter().enumerate() {
        let below = block.depth > depth || (block.element == element && block.continuation);
        if offset > 0 && !below {
            break;
        }
        for item in block.items.iter().filter(|item| !item.artifact) {
            boxes
                .entry(item.page)
                .and_modify(|held| {
                    *held = [
                        held[0].min(item.bounds[0]),
                        held[1].min(item.bounds[1]),
                        held[2].max(item.bounds[2]),
                        held[3].max(item.bounds[3]),
                    ];
                })
                .or_insert(item.bounds);
        }
    }
    boxes.into_iter().collect()
}

/// Choose an element.
pub(super) fn select(state: &mut TagsState, element: u32) {
    state.selected = Some(element);
}

/// Open an element, or fold it when it is open.
pub(super) fn toggle(state: &mut TagsState, element: u32) {
    if !state.expanded.remove(&element) {
        state.expanded.insert(element);
    }
}

pub(super) fn run(
    state: &mut super::NavigationPanesState,
    canvas: Option<&gpui::Entity<super::Canvas>>,
    action: TagAction,
    cx: &mut Context<ShellFrame>,
) {
    let element = match action {
        TagAction::Toggle(element) => {
            toggle(&mut state.tags, element);
            return;
        }
        TagAction::Select(element) => element,
    };
    let Some(super::PaneContent::Tags(Ok(Some(blocks)))) = state.content.clone() else {
        return;
    };
    select(&mut state.tags, element);
    let boxes = content_boxes(&blocks, element);
    let page = boxes.first().map(|(page, _)| *page).or_else(|| {
        blocks
            .iter()
            .find(|block| block.element == element && !block.continuation)
            .and_then(|block| block.page)
    });
    super::navigate(state, canvas, cx, move |canvas| {
        canvas.model.set_structure_highlight(boxes);
        match page {
            Some(page) => canvas.model.go_to_page(page),
            None => Ok(true),
        }
    });
}

/// What the tags pane tells a screen reader.
pub(super) fn accessible(
    blocks: Result<&Option<Arc<Vec<Block>>>, &String>,
    state: &TagsState,
) -> Vec<Element> {
    let blocks = match blocks {
        Ok(Some(blocks)) => blocks,
        Ok(None) => return vec![Element::new("tag-rows-empty", Role::Label, NO_TAGS)],
        Err(message) => return vec![Element::new("tag-rows-error", Role::Alert, message.clone())],
    };
    let (rows, hidden) = capped(blocks, state);
    let mut described: Vec<Element> = rows
        .into_iter()
        .map(|row| {
            let mut item = Element::new(
                ("tag-row", row.element as usize),
                Role::TreeItem,
                row.text(),
            )
            .with_level(row.depth + 1)
            .with_state(A11yState::selected(row.selected))
            .with_activation(Activation::Pane(PaneAction::Tag(TagAction::Select(
                row.element,
            ))));
            item = item.with_description(row.announcement());
            if row.has_children {
                // A tree item that opens has to say so, and be openable from the
                // keyboard without being chosen.
                item = item.with_expanded(row.expanded).child(
                    Element::new(
                        ("tag-toggle", row.element as usize),
                        Role::DisclosureTriangle,
                        format!(
                            "{} {}",
                            if row.expanded { "Fold" } else { "Open" },
                            row.name
                        ),
                    )
                    .with_activation(Activation::Pane(PaneAction::Tag(
                        TagAction::Toggle(row.element),
                    ))),
                );
            }
            item
        })
        .collect();
    if hidden > 0 {
        described.push(Element::new(
            "tag-rows-more",
            Role::Label,
            format!("{hidden} more elements are not listed"),
        ));
    }
    vec![Element::new("tag-rows", Role::Tree, "Tags").with_children(described)]
}

pub(super) fn render(
    blocks: Result<&Option<Arc<Vec<Block>>>, &String>,
    state: &TagsState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let blocks = match blocks {
        Ok(Some(blocks)) => blocks,
        Ok(None) => return empty_message(NO_TAGS, theme).into_any_element(),
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    let mut body = list("tag-rows");
    let (rows, hidden) = capped(blocks, state);
    for row in rows {
        let element = row.element;
        let marker = match (row.has_children, row.expanded) {
            (false, _) => "  ",
            (true, true) => "▾ ",
            (true, false) => "▸ ",
        };
        let disclosure = div()
            .id(("tag-disclosure", element as usize))
            .w(px(16.0))
            .child(marker)
            .when(row.has_children, |marker| {
                marker
                    .cursor_pointer()
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        cx.stop_propagation();
                        frame.run_activation(
                            Activation::Pane(PaneAction::Tag(TagAction::Toggle(element))),
                            window,
                            cx,
                        );
                    }))
            });
        body = body.child(
            div()
                .id(("tag-row", element as usize))
                .min_h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .py_1()
                .pr_2()
                .pl(px(8.0 + INDENT * row.depth.min(MAX_INDENT) as f32))
                .text_sm()
                .text_color(theme.text)
                .when(row.selected, |row| row.bg(theme.selected))
                .cursor_pointer()
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(
                        Activation::Pane(PaneAction::Tag(TagAction::Select(element))),
                        window,
                        cx,
                    );
                }))
                .child(disclosure)
                .child(row.text()),
        );
    }
    if hidden > 0 {
        body = body.child(empty_message(
            &format!("{hidden} more elements are not listed"),
            theme,
        ));
    }
    body.into_any_element()
}

#[cfg(test)]
mod tests {
    use onionskin_core::{ContentItem, ItemKind};
    use onionskin_cos::Name;

    use super::*;

    fn block(element: u32, depth: usize, kind: &str) -> Block {
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
            page: None,
            text: String::new(),
            items: Vec::new(),
            replacement: None,
            excluded: false,
            objects: Vec::new(),
            unplaced: Vec::new(),
        }
    }

    fn with_text(mut block: Block, page: usize, words: &str, bounds: [f64; 4]) -> Block {
        block.page = Some(page);
        block.text = words.to_owned();
        block.items = vec![ContentItem {
            page,
            kind: ItemKind::Text,
            bounds,
            text: Some(words.to_owned()),
            artifact: false,
        }];
        block
    }

    fn tree() -> Vec<Block> {
        vec![
            block(1, 0, "Document"),
            with_text(block(2, 1, "H1"), 0, "Title", [0.0, 80.0, 40.0, 92.0]),
            block(3, 1, "Sect"),
            with_text(block(4, 2, "P"), 1, "Body text", [0.0, 10.0, 50.0, 22.0]),
        ]
    }

    fn shown(rows: &[TagRow]) -> Vec<(u32, usize, bool)> {
        rows.iter()
            .map(|row| (row.element, row.depth, row.has_children))
            .collect()
    }

    #[test]
    fn a_fresh_pane_shows_the_top_level_folded() {
        let rows = rows(&tree(), &TagsState::default());
        assert_eq!(shown(&rows), [(1, 0, true)]);
        assert!(!rows[0].expanded);
    }

    #[test]
    fn opening_an_element_shows_its_children_and_folds_what_is_below_them() {
        let mut state = TagsState::default();
        toggle(&mut state, 1);
        assert_eq!(
            shown(&rows(&tree(), &state)),
            [(1, 0, true), (2, 1, false), (3, 1, true)],
            "Sect has a child of its own and is still folded"
        );
        toggle(&mut state, 3);
        assert_eq!(rows(&tree(), &state).len(), 4);
        toggle(&mut state, 1);
        assert_eq!(
            shown(&rows(&tree(), &state)),
            [(1, 0, true)],
            "folding an element hides everything below it"
        );
    }

    #[test]
    fn a_folded_element_does_not_hide_the_children_of_an_open_sibling_after_it() {
        let blocks = [
            block(1, 0, "Sect"),
            block(2, 1, "P"),
            block(3, 0, "Sect"),
            block(4, 1, "P"),
        ];
        let mut state = TagsState::default();
        toggle(&mut state, 3);
        assert_eq!(
            shown(&rows(&blocks, &state)),
            [(1, 0, true), (3, 0, true), (4, 1, false)]
        );
    }

    #[test]
    fn choosing_an_element_selects_it_and_does_not_open_or_fold_it() {
        let mut state = TagsState::default();
        toggle(&mut state, 1);
        select(&mut state, 1);
        select(&mut state, 1);
        let rows = rows(&tree(), &state);
        assert!(rows.iter().find(|r| r.element == 1).unwrap().selected);
        assert_eq!(
            rows.len(),
            3,
            "choosing an open element again leaves it open"
        );
    }

    #[test]
    fn a_continuation_is_not_a_row_and_does_not_count_as_a_child() {
        let mut after = block(1, 0, "P");
        after.continuation = true;
        let blocks = [block(1, 0, "P"), block(2, 0, "P"), after];
        let rows = rows(&blocks, &TagsState::default());
        assert_eq!(shown(&rows), [(1, 0, false), (2, 0, false)]);
    }

    #[test]
    fn a_long_tree_is_cut_and_says_how_many_are_left() {
        let mut blocks = vec![block(0, 0, "Document")];
        blocks.extend((1..=MAX_ROWS as u32 + 5).map(|n| block(n, 1, "P")));
        let mut state = TagsState::default();
        toggle(&mut state, 0);
        let (shown, hidden) = capped(&blocks, &state);
        assert_eq!((shown.len(), hidden), (MAX_ROWS, 6));
        let (_, none) = capped(&tree(), &TagsState::default());
        assert_eq!(none, 0);
    }

    #[test]
    fn a_row_names_the_type_as_written_and_what_a_role_map_made_of_it() {
        let mut custom = block(5, 0, "Chapter");
        custom.standard_type = Some(Name::new("Sect"));
        let mut unmapped = block(6, 0, "Weird");
        unmapped.standard_type = None;
        let rows = rows(&[custom, unmapped], &TagsState::default());
        assert_eq!(rows[0].name, "Chapter (as Sect)");
        assert_eq!(rows[1].name, "Weird");
    }

    #[test]
    fn the_detail_is_the_title_then_the_alt_then_the_start_of_the_text() {
        let mut titled = with_text(block(1, 0, "H1"), 0, "words", [0.0; 4]);
        titled.title = Some("The title".to_owned());
        let mut alt = with_text(block(2, 0, "Figure"), 0, "", [0.0; 4]);
        alt.alt = Some("A cat".to_owned());
        let long = with_text(block(3, 0, "P"), 0, &"x".repeat(60), [0.0; 4]);
        let mut both = with_text(block(4, 0, "Figure"), 0, "words", [0.0; 4]);
        both.title = Some("Title wins".to_owned());
        both.alt = Some("not this".to_owned());
        let mut alt_and_text = with_text(block(5, 0, "Figure"), 0, "words", [0.0; 4]);
        alt_and_text.alt = Some("Alt wins".to_owned());
        let rows = rows(
            &[titled, alt, long, both, alt_and_text],
            &TagsState::default(),
        );
        assert_eq!(rows[0].detail, "The title");
        assert_eq!(rows[1].detail, "A cat");
        assert_eq!(rows[2].detail, format!("{}…", "x".repeat(DETAIL_CHARS)));
        assert_eq!(rows[3].detail, "Title wins");
        assert_eq!(rows[4].detail, "Alt wins");
    }

    #[test]
    fn an_elements_boxes_cover_its_subtree_one_per_page_and_leave_artifacts_out() {
        let mut blocks = tree();
        blocks.push(with_text(
            block(5, 1, "P"),
            1,
            "elsewhere",
            [100.0, 100.0, 120.0, 110.0],
        ));
        blocks[1].items.push(ContentItem {
            page: 0,
            kind: ItemKind::Text,
            bounds: [500.0, 500.0, 600.0, 600.0],
            text: Some("page 3".to_owned()),
            artifact: true,
        });

        assert_eq!(
            content_boxes(&blocks, 1),
            [(0, [0.0, 80.0, 40.0, 92.0]), (1, [0.0, 10.0, 120.0, 110.0])]
        );
        assert_eq!(
            content_boxes(&blocks, 3),
            [(1, [0.0, 10.0, 50.0, 22.0])],
            "a sibling's content is not the element's"
        );
        assert!(content_boxes(&blocks, 99).is_empty());
    }

    #[test]
    fn content_a_replacement_or_an_artifact_element_keeps_out_of_the_reading_is_still_boxed() {
        let mut blocks = tree();
        blocks[3].excluded = true;
        assert_eq!(
            content_boxes(&blocks, 1),
            [(0, [0.0, 80.0, 40.0, 92.0]), (1, [0.0, 10.0, 50.0, 22.0])],
            "the tree marks it, so the pane boxes it"
        );
        assert_eq!(content_boxes(&blocks, 4), [(1, [0.0, 10.0, 50.0, 22.0])]);
    }

    #[test]
    fn an_elements_later_blocks_are_boxed_with_it() {
        let mut after = with_text(block(2, 1, "H1"), 0, "more", [60.0, 80.0, 90.0, 92.0]);
        after.continuation = true;
        let blocks = [
            block(1, 0, "Document"),
            with_text(block(2, 1, "H1"), 0, "Title", [0.0, 80.0, 40.0, 92.0]),
            block(3, 2, "Link"),
            after,
        ];
        assert_eq!(content_boxes(&blocks, 2), [(0, [0.0, 80.0, 90.0, 92.0])]);
    }

    #[test]
    fn rows_are_described_with_their_level_state_and_page() {
        let mut state = TagsState::default();
        toggle(&mut state, 1);
        let blocks = Some(Arc::new(tree()));
        let described = accessible(Ok(&blocks), &state);
        let rows = &described[0].children;
        assert_eq!(described[0].role, Role::Tree);
        assert_eq!(rows[0].level, Some(1));
        assert_eq!(rows[0].expanded, Some(true));
        assert_eq!(rows[1].level, Some(2));
        assert_eq!(rows[1].expanded, None, "a leaf does not open");
        assert_eq!(
            rows[1].description.as_deref(),
            Some("Level 2, page 1"),
            "level, openness and place are words, because macOS reads no node property for them"
        );
        assert_eq!(
            rows[1].activation,
            Some(Activation::Pane(PaneAction::Tag(TagAction::Select(2))))
        );
        let toggle_button = &rows[0].children[0];
        assert_eq!(toggle_button.role, Role::DisclosureTriangle);
        assert_eq!(toggle_button.label, "Fold Document");
        assert_eq!(rows[0].description.as_deref(), Some("Level 1, expanded"));
        assert_eq!(
            toggle_button.activation,
            Some(Activation::Pane(PaneAction::Tag(TagAction::Toggle(1)))),
            "a keyboard user can open and fold without choosing"
        );
    }

    #[test]
    fn an_untagged_document_and_a_failed_read_are_announced_differently() {
        let none: Option<Arc<Vec<Block>>> = None;
        let empty = accessible(Ok(&none), &TagsState::default());
        assert_eq!(
            (empty[0].role, empty[0].label.as_str()),
            (Role::Label, NO_TAGS)
        );
        let failure = "the structure could not be read".to_owned();
        let broken = accessible(Err(&failure), &TagsState::default());
        assert_eq!(
            (broken[0].role, broken[0].label.as_str()),
            (Role::Alert, failure.as_str())
        );
    }
}
