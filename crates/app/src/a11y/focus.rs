//! Keyboard focus over the accessible tree.
//!
//! The shell has one GPUI focus handle for its chrome, not one per control,
//! so "which control is focused" is this ring's cursor rather than a hundred
//! handles GPUI would have to arbitrate between. The order is the tree's own
//! depth-first order, which is the order the surfaces are built in, which is
//! the order they read in.

use gpui::ElementId;

use super::tree::Element;

/// Where a keystroke moves the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Next,
    Previous,
}

/// The tab order, and where in it focus sits.
#[derive(Debug, Default)]
pub(crate) struct Ring {
    order: Vec<ElementId>,
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
        let previous = self
            .focused
            .as_ref()
            .and_then(|key| self.order.iter().position(|entry| entry == key));
        self.order = root
            .walk()
            .filter(|element| element.is_tab_stop())
            .map(|element| element.key.clone())
            .collect();
        if let Some(key) = &self.focused {
            if self.order.contains(key) {
                return;
            }
        }
        self.focused = match (self.focused.is_some(), previous) {
            (true, Some(index)) => self.order.get(index.min(self.order.len().saturating_sub(1))).cloned(),
            _ => None,
        };
    }

    pub(crate) fn focused(&self) -> Option<&ElementId> {
        self.focused.as_ref()
    }

    /// Put focus on a named element, if it is a tab stop.
    pub(crate) fn focus(&mut self, key: &ElementId) -> bool {
        if !self.order.contains(key) {
            return false;
        }
        self.focused = Some(key.clone());
        true
    }

    /// Move the cursor. Wraps at both ends, which is what a toolbar does and
    /// what VoiceOver users expect from a single focus ring.
    ///
    /// Returns false when there is nothing to focus, so the caller can let
    /// the keystroke keep going rather than swallowing it.
    pub(crate) fn step(&mut self, step: Step) -> bool {
        if self.order.is_empty() {
            return false;
        }
        let last = self.order.len() - 1;
        let current = self
            .focused
            .as_ref()
            .and_then(|key| self.order.iter().position(|entry| entry == key));
        let next = match (step, current) {
            (Step::Next, None) => 0,
            (Step::Previous, None) => last,
            (Step::Next, Some(index)) if index == last => 0,
            (Step::Next, Some(index)) => index + 1,
            (Step::Previous, Some(0)) => last,
            (Step::Previous, Some(index)) => index - 1,
        };
        self.focused = Some(self.order[next].clone());
        true
    }

    #[cfg(test)]
    pub(crate) fn order(&self) -> &[ElementId] {
        &self.order
    }
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
            element("menu", Role::Button),
            element("search", Role::SearchInput),
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

    #[test]
    fn stepping_walks_the_order_and_wraps_at_both_ends() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        assert!(ring.step(Step::Next));
        assert_eq!(ring.focused(), Some(&"menu".into()));
        ring.step(Step::Next);
        ring.step(Step::Next);
        ring.step(Step::Next);
        assert_eq!(ring.focused(), Some(&"last-page".into()));
        ring.step(Step::Next);
        assert_eq!(ring.focused(), Some(&"menu".into()));
        ring.step(Step::Previous);
        assert_eq!(ring.focused(), Some(&"last-page".into()));
    }

    #[test]
    fn shift_tab_from_nowhere_lands_on_the_last_stop() {
        let mut ring = Ring::default();
        ring.rebuild(&tree());

        ring.step(Step::Previous);

        assert_eq!(ring.focused(), Some(&"last-page".into()));
    }

    #[test]
    fn stepping_an_empty_ring_reports_that_it_did_nothing() {
        let mut ring = Ring::default();
        ring.rebuild(&Element::<()>::new("window", Role::Window, "Onionskin"));

        assert!(ring.order().is_empty());
        assert!(!ring.step(Step::Next));
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
            element("menu", Role::Button),
            element("search", Role::SearchInput),
            element("last-page", Role::Button),
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
            .child(element("menu", Role::Button));
        ring.rebuild(&shorter);

        assert_eq!(ring.focused(), Some(&"menu".into()));
    }
}
