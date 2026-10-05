//! The content pane: what the page the view was on draws, grouped by kind and
//! tagged with the marked-content sequence each piece sits in.
//!
//! It answers the question the tags pane cannot: which content has no place in
//! the structure. A piece drawn outside any marked-content sequence is
//! `untagged`, one inside an `/Artifact` says so, and the rest name their tag
//! and id, which is the id an element's `/K` refers to it by on this page. (A
//! sequence in a form carries the form's own numbering, which an element
//! reaches only through an `/MCR` with a `/Stm`, so its id here does not name
//! an element.) Choosing a piece boxes it on the page. Nothing here edits the
//! document.

use std::collections::BTreeSet;

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::{MarkedPage, MarkedRef, PageIndex};

use super::super::chrome::accessible::{Activation, Element};
use super::super::chrome::{ShellFrame, ThemeTokens};
use super::{empty_message, error_message, list, PaneAction, ROW_HEIGHT};
use crate::a11y::State as A11yState;

const NO_CONTENT: &str = "This page draws no text, images or paths.";
/// A page of line art can draw tens of thousands of paths, and a list that long
/// is neither drawn nor read out; the rest are counted.
const MAX_ROWS_PER_GROUP: usize = 500;
const DETAIL_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::shell) enum Group {
    Text,
    Images,
    Paths,
}

impl ContentSnapshot {
    /// What the pane has with no document behind it.
    pub(super) fn none() -> Self {
        Self {
            page: 0,
            entries: Vec::new(),
        }
    }
}

impl Group {
    const ALL: [Group; 3] = [Group::Text, Group::Images, Group::Paths];

    fn label(self) -> &'static str {
        match self {
            Group::Text => "Text",
            Group::Images => "Images",
            Group::Paths => "Paths",
        }
    }
}

/// One piece of the page's content.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct ContentEntry {
    group: Group,
    label: String,
    /// Its marked-content sequence: `untagged`, `artifact`, or the tag and id.
    sequence: String,
    /// Page space, as content reports it.
    bounds: [f64; 4],
}

/// The page's content as the pane lists it, read when the pane opens.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct ContentSnapshot {
    page: PageIndex,
    entries: Vec<ContentEntry>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::shell) struct ContentState {
    open: BTreeSet<Group>,
    pub(super) selected: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ContentAction {
    /// Open or fold a group.
    Toggle(Group),
    /// Box one piece, by its place in the snapshot.
    Select(usize),
}

fn sequence_of(marked: &Option<MarkedRef>) -> String {
    let Some(marked) = marked else {
        return "untagged".to_owned();
    };
    if marked.artifact {
        return "artifact".to_owned();
    }
    let tag = marked
        .tag
        .as_ref()
        .map(|tag| String::from_utf8_lossy(tag.as_bytes()).into_owned())
        .unwrap_or_else(|| "(no tag)".to_owned());
    match marked.mcid {
        Some(mcid) => format!("{tag}, id {mcid}"),
        None => format!("{tag}, no id"),
    }
}

fn snippet(text: &str) -> String {
    let mut cut: String = text.chars().take(DETAIL_CHARS).collect();
    if text.chars().count() > DETAIL_CHARS {
        cut.push('…');
    }
    cut
}

/// What page `page` drew, as entries: text runs, then images, then paths, each
/// in drawing order.
pub(super) fn snapshot(page: PageIndex, marked: &MarkedPage) -> ContentSnapshot {
    let mut entries = Vec::new();
    for run in &marked.text.runs {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (x, y) in run.glyphs.iter().flat_map(|glyph| glyph.quad.corners) {
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
        entries.push(ContentEntry {
            group: Group::Text,
            label: snippet(&run.decoded_text),
            sequence: sequence_of(&run.marked),
            bounds,
        });
    }
    for image in &marked.images {
        entries.push(ContentEntry {
            group: Group::Images,
            label: format!("Image {}", image.name),
            sequence: sequence_of(&image.marked),
            bounds: image.bounds(),
        });
    }
    for shape in &marked.shapes {
        let how = match (shape.filled, shape.stroked) {
            (true, true) => "filled and stroked",
            (true, false) => "filled",
            _ => "stroked",
        };
        entries.push(ContentEntry {
            group: Group::Paths,
            label: format!("Path, {how}"),
            sequence: sequence_of(&shape.marked),
            bounds: shape.bounds(),
        });
    }
    ContentSnapshot { page, entries }
}

/// One drawn row: a group heading, or one of its pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Row {
    Heading {
        group: Group,
        count: usize,
        open: bool,
    },
    Piece {
        index: usize,
        text: String,
        selected: bool,
    },
    /// What is left of a group longer than [`MAX_ROWS_PER_GROUP`].
    More(usize),
}

pub(super) fn rows(snapshot: &ContentSnapshot, state: &ContentState) -> Vec<Row> {
    let mut out = Vec::new();
    for group in Group::ALL {
        let members: Vec<(usize, &ContentEntry)> = snapshot
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.group == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        let open = state.open.contains(&group);
        out.push(Row::Heading {
            group,
            count: members.len(),
            open,
        });
        if !open {
            continue;
        }
        for (index, entry) in members.iter().take(MAX_ROWS_PER_GROUP) {
            out.push(Row::Piece {
                index: *index,
                text: format!("{}  ({})", entry.label, entry.sequence),
                selected: state.selected == Some(*index),
            });
        }
        if members.len() > MAX_ROWS_PER_GROUP {
            out.push(Row::More(members.len() - MAX_ROWS_PER_GROUP));
        }
    }
    out
}

/// The box to draw for entry `index`, with its page.
pub(super) fn box_of(snapshot: &ContentSnapshot, index: usize) -> Option<(PageIndex, [f64; 4])> {
    snapshot
        .entries
        .get(index)
        .map(|entry| (snapshot.page, entry.bounds))
}

pub(super) fn toggle(state: &mut ContentState, group: Group) {
    if !state.open.remove(&group) {
        state.open.insert(group);
    }
}

pub(super) fn select(state: &mut ContentState, index: usize) {
    state.selected = Some(index);
}

pub(super) fn run(
    state: &mut super::NavigationPanesState,
    canvas: Option<&gpui::Entity<super::Canvas>>,
    action: ContentAction,
    cx: &mut Context<ShellFrame>,
) {
    match action {
        ContentAction::Toggle(group) => toggle(&mut state.content_state, group),
        ContentAction::Select(index) => {
            let Some(super::PaneContent::Content(Ok(snapshot))) = state.content.clone() else {
                return;
            };
            select(&mut state.content_state, index);
            let target = box_of(&snapshot, index);
            let page = snapshot.page;
            super::navigate(state, canvas, cx, move |canvas| {
                canvas
                    .model
                    .set_structure_highlight(target.into_iter().collect());
                // The snapshot is of the page the pane opened on, and the view
                // may have scrolled since: the box is no use off screen.
                canvas.model.go_to_page(page)
            });
        }
    }
}

pub(super) fn accessible(
    snapshot: Result<&ContentSnapshot, &String>,
    state: &ContentState,
) -> Vec<Element> {
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(message) => {
            return vec![Element::new(
                "content-rows-error",
                Role::Alert,
                message.clone(),
            )]
        }
    };
    if snapshot.entries.is_empty() {
        return vec![Element::new("content-rows-empty", Role::Label, NO_CONTENT)];
    }
    vec![Element::new(
        "content-rows",
        Role::Tree,
        format!("Content of page {}", snapshot.page + 1),
    )
    .with_children(
        rows(snapshot, state)
            .into_iter()
            .enumerate()
            .map(|(position, row)| match row {
                Row::Heading { group, count, open } => Element::new(
                    ("content-row", position),
                    Role::TreeItem,
                    format!("{} ({count})", group.label()),
                )
                .with_description(if open { "Expanded" } else { "Collapsed" })
                .with_activation(Activation::Pane(PaneAction::Content(
                    ContentAction::Toggle(group),
                ))),
                Row::Piece {
                    index,
                    text,
                    selected,
                } => Element::new(("content-row", position), Role::TreeItem, text)
                    .with_state(A11yState::selected(selected))
                    .with_description("Level 2")
                    .with_activation(Activation::Pane(PaneAction::Content(
                        ContentAction::Select(index),
                    ))),
                Row::More(count) => Element::new(
                    ("content-row", position),
                    Role::Label,
                    format!("{count} more are not listed"),
                ),
            })
            .collect(),
    )]
}

pub(super) fn render(
    snapshot: Result<&ContentSnapshot, &String>,
    state: &ContentState,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(message) => return error_message(message, theme).into_any_element(),
    };
    if snapshot.entries.is_empty() {
        return empty_message(NO_CONTENT, theme).into_any_element();
    }
    let mut body = list("content-rows");
    for (position, row) in rows(snapshot, state).into_iter().enumerate() {
        let (text, indent, action, selected) = match row {
            Row::Heading { group, count, open } => (
                format!(
                    "{} {} ({count})",
                    if open { "▾" } else { "▸" },
                    group.label()
                ),
                8.0,
                Some(ContentAction::Toggle(group)),
                false,
            ),
            Row::Piece {
                index,
                text,
                selected,
            } => (text, 24.0, Some(ContentAction::Select(index)), selected),
            Row::More(count) => (format!("{count} more are not listed"), 24.0, None, false),
        };
        let mut element = div()
            .id(("content-row", position))
            .min_h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .py_1()
            .pr_2()
            .pl(px(indent))
            .text_sm()
            .text_color(if action.is_some() {
                theme.text
            } else {
                theme.muted_text
            })
            .when(selected, |row| row.bg(theme.selected))
            .child(text);
        if let Some(action) = action {
            element = element
                .cursor_pointer()
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::Pane(PaneAction::Content(action)), window, cx);
                }));
        }
        body = body.child(element);
    }
    body.into_any_element()
}

#[cfg(test)]
mod tests {
    use onionskin_core::MarkedRef;
    use onionskin_cos::Name;

    use super::*;

    fn entry(group: Group, label: &str, sequence: &str) -> ContentEntry {
        ContentEntry {
            group,
            label: label.to_owned(),
            sequence: sequence.to_owned(),
            bounds: [0.0, 0.0, 10.0, 10.0],
        }
    }

    fn page() -> ContentSnapshot {
        ContentSnapshot {
            page: 2,
            entries: vec![
                entry(Group::Text, "Title", "H1, id 0"),
                entry(Group::Text, "page 3", "artifact"),
                entry(Group::Images, "Image Im0", "untagged"),
                entry(Group::Paths, "Path, filled", "Figure, id 4"),
            ],
        }
    }

    #[test]
    fn groups_start_folded_with_their_counts() {
        assert_eq!(
            rows(&page(), &ContentState::default()),
            [
                Row::Heading {
                    group: Group::Text,
                    count: 2,
                    open: false
                },
                Row::Heading {
                    group: Group::Images,
                    count: 1,
                    open: false
                },
                Row::Heading {
                    group: Group::Paths,
                    count: 1,
                    open: false
                },
            ]
        );
    }

    #[test]
    fn an_open_group_lists_its_pieces_with_their_sequences() {
        let mut state = ContentState::default();
        toggle(&mut state, Group::Text);
        select(&mut state, 1);
        let listed = rows(&page(), &state);
        assert_eq!(
            listed[1..3],
            [
                Row::Piece {
                    index: 0,
                    text: "Title  (H1, id 0)".to_owned(),
                    selected: false
                },
                Row::Piece {
                    index: 1,
                    text: "page 3  (artifact)".to_owned(),
                    selected: true
                },
            ]
        );
        toggle(&mut state, Group::Text);
        assert_eq!(rows(&page(), &state).len(), 3, "folded again");
    }

    #[test]
    fn a_group_with_nothing_in_it_is_not_listed() {
        let only_images = ContentSnapshot {
            page: 0,
            entries: vec![entry(Group::Images, "Image Im0", "untagged")],
        };
        assert_eq!(rows(&only_images, &ContentState::default()).len(), 1);
    }

    #[test]
    fn a_long_group_is_cut_and_says_how_many_are_left() {
        let long = ContentSnapshot {
            page: 0,
            entries: (0..MAX_ROWS_PER_GROUP + 7)
                .map(|n| entry(Group::Paths, &format!("Path {n}"), "untagged"))
                .collect(),
        };
        let mut state = ContentState::default();
        toggle(&mut state, Group::Paths);
        let listed = rows(&long, &state);
        assert_eq!(listed.len(), 1 + MAX_ROWS_PER_GROUP + 1);
        assert_eq!(listed.last(), Some(&Row::More(7)));
    }

    #[test]
    fn a_group_of_exactly_the_cap_is_listed_whole() {
        let exact = ContentSnapshot {
            page: 0,
            entries: (0..MAX_ROWS_PER_GROUP)
                .map(|n| entry(Group::Paths, &format!("Path {n}"), "untagged"))
                .collect(),
        };
        let mut state = ContentState::default();
        toggle(&mut state, Group::Paths);
        let listed = rows(&exact, &state);
        assert_eq!(listed.len(), 1 + MAX_ROWS_PER_GROUP);
        assert!(!listed.iter().any(|row| matches!(row, Row::More(_))));
    }

    #[test]
    fn a_pieces_box_is_on_the_snapshots_page() {
        assert_eq!(box_of(&page(), 2), Some((2, [0.0, 0.0, 10.0, 10.0])));
        assert_eq!(box_of(&page(), 9), None);
    }

    #[test]
    fn a_sequence_reads_as_untagged_artifact_or_its_tag_and_id() {
        let marked = |mcid: Option<i64>, artifact: bool| {
            Some(MarkedRef {
                mcid,
                tag: Some(Name::new("P")),
                artifact,
                depth: 1,
            })
        };
        assert_eq!(sequence_of(&None), "untagged");
        assert_eq!(sequence_of(&marked(Some(3), false)), "P, id 3");
        assert_eq!(sequence_of(&marked(None, false)), "P, no id");
        assert_eq!(sequence_of(&marked(Some(3), true)), "artifact");
    }

    #[test]
    fn rows_are_described_and_an_empty_page_and_a_failure_say_so() {
        let mut state = ContentState::default();
        toggle(&mut state, Group::Text);
        let described = accessible(Ok(&page()), &state);
        let children = &described[0].children;
        assert_eq!(described[0].label, "Content of page 3");
        assert_eq!(children[0].label, "Text (2)");
        assert_eq!(children[0].description.as_deref(), Some("Expanded"));
        assert_eq!(
            children[1].activation,
            Some(Activation::Pane(PaneAction::Content(
                ContentAction::Select(0)
            )))
        );

        let empty = ContentSnapshot {
            page: 0,
            entries: Vec::new(),
        };
        assert_eq!(accessible(Ok(&empty), &state)[0].label, NO_CONTENT);
        let failure = "the page could not be read".to_owned();
        assert_eq!(accessible(Err(&failure), &state)[0].role, Role::Alert);
    }
}
