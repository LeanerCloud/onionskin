use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, point, px, size, AppContext as _, Bounds, Context, Div, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use onionskin_plugin_api::{PluginRegistry, ToolCapability};

use super::tabs::ShellFrame;
use super::theme::ThemeTokens;

const PREFERRED_TOOLBAR_WIDTH: f32 = 640.0;
const TOOLBAR_HEIGHT: f32 = 64.0;
const CUSTOMIZATION_HEIGHT: f32 = 296.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickAction {
    Select,
    Comment,
    Highlight,
    Draw,
    FillTextFields,
    AddSignature,
}

impl QuickAction {
    pub(super) const ALL: [Self; 6] = [
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
    Enabled { tool_index: usize },
    Disabled { stage: DeliveryStage },
}

impl QuickActionAvailability {
    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    pub(super) fn tool_index(self) -> Option<usize> {
        match self {
            Self::Enabled { tool_index } => Some(tool_index),
            Self::Disabled { .. } => None,
        }
    }

    pub(super) fn reason(self) -> Option<&'static str> {
        match self {
            Self::Enabled { .. } => None,
            Self::Disabled { stage } => Some(stage.reason()),
        }
    }

    fn unavailable_stage(self) -> Option<DeliveryStage> {
        match self {
            Self::Enabled { .. } => None,
            Self::Disabled { stage } => Some(stage),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QuickActionEntry {
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
    pub(super) fn entries(&self, registry: &PluginRegistry) -> Vec<QuickActionEntry> {
        self.visible_actions()
            .into_iter()
            .map(|action| quick_action_entry(action, registry))
            .collect()
    }

    pub(super) fn all_entries(&self, registry: &PluginRegistry) -> Vec<QuickActionEntry> {
        QuickAction::ALL
            .into_iter()
            .map(|action| quick_action_entry(action, registry))
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

fn quick_action_entry(action: QuickAction, registry: &PluginRegistry) -> QuickActionEntry {
    let availability = registry
        .tools()
        .enumerate()
        .find(|(_, tool)| tool.capabilities().contains(&action.capability()))
        .map_or(
            QuickActionAvailability::Disabled {
                stage: action.unavailable_stage(),
            },
            |(tool_index, _)| QuickActionAvailability::Enabled { tool_index },
        );
    QuickActionEntry {
        action,
        availability,
    }
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

pub(super) fn render_quick_actions(
    entries: Vec<QuickActionEntry>,
    all_entries: Vec<QuickActionEntry>,
    state: &QuickActionsState,
    document: Size<Pixels>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let position = state.position();
    let toolbar_size = state.toolbar_size(document);
    let customizing = state.customizing();
    let drag = QuickActionDrag {
        id: state.next_drag(),
    };
    let mut row = quick_actions_row().child(
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
            .child("⠿"),
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
                .on_click(cx.listener(move |frame, _event, _window, cx| {
                    if enabled {
                        frame.select_quick_action(entry, cx);
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
            .on_click(cx.listener(|frame, _event, _window, cx| {
                frame.toggle_quick_action_customization(cx);
            }))
            .text_xs()
            .child(if customizing { "Done" } else { "Customize" }),
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
                    .on_click(cx.listener(move |frame, _event, _window, cx| {
                        frame.toggle_quick_action_visibility(action, cx);
                    }))
                    .child(if visible { "✓" } else { "○" })
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
        let entries = QuickActionsState::default().entries(&PluginRegistry::new());

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

    /// The Select quick action goes live through the registry, with no
    /// application-code change: `tools-basic`'s text-selection tool declares
    /// the capability and the toolbar finds it. The rest wait for the
    /// plugins that own them.
    #[test]
    fn the_registry_makes_exactly_the_select_quick_action_live() {
        let registry = crate::build_registry();
        let entries = QuickActionsState::default().entries(&registry);

        let live = entries
            .iter()
            .filter(|entry| entry.availability.is_enabled())
            .map(|entry| entry.action)
            .collect::<Vec<_>>();
        let expected = if cfg!(feature = "tools-basic") {
            vec![QuickAction::Select]
        } else {
            Vec::new()
        };
        assert_eq!(live, expected);
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

    #[test]
    fn each_typed_capability_enables_only_its_matching_action() {
        for action in QuickAction::ALL {
            let mut registry = PluginRegistry::new();
            registry.register_tool(Box::new(CapabilityTool {
                id: "capability",
                capabilities: single_capability(action.capability()),
            }));

            let entries = QuickActionsState::default().entries(&registry);
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
    fn multiple_capabilities_enable_every_match_and_the_first_matching_tool_wins() {
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
            .entries(&registry)
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
        assert!(enabled
            .iter()
            .all(|entry| entry.availability.tool_index() == Some(0)));
    }

    #[test]
    fn an_installed_manifest_without_tools_enables_nothing() {
        let mut registry = PluginRegistry::new();
        registry.install(&EmptyManifest);

        assert_eq!(registry.plugins().len(), 1);
        assert!(QuickActionsState::default()
            .entries(&registry)
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
