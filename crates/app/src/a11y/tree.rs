//! The accessible description of the shell, and its translation into an
//! AccessKit tree.
//!
//! [`Element`] is what the shell knows how to produce: one node per thing a
//! screen reader should reach, keyed by the same `gpui::ElementId` the thing
//! renders with. [`Ids`] turns those keys into `accesskit::NodeId`s that stay
//! the same for the same element across frames, and [`update`] flattens the
//! description into the `TreeUpdate` the platform adapter wants.

use std::collections::{HashMap, HashSet};

use accesskit::{Node, NodeId, Rect, Role, Toggled, Tree, TreeId, TreeUpdate};
use gpui::ElementId;

/// The state a control carries beyond its name.
///
/// Split out from [`Element`] because most nodes carry none of it, and
/// because a screen reader announces state and name from different places:
/// a checked toolbar button is "Actual Size, checked", never "1:1 ✓".
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct State {
    /// Set on anything with an on/off state: a toggle button, a checkbox, a
    /// menu entry with a tick.
    pub(crate) toggled: Option<bool>,
    /// Set on anything that is one of a set: a tab, a rail entry, a
    /// preference choice.
    pub(crate) selected: Option<bool>,
    /// The control is on screen but cannot be used now.
    pub(crate) disabled: bool,
}

impl State {
    pub(crate) fn toggled(toggled: bool) -> Self {
        Self {
            toggled: Some(toggled),
            ..Self::default()
        }
    }

    pub(crate) fn selected(selected: bool) -> Self {
        Self {
            selected: Some(selected),
            ..Self::default()
        }
    }

    pub(crate) fn enabled(enabled: bool) -> Self {
        Self {
            disabled: !enabled,
            ..Self::default()
        }
    }
}

/// One accessible element of the shell.
///
/// `key` is the `gpui::ElementId` the element renders with, so a node and the
/// pixels it describes cannot be given different identities. `bounds` is in
/// window coordinates and is `None` where the shell does not know the
/// element's real rectangle; AccessKit tolerates that, VoiceOver just cannot
/// draw its cursor around the element.
///
/// `A` is what activating the element does. This module never looks inside
/// it: it is the shell's own action value, carried on the node so that the
/// keyboard path and the click path cannot run different things.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Element<A> {
    pub(crate) key: ElementId,
    pub(crate) role: Role,
    pub(crate) label: String,
    pub(crate) value: Option<String>,
    /// Why a disabled control is disabled, or anything else worth hearing
    /// after the name. Announced by VoiceOver after the label.
    pub(crate) description: Option<String>,
    /// What VoiceOver calls this kind of thing, when the default for the role
    /// is wrong or absent. See [`role_description`].
    pub(crate) role_description: Option<&'static str>,
    pub(crate) state: State,
    pub(crate) bounds: Option<Rect>,
    /// What Enter or Space on this element runs. `None` on containers and
    /// static text.
    pub(crate) activation: Option<A>,
    pub(crate) children: Vec<Element<A>>,
}

impl<A> Element<A> {
    pub(crate) fn new(key: impl Into<ElementId>, role: Role, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            role,
            label: label.into(),
            value: None,
            description: None,
            role_description: role_description(role),
            state: State::default(),
            bounds: None,
            activation: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn with_activation(mut self, activation: A) -> Self {
        self.activation = Some(activation);
        self
    }

    pub(crate) fn with_state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    pub(crate) fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(crate) fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Override what VoiceOver calls this element. A page is a "page", not
    /// the "region" its role would otherwise be announced as.
    pub(crate) fn with_role_description(mut self, role_description: &'static str) -> Self {
        self.role_description = Some(role_description);
        self
    }

    pub(crate) fn with_children(mut self, children: Vec<Element<A>>) -> Self {
        self.children = children;
        self
    }

    pub(crate) fn child(mut self, child: Element<A>) -> Self {
        self.children.push(child);
        self
    }

    /// Attach real rectangles to this element's direct children, in the order
    /// the children were built.
    ///
    /// The bounds come from `gpui::Div::on_children_prepainted`, which reports
    /// one rectangle per child of a div in child order. The caller's job is to
    /// build the description in the same order it builds the children, which
    /// is why every surface drives both from one item list.
    ///
    /// A short slice leaves the remaining children without bounds rather than
    /// pairing a node with the wrong rectangle.
    pub(crate) fn place_children(&mut self, bounds: &[Rect]) {
        for (child, rect) in self.children.iter_mut().zip(bounds) {
            child.bounds = Some(*rect);
        }
    }

    /// Depth-first walk, self first.
    pub(crate) fn walk(&self) -> impl Iterator<Item = &Element<A>> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let element = stack.pop()?;
            stack.extend(element.children.iter().rev());
            Some(element)
        })
    }

    /// Whether Tab stops here.
    ///
    /// A tab stop is an enabled control of an interactive role that has
    /// something to run. The last condition is what keeps a drag handle, or
    /// any other control the mouse alone can work, out of the ring: focusing
    /// something Enter cannot then operate is a dead end.
    pub(crate) fn is_tab_stop(&self) -> bool {
        is_focusable(self.role) && !self.state.disabled && self.activation.is_some()
    }

    /// Find a descendant by key, for tests and for focus lookups.
    pub(crate) fn find(&self, key: &ElementId) -> Option<&Element<A>> {
        self.walk().find(|element| &element.key == key)
    }
}

/// Whether a screen reader user can put keyboard focus on this role.
///
/// Containers and static text are reachable by a screen reader's own cursor
/// but are not tab stops, so the ring skips them.
pub(crate) fn is_focusable(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::ComboBox
            | Role::Document
            | Role::Link
            | Role::ListBoxOption
            | Role::ListItem
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::MultilineTextInput
            | Role::NumberInput
            | Role::RadioButton
            | Role::SearchInput
            | Role::Switch
            | Role::Tab
            | Role::TextInput
            | Role::TreeItem
    )
}

/// What VoiceOver should say instead of AppKit's default for the role.
///
/// AppKit answers "group" for anything AccessKit maps to
/// `NSAccessibilityGroupRole`, which is every one of `Role::Document`,
/// `Role::Group` and `Role::ListItem`. A document that announces as "group"
/// is the defect M1's spike recorded, and this is the half of it that is
/// ours: AccessKit reads a node's own role description before falling back
/// (accesskit_macos 0.26.3 `node.rs`, `accessibilityRoleDescription`).
pub(crate) fn role_description(role: Role) -> Option<&'static str> {
    match role {
        Role::Document => Some("document"),
        Role::ListItem => Some("item"),
        _ => None,
    }
}

/// Stable `NodeId`s for element keys.
///
/// A node has to keep its id for as long as it is the same element, or a
/// screen reader loses its place every time the tree is pushed. Ids are
/// handed out on first sight of a key, so they are stable without hashing and
/// cannot collide.
///
/// A number is never handed out twice, even after the key it belonged to
/// leaves the tree, so a stale request naming it resolves to nothing rather
/// than to whatever took its place.
#[derive(Debug, Default)]
pub(crate) struct Ids {
    assigned: HashMap<ElementId, NodeId>,
    keys: HashMap<NodeId, ElementId>,
    next: u64,
}

impl Ids {
    pub(crate) fn id_for(&mut self, key: &ElementId) -> NodeId {
        if let Some(id) = self.assigned.get(key) {
            return *id;
        }
        let id = NodeId(self.next);
        self.next += 1;
        self.assigned.insert(key.clone(), id);
        self.keys.insert(id, key.clone());
        id
    }

    /// The element a screen reader's action request named.
    pub(crate) fn key_for(&self, id: NodeId) -> Option<&ElementId> {
        self.keys.get(&id)
    }

    /// The node an element was given, without minting one for a key that has
    /// never been published. For reading the published tree back in a test.
    #[cfg(test)]
    pub(crate) fn assigned(&self, key: &ElementId) -> Option<NodeId> {
        self.assigned.get(key).copied()
    }

    /// Forget every key that is not in the tree just published.
    ///
    /// Without this the map grows for the life of the process: scrolling a
    /// thousand-page document mints a key for every run of text on every page
    /// it passes, and none of them ever comes back.
    fn retain(&mut self, live: &HashSet<ElementId>) {
        self.assigned.retain(|key, _| live.contains(key));
        let assigned = &self.assigned;
        self.keys.retain(|_, key| assigned.contains_key(key));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.assigned.len()
    }
}

/// Flatten a description into the update the platform adapter consumes.
///
/// `focus` names the element keyboard focus sits on. It falls back to the
/// root rather than to an arbitrary node: an update whose focus names a node
/// that is not in the tree is rejected by AccessKit.
pub(crate) fn update<A>(root: &Element<A>, focus: Option<&ElementId>, ids: &mut Ids) -> TreeUpdate {
    let live: HashSet<ElementId> = root.walk().map(|element| element.key.clone()).collect();
    debug_assert_eq!(
        live.len(),
        root.walk().count(),
        "two elements were published with the same key, so they would share one node"
    );
    ids.retain(&live);
    let root_id = ids.id_for(&root.key);
    let mut nodes = Vec::new();
    push(root, ids, &mut nodes);
    let focus = focus
        .filter(|key| root.find(key).is_some())
        .map_or(root_id, |key| ids.id_for(key));
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(root_id)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

fn push<A>(element: &Element<A>, ids: &mut Ids, nodes: &mut Vec<(NodeId, Node)>) {
    let id = ids.id_for(&element.key);
    let mut node = Node::new(element.role);
    node.set_label(element.label.clone());
    if let Some(value) = &element.value {
        node.set_value(value.clone());
    }
    // The key doubles as the element's `accessibilityIdentifier`, which is
    // how the automated probe reads a specific control back out of the tree
    // rather than matching on a label a copy edit can change.
    node.set_author_id(element.key.to_string());
    if let Some(description) = &element.description {
        node.set_description(description.clone());
    }
    if let Some(role_description) = element.role_description {
        node.set_role_description(role_description);
    }
    if let Some(toggled) = element.state.toggled {
        node.set_toggled(Toggled::from(toggled));
    }
    if let Some(selected) = element.state.selected {
        node.set_selected(selected);
    }
    if element.state.disabled {
        node.set_disabled();
    }
    // A screen reader only announces an alert on its own if the node says it
    // is a live region; without this every error the shell raises waits
    // silently for the user to navigate onto it.
    if element.role == Role::Alert {
        node.set_live(accesskit::Live::Assertive);
    }
    if let Some(bounds) = element.bounds {
        node.set_bounds(bounds);
    }
    if element.is_tab_stop() {
        node.add_action(accesskit::Action::Focus);
        node.add_action(accesskit::Action::Click);
    }
    node.set_children(
        element
            .children
            .iter()
            .map(|child| ids.id_for(&child.key))
            .collect::<Vec<_>>(),
    );
    nodes.push((id, node));
    for child in &element.children {
        push(child, ids, nodes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Element<()> {
        Element::new("window", Role::Window, "Onionskin").with_children(vec![
            Element::new("actual-size", Role::Button, "Actual Size")
                .with_activation(())
                .with_state(State::toggled(true)),
            Element::new("next-page", Role::Button, "Next Page")
                .with_activation(())
                .with_state(State::enabled(false))
                .with_description("This is the last page"),
            Element::new("document", Role::Document, "hello.pdf")
                .with_activation(())
                .with_children(vec![Element::new(
                    ("page", 0usize),
                    Role::Region,
                    "Page 1 of 1",
                )]),
        ])
    }

    #[test]
    fn every_element_becomes_one_node_and_the_root_is_the_tree_root() {
        let mut ids = Ids::default();
        let update = update(&tree(), None, &mut ids);

        assert_eq!(update.nodes.len(), 5);
        let root = ids.id_for(&"window".into());
        assert_eq!(update.tree.as_ref().unwrap().root, root);
        assert_eq!(update.focus, root);
    }

    #[test]
    fn a_toggled_button_carries_state_rather_than_a_tick_in_its_name() {
        let mut ids = Ids::default();
        let update = update(&tree(), None, &mut ids);
        let id = ids.id_for(&"actual-size".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();

        assert_eq!(node.label(), Some("Actual Size"));
        assert_eq!(node.toggled(), Some(Toggled::True));
        assert!(!node.label().unwrap().contains('✓'));
    }

    #[test]
    fn a_disabled_control_says_why_and_offers_no_actions() {
        let mut ids = Ids::default();
        let update = update(&tree(), None, &mut ids);
        let id = ids.id_for(&"next-page".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();

        assert!(node.is_disabled());
        assert_eq!(node.description(), Some("This is the last page"));
        assert!(!node.supports_action(accesskit::Action::Click));
    }

    /// A screen reader announces an alert only if the node says it is a live
    /// region, so every error the shell raises would otherwise be silent.
    #[test]
    fn an_alert_is_published_as_a_live_region() {
        let mut ids = Ids::default();
        let alert: Element<()> = Element::new("root", Role::Window, "W")
            .child(Element::new("boom", Role::Alert, "It broke"))
            .child(Element::new("quiet", Role::Label, "Page 1"));
        let update = update(&alert, None, &mut ids);

        let id = ids.id_for(&"boom".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();
        assert_eq!(node.live(), Some(accesskit::Live::Assertive));

        let id = ids.id_for(&"quiet".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();
        assert_eq!(node.live(), None);
    }

    /// The M1 spike's finding: AccessKit maps `Role::Document` to
    /// `NSAccessibilityGroupRole`, so without a role description of our own
    /// AppKit answers "group" and VoiceOver announces a page as a group.
    #[test]
    fn the_document_node_carries_a_role_description_so_it_is_not_announced_as_a_group() {
        let mut ids = Ids::default();
        let update = update(&tree(), None, &mut ids);
        let id = ids.id_for(&"document".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();

        assert_eq!(node.role(), Role::Document);
        assert_eq!(node.role_description(), Some("document"));
    }

    #[test]
    fn an_id_survives_a_rebuild_of_the_same_tree() {
        let mut ids = Ids::default();
        let first = update(&tree(), None, &mut ids);
        let second = update(&tree(), None, &mut ids);

        assert_eq!(
            first.nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            second.nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
        assert_eq!(ids.len(), 5);
    }

    /// The probe reads a control back out of the platform tree by its
    /// identifier, so the identifier has to be the key and not a rendering of
    /// it.
    #[test]
    fn every_node_carries_its_element_id_as_its_author_id() {
        let mut ids = Ids::default();
        let update = update(&tree(), None, &mut ids);
        let id = ids.id_for(&("page", 0usize).into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();

        assert_eq!(node.author_id(), Some("page-0"));
    }

    /// Two elements sharing a key would become one node with two parents,
    /// which is a tree a screen reader cannot walk.
    #[test]
    #[should_panic(expected = "the same key")]
    fn a_duplicate_key_is_caught_rather_than_published() {
        let clashing: Element<()> = Element::new("window", Role::Window, "Onionskin")
            .with_children(vec![
                Element::new("row", Role::Button, "One").with_activation(()),
                Element::new("row", Role::Button, "Two").with_activation(()),
            ]);

        update(&clashing, None, &mut Ids::default());
    }

    /// Scrolling a long document mints a key per run of text per page. They
    /// have to be forgotten when the page leaves the tree, or the map grows
    /// for the life of the process.
    #[test]
    fn a_key_that_leaves_the_tree_is_forgotten_and_its_number_is_not_reused() {
        let mut ids = Ids::default();
        let full = tree();
        update(&full, None, &mut ids);
        let gone = ids.id_for(&("page", 0usize).into());
        assert_eq!(ids.len(), 5);

        let smaller: Element<()> = Element::new("window", Role::Window, "Onionskin")
            .child(Element::new("actual-size", Role::Button, "Actual Size").with_activation(()));
        update(&smaller, None, &mut ids);

        assert_eq!(ids.len(), 2);
        assert_eq!(ids.key_for(gone), None);
        assert_ne!(ids.id_for(&"newcomer".into()), gone);
    }

    #[test]
    fn focus_names_the_focused_element_and_falls_back_to_the_root_when_it_is_gone() {
        let mut ids = Ids::default();
        let tree = tree();

        let focused = update(&tree, Some(&"next-page".into()), &mut ids);
        assert_eq!(focused.focus, ids.id_for(&"next-page".into()));

        let stale = update(&tree, Some(&"gone".into()), &mut ids);
        assert_eq!(stale.focus, ids.id_for(&"window".into()));
    }

    #[test]
    fn children_take_the_rectangles_they_were_painted_at_in_order() {
        let mut root: Element<()> = Element::new("row", Role::Group, "Row").with_children(vec![
            Element::new("a", Role::Button, "A"),
            Element::new("b", Role::Button, "B"),
            Element::new("c", Role::Button, "C"),
        ]);

        root.place_children(&[
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(10.0, 0.0, 20.0, 10.0),
        ]);

        assert_eq!(
            root.children[0].bounds,
            Some(Rect::new(0.0, 0.0, 10.0, 10.0))
        );
        assert_eq!(
            root.children[1].bounds,
            Some(Rect::new(10.0, 0.0, 20.0, 10.0))
        );
        assert_eq!(root.children[2].bounds, None);
    }

    #[test]
    fn a_container_is_not_a_tab_stop_but_a_button_is() {
        assert!(is_focusable(Role::Button));
        assert!(is_focusable(Role::Document));
        assert!(!is_focusable(Role::Group));
        assert!(!is_focusable(Role::Label));
    }

    /// A control the mouse alone can work, such as the quick actions drag
    /// handle, must not be a tab stop: Enter on it would do nothing.
    #[test]
    fn a_control_with_nothing_to_run_is_not_a_tab_stop() {
        let handle: Element<()> = Element::new("drag", Role::Button, "Move");
        let button = Element::new("zoom", Role::Button, "Zoom In").with_activation(());

        assert!(!handle.is_tab_stop());
        assert!(button.is_tab_stop());

        let mut ids = Ids::default();
        let update = update(
            &Element::new("root", Role::Window, "W").with_children(vec![handle]),
            None,
            &mut ids,
        );
        let id = ids.id_for(&"drag".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();
        assert!(!node.supports_action(accesskit::Action::Click));
    }

    #[test]
    fn an_enabled_control_offers_focus_and_click_to_a_screen_reader() {
        let mut ids = Ids::default();
        let update = update(&tree(), None, &mut ids);
        let id = ids.id_for(&"actual-size".into());
        let (_, node) = update.nodes.iter().find(|(node, _)| *node == id).unwrap();

        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::Click));
    }

    #[test]
    fn the_walk_reaches_every_element_depth_first() {
        let tree = tree();
        let keys: Vec<String> = tree.walk().map(|e| e.key.to_string()).collect();

        assert_eq!(keys.len(), 5);
        assert!(tree.find(&("page", 0usize).into()).is_some());
        assert!(tree.find(&"absent".into()).is_none());
    }
}
