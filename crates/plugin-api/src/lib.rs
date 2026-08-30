//! The plugin contract surface.
//!
//! Every user-facing feature implements one of these traits and registers
//! through a `PluginManifest`. First-party plugins are workspace crates
//! compiled in; the same conceptual API is projected to sandboxed WASM
//! plugins post-1.0. The surface is deliberately GPUI-free: pointer input
//! arrives already resolved to a page and that page's user space, and
//! tools hand back `Overlay` primitives for the canvas to draw. `app`
//! translates in both directions, which is what makes every tool
//! unit-testable headless and confines a framework swap to one crate.
//!
//! `CodecPlugin` joins these in M2, with `codecs-common`: its shape is
//! `Document` in and out, so it waits for a `Document` worth naming.

use onionskin_core::Document;

pub use onionskin_core::{Modifiers, PageIndex, PagePoint, PageQuad, PageRect};
pub use registry::{PluginEntry, PluginManifest, PluginRegistry};

pub mod registry;

/// A pointer event, already transformed out of window space into the page
/// it landed on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerInput {
    pub at: PagePoint,
    /// 0.0..=1.0; a mouse reports 1.0.
    pub pressure: f32,
    pub modifiers: Modifiers,
}

/// Overlay primitives a tool asks the canvas to draw: selection ants,
/// annotation handles, an ink stroke in progress. Coordinates are page
/// space; `app` maps them to the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    /// Dashed "marching ants" rectangle, e.g. a region selection.
    AntsRect(PageRect),
    /// Solid outline rectangle, e.g. the bounds of a selected annotation.
    Rect(PageRect),
    /// Filled translucent quads: a text selection, or a highlight preview.
    Quads(Vec<PageQuad>),
    /// Open polyline: an ink stroke in progress, a measured path.
    Polyline(Vec<PagePoint>),
    /// Straight segment, e.g. the distance tool's live measurement.
    Line { from: PagePoint, to: PagePoint },
    /// Circle outline, e.g. a search-hit marker. Radius is in page units.
    Circle { center: PagePoint, radius: f64 },
}

/// Features a tool contributes to shared shell surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCapability {
    Select,
    Comment,
    Highlight,
    Draw,
    FillTextFields,
    AddSignature,
}

/// Everything a tool may touch while handling input.
pub struct ToolCtx<'a> {
    pub doc: &'a mut Document,
}

/// A canvas tool. One is active at a time; the canvas routes pointer
/// events to it.
pub trait ToolPlugin: Send {
    /// Stable identifier, e.g. "highlight".
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Icon asset name (the app shell renders `icons/<name>.svg`).
    fn icon(&self) -> &'static str;
    /// Default activation key, e.g. "h". Registered into the keymap.
    fn shortcut(&self) -> Option<&'static str> {
        None
    }

    /// Tools sharing a group id occupy one slot on the tool rail, the way
    /// Acrobat groups a toolset's variants: the slot shows whichever was
    /// last used and a flyout lists the rest. Defaults to the tool's own
    /// id, i.e. a group of one.
    fn group(&self) -> &'static str {
        self.id()
    }

    /// False for tools reachable only by command or shortcut, which should
    /// not take a rail slot.
    fn in_rail(&self) -> bool {
        true
    }

    /// Typed features this tool exposes to shared shell surfaces.
    fn capabilities(&self) -> &'static [ToolCapability] {
        &[]
    }

    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, input: PointerInput);
    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, input: PointerInput);
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput);

    /// The user switched to this tool. Modal tools start their session here.
    fn on_activate(&mut self, _ctx: &mut ToolCtx) {}
    /// Enter pressed: commit whatever the tool has pending.
    fn on_commit(&mut self, _ctx: &mut ToolCtx) {}
    /// Escape pressed while the tool is mid-gesture.
    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {}
    /// The user switched away from this tool.
    fn on_deactivate(&mut self, _ctx: &mut ToolCtx) {}

    /// Shapes to draw this frame, over the rendered pages.
    fn overlays(&self, _doc: &Document) -> Vec<Overlay> {
        Vec::new()
    }
}

/// Context handed to commands (menu items and keybound actions).
pub struct CommandCtx<'a> {
    pub doc: &'a mut Document,
}

/// A named, keybindable command. `id` is namespaced like "pages.rotate".
pub struct Command {
    pub id: &'static str,
    pub title: &'static str,
    /// Default keybinding in GPUI keystroke syntax, e.g. "cmd-shift-r"
    /// ("cmd" is mapped to ctrl on Linux and Windows by the app shell).
    pub keybind: Option<&'static str>,
    pub run: Box<dyn Fn(&mut CommandCtx) + Send>,
}

/// A bag of commands contributed by a plugin.
pub trait CommandPlugin: Send {
    fn commands(&self) -> Vec<Command>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LegacyTool;

    impl ToolPlugin for LegacyTool {
        fn id(&self) -> &'static str {
            "legacy"
        }

        fn name(&self) -> &'static str {
            "Legacy"
        }

        fn icon(&self) -> &'static str {
            "legacy"
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    }

    struct QuickActionTool;

    impl ToolPlugin for QuickActionTool {
        fn id(&self) -> &'static str {
            "quick-actions"
        }

        fn name(&self) -> &'static str {
            "Quick Actions"
        }

        fn icon(&self) -> &'static str {
            "quick-actions"
        }

        fn capabilities(&self) -> &'static [ToolCapability] {
            &[
                ToolCapability::Select,
                ToolCapability::Comment,
                ToolCapability::Highlight,
                ToolCapability::Draw,
                ToolCapability::FillTextFields,
                ToolCapability::AddSignature,
            ]
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    }

    #[test]
    fn existing_tool_implementations_default_to_no_capabilities() {
        let tool: &dyn ToolPlugin = &LegacyTool;

        assert!(tool.capabilities().is_empty());
    }

    #[test]
    fn tools_expose_typed_quick_action_capabilities() {
        let tool: &dyn ToolPlugin = &QuickActionTool;

        assert_eq!(
            tool.capabilities(),
            &[
                ToolCapability::Select,
                ToolCapability::Comment,
                ToolCapability::Highlight,
                ToolCapability::Draw,
                ToolCapability::FillTextFields,
                ToolCapability::AddSignature,
            ]
        );
    }
}
