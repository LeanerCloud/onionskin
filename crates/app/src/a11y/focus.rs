//! Keyboard focus over the accessible tree.
//!
//! The shell has one GPUI focus handle for its chrome, not one per control,
//! so "which control is focused" is this ring's cursor rather than a hundred
//! handles GPUI would have to arbitrate between. The order is the tree's own
//! depth-first order, which is the order the surfaces are built in, which is
//! the order they read in.
//!
//! The ring has two levels, because a flat one makes Tab cross an open
//! thumbnails pane a row at a time: Tab moves between groups (the global bar,
//! the rail, the pane strip, the open pane, the canvas, the find bar, the
//! page controls) and the arrow keys move inside the group focus is in. That
//! is the platform convention and what a screen-reader user expects.
//!
//! A group is the nearest enclosing element whose role is a container. The
//! tree already says which surface a control belongs to, so a surface adding
//! a control has nothing extra to declare for it to land in the right group.

use accesskit::Role;
use gpui::ElementId;

use super::tree::Element;

/// Where a keystroke moves the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Next,
    Previous,
}

/// The tab order, split into groups, and where in it focus sits.
#[derive(Debug, Default)]
pub(crate) struct Ring {
    /// One entry per group, each holding that group's stops in reading
    /// order. Groups are in the reading order of their first stop, and none
    /// of them is empty.
    groups: Vec<Vec<ElementId>>,
    focused: Option<ElementId>,
}

impl Ring {
    /// Take the tab order from a freshly built tree.
    ///
    /// Focus survives a rebuild when the focused element is still there. When
    /// it is not, focus falls to the element that took its place in the
    /// order, which is what a user expects after closing a menu or deleting
    /// the row they were on: the cursor stays where it was rather than
    /// jumping back to the top.
    pub(crate) fn rebuild<A>(&mut self, root: &Element<A>) {
        let previous = self.focused.as_ref().and_then(|key| self.position(key));
        self.groups = grouped(root);
        if let Some(key) = &self.focused {
            if self.position(key).is_some() {
                return;
            }
        }
        self.focused = match (self.focused.is_some(), previous) {
            (true, Some(index)) => {
                let last = self.stops().count().saturating_sub(1);
                self.stops().nth(index.min(last)).cloned()
            }
            _ => None,
        };
    }

    pub(crate) fn focused(&self) -> Option<&ElementId> {
        self.focused.as_ref()
    }

    /// Put focus on a named element, if it is a tab stop.
    pub(crate) fn focus(&mut self, key: &ElementId) -> bool {
        if self.position(key).is_none() {
            return false;
        }
        self.focused = Some(key.clone());
        true
    }

    /// Tab and Shift-Tab: the next group, entered at its first stop.
    ///
    /// Wraps at both ends, and lands on the same stop in both directions, so
    /// Tab and Shift-Tab undo each other. Entering at the first stop is what
    /// makes leaving an open pane one keystroke rather than one per row.
    ///
    /// Returns false when there is nothing to focus, so the caller can let
    /// the keystroke keep going rather than swallowing it.
    pub(crate) fn step(&mut self, step: Step) -> bool {
        if self.groups.is_empty() {
            return false;
        }
        let last = self.groups.len() - 1;
        let next = match (step, self.cursor()) {
            (Step::Next, None) => 0,
            (Step::Previous, None) => last,
            (Step::Next, Some((group, _))) if group == last => 0,
            (Step::Next, Some((group, _))) => group + 1,
            (Step::Previous, Some((0, _))) => last,
            (Step::Previous, Some((group, _))) => group - 1,
        };
        self.focused = Some(self.groups[next][0].clone());
        true
    }

    /// The arrow keys: the next stop inside the group focus is in.
    ///
    /// Wraps inside the group and never leaves it, which is what a toolbar,
    /// a tab strip and a list of rows all do. With nothing focused there is
    /// no group to move inside, so this enters the ring the way Tab does.
    pub(crate) fn step_within(&mut self, step: Step) -> bool {
        let Some((group, index)) = self.cursor() else {
            return self.step(step);
        };
        let stops = &self.groups[group];
        let last = stops.len() - 1;
        let next = match step {
            Step::Next if index == last => 0,
            Step::Next => index + 1,
            Step::Previous if index == 0 => last,
            Step::Previous => index - 1,
        };
        self.focused = Some(stops[next].clone());
        true
    }

    /// Every stop, in reading order, groups run together.
    fn stops(&self) -> impl Iterator<Item = &ElementId> {
        self.groups.iter().flatten()
    }

    fn position(&self, key: &ElementId) -> Option<usize> {
        self.stops().position(|entry| entry == key)
    }

    /// Which group focus is in, and where in it.
    fn cursor(&self) -> Option<(usize, usize)> {
        let key = self.focused.as_ref()?;
        self.groups.iter().enumerate().find_map(|(group, stops)| {
            Some((group, stops.iter().position(|entry| entry == key)?))
        })
    }

    #[cfg(test)]
    pub(crate) fn order(&self) -> Vec<ElementId> {
        self.stops().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn groups(&self) -> &[Vec<ElementId>] {
        &self.groups
    }
}

/// Split a tree's tab stops into groups.
///
/// Ordered by where each group's first stop reads, not by where its container
/// sits: a container declared before the stops it ends up holding would
/// otherwise put those stops ahead of controls that read before them.
///
/// Groups run one after another, so flattening them is reading order only
/// while containers do not nest with stops on both sides of the nesting: a
/// toolbar holding a, then a group holding b, then c gives a, c, b. No
/// surface is built that way today. It matters only to [`Ring::rebuild`],
/// which uses the flattened index to keep focus somewhere sensible when the
/// element it was on disappears.
fn grouped<A>(root: &Element<A>) -> Vec<Vec<ElementId>> {
    let mut stops = Vec::new();
    collect(root, 0, &mut 0, &mut stops);
    let mut owners: Vec<usize> = Vec::new();
    let mut groups: Vec<Vec<ElementId>> = Vec::new();
    for (owner, key) in stops {
        match owners.iter().position(|entry| *entry == owner) {
            Some(index) => groups[index].push(key),
            None => {
                owners.push(owner);
                groups.push(vec![key]);
            }
        }
    }
    groups
}

/// Walk the tree in reading order, tagging each stop with the container it is
/// inside. A container that turns out to hold no stops never reaches
/// [`grouped`]'s output, so an all-disabled toolbar is not a group Tab stops
/// at.
fn collect<A>(
    element: &Element<A>,
    owner: usize,
    next_owner: &mut usize,
    stops: &mut Vec<(usize, ElementId)>,
) {
    let owner = if is_container(element.role) {
        *next_owner += 1;
        *next_owner
    } else {
        owner
    };
    if element.is_tab_stop() {
        stops.push((owner, element.key.clone()));
    }
    for child in &element.children {
        collect(child, owner, next_owner, stops);
    }
}

/// Whether this role encloses a group of controls.
///
/// One entry per container role the shell publishes. A role that is not here
/// is transparent: its stops join the group its own container is, which is
/// what a wrapper around two buttons should do.
///
/// `Role::Document` is both, and is deliberately both: it is a stop a user
/// tabs to, and it is the canvas group, which is a group of one.
fn is_container(role: Role) -> bool {
    matches!(
        role,
        Role::Window
            | Role::Dialog
            | Role::Toolbar
            | Role::TabList
            | Role::TabPanel
            | Role::Menu
            | Role::List
            | Role::Tree
            | Role::RadioGroup
            | Role::Group
            | Role::Navigation
            | Role::Main
            | Role::Complementary
            | Role::Region
            | Role::Document
    )
}

#[cfg(test)]
mod tests {
    use accesskit::Role;

    use super::*;
    use crate::a11y::tree::State;

    fn element(key: &'static str, role: Role) -> Element<()> {
        Element::new(key, role, key).with_activation(())
    }

    fn tree() -> Element<()> {
        Element::new("window", Role::Window, "Onionskin").with_children(vec![
            Element::new("bar", Role::Toolbar, "Bar").with_children(vec![
                element("menu", Role::Button),
                element("search", Role::SearchInput),
            ]),
            Element::new("group", Role::Group, "Group").with_children(vec![
                element("first-page", Role::Button),
                element("next-page", Role::Button).with_state(State::enabled(false)),
                element("last-page", Role::Button),
                // A drag handle: a button the mouse alone can work.
                Element::new("drag-handle", Role::Button, "Move"),
            ]),
            element("label", Role::Label),
        ])
    }

    fn keys(ring: &Ring) -> Vec<String> {
        ring.order().iter().map(ToString::to_string).collect()
    }

    fn groups(ring: &Ring) -> Vec<Vec<String>> {
        ring.groups()
            .iter()
            .map(|stops| stops.iter().map(ToString::to_string).collect())
            .collect()
    }

    #[test]
    fn the_order_is_reading_order_and_skips_containers_text_disabled_and_inert_controls() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        assert_eq!(
            keys(&ring),
            vec![
                "menu".to_owned(),
                "search".to_owned(),
                "first-page".to_owned(),
                "last-page".to_owned(),
            ]
        );
    }

    /// The structure Tab moves over: one group per surface, not one group
    /// holding everything.
    #[test]
    fn the_stops_are_split_by_the_container_they_are_in() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        assert_eq!(
            groups(&ring),
            vec![
                vec!["menu".to_owned(), "search".to_owned()],
                vec!["first-page".to_owned(), "last-page".to_owned()],
            ]
        );
    }

    /// A control with no container of its own belongs to the window, and its
    /// group sits where the control reads rather than where the window was
    /// declared. The window is declared first, so a group ordered by its
    /// container would put a late root-level control ahead of the toolbar.
    #[test]
    fn a_group_sits_where_its_first_stop_reads() {
        let mut ring = Ring::default();
        ring.rebuild(
            &Element::new("window", Role::Window, "Onionskin").with_children(vec![
                Element::new("bar", Role::Toolbar, "Bar").child(element("menu", Role::Button)),
                // Directly on the window, after the toolbar.
                element("cancel-export", Role::Button),
            ]),
        );

        assert_eq!(
            groups(&ring),
            vec![
                vec!["menu".to_owned()],
                vec!["cancel-export".to_owned()],
            ]
        );
    }

    /// A pane strip and the pane it opens are two groups, so leaving an open
    /// pane is one Tab rather than one per row. This is the whole point of
    /// the two levels.
    #[test]
    fn tab_leaves_a_pane_with_many_rows_in_one_press() {
        let mut ring = Ring::default();
        ring.rebuild(
            &Element::new("window", Role::Window, "Onionskin").with_children(vec![
                Element::new("strip", Role::TabList, "Panes")
                    .child(element("thumbnails-tab", Role::Tab)),
                Element::new("body", Role::TabPanel, "Page Thumbnails").with_children(
                    (0..40_usize)
                        .map(|index| {
                            Element::new(("thumbnail-row", index), Role::ListItem, "Page")
                                .with_activation(())
                        })
                        .collect(),
                ),
                Element::new("page-controls", Role::Toolbar, "Page Controls")
                    .child(element("zoom-in", Role::Button)),
            ]),
        );

        ring.focus(&"thumbnails-tab".into());
        ring.step(Step::Next);
        assert_eq!(ring.focused(), Some(&("thumbnail-row", 0usize).into()));

        // One press, forty rows.
        ring.step(Step::Next);
        assert_eq!(ring.focused(), Some(&"zoom-in".into()));
    }

    /// Tab moves between groups, so it must skip the rest of the group focus
    /// is in rather than stepping to the stop next to it.
    #[test]
    fn tab_moves_to_the_next_group_and_wraps_at_both_ends() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        assert!(ring.step(Step::Next));
        assert_eq!(ring.focused(), Some(&"menu".into()));
        ring.step(Step::Next);
        assert_eq!(
            ring.focused(),
            Some(&"first-page".into()),
            "Tab stopped inside the group it started in"
        );
        ring.step(Step::Next);
        assert_eq!(ring.focused(), Some(&"menu".into()));
        ring.step(Step::Previous);
        assert_eq!(ring.focused(), Some(&"first-page".into()));
    }

    /// Tab and Shift-Tab enter a group at the same stop, so one undoes the
    /// other.
    #[test]
    fn shift_tab_undoes_tab() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());
        ring.focus(&"search".into());

        ring.step(Step::Next);
        ring.step(Step::Previous);

        assert_eq!(ring.focused(), Some(&"menu".into()));
    }

    /// The arrows are the only way to reach the second control in a group,
    /// and they stay in it: at the end of the group the next one wraps back
    /// to its first stop rather than falling into the group after it.
    #[test]
    fn an_arrow_moves_inside_the_group_and_wraps_there() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());
        ring.focus(&"menu".into());

        assert!(ring.step_within(Step::Next));
        assert_eq!(ring.focused(), Some(&"search".into()));

        ring.step_within(Step::Next);
        assert_eq!(
            ring.focused(),
            Some(&"menu".into()),
            "an arrow left the group it started in"
        );
        ring.step_within(Step::Previous);
        assert_eq!(ring.focused(), Some(&"search".into()));
    }

    #[test]
    fn shift_tab_from_nowhere_lands_on_the_last_group() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        ring.step(Step::Previous);

        assert_eq!(ring.focused(), Some(&"first-page".into()));
    }

    /// An arrow with nothing focused has no group to move inside, so it
    /// enters the ring rather than doing nothing.
    #[test]
    fn an_arrow_from_nowhere_enters_the_ring() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        assert!(ring.step_within(Step::Next));

        assert_eq!(ring.focused(), Some(&"menu".into()));
    }

    #[test]
    fn stepping_an_empty_ring_reports_that_it_did_nothing() {
        let mut ring = Ring::default();
        ring.rebuild(&Element::<()>::new("window", Role::Window, "Onionskin"));

        assert!(ring.order().is_empty());
        assert!(!ring.step(Step::Next));
        assert!(!ring.step_within(Step::Next));
        assert_eq!(ring.focused(), None);
    }

    #[test]
    fn focus_only_lands_on_a_tab_stop() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        assert!(ring.focus(&"search".into()));
        assert_eq!(ring.focused(), Some(&"search".into()));
        assert!(!ring.focus(&"next-page".into()));
        assert_eq!(ring.focused(), Some(&"search".into()));
    }

    #[test]
    fn focus_survives_a_rebuild_that_keeps_the_element() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());
        ring.focus(&"search".into());

        ring.rebuild(&tree());

        assert_eq!(ring.focused(), Some(&"search".into()));
    }

    /// Closing a menu takes the focused entry away. Focus should land on
    /// whatever now occupies that place in the order, not reset to the top
    /// and not stay pointing at something that is gone.
    #[test]
    fn focus_moves_to_the_element_that_took_its_place_when_it_disappears() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());
        ring.focus(&"first-page".into());

        let shorter = Element::new("window", Role::Window, "Onionskin").with_children(vec![
            Element::new("bar", Role::Toolbar, "Bar").with_children(vec![
                element("menu", Role::Button),
                element("search", Role::SearchInput),
            ]),
            Element::new("group", Role::Group, "Group").child(element("last-page", Role::Button)),
        ]);
        ring.rebuild(&shorter);

        assert_eq!(ring.focused(), Some(&"last-page".into()));
    }

    #[test]
    fn focus_stays_unset_across_a_rebuild_when_it_was_never_set() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());
        ring.rebuild(&tree());

        assert_eq!(ring.focused(), None);
    }

    /// Focus was on the last stop and the new order is shorter, so the index
    /// it held no longer exists. It lands on the new last stop rather than
    /// falling off the end.
    #[test]
    fn focus_on_a_vanished_last_stop_lands_on_the_new_last_one() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());
        ring.focus(&"last-page".into());

        let shorter = Element::new("window", Role::Window, "Onionskin")
            .child(Element::new("bar", Role::Toolbar, "Bar").child(element("menu", Role::Button)));
        ring.rebuild(&shorter);

        assert_eq!(ring.focused(), Some(&"menu".into()));
    }
}
