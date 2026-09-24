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
//! [`CodecPlugin`] joined them in M2 with `codecs-common`, export only:
//! pages out to text, PNG and SVG. Import waits for the edit graph that can
//! build a `Document` from something that is not a PDF, which is M3.

pub use codec::{
    CodecPlugin, ExportError, ExportOutputKind, ExportRequest, ImportError, PageRange,
};
/// Re-exported so a plugin crate needs this one dependency to name what the
/// contract hands it: `Document` and `Viewport` for every trait here, the
/// page-render types for what a codec gets back when it asks for a page.
pub use onionskin_core::{
    BaseRaster, Document, Modifiers, PageIndex, PagePoint, PageQuad, PageRect, PageRender, PageSvg,
    TextSelection, Viewport,
};
pub use registry::{PluginEntry, PluginManifest, PluginRegistry};
pub use requirement::{tool_with, Availability, Requirement, Session};

pub mod codec;
pub mod command_ids;
pub mod contract;
pub mod marquee;
pub mod registry;
pub mod requirement;

/// A pointer event, already transformed out of window space into the page
/// it landed on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerInput {
    pub at: PagePoint,
    /// 0.0..=1.0; a mouse reports 1.0.
    pub pressure: f32,
    pub modifiers: Modifiers,
    /// Which click of a run this press is: 1 for a single click, 2 for a
    /// double click. What ends a shape built by clicking, the way Acrobat's
    /// Polygon and Connected Lines end on a double click. 1 on a move or a
    /// release.
    pub clicks: u8,
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
    /// A polyline: an ink stroke in progress, a measured path, a polygon.
    ///
    /// `closed` joins the last point back to the first. Without it the preview
    /// of a polygon or a cloud is missing its closing edge, which is the one
    /// edge that tells a user they have closed the shape.
    Polyline {
        points: Vec<PagePoint>,
        closed: bool,
    },
    /// Straight segment, e.g. the distance tool's live measurement.
    Line { from: PagePoint, to: PagePoint },
    /// Ellipse inscribed in `bounds`.
    ///
    /// A rectangle rather than a centre and a radius, because the Oval tool
    /// inscribes an ellipse in a dragged rectangle and a centre-and-radius
    /// circle cannot express one: the preview would be a circle and the commit
    /// an ellipse, which is exactly the preview-does-not-match-the-commit class
    /// the preview buffer exists to abolish.
    Ellipse { bounds: PageRect },
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
    /// Copies a page region as an image. The canvas context menu's Take A
    /// Snapshot entry finds its tool through this rather than by id.
    Snapshot,
    /// Zooms continuously while the pointer is dragged. The View menu's
    /// Dynamic Zoom entry selects its tool through this rather than by id.
    DynamicZoom,
    /// Places a file the user picks. A tool cannot open a file dialog, so the
    /// shell asks for a file when the tool is chosen and hands its path to
    /// [`ToolPlugin::choose`]. Attach File is the one tool that needs it.
    ChoosesFile,
    /// Places stamps. The Stamps dialog lists its choices and the Paste
    /// Clipboard Image as Stamp entry chooses one for it.
    Stamp,
    /// Changes the pages themselves, such as their boxes: the Crop Pages
    /// tool.
    EditPages,
    /// Makes and changes links. The canvas context menu's Create Link finds
    /// its tool through this.
    Link,
    /// Marks text and regions for redaction. The canvas context menu's
    /// Redact Text finds its tool through this.
    Redact,
    /// Adds form fields and changes them: Prepare Form's tools.
    PrepareForm,
    /// Selects the images a page draws to move, resize, turn, flip,
    /// replace, save or delete them. The Edit menu's image entries find
    /// their tool through this.
    EditImages,
    /// Places a picture the user picks, handed over as a one-page PDF: the
    /// shell makes one from an image file before [`ToolPlugin::choose`].
    PlacesImage,
    /// Edits a line of text where it is: the shell opens an editor on the
    /// line the tool asks for.
    EditText,
    /// Measures distances, perimeters and areas. Measuring reads the page;
    /// a measurement kept as a comment is written as one, and refused where
    /// comments are.
    Measure,
}

impl ToolCapability {
    /// Whether a tool with this capability changes the document.
    ///
    /// Stated per capability rather than per tool, so a new tool inherits the
    /// answer by declaring what it does - and so a document that may not be
    /// edited disables every such tool through the requirement query, without
    /// the shell keeping a list of which tools write.
    pub fn edits_document(self) -> bool {
        match self {
            ToolCapability::Comment
            | ToolCapability::Highlight
            | ToolCapability::Draw
            | ToolCapability::FillTextFields
            | ToolCapability::AddSignature
            | ToolCapability::Stamp
            | ToolCapability::EditPages
            | ToolCapability::Link
            | ToolCapability::Redact
            | ToolCapability::PrepareForm
            | ToolCapability::EditImages
            | ToolCapability::PlacesImage
            | ToolCapability::EditText => true,
            ToolCapability::Select
            | ToolCapability::Snapshot
            | ToolCapability::DynamicZoom
            | ToolCapability::ChoosesFile
            | ToolCapability::Measure => false,
        }
    }
}

/// Everything a tool may touch while handling input.
///
/// The viewport is here because pan and zoom are view state, not document
/// state: without it a hand tool cannot pan and a zoom tool cannot zoom.
/// Its named consumers are `tools-basic`'s hand and zoom tools; a tool that
/// only marks up the document has no reason to touch it.
pub struct ToolCtx<'a> {
    pub doc: &'a mut Document,
    pub viewport: &'a mut Viewport,
}

/// One thing a tool can be set to place: a stamp from its library, say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolChoice {
    /// What [`ToolPlugin::choose`] takes.
    pub id: String,
    pub label: String,
    /// The heading it is listed under, e.g. "Sign Here".
    pub category: String,
}

/// One line of what a tool is reading off the page, for the side panel:
/// a measurement as it is being made, say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub label: &'static str,
    pub value: String,
}

impl Reading {
    pub fn new(label: &'static str, value: impl Into<String>) -> Self {
        Reading {
            label,
            value: value.into(),
        }
    }
}

/// What the shell hands every tool before it is used: the user's settings a
/// tool may need, and a place of its own on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolEnvironment {
    /// The name the user chose to comment as. `None` until they chose one:
    /// a tool never falls back to the operating system's account name.
    pub author: Option<String>,
    /// A directory the tool may keep its own files in - a stamp library - or
    /// `None` when the shell has nowhere to put one.
    pub data_dir: Option<std::path::PathBuf>,
    /// What the user made the default look for a kind of comment, keyed by
    /// its `/Subtype` ("Square", "Highlight"): Acrobat's "Make Current
    /// Properties Default". A kind with no entry keeps the tool's own look.
    pub comment_defaults: std::collections::BTreeMap<String, CommentDefault>,
    /// The look new redaction marks take, as Redaction Properties last set
    /// it; `None` for the redaction tool's own default.
    pub redaction: Option<RedactionDefault>,
    /// Preferences > JavaScript has Acrobat JavaScript turned off, so a
    /// form's scripts do not run when it is filled. Off by default, which
    /// is scripts running.
    pub javascript_off: bool,
}

/// How a redaction mark looks, as a preference: whole numbers, so the file
/// round-trips exactly, as [`CommentDefault`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionDefault {
    /// The area's fill once applied; `None` leaves it empty.
    pub fill: Option<[u8; 3]>,
    /// The mark's outline before it is applied.
    pub outline: [u8; 3],
    pub overlay: Option<OverlayDefault>,
}

/// Text written over a redacted area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayDefault {
    pub text: String,
    /// In tenths of a point; `0` fits the text to the area.
    pub size_tenths: u32,
    pub color: [u8; 3],
    /// `0` left, `1` centre, `2` right: `/Q`.
    pub align: u8,
    pub repeat: bool,
}

/// One kind of comment's default look. Whole numbers, so a preference file
/// round-trips exactly: colour channels 0 to 255 and opacity in percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentDefault {
    pub color: Option<[u8; 3]>,
    pub opacity_percent: u8,
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

    /// One sentence saying how the tool is used, for the side panel: what to
    /// click or drag, and anything the tool cannot do yet. `None` leaves the
    /// panel with the tool's name alone.
    fn hint(&self) -> Option<&'static str> {
        None
    }

    /// Whether what this tool places is written in: a note, a text box, a
    /// caret. The shell then opens a text field on the comment it just
    /// placed, where Acrobat opens its pop-up note or puts the cursor in the
    /// box, so the text is typed where the comment is.
    fn takes_text(&self) -> bool {
        false
    }

    /// Whether this tool answers Edit > Cut, Copy, Paste or Delete. The menu
    /// entries are live only for what the active tool claims, and disabled
    /// with a reason naming the tool otherwise, so the shell never guesses
    /// what "Delete" means for a tool.
    fn claims(&self, _verb: EditVerb) -> bool {
        false
    }

    /// Run one Edit verb this tool claims. `pasted` is the clipboard's text
    /// for Paste. Returns the text the shell puts on the clipboard, for Cut
    /// and Copy; `None` when there is nothing to put there.
    fn edit(
        &mut self,
        _ctx: &mut ToolCtx,
        _verb: EditVerb,
        _pasted: Option<&str>,
    ) -> Option<String> {
        None
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

    /// The user's settings and the tool's own directory. Called before the
    /// tool is used and again whenever they change.
    fn configure(&mut self, _environment: &ToolEnvironment) {}

    /// What the tool can be set to place, for the shell to list. Empty for a
    /// tool with nothing to choose between.
    fn choices(&self) -> Vec<ToolChoice> {
        Vec::new()
    }

    /// Set what the tool places next: one of [`Self::choices`], or a file's
    /// path for a [`ToolCapability::ChoosesFile`] tool. `false` when the tool
    /// has no such choice, and then nothing changed.
    fn choose(&mut self, _id: &str) -> bool {
        false
    }

    /// The choice in effect, if the tool has one.
    fn chosen(&self) -> Option<String> {
        None
    }

    /// The tool's settings, for the side panel to list under its name: each
    /// shown on or off by [`Self::picked`] and changed by [`Self::choose`].
    /// Empty for a tool with none.
    fn settings(&self) -> Vec<ToolChoice> {
        Vec::new()
    }

    /// Whether choice or setting `id` is in effect. A tool with settings,
    /// several on at once, answers for each; by default it is the one
    /// [`Self::chosen`] names.
    fn picked(&self, id: &str) -> bool {
        self.chosen().as_deref() == Some(id)
    }

    /// What the tool is reading off the page now, for the side panel to show
    /// under its name. Empty for a tool that reads nothing.
    fn readings(&self) -> Vec<Reading> {
        Vec::new()
    }
}

/// Context handed to commands (menu items and keybound actions).
///
/// `page` is the page the viewport is on, which is what "the current page"
/// means to Acrobat's Edit menu. The viewport itself is not here because no
/// command changes view state: panning and zooming are tools, and they have
/// [`ToolCtx`].
pub struct CommandCtx<'a> {
    pub doc: &'a mut Document,
    pub page: PageIndex,
}

/// Why a command could not do what it was asked.
///
/// A command reports instead of returning nothing because the work behind
/// one fails on real documents: a page whose text will not extract makes
/// Select All select nothing, and a caller that cannot tell that apart from
/// a page with no text has nothing to put on the status line.
#[derive(Debug)]
pub enum CommandError {
    /// A page the command needed could not be read.
    Page {
        page: PageIndex,
        source: onionskin_core::Error,
    },
    /// The edit the command is named for was refused or failed, and nothing
    /// was changed: `label` is the undo label it would have had.
    Edit {
        label: &'static str,
        source: onionskin_core::Error,
    },
    /// A command that writes files could not: `label` names the command and
    /// `reason` says what went wrong, in words for the notice bar.
    Failed { label: &'static str, reason: String },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Page { page, source } => write!(f, "page {}: {source}", page + 1),
            Self::Edit { label, source } => write!(f, "{label}: {source}"),
            Self::Failed { label, reason } => write!(f, "{label}: {reason}"),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Page { source, .. } | Self::Edit { source, .. } => Some(source),
            Self::Failed { .. } => None,
        }
    }
}

/// What a command does when it is run.
pub type CommandBody = Box<dyn Fn(&mut CommandCtx) -> Result<(), CommandError> + Send>;

/// What running a command does to, or with, the open document.
///
/// Declared once per command, like a tool's [`ToolCapability`], so a document
/// that refuses something disables every command that would do it through the
/// one requirement query - rather than through a list in the shell of which
/// commands happen to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    /// Changes nothing in the document: a selection, a view.
    Reads,
    /// Changes the document, so it is refused where editing is.
    Edits,
    /// Copies the document's objects out into another file, so it is refused
    /// where the encrypted-source rule refuses reading out.
    ReadsOut,
}

/// Edit > Cut, Copy, Paste and Delete, which mean what the active tool says
/// they mean: text for the text tool, a comment for a comment tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditVerb {
    Cut,
    Copy,
    Paste,
    Delete,
}

impl EditVerb {
    pub const ALL: [Self; 4] = [Self::Cut, Self::Copy, Self::Paste, Self::Delete];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Delete => "Delete",
        }
    }
}

/// A named, keybindable command. `id` is namespaced like "pages.rotate".
pub struct Command {
    pub id: &'static str,
    pub title: &'static str,
    /// Default keybinding in GPUI keystroke syntax, e.g. "cmd-shift-r"
    /// ("cmd" is mapped to ctrl on Linux and Windows by the app shell).
    pub keybind: Option<&'static str>,
    pub effect: CommandEffect,
    pub run: CommandBody,
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

    /// One-based, like every other page number a user reads. A command's
    /// failure lands on the canvas status line beside the export failures,
    /// which already count from one.
    #[test]
    fn a_command_failure_names_the_page_the_way_the_user_counts_pages() {
        let error = CommandError::Page {
            page: 4,
            source: onionskin_core::Error::NoSuchPage { page: 4, count: 2 },
        };

        assert!(
            error.to_string().starts_with("page 5: "),
            "{}",
            error.to_string()
        );
    }

    #[test]
    fn a_tool_reads_nothing_and_picks_only_what_it_chose_by_default() {
        let tool: &dyn ToolPlugin = &LegacyTool;
        assert!(tool.readings().is_empty());
        assert!(tool.settings().is_empty());
        assert!(!tool.picked("anything"));
        assert_eq!(
            Reading::new("Distance", "2 in"),
            Reading {
                label: "Distance",
                value: "2 in".to_owned()
            }
        );
        assert!(!ToolCapability::Measure.edits_document());
    }

    #[test]
    fn existing_tool_implementations_default_to_no_capabilities() {
        let tool: &dyn ToolPlugin = &LegacyTool;

        assert!(tool.capabilities().is_empty());
    }

    #[test]
    fn a_tool_with_nothing_to_choose_refuses_every_choice() {
        let mut tool = LegacyTool;
        tool.configure(&ToolEnvironment::default());
        assert!(tool.choices().is_empty());
        assert!(!tool.choose("anything"));
        assert_eq!(tool.chosen(), None);
        assert!(!ToolCapability::ChoosesFile.edits_document());
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
