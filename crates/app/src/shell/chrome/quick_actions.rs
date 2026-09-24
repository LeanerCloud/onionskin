use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, point, px, size, AppContext as _, Bounds, Context, Div, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use onionskin_plugin_api::{PluginRegistry, ToolCapability};

use super::accessible::{Activation, Element, Rects, Surface};
use super::tabs::ShellFrame;
use super::theme::ThemeTokens;
use crate::a11y::State as A11yState;

const PREFERRED_TOOLBAR_WIDTH: f32 = 640.0;
const TOOLBAR_HEIGHT: f32 = 64.0;
const CUSTOMIZATION_HEIGHT: f32 = 296.0;

/// The braille-pattern glyph the drag handle draws, and the words a screen
/// reader says instead of it.
const DRAG_HANDLE_GLYPH: &str = "⠿";
const DRAG_HANDLE_NAME: &str = "Move Quick Actions";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum QuickAction {
    Select,
    Comment,
    Highlight,
    Draw,
    FillTextFields,
    AddSignature,
}

impl QuickAction {
    pub(in crate::shell) const ALL: [Self; 6] = [
        Self::Select,
        Self::Comment,
        Self::Highlight,
        Self::Draw,
        Self::FillTextFields,
        Self::AddSignature,
    ];

    pub(super) fn index(self) -> usize {
        match self {
            Self::Select => 0,
            Self::Comment => 1,
            Self::Highlight => 2,
            Self::Draw => 3,
            Self::FillTextFields => 4,
            Self::AddSignature => 5,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Comment => "Comment",
            Self::Highlight => "Highlight",
            Self::Draw => "Draw",
            Self::FillTextFields => "Fill text",
            Self::AddSignature => "Add sign",
        }
    }

    pub(super) fn menu_label(self) -> &'static str {
        match self {
            Self::Select => "Toolbar: Select",
            Self::Comment => "Toolbar: Comment",
            Self::Highlight => "Toolbar: Highlight",
            Self::Draw => "Toolbar: Draw",
            Self::FillTextFields => "Toolbar: Fill text",
            Self::AddSignature => "Toolbar: Add signature",
        }
    }

    pub(super) fn capability(self) -> ToolCapability {
        match self {
            Self::Select => ToolCapability::Select,
            Self::Comment => ToolCapability::Comment,
            Self::Highlight => ToolCapability::Highlight,
            Self::Draw => ToolCapability::Draw,
            Self::FillTextFields => ToolCapability::FillTextFields,
            Self::AddSignature => ToolCapability::AddSignature,
        }
    }

    fn unavailable_stage(self) -> DeliveryStage {
        match self {
            Self::Select => DeliveryStage::P10,
            Self::Comment | Self::Highlight | Self::Draw => DeliveryStage::M3,
            Self::FillTextFields | Self::AddSignature => DeliveryStage::M5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeliveryStage {
    P10,
    M3,
    M5,
}

impl DeliveryStage {
    fn label(self) -> &'static str {
        match self {
            Self::P10 => "P10",
            Self::M3 => "M3",
            Self::M5 => "M5",
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::P10 => "Available in P10 tools-basic",
            Self::M3 => "Available in M3 tools-comment",
            Self::M5 => "Available in M5 tools-fill-sign",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickActionAvailability {
    Enabled {
        tool_index: usize,
    },
    Disabled {
        stage: DeliveryStage,
    },
    /// The tool exists, and the open document may not be edited. Carries the
    /// document's own reason rather than a delivery stage, because the tool
    /// has been delivered - it is the document that refuses.
    Refused {
        reason: &'static str,
    },
}

impl QuickActionAvailability {
    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    pub(super) fn tool_index(self) -> Option<usize> {
        match self {
            Self::Enabled { tool_index } => Some(tool_index),
            Self::Disabled { .. } | Self::Refused { .. } => None,
        }
    }

    pub(super) fn reason(self) -> Option<&'static str> {
        match self {
            Self::Enabled { .. } => None,
            Self::Disabled { stage } => Some(stage.reason()),
            Self::Refused { reason } => Some(reason),
        }
    }

    fn unavailable_stage(self) -> Option<DeliveryStage> {
        match self {
            Self::Enabled { .. } | Self::Refused { .. } => None,
            Self::Disabled { stage } => Some(stage),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct QuickActionEntry {
    pub(super) action: QuickAction,
    pub(super) availability: QuickActionAvailability,
}

#[derive(Debug, Clone, Copy)]
struct ActiveDrag {
    id: u64,
    pointer: Point<Pixels>,
    position: Point<Pixels>,
}

#[derive(Debug)]
pub(super) struct QuickActionsState {
    visible: [bool; QuickAction::ALL.len()],
    customizing: bool,
    position: Point<Pixels>,
    active_drag: Option<ActiveDrag>,
    next_drag: u64,
}

impl Default for QuickActionsState {
    fn default() -> Self {
        Self {
            visible: [true; QuickAction::ALL.len()],
            customizing: false,
            position: point(px(24.0), px(24.0)),
            active_drag: None,
            next_drag: 1,
        }
    }
}

impl QuickActionsState {
    /// `edit_refusal` is the open document's reason it may not be edited, from
    /// `core`'s protection query; `None` for a document that may be. An action
    /// whose tool edits is refused with that reason.
    pub(super) fn entries(
        &self,
        registry: &PluginRegistry,
        edit_refusal: Option<&'static str>,
    ) -> Vec<QuickActionEntry> {
        self.visible_actions()
            .into_iter()
            .map(|action| quick_action_entry(action, registry, edit_refusal))
            .collect()
    }

    pub(super) fn all_entries(
        &self,
        registry: &PluginRegistry,
        edit_refusal: Option<&'static str>,
    ) -> Vec<QuickActionEntry> {
        QuickAction::ALL
            .into_iter()
            .map(|action| quick_action_entry(action, registry, edit_refusal))
            .collect()
    }

    fn visible_actions(&self) -> Vec<QuickAction> {
        QuickAction::ALL
            .into_iter()
            .filter(|action| self.visible[action.index()])
            .collect()
    }

    pub(super) fn is_visible(&self, action: QuickAction) -> bool {
        self.visible[action.index()]
    }

    pub(super) fn visibility(&self) -> [bool; QuickAction::ALL.len()] {
        self.visible
    }

    pub(super) fn toggle_visibility(&mut self, action: QuickAction) {
        let visible = &mut self.visible[action.index()];
        *visible = !*visible;
    }

    pub(super) fn customizing(&self) -> bool {
        self.customizing
    }

    pub(super) fn toggle_customizing(&mut self) {
        self.customizing = !self.customizing;
    }

    pub(super) fn position(&self) -> Point<Pixels> {
        self.position
    }

    pub(super) fn toolbar_size(&self, document: Size<Pixels>) -> Size<Pixels> {
        size(
            toolbar_width(document.width),
            px(if self.customizing {
                CUSTOMIZATION_HEIGHT
            } else {
                TOOLBAR_HEIGHT
            }),
        )
    }

    pub(super) fn next_drag(&self) -> u64 {
        self.next_drag
    }

    pub(super) fn drag_to(
        &mut self,
        id: u64,
        pointer: Point<Pixels>,
        document: Bounds<Pixels>,
        toolbar: Size<Pixels>,
    ) {
        if self.active_drag.map(|drag| drag.id) != Some(id) {
            self.active_drag = Some(ActiveDrag {
                id,
                pointer,
                position: self.position,
            });
            self.next_drag = self
                .next_drag
                .checked_add(1)
                .expect("quick-action drag id exhausted");
        }
        let drag = self
            .active_drag
            .expect("a quick-action drag is initialized before it moves");
        let relative = point(
            drag.position.x + pointer.x - drag.pointer.x,
            drag.position.y + pointer.y - drag.pointer.y,
        );
        self.position = constrained_position(relative, document.size, toolbar);
    }

    pub(super) fn constrain_to(&mut self, document: Size<Pixels>) {
        self.position = constrained_position(self.position, document, self.toolbar_size(document));
    }
}

fn toolbar_width(document_width: Pixels) -> Pixels {
    document_width.min(px(PREFERRED_TOOLBAR_WIDTH)).max(px(0.0))
}

fn constrained_position(
    position: Point<Pixels>,
    document: Size<Pixels>,
    toolbar: Size<Pixels>,
) -> Point<Pixels> {
    point(
        clamp_axis(position.x, document.width - toolbar.width),
        clamp_axis(position.y, document.height - toolbar.height),
    )
}

fn clamp_axis(value: Pixels, maximum: Pixels) -> Pixels {
    let minimum = px(0.0);
    let maximum = if maximum < minimum { minimum } else { maximum };
    if value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    }
}

fn quick_action_entry(
    action: QuickAction,
    registry: &PluginRegistry,
    edit_refusal: Option<&'static str>,
) -> QuickActionEntry {
    if let (Some(reason), true) = (edit_refusal, action.capability().edits_document()) {
        return QuickActionEntry {
            action,
            availability: QuickActionAvailability::Refused { reason },
        };
    }
    let availability = tool_for(registry, action.capability()).map_or(
        QuickActionAvailability::Disabled {
            stage: action.unavailable_stage(),
        },
        |tool_index| QuickActionAvailability::Enabled { tool_index },
    );
    QuickActionEntry {
        action,
        availability,
    }
}

/// The tool a quick action picks: the first whose own purpose is the
/// capability (it lists it first), else the first that lists it at all.
///
/// Highlight also lists Comment, because a highlight is a comment, but the
/// Comment quick action is Acrobat's sticky note, not the highlighter that
/// happens to be registered before it.
fn tool_for(registry: &PluginRegistry, capability: ToolCapability) -> Option<usize> {
    let listing = |primary_only: bool| {
        registry.tools().position(|tool| {
            let capabilities = tool.capabilities();
            if primary_only {
                capabilities.first() == Some(&capability)
            } else {
                capabilities.contains(&capability)
            }
        })
    };
    listing(true).or_else(|| listing(false))
}

#[derive(Debug, Clone, Copy)]
pub(super) struct QuickActionDrag {
    id: u64,
}

impl QuickActionDrag {
    pub(super) fn id(self) -> u64 {
        self.id
    }
}

struct QuickActionDragPreview {
    offset: Point<Pixels>,
    color: gpui::Rgba,
}

impl Render for QuickActionDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.offset.x)
            .pt(self.offset.y)
            .child(div().w(px(20.0)).h(px(28.0)).rounded_sm().bg(self.color))
    }
}

fn customize_label(customizing: bool) -> &'static str {
    if customizing {
        "Done"
    } else {
        "Customize"
    }
}

fn visibility_glyph(visible: bool) -> &'static str {
    if visible {
        "✓"
    } else {
        "○"
    }
}

/// What the quick-action toolbar tells a screen reader.
///
/// The row's children come first, in row order, so the rectangles the row
/// reports after prepaint land on the right nodes. The customization rows
/// follow, in the panel's order.
pub(super) fn accessible(
    entries: &[QuickActionEntry],
    all_entries: &[QuickActionEntry],
    state: &QuickActionsState,
) -> Element {
    let customizing = state.customizing();
    // The handle is dragged, not activated, so it carries no activation.
    let mut toolbar = Element::new("quick-actions", Role::Toolbar, "Quick Actions").child(
        Element::new("quick-actions-drag-handle", Role::Button, DRAG_HANDLE_NAME),
    );

    for entry in entries {
        let mut button = Element::new(
            ("quick-action", entry.action.index()),
            Role::Button,
            entry.action.label(),
        )
        .with_state(A11yState::enabled(entry.availability.is_enabled()))
        .with_activation(Activation::QuickAction(*entry));
        if let Some(reason) = entry.availability.reason() {
            button = button.with_description(reason);
        }
        toolbar = toolbar.child(button);
    }

    toolbar = toolbar.child(
        Element::new(
            "quick-actions-customize",
            Role::Button,
            customize_label(customizing),
        )
        .with_state(A11yState::toggled(customizing))
        .with_activation(Activation::ToggleQuickActionCustomization),
    );

    if customizing {
        for entry in all_entries {
            let action = entry.action;
            // The panel draws a tick beside the label. The tick is the
            // checkbox's state here, so the name stays just the label.
            let mut item = Element::new(
                ("quick-action-customization", action.index()),
                Role::CheckBox,
                action.label(),
            )
            .with_state(A11yState::toggled(state.is_visible(action)))
            .with_activation(Activation::ToggleQuickActionVisibility(action));
            if let Some(reason) = entry.availability.reason() {
                item = item.with_description(reason);
            }
            toolbar = toolbar.child(item);
        }
    }

    toolbar
}

pub(super) fn render_quick_actions(
    entries: Vec<QuickActionEntry>,
    all_entries: Vec<QuickActionEntry>,
    state: &QuickActionsState,
    document: Size<Pixels>,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let position = state.position();
    let toolbar_size = state.toolbar_size(document);
    let customizing = state.customizing();
    let drag = QuickActionDrag {
        id: state.next_drag(),
    };
    let mut row = quick_actions_row()
        .on_children_prepainted(move |bounds, window, _cx| {
            rects.record(Surface::QuickActions, &bounds, window);
        })
        .child(
            div()
                .id("quick-actions-drag-handle")
                .w(px(24.0))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_move()
                .text_color(theme.secondary_text)
                .on_drag(drag, move |_drag, offset, _window, cx| {
                    cx.new(|_| QuickActionDragPreview {
                        offset,
                        color: theme.drag_preview,
                    })
                })
                .on_drag_move::<QuickActionDrag>(cx.listener(
                    |frame, event: &gpui::DragMoveEvent<QuickActionDrag>, window, cx| {
                        let id = event.drag(cx).id();
                        frame.drag_quick_actions(id, event.event.position, window, cx);
                    },
                ))
                .child(DRAG_HANDLE_GLYPH),
        );

    for entry in entries {
        let enabled = entry.availability.is_enabled();
        row = row.child(
            div()
                .id(("quick-action", entry.action.index()))
                .w(px(82.0))
                .h(px(52.0))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .rounded_sm()
                .text_color(if enabled {
                    theme.text
                } else {
                    theme.muted_text
                })
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |button| button.bg(theme.hover))
                })
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    if enabled {
                        frame.run_activation(Activation::QuickAction(entry), window, cx);
                    }
                }))
                .child(div().text_xs().child(entry.action.label()))
                .when_some(entry.availability.unavailable_stage(), |button, stage| {
                    button.child(
                        div()
                            .mt_1()
                            .text_xs()
                            .text_color(theme.muted_text)
                            .child(stage.label()),
                    )
                }),
        );
    }

    row = row.child(
        div()
            .id("quick-actions-customize")
            .w(px(76.0))
            .h(px(52.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .hover(move |button| button.bg(theme.hover))
            .on_click(cx.listener(|frame, _event, window, cx| {
                frame.run_activation(Activation::ToggleQuickActionCustomization, window, cx);
            }))
            .text_xs()
            .child(customize_label(customizing)),
    );

    let mut toolbar = div()
        .absolute()
        .left(position.x)
        .top(position.y)
        .w(toolbar_size.width)
        .rounded_md()
        .occlude()
        .bg(theme.raised)
        .text_color(theme.text)
        .shadow_md()
        .child(toolbar_row_scroller(row));

    if customizing {
        let mut panel = div()
            .h(px(CUSTOMIZATION_HEIGHT - TOOLBAR_HEIGHT))
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .pb_2();
        for entry in all_entries {
            let action = entry.action;
            let visible = state.is_visible(action);
            panel = panel.child(
                div()
                    .id(("quick-action-customization", action.index()))
                    .w_full()
                    .h(px(36.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |item| item.bg(theme.selected))
                    .on_click(cx.listener(move |frame, _event, window, cx| {
                        frame.run_activation(
                            Activation::ToggleQuickActionVisibility(action),
                            window,
                            cx,
                        );
                    }))
                    .child(visibility_glyph(visible))
                    .child(action.label())
                    .when_some(entry.availability.reason(), |item, reason| {
                        item.child(
                            div()
                                .ml_auto()
                                .text_xs()
                                .text_color(theme.muted_text)
                                .child(reason),
                        )
                    }),
            );
        }
        toolbar = toolbar.child(panel);
    }

    toolbar
}

fn quick_actions_row() -> Div {
    div()
        .flex_none()
        .min_w_full()
        .h(px(TOOLBAR_HEIGHT))
        .flex()
        .items_center()
        .gap_1()
        .p_1()
}

fn toolbar_row_scroller(row: impl IntoElement) -> Stateful<Div> {
    div()
        .id("quick-actions-scroll")
        .w_full()
        .h(px(TOOLBAR_HEIGHT))
        .flex()
        .overflow_x_scroll()
        .child(row)
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size, Bounds};
    #[cfg(feature = "shell-test-support")]
    use gpui::{Render, ScrollHandle, TestAppContext, Window};
    use onionskin_plugin_api::{
        PluginManifest, PluginRegistry, PointerInput, ToolCapability, ToolCtx, ToolPlugin,
    };

    use super::*;

    struct CapabilityTool {
        id: &'static str,
        capabilities: &'static [ToolCapability],
    }

    impl ToolPlugin for CapabilityTool {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            "Capability"
        }

        fn icon(&self) -> &'static str {
            "capability"
        }

        fn capabilities(&self) -> &'static [ToolCapability] {
            self.capabilities
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    }

    struct EmptyManifest;

    fn single_capability(capability: ToolCapability) -> &'static [ToolCapability] {
        match capability {
            ToolCapability::Select => &[ToolCapability::Select],
            ToolCapability::Comment => &[ToolCapability::Comment],
            ToolCapability::Highlight => &[ToolCapability::Highlight],
            ToolCapability::Draw => &[ToolCapability::Draw],
            ToolCapability::FillTextFields => &[ToolCapability::FillTextFields],
            ToolCapability::AddSignature => &[ToolCapability::AddSignature],
            ToolCapability::Snapshot => &[ToolCapability::Snapshot],
            ToolCapability::DynamicZoom => &[ToolCapability::DynamicZoom],
            ToolCapability::ChoosesFile => &[ToolCapability::ChoosesFile],
            ToolCapability::Stamp => &[ToolCapability::Stamp],
            ToolCapability::EditPages => &[ToolCapability::EditPages],
        }
    }

    impl PluginManifest for EmptyManifest {
        fn id(&self) -> &'static str {
            "empty"
        }

        fn name(&self) -> &'static str {
            "Empty"
        }

        fn register(&self, _registry: &mut PluginRegistry) {}
    }

    #[test]
    fn default_slots_and_missing_reasons_name_their_delivery_milestones() {
        let entries = QuickActionsState::default().entries(&PluginRegistry::new(), None);

        assert_eq!(
            entries.iter().map(|entry| entry.action).collect::<Vec<_>>(),
            QuickAction::ALL
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.availability.unavailable_stage())
                .collect::<Vec<_>>(),
            vec![
                Some(DeliveryStage::P10),
                Some(DeliveryStage::M3),
                Some(DeliveryStage::M3),
                Some(DeliveryStage::M3),
                Some(DeliveryStage::M5),
                Some(DeliveryStage::M5),
            ]
        );
        for entry in entries {
            let reason = entry.availability.reason().unwrap();
            assert!(reason.contains(entry.action.unavailable_stage().label()));
        }
    }

    /// A quick action goes live through the registry, with no
    /// application-code change: a tool declares the capability and the toolbar
    /// finds it. The rest wait for the plugins that own them.
    ///
    /// The expected set is derived per feature rather than written out once:
    /// `tools-basic`'s text selection lights Select, and M3's comment tools
    /// light Comment and Highlight as they land. A fixed list here would have
    /// to be edited by every tool package, and the edit that matters - a
    /// capability that stops being reachable - looks exactly like the edit that
    /// does not.
    #[test]
    fn the_registry_makes_exactly_the_declared_quick_actions_live() {
        let registry = crate::build_registry();
        let entries = QuickActionsState::default().entries(&registry, None);

        let live = entries
            .iter()
            .filter(|entry| entry.availability.is_enabled())
            .map(|entry| entry.action)
            .collect::<Vec<_>>();
        let expected = entries
            .iter()
            .map(|entry| entry.action)
            .filter(|action| {
                registry
                    .tools()
                    .any(|tool| tool.capabilities().contains(&action.capability()))
            })
            .collect::<Vec<_>>();
        assert_eq!(live, expected);
        assert!(
            expected.contains(&QuickAction::Select) == cfg!(feature = "tools-basic"),
            "tools-basic is what lights Select"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.action == QuickAction::Select)
                .and_then(|entry| entry.availability.tool_index()),
            registry
                .tools()
                .position(|tool| tool.capabilities().contains(&ToolCapability::Select)),
        );
    }

    /// P1b's editing gate on the toolbar: on a document that may not be
    /// edited, every action whose tool edits is refused with the document's
    /// reason - not a delivery stage, because the tool was delivered - and the
    /// ones that do not edit are untouched.
    #[test]
    fn a_document_that_may_not_be_edited_refuses_exactly_the_editing_actions() {
        let registry = crate::build_registry();
        let reason = "Encrypted document: editing arrives in M6";
        let open = QuickActionsState::default().all_entries(&registry, None);
        let locked = QuickActionsState::default().all_entries(&registry, Some(reason));

        for (free, gated) in open.iter().zip(&locked) {
            if free.action.capability().edits_document() {
                assert_eq!(
                    gated.availability,
                    QuickActionAvailability::Refused { reason },
                    "{:?} edits and is not refused",
                    free.action
                );
                assert_eq!(gated.availability.reason(), Some(reason));
            } else {
                assert_eq!(gated.availability, free.availability, "{:?}", free.action);
            }
        }
    }

    #[test]
    fn each_typed_capability_enables_only_its_matching_action() {
        for action in QuickAction::ALL {
            let mut registry = PluginRegistry::new();
            registry.register_tool(Box::new(CapabilityTool {
                id: "capability",
                capabilities: single_capability(action.capability()),
            }));

            let entries = QuickActionsState::default().entries(&registry, None);
            let enabled = entries
                .iter()
                .filter(|entry| entry.availability.is_enabled())
                .collect::<Vec<_>>();

            assert_eq!(enabled.len(), 1, "{}", action.label());
            assert_eq!(enabled[0].action, action);
            assert_eq!(enabled[0].availability.tool_index(), Some(0));
        }
    }

    #[test]
    fn multiple_capabilities_enable_every_match_and_a_tool_made_for_the_action_wins() {
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(CapabilityTool {
            id: "multi",
            capabilities: &[
                ToolCapability::Select,
                ToolCapability::Draw,
                ToolCapability::AddSignature,
            ],
        }));
        registry.register_tool(Box::new(CapabilityTool {
            id: "later-draw",
            capabilities: &[ToolCapability::Draw],
        }));

        let enabled = QuickActionsState::default()
            .entries(&registry, None)
            .into_iter()
            .filter(|entry| entry.availability.is_enabled())
            .collect::<Vec<_>>();

        assert_eq!(
            enabled.iter().map(|entry| entry.action).collect::<Vec<_>>(),
            vec![
                QuickAction::Select,
                QuickAction::Draw,
                QuickAction::AddSignature,
            ]
        );
        let tool = |action| {
            enabled
                .iter()
                .find(|entry| entry.action == action)
                .and_then(|entry| entry.availability.tool_index())
        };
        assert_eq!(
            tool(QuickAction::Select),
            Some(0),
            "multi is made for Select"
        );
        assert_eq!(
            tool(QuickAction::Draw),
            Some(1),
            "later-draw is made for drawing; multi only also draws"
        );
        assert_eq!(tool(QuickAction::AddSignature), Some(0), "the only one");
    }

    /// Acrobat's Comment quick action places a sticky note, and Highlight
    /// and Draw are the highlighter and the pencil, all live and carrying no
    /// reason. Read from the registry the app builds, not from a list.
    #[cfg(feature = "tools-comment")]
    #[test]
    fn comment_highlight_and_draw_are_live_on_the_tools_acrobat_uses() {
        let registry = crate::build_registry();
        let entries = QuickActionsState::default().entries(&registry, None);
        let tool_id = |action| {
            let entry = entries
                .iter()
                .find(|entry| entry.action == action)
                .expect("offered");
            assert!(entry.availability.is_enabled(), "{action:?}");
            assert_eq!(entry.availability.reason(), None, "{action:?}");
            let index = entry.availability.tool_index().expect("a tool");
            registry.tools().nth(index).expect("registered").id()
        };
        assert_eq!(tool_id(QuickAction::Comment), "sticky-note");
        assert_eq!(tool_id(QuickAction::Highlight), "highlight");
        assert_eq!(tool_id(QuickAction::Draw), "ink");
    }

    #[test]
    fn an_installed_manifest_without_tools_enables_nothing() {
        let mut registry = PluginRegistry::new();
        registry.install(&EmptyManifest);

        assert_eq!(registry.plugins().len(), 1);
        assert!(QuickActionsState::default()
            .entries(&registry, None)
            .iter()
            .all(|entry| !entry.availability.is_enabled()));
    }

    #[test]
    fn customization_state_contains_only_known_typed_actions() {
        let mut state = QuickActionsState::default();
        for action in QuickAction::ALL {
            state.toggle_visibility(action);
        }
        assert!(state.visible_actions().is_empty());

        for action in QuickAction::ALL {
            state.toggle_visibility(action);
        }
        assert_eq!(state.visible_actions(), QuickAction::ALL);
    }

    #[test]
    fn dragging_clamps_the_toolbar_to_every_document_edge() {
        let bounds = Bounds {
            origin: point(px(100.0), px(50.0)),
            size: size(px(500.0), px(300.0)),
        };
        let toolbar_size = size(px(200.0), px(80.0));
        let mut state = QuickActionsState::default();
        let drag = state.next_drag();
        state.drag_to(drag, point(px(150.0), px(100.0)), bounds, toolbar_size);
        state.drag_to(drag, point(px(-500.0), px(-500.0)), bounds, toolbar_size);
        assert_eq!(state.position(), point(px(0.0), px(0.0)));

        let drag = state.next_drag();
        state.drag_to(drag, point(px(100.0), px(50.0)), bounds, toolbar_size);
        state.drag_to(drag, point(px(1_000.0), px(1_000.0)), bounds, toolbar_size);
        assert_eq!(state.position(), point(px(300.0), px(220.0)));
    }

    #[test]
    fn opening_a_side_panel_reclamps_a_toolbar_at_the_closed_document_edge() {
        let mut state = QuickActionsState::default();
        let closed_document = size(px(972.0), px(784.0));
        let open_document = size(px(732.0), px(784.0));
        let toolbar = state.toolbar_size(closed_document);
        let drag = state.next_drag();
        state.drag_to(
            drag,
            point(px(0.0), px(0.0)),
            Bounds {
                origin: Point::default(),
                size: closed_document,
            },
            toolbar,
        );
        state.drag_to(
            drag,
            point(px(2_000.0), px(0.0)),
            Bounds {
                origin: Point::default(),
                size: closed_document,
            },
            toolbar,
        );
        assert_eq!(state.position().x, px(332.0));

        state.constrain_to(open_document);

        assert_eq!(state.position().x, px(92.0));
    }

    #[test]
    fn expanded_rail_and_open_panel_keep_the_toolbar_inside_the_document() {
        let document = size(px(580.0), px(736.0));
        let mut state = QuickActionsState::default();

        state.constrain_to(document);

        assert_eq!(state.toolbar_size(document).width, document.width);
        assert_eq!(state.position().x, px(0.0));
    }

    fn described(state: &QuickActionsState, registry: &PluginRegistry) -> Element {
        accessible(
            &state.entries(registry, None),
            &state.all_entries(registry, None),
            state,
        )
    }

    /// The handle is a braille-pattern glyph and the toolbar's only unlabeled
    /// control. A screen reader reading the glyph says nothing useful, so the
    /// name has to be words.
    #[test]
    fn the_drag_handle_is_announced_by_words_rather_than_by_its_braille_glyph() {
        let handle = described(&QuickActionsState::default(), &PluginRegistry::new())
            .find(&"quick-actions-drag-handle".into())
            .expect("the toolbar describes its drag handle")
            .clone();

        assert_eq!(handle.label, "Move Quick Actions");
        assert!(!handle.label.contains(DRAG_HANDLE_GLYPH));
        assert!(handle.label.chars().all(|c| c.is_alphabetic() || c == ' '));
        assert_eq!(handle.activation, None);
    }

    fn described_comment(registry: &PluginRegistry) -> Element {
        described(&QuickActionsState::default(), registry)
            .find(&("quick-action", QuickAction::Comment.index()).into())
            .expect("the toolbar describes the comment action")
            .clone()
    }

    #[test]
    fn a_quick_action_that_is_not_delivered_yet_is_disabled_and_says_why() {
        let missing = described_comment(&PluginRegistry::new());
        assert!(missing.state.disabled);
        assert_eq!(
            missing.description.as_deref(),
            Some("Available in M3 tools-comment")
        );

        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(CapabilityTool {
            id: "comment",
            capabilities: single_capability(ToolCapability::Comment),
        }));
        let live = described_comment(&registry);
        assert!(!live.state.disabled);
        assert_eq!(live.description, None);
    }

    #[test]
    fn each_described_quick_action_carries_the_action_its_click_runs() {
        let state = QuickActionsState::default();
        let registry = PluginRegistry::new();
        let entries = state.entries(&registry, None);
        let described = accessible(&entries, &state.all_entries(&registry, None), &state);

        for entry in &entries {
            let node = described
                .find(&("quick-action", entry.action.index()).into())
                .unwrap_or_else(|| panic!("{} is not in the description", entry.action.label()));
            assert_eq!(node.activation, Some(Activation::QuickAction(*entry)));
        }
    }

    /// The panel draws a tick beside each label. The tick has to be the
    /// checkbox's state, or a screen reader user hears "Comment" whether the
    /// action is in the toolbar or not.
    #[test]
    fn a_visibility_checkbox_carries_its_tick_as_state_rather_than_in_its_name() {
        let mut state = QuickActionsState::default();
        state.toggle_customizing();
        state.toggle_visibility(QuickAction::Comment);
        let registry = PluginRegistry::new();

        let described = described(&state, &registry);

        let on = described
            .find(&("quick-action-customization", QuickAction::Select.index()).into())
            .unwrap();
        let off = described
            .find(&("quick-action-customization", QuickAction::Comment.index()).into())
            .unwrap();
        assert_eq!(on.role, Role::CheckBox);
        assert_eq!(on.state.toggled, Some(true));
        assert_eq!(off.state.toggled, Some(false));
        assert_eq!(on.label, QuickAction::Select.label());
        assert_eq!(off.label, QuickAction::Comment.label());
        assert!(!on.label.contains(visibility_glyph(true)));
        assert!(!off.label.contains(visibility_glyph(false)));
        assert_eq!(
            off.activation,
            Some(Activation::ToggleQuickActionVisibility(
                QuickAction::Comment
            ))
        );
        assert_eq!(
            off.description.as_deref(),
            Some("Available in M3 tools-comment")
        );
    }

    #[test]
    fn the_customize_button_announces_what_it_draws_and_carries_the_panel_state() {
        let registry = PluginRegistry::new();
        let closed = QuickActionsState::default();
        let mut open = QuickActionsState::default();
        open.toggle_customizing();

        let closed = described(&closed, &registry)
            .find(&"quick-actions-customize".into())
            .unwrap()
            .clone();
        let open = described(&open, &registry)
            .find(&"quick-actions-customize".into())
            .unwrap()
            .clone();
        assert_eq!(closed.label, "Customize");
        assert_eq!(open.label, "Done");
        assert_eq!(closed.state.toggled, Some(false));
        assert_eq!(open.state.toggled, Some(true));
        assert_eq!(
            closed.activation,
            Some(Activation::ToggleQuickActionCustomization)
        );
    }

    /// Both halves of "one list drives both": the row renders the handle,
    /// then one button per visible action, then the customize button, and the
    /// description has to match that count and order for the prepainted
    /// rectangles to line up. The customization rows come after, because the
    /// panel paints them below the row.
    #[test]
    fn the_description_has_one_node_per_rendered_row_child_in_row_order() {
        let registry = PluginRegistry::new();
        let mut state = QuickActionsState::default();
        state.toggle_visibility(QuickAction::Draw);

        let row_only = described(&state, &registry);
        let visible = state.entries(&registry, None);

        assert_eq!(visible.len(), QuickAction::ALL.len() - 1);
        assert_eq!(row_only.children.len(), visible.len() + 2);
        let mut expected: Vec<gpui::ElementId> = vec!["quick-actions-drag-handle".into()];
        expected.extend(
            visible
                .iter()
                .map(|entry| ("quick-action", entry.action.index()).into()),
        );
        expected.push("quick-actions-customize".into());
        assert_eq!(
            row_only
                .children
                .iter()
                .map(|child| child.key.clone())
                .collect::<Vec<_>>(),
            expected
        );

        state.toggle_customizing();
        let with_panel = described(&state, &registry);
        assert_eq!(
            with_panel.children.len(),
            row_only.children.len() + QuickAction::ALL.len()
        );
        assert_eq!(
            with_panel.children[..row_only.children.len()]
                .iter()
                .map(|child| child.key.clone())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[cfg(feature = "shell-test-support")]
    struct QuickActionsLayoutProbe {
        scroller: ScrollHandle,
        row: ScrollHandle,
    }

    #[cfg(feature = "shell-test-support")]
    impl Render for QuickActionsLayoutProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let mut row = quick_actions_row()
                .id("quick-actions-probe-row")
                .track_scroll(&self.row)
                .child(div().flex_none().w(px(24.0)).h(px(52.0)));
            for _ in QuickAction::ALL {
                row = row.child(div().flex_none().w(px(82.0)).h(px(52.0)));
            }
            row = row.child(div().flex_none().w(px(76.0)).h(px(52.0)));

            div()
                .w(px(580.0))
                .h(px(TOOLBAR_HEIGHT))
                .child(toolbar_row_scroller(row).track_scroll(&self.scroller))
        }
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn narrow_toolbar_scrolls_until_the_last_control_is_reachable(cx: &mut TestAppContext) {
        let scroller = ScrollHandle::new();
        let row = ScrollHandle::new();
        let (_, cx) = cx.add_window_view(|_window, _cx| QuickActionsLayoutProbe {
            scroller: scroller.clone(),
            row: row.clone(),
        });
        cx.run_until_parked();

        let viewport = scroller.bounds();
        let first = row.bounds_for_item(0).unwrap();
        let last = row.bounds_for_item(7).unwrap();
        let max_scroll = scroller.max_offset().width;

        assert_eq!(viewport.size.width, px(580.0));
        assert!(max_scroll > px(0.0));
        assert!(first.left() >= viewport.left());
        assert!(last.left() - max_scroll < viewport.right());
        assert!(last.right() - max_scroll <= viewport.right());
        assert!(last.right() - max_scroll > viewport.left());
    }
}
