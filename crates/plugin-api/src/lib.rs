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

pub use registry::{PluginEntry, PluginManifest, PluginRegistry};

pub mod registry;

/// Zero-based index into the document's page tree.
pub type PageIndex = usize;

/// Keyboard modifiers accompanying a pointer event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl_or_cmd: bool,
}

/// A point in a page's default user space: origin at the lower-left
/// corner, y increasing upwards, units of 1/72 inch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePoint {
    pub page: PageIndex,
    pub x: f64,
    pub y: f64,
}

/// An axis-aligned rectangle in a page's user space, given in the corner
/// order PDF itself uses for `/Rect`: lower-left, then upper-right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRect {
    pub page: PageIndex,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Four corners in a page's user space, in `/QuadPoints` order: upper-left,
/// upper-right, lower-left, lower-right. Text selection needs a quad rather
/// than a rect because a text run is not axis-aligned once it is rotated.
///
/// That corner order is deliberate and is not an error to correct: ISO
/// 32000-1 12.5.6.10 describes the four points counterclockwise, but every
/// producer follows Acrobat, which writes them in the Z order above, and
/// every consumer reads them that way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageQuad {
    pub page: PageIndex,
    pub corners: [(f64, f64); 4],
}

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
