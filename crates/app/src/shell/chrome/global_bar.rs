use gpui::{Action, App, Menu, MenuItem, WindowHandle};
use onionskin_core::{FitMode, PageLayoutMode};
use onionskin_plugin_api::{CommandEffect, PluginRegistry, ToolCapability};

use super::quick_actions::QuickAction;
use super::theme::{ShellViewAction, ShellViewState};
use super::ShellFrame;
use crate::preferences::{layout_label, ThemePreference};
use crate::shell::canvas::{CanvasViewState, ViewAction};
use crate::shell::context_menu::tool_with;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuSectionId {
    File,
    Edit,
    View,
    Window,
    Help,
    /// The global bar's Convert entry point, a panel of its own rather than
    /// a section of the main menu.
    Convert,
    /// File > Save as Other, a panel of its own listing the formats this
    /// build exports to.
    SaveAsOther,
}

impl MenuSectionId {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Window => "Window",
            Self::Help => "Help",
            Self::Convert => "Convert",
            Self::SaveAsOther => "Save as Other",
        }
    }
}

/// The export formats the File menu offers, each backed by a codec the
/// registry may or may not have: `codecs-common` can be compiled out, and
/// then the entry ships disabled with that as its reason rather than
/// silently missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ExportTarget {
    Text,
    Png,
    Svg,
    Jpeg,
    Tiff,
}

impl ExportTarget {
    pub(super) const ALL: [ExportTarget; 5] = [
        ExportTarget::Text,
        ExportTarget::Png,
        ExportTarget::Svg,
        ExportTarget::Jpeg,
        ExportTarget::Tiff,
    ];

    /// The codec id this entry runs, as `codecs-common` registers it.
    pub(super) fn codec(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Png => "png",
            Self::Svg => "svg",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Text => 0,
            Self::Png => 1,
            Self::Svg => 2,
            Self::Jpeg => 3,
            Self::Tiff => 4,
        }
    }

    /// Whether the export is pixels, which a resolution sizes.
    pub(in crate::shell) fn is_raster(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Tiff)
    }

    /// Whether the export trades quality for size, which the user chooses.
    pub(in crate::shell) fn is_lossy(self) -> bool {
        self == Self::Jpeg
    }

    /// Acrobat words these "Export To ..."; the text entry says what it does
    /// and does not, because document order is not reading order.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Text => "Export To Plain Text (Document Order)…",
            Self::Png => "Export Pages To PNG…",
            Self::Svg => "Export Pages To SVG…",
            Self::Jpeg => "Export Pages To JPEG…",
            Self::Tiff => "Export Pages To TIFF…",
        }
    }
}

/// Which export codecs this build installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) struct ExportCodecs([bool; ExportTarget::ALL.len()]);

impl ExportCodecs {
    pub(in crate::shell) fn installed(has_codec: impl Fn(&'static str) -> bool) -> Self {
        Self(ExportTarget::ALL.map(|target| has_codec(target.codec())))
    }

    fn has(self, target: ExportTarget) -> bool {
        self.0[target.index()]
    }

    fn any(self) -> bool {
        self.0.contains(&true)
    }
}

/// The page-organization commands `tools-organize` registers, each acting on
/// the page the viewport is on. The menu carries only their ids: the plugin
/// is optional, and a build without it shows the entries saying so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PageCommand {
    RotateClockwise,
    RotateCounterclockwise,
    InsertBlank,
    MoveEarlier,
    MoveLater,
    Delete,
    ResetNumbering,
}

impl PageCommand {
    pub(in crate::shell) const ALL: [Self; 7] = [
        Self::RotateClockwise,
        Self::RotateCounterclockwise,
        Self::InsertBlank,
        Self::MoveEarlier,
        Self::MoveLater,
        Self::Delete,
        Self::ResetNumbering,
    ];

    pub(in crate::shell) fn id(self) -> &'static str {
        use onionskin_plugin_api::command_ids as ids;
        match self {
            Self::RotateClockwise => ids::ROTATE_PAGE_CLOCKWISE,
            Self::RotateCounterclockwise => ids::ROTATE_PAGE_COUNTERCLOCKWISE,
            Self::InsertBlank => ids::INSERT_BLANK_PAGE,
            Self::MoveEarlier => ids::MOVE_PAGE_EARLIER,
            Self::MoveLater => ids::MOVE_PAGE_LATER,
            Self::Delete => ids::DELETE_PAGE,
            Self::ResetNumbering => ids::RESET_PAGE_NUMBERING,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::RotateClockwise => "Rotate Page Clockwise",
            Self::RotateCounterclockwise => "Rotate Page Counterclockwise",
            Self::InsertBlank => "Insert Blank Page",
            Self::MoveEarlier => "Move Page Earlier",
            Self::MoveLater => "Move Page Later",
            Self::Delete => "Delete Page",
            Self::ResetNumbering => "Number Pages From 1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum MenuCommand {
    Open,
    OpenRecent,
    Quit,
    Save,
    SaveAs,
    Revert,
    AttachToEmail,
    CopyFileToClipboard,
    /// Edit > Cut, Copy, Paste or Delete, answered by the active tool.
    Edit(onionskin_plugin_api::EditVerb),
    CombineFiles,
    CreateFromFiles,
    CreateFromFile,
    CreateFromClipboard,
    Stamps,
    PasteStamp,
    /// Add Signature…, or with `initials`, Add Initials…: the dialog that
    /// makes one and chooses the Sign tool.
    Signature {
        initials: bool,
    },
    /// Clear Form: every field of the document's form back to its default.
    ClearForm,
    /// The redaction commands: marking pages, finding, properties, and
    /// writing a redacted or sanitized copy.
    Redact(super::tabs::RedactCommand),
    SummarizeComments,
    SplitDocument,
    /// Crop Pages: the dialog, on the pages chosen.
    CropPages,
    /// Watermark, Background, Header & Footer or Bates Numbering: the
    /// dialog that adds, updates or removes that kind of page mark.
    PageMarks(onionskin_core::pages::MarkKind),
    /// Edit > Create Links from URLs, or with `remove`, Remove Web Links.
    WebLinks {
        remove: bool,
    },
    Properties,
    SaveAsOther,
    ReduceFileSize,
    PageSetup,
    Print,
    Skins,
    OrganizePages,
    Export(ExportTarget),
    ExportAllImages,
    CloseTab,
    CloseOtherTabs,
    CloseAllTabs,
    Undo,
    Redo,
    SelectAll,
    DeselectAll,
    TakeSnapshot,
    Find,
    AdvancedSearch,
    Page(PageCommand),
    Preferences,
    PreviousView,
    NextView,
    FirstPage,
    PreviousPage,
    NextPage,
    LastPage,
    RotateClockwise,
    ActualSize,
    ZoomOut,
    ZoomIn,
    ZoomTo,
    DynamicZoom,
    FitPage,
    FitWidth,
    FitHeight,
    FitVisible,
    SinglePage,
    SinglePageContinuous,
    TwoPage,
    TwoPageContinuous,
    ToggleCover,
    Tools,
    ManageTools,
    AutoScroll,
    ToggleNavigationPane,
    ToggleQuickAction(QuickAction),
    TogglePageControls,
    LineWeights,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    ReadMode,
    FullScreen,
    NewWindow,
    Minimize,
    ZoomWindow,
    BringAllToFront,
    About,
    KeyboardShortcuts,
}

/// What the Take a Snapshot entry says when no installed tool carries the
/// capability, and what the frame says if one disappears between the menu
/// being built and the entry being chosen.
pub(super) const NO_SNAPSHOT_TOOL: &str = "No installed tool takes a snapshot";

/// The same, for Dynamic Zoom, which is likewise an entry that selects a
/// tool the registry may not have.
pub(super) const NO_DYNAMIC_ZOOM_TOOL: &str = "No installed tool zooms dynamically";

/// The menu entries a registered command runs, rather than shell code.
const REGISTRY_BACKED: [MenuCommand; 13] = [
    MenuCommand::SelectAll,
    MenuCommand::DeselectAll,
    MenuCommand::Page(PageCommand::RotateClockwise),
    MenuCommand::Page(PageCommand::RotateCounterclockwise),
    MenuCommand::Page(PageCommand::InsertBlank),
    MenuCommand::Page(PageCommand::MoveEarlier),
    MenuCommand::Page(PageCommand::MoveLater),
    MenuCommand::Page(PageCommand::Delete),
    MenuCommand::Page(PageCommand::ResetNumbering),
    MenuCommand::SplitDocument,
    MenuCommand::SummarizeComments,
    MenuCommand::ReduceFileSize,
    MenuCommand::CropPages,
];

/// Which of those commands this build's plugins registered, and what each
/// declares it does: the effect is what decides whether a document that may
/// not be edited disables the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct RegisteredCommands([Option<CommandEffect>; REGISTRY_BACKED.len()]);

impl RegisteredCommands {
    fn installed(effect_of: impl Fn(&str) -> Option<CommandEffect>) -> Self {
        // `registry_command_id` answers for exactly these, but this runs on
        // every menu refresh, and an entry added to REGISTRY_BACKED by
        // mistake should read as "no plugin provides it" rather than take
        // the window down.
        Self(REGISTRY_BACKED.map(|command| command.registry_command_id().and_then(&effect_of)))
    }

    fn effect(self, command: MenuCommand) -> Option<CommandEffect> {
        REGISTRY_BACKED
            .iter()
            .position(|backed| *backed == command)
            .and_then(|index| self.0[index])
    }

    #[cfg(test)]
    fn has(self, command: MenuCommand) -> bool {
        self.effect(command).is_some()
    }
}

/// Everything the menus ask the registry, asked once.
///
/// An entry backed by a plugin is live because the plugin is installed, not
/// because a table here says the milestone landed: a build with the plugin
/// compiled out shows the entry saying so, and a plugin that arrives later
/// makes it live with no change in the chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) struct RegistryFacts {
    codecs: ExportCodecs,
    commands: RegisteredCommands,
    snapshot_tool: bool,
    dynamic_zoom_tool: bool,
    any_tool: bool,
    /// Whether some codec imports a format into a new document, which is
    /// what the Create entries run.
    image_import: bool,
    /// Whether a tool places stamps, which the Stamps entries open.
    stamp_tool: bool,
    /// Whether a tool places a saved signature, which Add Signature arms.
    sign_tool: bool,
    /// Whether a tool marks for redaction, which the redaction entries need.
    redact_tool: bool,
    /// Why the active document may not be edited, from `core`. Carried with
    /// the registry's answers because it is asked the same way: an entry whose
    /// command declares [`CommandEffect::Edits`] is disabled with it.
    edit_refusal: Option<&'static str>,
    /// Why the active document's objects may not be copied into another file:
    /// an entry whose command declares [`CommandEffect::ReadsOut`] is disabled
    /// with it.
    read_out_refusal: Option<&'static str>,
}

impl RegistryFacts {
    pub(in crate::shell) fn of(registry: &PluginRegistry) -> Self {
        Self {
            codecs: ExportCodecs::installed(|id| registry.codec(id).is_some()),
            commands: RegisteredCommands::installed(|id| {
                registry
                    .commands()
                    .iter()
                    .find(|command| command.id == id)
                    .map(|command| command.effect)
            }),
            snapshot_tool: tool_with(registry, ToolCapability::Snapshot).is_some(),
            dynamic_zoom_tool: tool_with(registry, ToolCapability::DynamicZoom).is_some(),
            any_tool: registry.tools().next().is_some(),
            image_import: registry.codecs().any(|codec| codec.imports()),
            stamp_tool: tool_with(registry, ToolCapability::Stamp).is_some(),
            sign_tool: tool_with(registry, ToolCapability::AddSignature).is_some(),
            redact_tool: tool_with(registry, ToolCapability::Redact).is_some(),
            edit_refusal: None,
            read_out_refusal: None,
        }
    }

    /// The same facts for a document that refuses reading out for `refusal`.
    pub(in crate::shell) fn refusing_read_out(self, refusal: Option<&'static str>) -> Self {
        Self {
            read_out_refusal: refusal,
            ..self
        }
    }

    /// The same facts for a document that refuses editing for `refusal`.
    pub(in crate::shell) fn refusing_edits(self, refusal: Option<&'static str>) -> Self {
        Self {
            edit_refusal: refusal,
            ..self
        }
    }
}

/// Whether a menu entry is live, and why not. The plugin API's type, so the
/// menus, the toolbar and the context menu answer with one query rather than
/// three copies of it.
pub(in crate::shell) use onionskin_plugin_api::Availability as MenuAvailability;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) struct MenuState {
    tab_count: usize,
    has_active_tab: bool,
    view: Option<CanvasViewState>,
    shell_view: ShellViewState,
    quick_actions_visible: [bool; QuickAction::ALL.len()],
    registry: RegistryFacts,
    recent_count: usize,
    /// The active document's history and file, when one is open.
    history: Option<crate::shell::canvas::HistoryFacts>,
    /// Whether the active tool answers each Edit verb, in `EditVerb::ALL`
    /// order, and why not when it does not.
    edit_verbs: [MenuAvailability; 4],
}

impl MenuState {
    /// The same state, knowing what the active document can undo and save.
    pub(in crate::shell) fn with_history(
        self,
        history: Option<crate::shell::canvas::HistoryFacts>,
    ) -> Self {
        Self { history, ..self }
    }

    /// The same state, knowing which Edit verbs the active tool answers.
    pub(in crate::shell) fn with_edit_verbs(self, edit_verbs: [MenuAvailability; 4]) -> Self {
        Self { edit_verbs, ..self }
    }

    pub(in crate::shell) fn new(
        tab_count: usize,
        view: Option<CanvasViewState>,
        shell_view: ShellViewState,
        quick_actions_visible: [bool; QuickAction::ALL.len()],
        registry: RegistryFacts,
        recent_count: usize,
    ) -> Self {
        Self {
            tab_count,
            has_active_tab: tab_count > 0,
            view,
            shell_view,
            quick_actions_visible,
            registry,
            recent_count,
            history: None,
            edit_verbs: [MenuAvailability::Disabled("No document is open"); 4],
        }
    }
}

/// Save: live with unsaved edits, and for a document with no file yet,
/// where it asks where to write, as Save As does.
fn save_availability(state: MenuState) -> MenuAvailability {
    match state.history {
        None => MenuAvailability::Disabled("No document is open"),
        Some(facts) if facts.dirty || !facts.has_path => MenuAvailability::Enabled,
        Some(_) => MenuAvailability::Disabled("No unsaved changes"),
    }
}

/// Attach to Email sends the file on disk, so it wants one, and one that
/// carries the user's changes.
fn attach_availability(state: MenuState) -> MenuAvailability {
    match state.history {
        None => MenuAvailability::Disabled("No document is open"),
        Some(facts) if !facts.has_path => {
            MenuAvailability::Disabled("This document has never been saved")
        }
        Some(facts) if facts.dirty => {
            MenuAvailability::Disabled("Save first, so the email carries your changes")
        }
        Some(_) if cfg!(not(unix)) => {
            MenuAvailability::Disabled("No mail client can be asked on this platform")
        }
        Some(_) => MenuAvailability::Enabled,
    }
}

/// Copy File to Clipboard names the file on disk.
fn copy_file_availability(state: MenuState) -> MenuAvailability {
    match state.history {
        None => MenuAvailability::Disabled("No document is open"),
        Some(facts) if !facts.has_path => {
            MenuAvailability::Disabled("This document has never been saved")
        }
        Some(_) => MenuAvailability::Enabled,
    }
}

/// The skins list the file's versions, so they want a file.
fn skins_availability(state: MenuState) -> MenuAvailability {
    match state.history {
        None => MenuAvailability::Disabled("No document is open"),
        Some(facts) if !facts.has_path => {
            MenuAvailability::Disabled("This document has never been saved")
        }
        Some(_) => MenuAvailability::Enabled,
    }
}

fn revert_availability(state: MenuState) -> MenuAvailability {
    match state.history {
        None => MenuAvailability::Disabled("No document is open"),
        Some(facts) if !facts.has_path => {
            MenuAvailability::Disabled("This document has never been saved")
        }
        Some(facts) if facts.dirty => MenuAvailability::Enabled,
        Some(_) => MenuAvailability::Disabled("No unsaved changes"),
    }
}

/// Undo and Redo: live when the history has a step that way, and saying so
/// when it has none, rather than being absent.
fn history_availability(state: MenuState, redo: bool) -> MenuAvailability {
    match state.history {
        None => MenuAvailability::Disabled("No document is open"),
        Some(facts) if !redo && facts.undo.is_some() => MenuAvailability::Enabled,
        Some(facts) if redo && facts.redo.is_some() => MenuAvailability::Enabled,
        Some(_) if redo => MenuAvailability::Disabled("Nothing to redo"),
        Some(_) => MenuAvailability::Disabled("Nothing to undo"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MenuEntry {
    pub(super) command: MenuCommand,
    pub(super) label: &'static str,
    pub(super) availability: MenuAvailability,
    pub(super) selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MenuSection {
    pub(super) id: MenuSectionId,
    pub(super) entries: Vec<MenuEntry>,
}

pub(super) fn main_menu_schema(state: MenuState) -> Vec<MenuSection> {
    use MenuAvailability::{Disabled, Enabled};

    let document_command = if state.has_active_tab {
        Enabled
    } else {
        Disabled("No document is open")
    };
    let close_others = if state.tab_count > 1 {
        Enabled
    } else {
        Disabled("No other tabs are open")
    };

    vec![
        MenuSection {
            id: MenuSectionId::File,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::Open,
                    label: "Open…",
                    availability: Enabled,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::OpenRecent,
                    label: "Open Recent",
                    availability: if state.recent_count > 0 {
                        Enabled
                    } else {
                        Disabled("No documents have been opened yet")
                    },
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CloseTab,
                    label: "Close",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CloseOtherTabs,
                    label: "Close Others",
                    availability: close_others,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CloseAllTabs,
                    label: "Close All",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Save,
                    label: "Save",
                    availability: save_availability(state),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::SaveAs,
                    label: "Save As…",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Revert,
                    label: "Revert",
                    availability: revert_availability(state),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Skins,
                    label: "Skins (Version History)",
                    availability: skins_availability(state),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::AttachToEmail,
                    label: "Attach to Email…",
                    availability: attach_availability(state),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CopyFileToClipboard,
                    label: "Copy File to Clipboard",
                    availability: copy_file_availability(state),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CombineFiles,
                    label: "Combine Files…",
                    availability: core_commands(),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CreateFromFiles,
                    label: "Create PDF From Multiple Files…",
                    availability: core_commands(),
                    selected: false,
                },
            ]
            .into_iter()
            .chain(create_entries(state))
            .chain([MenuEntry {
                command: MenuCommand::SplitDocument,
                label: "Split Document…",
                availability: registry_command(state, MenuCommand::SplitDocument),
                selected: false,
            }])
            .chain(export_entries(state))
            .chain([export_all_images_entry(state)])
            .chain([
                save_as_other_entry(state),
                MenuEntry {
                    command: MenuCommand::ReduceFileSize,
                    label: "Reduce File Size…",
                    availability: registry_command(state, MenuCommand::ReduceFileSize),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Properties,
                    label: "Properties…",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::PageSetup,
                    label: "Page Setup…",
                    availability: Enabled,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Print,
                    label: "Print…",
                    availability: document_command,
                    selected: false,
                },
            ])
            .chain([MenuEntry {
                command: MenuCommand::Quit,
                label: "Exit",
                availability: Enabled,
                selected: false,
            }])
            .collect(),
        },
        MenuSection {
            id: MenuSectionId::Edit,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::Undo,
                    label: "Undo",
                    availability: history_availability(state, false),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Redo,
                    label: "Redo",
                    availability: history_availability(state, true),
                    selected: false,
                },
            ]
            .into_iter()
            .chain(
                onionskin_plugin_api::EditVerb::ALL
                    .into_iter()
                    .zip(state.edit_verbs)
                    .map(|(verb, availability)| MenuEntry {
                        command: MenuCommand::Edit(verb),
                        label: verb.label(),
                        availability,
                        selected: false,
                    }),
            )
            .chain([
                MenuEntry {
                    command: MenuCommand::SelectAll,
                    label: "Select All",
                    availability: registry_command(state, MenuCommand::SelectAll),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::DeselectAll,
                    label: "Deselect All",
                    availability: registry_command(state, MenuCommand::DeselectAll),
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::TakeSnapshot,
                    label: "Take a Snapshot",
                    availability: match (state.registry.snapshot_tool, state.has_active_tab) {
                        (false, _) => Disabled(NO_SNAPSHOT_TOOL),
                        (true, false) => Disabled("No document is open"),
                        (true, true) => Enabled,
                    },
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::Find,
                    label: "Find…",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::AdvancedSearch,
                    label: "Advanced Search…",
                    availability: document_command,
                    selected: false,
                },
            ])
            .chain([
                MenuEntry {
                    command: MenuCommand::OrganizePages,
                    label: "Organize Pages",
                    availability: document_command,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::CropPages,
                    label: "Crop Pages…",
                    availability: registry_command(state, MenuCommand::CropPages),
                    selected: false,
                },
            ])
            .chain(mark_entries(state))
            .chain(link_entries(state))
            .chain(page_entries(state))
            .chain(stamp_entries(state))
            .chain(signature_entries(state))
            .chain([clear_form_entry(state)])
            .chain(redact_entries(state))
            .chain([MenuEntry {
                command: MenuCommand::SummarizeComments,
                label: "Summarize Comments…",
                availability: registry_command(state, MenuCommand::SummarizeComments),
                selected: false,
            }])
            .chain([MenuEntry {
                command: MenuCommand::Preferences,
                label: "Preferences…",
                availability: Enabled,
                selected: false,
            }])
            .collect(),
        },
        MenuSection {
            id: MenuSectionId::View,
            entries: view_menu_entries(
                state.view,
                state.shell_view,
                state.quick_actions_visible,
                state.registry.any_tool,
                state.registry.dynamic_zoom_tool,
            ),
        },
        MenuSection {
            id: MenuSectionId::Window,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::Minimize,
                    label: "Minimize",
                    availability: Enabled,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::ZoomWindow,
                    label: "Zoom",
                    availability: Enabled,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::BringAllToFront,
                    label: "Bring All to Front",
                    availability: Enabled,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::NewWindow,
                    label: "New Window",
                    availability: document_command,
                    selected: false,
                },
            ],
        },
        MenuSection {
            id: MenuSectionId::Help,
            entries: vec![
                MenuEntry {
                    command: MenuCommand::About,
                    label: "About Onionskin",
                    availability: Enabled,
                    selected: false,
                },
                MenuEntry {
                    command: MenuCommand::KeyboardShortcuts,
                    label: "Keyboard Shortcuts",
                    availability: Enabled,
                    selected: false,
                },
            ],
        },
    ]
}

/// An entry a plugin's command runs: live when that plugin registered the
/// command and there is a document for it to act on.
fn registry_command(state: MenuState, command: MenuCommand) -> MenuAvailability {
    let refusal = match state.registry.commands.effect(command) {
        None => return MenuAvailability::Disabled("No installed plugin provides this command"),
        Some(_) if !state.has_active_tab => {
            return MenuAvailability::Disabled("No document is open")
        }
        Some(CommandEffect::Edits) => state.registry.edit_refusal,
        Some(CommandEffect::ReadsOut) => state.registry.read_out_refusal,
        Some(CommandEffect::Reads) => None,
    };
    refusal.map_or(MenuAvailability::Enabled, MenuAvailability::Disabled)
}

/// Watermark, Background, Header & Footer and Bates Numbering: live on an
/// open document that may be edited, in a build with `tools-edit`.
fn mark_entries(state: MenuState) -> Vec<MenuEntry> {
    let availability = if !state.has_active_tab {
        MenuAvailability::Disabled("No document is open")
    } else {
        super::crop_dialog::crop_refusal(state.registry.edit_refusal)
            .map_or(MenuAvailability::Enabled, MenuAvailability::Disabled)
    };
    onionskin_core::pages::MarkKind::ALL
        .into_iter()
        .map(|kind| MenuEntry {
            command: MenuCommand::PageMarks(kind),
            label: mark_label(kind),
            availability,
            selected: false,
        })
        .collect()
}

/// Create Links from URLs and Remove Web Links, live as the page marks are.
fn link_entries(state: MenuState) -> Vec<MenuEntry> {
    let availability = mark_entries(state)
        .first()
        .map_or(MenuAvailability::Enabled, |entry| entry.availability);
    [
        (false, "Create Links from URLs"),
        (true, "Remove Web Links"),
    ]
    .into_iter()
    .map(|(remove, label)| MenuEntry {
        command: MenuCommand::WebLinks { remove },
        label,
        availability,
        selected: false,
    })
    .collect()
}

fn mark_label(kind: onionskin_core::pages::MarkKind) -> &'static str {
    use onionskin_core::pages::MarkKind;
    match kind {
        MarkKind::Watermark => "Watermark…",
        MarkKind::Background => "Background…",
        MarkKind::HeaderFooter => "Header & Footer…",
        MarkKind::Bates => "Bates Numbering…",
    }
}

/// Combine needs no open document, only the plugin that does the work. The
/// dialog calls that plugin's functions directly, so whether it was compiled
/// in is the whole answer: there is no registry command to ask about, because
/// a list of files is not something a command's context can carry.
fn core_commands() -> MenuAvailability {
    if cfg!(feature = "commands-core") {
        MenuAvailability::Enabled
    } else {
        MenuAvailability::Disabled(super::tabs::NO_CORE_COMMANDS)
    }
}

/// Stamps… and Paste Clipboard Image as Stamp: live when a tool places
/// stamps, a document is open, and it may be edited - a stamp is an edit.
fn stamp_entries(state: MenuState) -> [MenuEntry; 2] {
    let availability = match (state.registry.stamp_tool, state.has_active_tab) {
        (false, _) => MenuAvailability::Disabled(super::tabs::NO_STAMP_TOOL),
        (true, false) => MenuAvailability::Disabled("No document is open"),
        (true, true) => state
            .registry
            .edit_refusal
            .map_or(MenuAvailability::Enabled, MenuAvailability::Disabled),
    };
    [
        MenuEntry {
            command: MenuCommand::Stamps,
            label: "Stamps…",
            availability,
            selected: false,
        },
        MenuEntry {
            command: MenuCommand::PasteStamp,
            label: "Paste Clipboard Image as Stamp",
            availability,
            selected: false,
        },
    ]
}

/// Add Signature… and Add Initials…: live when a tool places signatures, a
/// document is open, and it may be edited, since the dialog ends by arming
/// that tool on it.
fn signature_entries(state: MenuState) -> [MenuEntry; 2] {
    let availability = match (state.registry.sign_tool, state.has_active_tab) {
        (false, _) => MenuAvailability::Disabled(super::tabs::NO_SIGN_TOOL),
        (true, false) => MenuAvailability::Disabled("No document is open"),
        (true, true) => state
            .registry
            .edit_refusal
            .map_or(MenuAvailability::Enabled, MenuAvailability::Disabled),
    };
    [(false, "Add Signature…"), (true, "Add Initials…")].map(|(initials, label)| MenuEntry {
        command: MenuCommand::Signature { initials },
        label,
        availability,
        selected: false,
    })
}

/// Clear Form: live in a build that fills forms, on a document that may be
/// edited. A document with no form says so when it is run.
fn clear_form_entry(state: MenuState) -> MenuEntry {
    let availability = match (cfg!(feature = "tools-form"), state.has_active_tab) {
        (false, _) => MenuAvailability::Disabled(super::tabs::NO_FORMS),
        (true, false) => MenuAvailability::Disabled("No document is open"),
        (true, true) => state
            .registry
            .edit_refusal
            .map_or(MenuAvailability::Enabled, MenuAvailability::Disabled),
    };
    MenuEntry {
        command: MenuCommand::ClearForm,
        label: "Clear Form",
        availability,
        selected: false,
    }
}

/// The redaction entries: live when a tool marks for redaction and a
/// document is open. Marking needs a document that may be edited; writing a
/// redacted copy needs one whose content may be read out.
fn redact_entries(state: MenuState) -> Vec<MenuEntry> {
    super::tabs::RedactCommand::ALL
        .into_iter()
        .map(|command| {
            let refusal = if command.writes_a_file() {
                state.registry.read_out_refusal
            } else {
                state.registry.edit_refusal
            };
            let availability = match (state.registry.redact_tool, state.has_active_tab) {
                (false, _) => MenuAvailability::Disabled(super::tabs::NO_REDACT),
                (true, false) => MenuAvailability::Disabled("No document is open"),
                (true, true) => {
                    refusal.map_or(MenuAvailability::Enabled, MenuAvailability::Disabled)
                }
            };
            MenuEntry {
                command: MenuCommand::Redact(command),
                label: command.label(),
                availability,
                selected: false,
            }
        })
        .collect()
}

/// The page commands, on the page the viewport is on.
fn page_entries(state: MenuState) -> Vec<MenuEntry> {
    PageCommand::ALL
        .into_iter()
        .map(|page| MenuEntry {
            command: MenuCommand::Page(page),
            label: page.label(),
            availability: registry_command(state, MenuCommand::Page(page)),
            selected: false,
        })
        .collect()
}

/// File > Create's single-source entries. Both run a codec's import half,
/// so they are live when some codec has one.
fn create_entries(state: MenuState) -> [MenuEntry; 2] {
    let availability = if state.registry.image_import {
        MenuAvailability::Enabled
    } else {
        MenuAvailability::Disabled(NO_IMAGE_IMPORT)
    };
    [
        MenuEntry {
            command: MenuCommand::CreateFromFile,
            label: "Create PDF From File…",
            availability,
            selected: false,
        },
        MenuEntry {
            command: MenuCommand::CreateFromClipboard,
            label: "Create PDF From Clipboard",
            availability,
            selected: false,
        },
    ]
}

/// What the Create entries say in a build with no codec that imports.
pub(super) const NO_IMAGE_IMPORT: &str = "No installed codec makes a PDF from an image";

/// Export All Images, which is `codecs-common`'s own function rather than a
/// registered codec: an image comes out in whichever format keeps it.
fn export_all_images_entry(state: MenuState) -> MenuEntry {
    use MenuAvailability::{Disabled, Enabled};

    MenuEntry {
        command: MenuCommand::ExportAllImages,
        label: "Export All Images…",
        availability: match (cfg!(feature = "codecs-common"), state.has_active_tab) {
            (false, _) => Disabled("The common codecs plugin is not installed"),
            (true, false) => Disabled("No document is open"),
            (true, true) => Enabled,
        },
        selected: false,
    }
}

/// The global bar's Convert panel: one surface over creating a PDF from
/// other formats and exporting one to them. Deliberately the formats this
/// build has, not Acrobat's list; each entry is the File menu's own, with
/// the same availability.
pub(super) fn convert_section(state: MenuState) -> MenuSection {
    let multiple = MenuEntry {
        command: MenuCommand::CreateFromFiles,
        label: "Create PDF From Multiple Files…",
        availability: core_commands(),
        selected: false,
    };
    MenuSection {
        id: MenuSectionId::Convert,
        entries: create_entries(state)
            .into_iter()
            .chain([multiple])
            .chain(export_entries(state))
            .chain([export_all_images_entry(state)])
            .collect(),
    }
}

/// File > Save as Other: the entry, live when some codec exports.
fn save_as_other_entry(state: MenuState) -> MenuEntry {
    use MenuAvailability::{Disabled, Enabled};

    MenuEntry {
        command: MenuCommand::SaveAsOther,
        label: "Save as Other…",
        availability: match (state.registry.codecs.any(), state.has_active_tab) {
            (false, _) => Disabled("No installed codec exports"),
            (true, false) => Disabled("No document is open"),
            (true, true) => Enabled,
        },
        selected: false,
    }
}

/// What Save as Other lists: exactly the export formats the registry has a
/// codec for. An absent codec is an absent entry, not a disabled one, and
/// formats Onionskin does not write (PDF/X, Reader Extended PDF) are not
/// listed at all: Acrobat's list is not a promise this build makes.
pub(super) fn save_as_other_section(state: MenuState) -> MenuSection {
    MenuSection {
        id: MenuSectionId::SaveAsOther,
        entries: export_entries(state)
            .into_iter()
            .filter(|entry| match entry.command {
                MenuCommand::Export(target) => state.registry.codecs.has(target),
                _ => false,
            })
            .collect(),
    }
}

/// The page export formats: M2's text, PNG and SVG, and M3's JPEG and TIFF.
/// JPEG 2000 has no entry; `codecs-common`'s crate doc says why.
fn export_entries(state: MenuState) -> Vec<MenuEntry> {
    use MenuAvailability::{Disabled, Enabled};

    ExportTarget::ALL
        .into_iter()
        .map(|target| MenuEntry {
            command: MenuCommand::Export(target),
            label: target.label(),
            availability: match (state.registry.codecs.has(target), state.has_active_tab) {
                (false, _) => Disabled("The common codecs plugin is not installed"),
                (true, false) => Disabled("No document is open"),
                (true, true) => Enabled,
            },
            selected: false,
        })
        .collect()
}

fn view_menu_entries(
    view: Option<CanvasViewState>,
    shell_view: ShellViewState,
    quick_actions_visible: [bool; QuickAction::ALL.len()],
    any_tool: bool,
    dynamic_zoom_tool: bool,
) -> Vec<MenuEntry> {
    use MenuAvailability::{Disabled, Enabled};

    let availability = view
        .map(|_| Enabled)
        .unwrap_or(Disabled("No document is open"));
    let at_first = view.is_none_or(|view| view.current_page == 0);
    let at_last = view.is_none_or(|view| view.current_page + 1 >= view.page_count);
    let can_previous_view = view.is_some_and(|view| view.can_previous_view);
    let can_next_view = view.is_some_and(|view| view.can_next_view);
    let layout = view.map(|view| view.layout_mode);
    let show_cover = view.is_some_and(|view| view.show_cover);
    let auto_scrolling = view.is_some_and(|view| view.auto_scrolling);
    let actual_size = view.is_some_and(CanvasViewState::is_actual_size);
    let fit_mode = view.and_then(CanvasViewState::fit_mode);
    let entry = |command, label, availability, selected| MenuEntry {
        command,
        label,
        availability,
        selected,
    };

    let mut entries = vec![
        entry(
            MenuCommand::PreviousView,
            "Previous View",
            if can_previous_view {
                Enabled
            } else {
                Disabled("No previous view")
            },
            false,
        ),
        entry(
            MenuCommand::NextView,
            "Next View",
            if can_next_view {
                Enabled
            } else {
                Disabled("No next view")
            },
            false,
        ),
        entry(
            MenuCommand::FirstPage,
            "First Page",
            if at_first {
                Disabled("Already at the first page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::PreviousPage,
            "Previous Page",
            if at_first {
                Disabled("Already at the first page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::NextPage,
            "Next Page",
            if at_last {
                Disabled("Already at the last page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::LastPage,
            "Last Page",
            if at_last {
                Disabled("Already at the last page")
            } else {
                Enabled
            },
            false,
        ),
        entry(
            MenuCommand::RotateClockwise,
            "Rotate Clockwise",
            availability,
            false,
        ),
        entry(
            MenuCommand::ActualSize,
            "Actual Size",
            availability,
            actual_size,
        ),
        entry(MenuCommand::ZoomOut, "Zoom Out", availability, false),
        entry(MenuCommand::ZoomIn, "Zoom In", availability, false),
        entry(MenuCommand::ZoomTo, "Zoom To…", availability, false),
        entry(
            MenuCommand::DynamicZoom,
            "Dynamic Zoom",
            match (dynamic_zoom_tool, view.is_some()) {
                (false, _) => Disabled(NO_DYNAMIC_ZOOM_TOOL),
                (true, false) => Disabled("No document is open"),
                (true, true) => Enabled,
            },
            false,
        ),
        entry(
            MenuCommand::FitPage,
            "Fit Page",
            availability,
            fit_mode == Some(FitMode::Page),
        ),
        entry(
            MenuCommand::FitWidth,
            "Fit Width",
            availability,
            fit_mode == Some(FitMode::Width),
        ),
        entry(
            MenuCommand::FitHeight,
            "Fit Height",
            availability,
            fit_mode == Some(FitMode::Height),
        ),
        entry(
            MenuCommand::FitVisible,
            "Fit Visible",
            availability,
            matches!(fit_mode, Some(FitMode::Visible(_))),
        ),
        entry(
            MenuCommand::SinglePage,
            layout_label(PageLayoutMode::SinglePage),
            availability,
            layout == Some(PageLayoutMode::SinglePage),
        ),
        entry(
            MenuCommand::SinglePageContinuous,
            layout_label(PageLayoutMode::SinglePageContinuous),
            availability,
            layout == Some(PageLayoutMode::SinglePageContinuous),
        ),
        entry(
            MenuCommand::TwoPage,
            layout_label(PageLayoutMode::TwoPage),
            availability,
            layout == Some(PageLayoutMode::TwoPage),
        ),
        entry(
            MenuCommand::TwoPageContinuous,
            layout_label(PageLayoutMode::TwoPageContinuous),
            availability,
            layout == Some(PageLayoutMode::TwoPageContinuous),
        ),
        entry(
            MenuCommand::ToggleCover,
            "Show Cover Page",
            availability,
            show_cover,
        ),
        entry(
            MenuCommand::AutoScroll,
            "Automatically Scroll",
            availability,
            auto_scrolling,
        ),
    ];
    entries.push(entry(
        MenuCommand::Tools,
        "Tools",
        if any_tool {
            Enabled
        } else {
            Disabled("No tools are installed")
        },
        false,
    ));
    entries.push(entry(
        MenuCommand::ManageTools,
        "Manage Tools…",
        if any_tool {
            Enabled
        } else {
            Disabled("No tools are installed")
        },
        false,
    ));
    entries.push(entry(
        MenuCommand::ToggleNavigationPane,
        "Show Navigation Panes",
        Enabled,
        shell_view.navigation_pane_visible(),
    ));
    entries.extend(QuickAction::ALL.into_iter().map(|action| {
        entry(
            MenuCommand::ToggleQuickAction(action),
            action.menu_label(),
            Enabled,
            quick_actions_visible[action.index()],
        )
    }));
    entries.extend([
        entry(
            MenuCommand::TogglePageControls,
            "Show Page Controls",
            Enabled,
            shell_view.page_controls_visible(),
        ),
        entry(
            MenuCommand::LineWeights,
            "Line Weights",
            Enabled,
            shell_view.line_weights(),
        ),
        entry(
            MenuCommand::ThemeSystem,
            ThemePreference::System.label(),
            Enabled,
            shell_view.theme() == ThemePreference::System,
        ),
        entry(
            MenuCommand::ThemeLight,
            ThemePreference::Light.label(),
            Enabled,
            shell_view.theme() == ThemePreference::Light,
        ),
        entry(
            MenuCommand::ThemeDark,
            ThemePreference::Dark.label(),
            Enabled,
            shell_view.theme() == ThemePreference::Dark,
        ),
        entry(
            MenuCommand::ReadMode,
            "Read Mode",
            Enabled,
            shell_view.read_mode(),
        ),
        entry(
            MenuCommand::FullScreen,
            "Full Screen",
            Enabled,
            shell_view.fullscreen(),
        ),
    ]);
    entries
}

impl MenuCommand {
    pub(super) fn view_action(self, view: CanvasViewState) -> Option<ViewAction> {
        Some(match self {
            Self::PreviousView => ViewAction::PreviousView,
            Self::NextView => ViewAction::NextView,
            Self::FirstPage => ViewAction::FirstPage,
            Self::PreviousPage => ViewAction::PreviousPage,
            Self::NextPage => ViewAction::NextPage,
            Self::LastPage => ViewAction::LastPage,
            Self::RotateClockwise => ViewAction::RotateClockwise,
            Self::ActualSize => ViewAction::ActualSize,
            Self::ZoomOut => ViewAction::ZoomOut,
            Self::ZoomIn => ViewAction::ZoomIn,
            Self::FitPage => ViewAction::Fit(FitMode::Page),
            Self::FitWidth => ViewAction::Fit(FitMode::Width),
            Self::FitHeight => ViewAction::Fit(FitMode::Height),
            Self::FitVisible => ViewAction::FitVisible,
            Self::SinglePage => ViewAction::SetLayout(PageLayoutMode::SinglePage),
            Self::SinglePageContinuous => {
                ViewAction::SetLayout(PageLayoutMode::SinglePageContinuous)
            }
            Self::TwoPage => ViewAction::SetLayout(PageLayoutMode::TwoPage),
            Self::TwoPageContinuous => ViewAction::SetLayout(PageLayoutMode::TwoPageContinuous),
            Self::ToggleCover => ViewAction::SetShowCover(!view.show_cover),
            // Opens the magnification chooser rather than changing the view
            // itself; the dialog's rows carry the view actions.
            Self::ZoomTo
            | Self::DynamicZoom
            | Self::Open
            | Self::OpenRecent
            | Self::Quit
            | Self::Save
            | Self::SaveAs
            | Self::Revert
            | Self::AttachToEmail
            | Self::ReduceFileSize
            | Self::PageSetup
            | Self::Print
            | Self::Skins
            | Self::OrganizePages
            | Self::CopyFileToClipboard
            | Self::Edit(_)
            | Self::Export(_)
            | Self::CombineFiles
            | Self::CreateFromFiles
            | Self::CreateFromFile
            | Self::CreateFromClipboard
            | Self::Stamps
            | Self::PasteStamp
            | Self::Signature { .. }
            | Self::ClearForm
            | Self::Redact(_)
            | Self::SummarizeComments
            | Self::ExportAllImages
            | Self::SplitDocument
            | Self::CropPages
            | Self::PageMarks(_)
            | Self::WebLinks { .. }
            | Self::Properties
            | Self::SaveAsOther
            | Self::CloseTab
            | Self::CloseOtherTabs
            | Self::CloseAllTabs
            | Self::Find
            | Self::AdvancedSearch
            | Self::Preferences
            | Self::Undo
            | Self::Redo
            | Self::SelectAll
            | Self::DeselectAll
            | Self::TakeSnapshot
            | Self::Page(_)
            | Self::Tools
            | Self::ManageTools
            | Self::AutoScroll
            | Self::ToggleNavigationPane
            | Self::ToggleQuickAction(_)
            | Self::TogglePageControls
            | Self::LineWeights
            | Self::ThemeSystem
            | Self::ThemeLight
            | Self::ThemeDark
            | Self::ReadMode
            | Self::FullScreen
            | Self::NewWindow
            | Self::Minimize
            | Self::ZoomWindow
            | Self::BringAllToFront
            | Self::About
            | Self::KeyboardShortcuts => return None,
        })
    }

    pub(super) fn shell_view_action(self) -> Option<ShellViewAction> {
        Some(match self {
            Self::ToggleNavigationPane => ShellViewAction::ToggleNavigationPane,
            Self::TogglePageControls => ShellViewAction::TogglePageControls,
            Self::ThemeSystem => ShellViewAction::SetTheme(ThemePreference::System),
            Self::ThemeLight => ShellViewAction::SetTheme(ThemePreference::Light),
            Self::ThemeDark => ShellViewAction::SetTheme(ThemePreference::Dark),
            Self::ReadMode => ShellViewAction::ToggleReadMode,
            Self::Open
            | Self::OpenRecent
            | Self::Quit
            | Self::Save
            | Self::SaveAs
            | Self::Revert
            | Self::AttachToEmail
            | Self::ReduceFileSize
            | Self::PageSetup
            | Self::Print
            | Self::Skins
            | Self::OrganizePages
            | Self::CopyFileToClipboard
            | Self::Edit(_)
            | Self::Export(_)
            | Self::CombineFiles
            | Self::CreateFromFiles
            | Self::CreateFromFile
            | Self::CreateFromClipboard
            | Self::Stamps
            | Self::PasteStamp
            | Self::Signature { .. }
            | Self::ClearForm
            | Self::Redact(_)
            | Self::SummarizeComments
            | Self::ExportAllImages
            | Self::SplitDocument
            | Self::CropPages
            | Self::PageMarks(_)
            | Self::WebLinks { .. }
            | Self::Properties
            | Self::SaveAsOther
            | Self::CloseTab
            | Self::CloseOtherTabs
            | Self::CloseAllTabs
            | Self::Find
            | Self::AdvancedSearch
            | Self::Preferences
            | Self::Undo
            | Self::Redo
            | Self::SelectAll
            | Self::DeselectAll
            | Self::TakeSnapshot
            | Self::Page(_)
            | Self::Tools
            | Self::ManageTools
            | Self::AutoScroll
            | Self::PreviousView
            | Self::NextView
            | Self::FirstPage
            | Self::PreviousPage
            | Self::NextPage
            | Self::LastPage
            | Self::RotateClockwise
            | Self::ActualSize
            | Self::ZoomOut
            | Self::ZoomIn
            | Self::ZoomTo
            | Self::DynamicZoom
            | Self::FitPage
            | Self::FitWidth
            | Self::FitHeight
            | Self::FitVisible
            | Self::SinglePage
            | Self::SinglePageContinuous
            | Self::TwoPage
            | Self::TwoPageContinuous
            | Self::ToggleCover
            | Self::ToggleQuickAction(_)
            | Self::LineWeights
            | Self::FullScreen
            | Self::NewWindow
            | Self::Minimize
            | Self::ZoomWindow
            | Self::BringAllToFront
            | Self::About
            | Self::KeyboardShortcuts => return None,
        })
    }
}

/// The route every runnable menu entry and every keystroke takes.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = onionskin_shell, no_json)]
pub(in crate::shell) struct RunCommand {
    pub(in crate::shell) command: MenuCommand,
}

/// The route a disabled native item takes. Nothing handles it, which is how it
/// stays greyed out; see [`native_menu_item`].
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = onionskin_shell, no_json)]
struct UnavailableCommand;

/// Route every command to the window, and put the menus up.
///
/// One app-level listener, not one per command: menu items and keystrokes
/// both dispatch [`RunCommand`], and action dispatch walks the focus path,
/// which no element in the chrome holds until something takes focus.
///
/// The body is deferred, and that is not optional. A global listener runs
/// inside the window update that dispatched the action, and a window is off
/// the app's window list for the length of its own update, so running the
/// command from here would look for the window that is dispatching it and
/// fail to find it. P9 hit this on Ctrl+F; the three per-command listeners
/// this replaces had the same shape and the same latent failure.
pub(in crate::shell) fn install_native_menus(
    cx: &mut App,
    window: WindowHandle<ShellFrame>,
    state: MenuState,
) {
    cx.on_action(move |action: &RunCommand, cx| {
        let command = action.command;
        cx.defer(move |cx| {
            // The window in front, which is not the first one once New
            // Window has opened another.
            let target = cx
                .active_window()
                .and_then(|active| active.downcast::<ShellFrame>())
                .unwrap_or(window);
            ShellFrame::run_native_command(target, command, cx);
        });
    });

    refresh_native_menus(cx, state);
}

pub(super) fn refresh_native_menus(cx: &App, state: MenuState) {
    cx.set_menus(native_menus(state));
}

fn native_menus(state: MenuState) -> Vec<Menu> {
    main_menu_schema(state)
        .into_iter()
        .map(|section| Menu {
            name: section.id.label().into(),
            items: section.entries.into_iter().map(native_menu_item).collect(),
        })
        .collect()
}

/// Every schema entry becomes a native item.
///
/// An entry the app cannot run right now is present and disabled, carrying its
/// reason, never absent. Omitting them left Edit, Window and Help with no items
/// at all, which reads as a broken application rather than as commands that
/// have not landed yet.
///
/// gpui's `MenuItem` has no disabled form, so a disabled entry is routed to
/// [`UnavailableCommand`], an action nothing registers a handler for. That is
/// exactly what greys it out: gpui answers macOS's `validateMenuItem:` with
/// `App::is_action_available`. The reason goes in the title because the native
/// menu has nowhere else to put it.
fn native_menu_item(entry: MenuEntry) -> MenuItem {
    let label = if entry.selected {
        format!("✓ {}", entry.label)
    } else {
        entry.label.to_owned()
    };
    match (entry.availability, native_action(entry.command)) {
        (MenuAvailability::Enabled, Some(action)) => MenuItem::Action {
            name: label.into(),
            action,
            os_action: None,
        },
        (MenuAvailability::Disabled(reason), _) => disabled_native_item(label, reason),
        // An entry the schema calls enabled but that routes nowhere is a wiring
        // mistake. Say so in the menu bar rather than dropping the item.
        (MenuAvailability::Enabled, None) => {
            disabled_native_item(label, "no command is wired to this entry")
        }
    }
}

fn disabled_native_item(label: String, reason: &str) -> MenuItem {
    MenuItem::Action {
        name: format!("{label} ({reason})").into(),
        action: Box::new(UnavailableCommand),
        os_action: None,
    }
}

fn native_action(command: MenuCommand) -> Option<Box<dyn Action>> {
    match command {
        MenuCommand::Open
        | MenuCommand::OpenRecent
        | MenuCommand::Quit
        | MenuCommand::CloseTab
        | MenuCommand::CloseOtherTabs
        | MenuCommand::CloseAllTabs
        | MenuCommand::SelectAll
        | MenuCommand::DeselectAll
        | MenuCommand::Page(_)
        | MenuCommand::TakeSnapshot
        | MenuCommand::Find
        | MenuCommand::AdvancedSearch
        | MenuCommand::Preferences
        | MenuCommand::Tools
        | MenuCommand::ManageTools
        | MenuCommand::AutoScroll
        | MenuCommand::About
        | MenuCommand::KeyboardShortcuts
        | MenuCommand::PreviousView
        | MenuCommand::NextView
        | MenuCommand::FirstPage
        | MenuCommand::PreviousPage
        | MenuCommand::NextPage
        | MenuCommand::LastPage
        | MenuCommand::RotateClockwise
        | MenuCommand::ActualSize
        | MenuCommand::ZoomOut
        | MenuCommand::ZoomIn
        | MenuCommand::ZoomTo
        | MenuCommand::DynamicZoom
        | MenuCommand::FitPage
        | MenuCommand::FitWidth
        | MenuCommand::FitHeight
        | MenuCommand::FitVisible
        | MenuCommand::SinglePage
        | MenuCommand::SinglePageContinuous
        | MenuCommand::TwoPage
        | MenuCommand::TwoPageContinuous
        | MenuCommand::ToggleCover
        | MenuCommand::ToggleNavigationPane
        | MenuCommand::ToggleQuickAction(_)
        | MenuCommand::TogglePageControls
        | MenuCommand::ThemeSystem
        | MenuCommand::ThemeLight
        | MenuCommand::ThemeDark
        | MenuCommand::ReadMode
        | MenuCommand::FullScreen
        | MenuCommand::Export(_)
        | MenuCommand::CombineFiles
        | MenuCommand::CreateFromFiles
        | MenuCommand::CreateFromFile
        | MenuCommand::CreateFromClipboard
        | MenuCommand::Stamps
        | MenuCommand::PasteStamp
        | MenuCommand::Signature { .. }
        | MenuCommand::ClearForm
        | MenuCommand::Redact(_)
        | MenuCommand::SummarizeComments
        | MenuCommand::ExportAllImages
        | MenuCommand::SplitDocument
        | MenuCommand::CropPages
        | MenuCommand::PageMarks(_)
        | MenuCommand::WebLinks { .. }
        | MenuCommand::Properties
        | MenuCommand::SaveAsOther
        | MenuCommand::Save
        | MenuCommand::SaveAs
        | MenuCommand::Revert
        | MenuCommand::AttachToEmail
        | MenuCommand::ReduceFileSize
        | MenuCommand::PageSetup
        | MenuCommand::Print
        | MenuCommand::Skins
        | MenuCommand::OrganizePages
        | MenuCommand::CopyFileToClipboard
        | MenuCommand::Edit(_)
        | MenuCommand::Undo
        | MenuCommand::Redo
        | MenuCommand::NewWindow
        | MenuCommand::Minimize
        | MenuCommand::ZoomWindow
        | MenuCommand::BringAllToFront
        | MenuCommand::LineWeights => Some(Box::new(RunCommand { command })),
    }
}

#[cfg(test)]
mod tests {
    use gpui::WindowAppearance;
    use onionskin_core::{ViewRotation, ZoomPolicy};

    use super::*;

    fn view() -> CanvasViewState {
        CanvasViewState {
            current_page: 1,
            page_count: 3,
            zoom: 1.25,
            zoom_policy: ZoomPolicy::Fixed,
            layout_mode: PageLayoutMode::TwoPage,
            show_cover: true,
            rotation: ViewRotation::Clockwise90,
            can_previous_view: true,
            can_next_view: true,
            auto_scrolling: false,
        }
    }

    /// A build with every plugin compiled in, which is the default set.
    fn everything_installed() -> RegistryFacts {
        RegistryFacts {
            codecs: ExportCodecs::installed(|_| true),
            commands: RegisteredCommands::installed(|id| {
                Some(
                    if id.starts_with("organize.")
                        || id == onionskin_plugin_api::command_ids::CROP_PAGES
                    {
                        CommandEffect::Edits
                    } else if id == onionskin_plugin_api::command_ids::SPLIT_DOCUMENT
                        || id == onionskin_plugin_api::command_ids::SUMMARIZE_COMMENTS
                    {
                        CommandEffect::ReadsOut
                    } else {
                        CommandEffect::Reads
                    },
                )
            }),
            snapshot_tool: true,
            dynamic_zoom_tool: true,
            any_tool: true,
            image_import: true,
            stamp_tool: true,
            sign_tool: true,
            redact_tool: true,
            edit_refusal: None,
            read_out_refusal: None,
        }
    }

    fn menu_state(tab_count: usize, view: Option<CanvasViewState>) -> MenuState {
        MenuState::new(
            tab_count,
            view,
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            everything_installed(),
            2,
        )
    }

    fn entry(state: MenuState, command: MenuCommand) -> MenuEntry {
        main_menu_schema(state)
            .into_iter()
            .flat_map(|section| section.entries)
            .find(|entry| entry.command == command)
            .unwrap_or_else(|| panic!("{} has a menu entry", command.id()))
    }

    fn view_entries(view: Option<CanvasViewState>) -> Vec<MenuEntry> {
        view_menu_entries(
            view,
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            true,
            true,
        )
    }

    /// Every command the enum has appears in the menus exactly once.
    ///
    /// Two things depend on it: the keymap resolves ids against this list,
    /// and `command_unavailable` reads the schema to decide whether a
    /// keystroke may run. A command missing from the schema would be bound
    /// to a key and silently treated as always available.
    #[test]
    fn every_command_has_exactly_one_menu_entry() {
        let entries: Vec<_> = main_menu_schema(menu_state(1, Some(view())))
            .into_iter()
            .flat_map(|section| section.entries)
            .collect();

        for command in MenuCommand::all() {
            assert_eq!(
                entries
                    .iter()
                    .filter(|entry| entry.command == command)
                    .count(),
                1,
                "{} is not in the menus exactly once",
                command.id()
            );
        }
        assert_eq!(entries.len(), MenuCommand::all().len());
        assert!(entries.iter().all(|entry| !entry.label.is_empty()));
    }

    /// The page commands are Edit-menu entries that run `tools-organize`'s
    /// commands. A document that may not be edited disables exactly them,
    /// with its own reason, because they declare themselves edits; Select All
    /// reads, and stays live.
    #[test]
    fn page_entries_are_disabled_exactly_where_editing_is_refused() {
        let refusal = "Encrypted document: editing arrives in M6";
        let open = menu_state(1, Some(view()));
        let locked = MenuState::new(
            1,
            Some(view()),
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            everything_installed().refusing_edits(Some(refusal)),
            2,
        );
        for page in PageCommand::ALL {
            let command = MenuCommand::Page(page);
            assert_eq!(entry(open, command).availability, MenuAvailability::Enabled);
            assert_eq!(
                entry(locked, command).availability,
                MenuAvailability::Disabled(refusal),
                "{}",
                page.label()
            );
            assert_eq!(command.registry_command_id(), Some(page.id()));
        }
        assert_eq!(
            entry(locked, MenuCommand::SelectAll).availability,
            MenuAvailability::Enabled,
            "a command that only reads is not an edit"
        );
    }

    /// Nothing the registry holds can be unreachable, and a keystroke alone
    /// does not count: a command only a keymap file mentions is invisible to
    /// anyone who has not read it.
    #[test]
    fn every_registered_command_has_a_menu_entry() {
        let registry = crate::build_registry();
        let entries: Vec<_> = main_menu_schema(menu_state(1, Some(view())))
            .into_iter()
            .flat_map(|section| section.entries)
            .collect();

        assert_eq!(
            registry.commands().is_empty(),
            cfg!(not(any(
                feature = "commands-core",
                feature = "tools-organize",
                feature = "tools-comment"
            ))),
            "there is nothing to check"
        );
        for command in registry.commands() {
            assert!(
                entries.iter().any(|entry| entry.command.id() == command.id),
                "{} is registered but no menu entry reaches it",
                command.id
            );
        }
    }

    #[test]
    fn main_menu_has_the_five_required_sections_in_order() {
        let ids: Vec<_> = main_menu_schema(menu_state(2, None))
            .into_iter()
            .map(|section| section.id)
            .collect();

        assert_eq!(
            ids,
            [
                MenuSectionId::File,
                MenuSectionId::Edit,
                MenuSectionId::View,
                MenuSectionId::Window,
                MenuSectionId::Help,
            ]
        );
    }

    /// Line Weights was the last main-menu entry greyed out until a later
    /// milestone. None may name one now, with or without a document open.
    #[test]
    fn no_menu_entry_is_deferred_to_a_milestone() {
        let deferred: Vec<_> = [menu_state(2, None), menu_state(1, Some(view()))]
            .into_iter()
            .flat_map(main_menu_schema)
            .flat_map(|section| section.entries)
            .filter_map(|entry| entry.availability.reason())
            .filter(|reason| {
                ["M3", "M4", "M5", "M6", "post-1.0"]
                    .iter()
                    .any(|stage| reason.contains(stage))
            })
            .collect();
        assert!(deferred.is_empty(), "{deferred:?}");
    }

    /// What P11 delivered. Each of these was a menu entry that named this
    /// package as the reason it was greyed out; none of them may name a
    /// milestone now.
    #[test]
    fn the_entries_this_package_delivers_are_live() {
        let state = menu_state(1, Some(view()));

        for command in [
            MenuCommand::Open,
            MenuCommand::OpenRecent,
            MenuCommand::Quit,
            MenuCommand::SelectAll,
            MenuCommand::DeselectAll,
            MenuCommand::TakeSnapshot,
            MenuCommand::Preferences,
            MenuCommand::Tools,
            MenuCommand::About,
            MenuCommand::KeyboardShortcuts,
        ] {
            assert_eq!(
                entry(state, command).availability,
                MenuAvailability::Enabled,
                "{} is not live",
                command.id()
            );
            assert!(
                native_action(command).is_some(),
                "{} has no route to run it",
                command.id()
            );
        }
    }

    /// The Edit entries are live because a plugin registered the command,
    /// not because a table here says the milestone landed. Compiling the
    /// plugin out has to change the answer, and the reason has to name the
    /// plugin rather than a date.
    #[test]
    fn a_registry_command_entry_follows_the_registry() {
        let without = MenuState::new(
            1,
            Some(view()),
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            RegistryFacts {
                commands: RegisteredCommands::installed(|_| None),
                ..everything_installed()
            },
            0,
        );

        assert_eq!(
            entry(without, MenuCommand::SelectAll).availability,
            MenuAvailability::Disabled("No installed plugin provides this command")
        );
        assert_eq!(
            entry(menu_state(1, Some(view())), MenuCommand::SelectAll).availability,
            MenuAvailability::Enabled
        );
        // And with the command installed but nothing to run it against, the
        // reason is the state, not the plugin.
        assert_eq!(
            entry(menu_state(0, None), MenuCommand::SelectAll).availability,
            MenuAvailability::Disabled("No document is open")
        );
    }

    /// Take a Snapshot asks for a capability, so the tool that carries it
    /// can arrive from any plugin.
    #[test]
    fn take_a_snapshot_follows_the_capability_rather_than_a_tool_id() {
        let without = MenuState::new(
            1,
            Some(view()),
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            RegistryFacts {
                snapshot_tool: false,
                ..everything_installed()
            },
            0,
        );

        assert_eq!(
            entry(without, MenuCommand::TakeSnapshot).availability,
            MenuAvailability::Disabled("No installed tool takes a snapshot")
        );
        assert_eq!(
            entry(menu_state(1, Some(view())), MenuCommand::TakeSnapshot).availability,
            MenuAvailability::Enabled
        );
    }

    /// Dynamic Zoom asks the same way, for the same reason: the entry
    /// selects a tool, so a build without that tool has to say so rather
    /// than offer an entry that reaches nothing.
    #[test]
    fn dynamic_zoom_follows_the_capability_rather_than_a_tool_id() {
        let without = MenuState::new(
            1,
            Some(view()),
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            RegistryFacts {
                dynamic_zoom_tool: false,
                ..everything_installed()
            },
            0,
        );

        assert_eq!(
            entry(without, MenuCommand::DynamicZoom).availability,
            MenuAvailability::Disabled(NO_DYNAMIC_ZOOM_TOOL)
        );
        assert_eq!(
            entry(menu_state(1, Some(view())), MenuCommand::DynamicZoom).availability,
            MenuAvailability::Enabled
        );
        // The tool is installed but there is nothing to zoom.
        assert_eq!(
            entry(menu_state(0, None), MenuCommand::DynamicZoom).availability,
            MenuAvailability::Disabled("No document is open")
        );
    }

    /// What the entries added for the zoom rows are called. The labels are
    /// what the user reads and what the accessibility tree announces, and
    /// nothing else pins them.
    #[test]
    fn the_zoom_entries_carry_acrobats_names() {
        let entries = view_entries(Some(view()));
        let label = |command| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .unwrap_or_else(|| panic!("{command:?} has a view menu entry"))
                .label
        };

        assert_eq!(label(MenuCommand::ZoomTo), "Zoom To…");
        assert_eq!(label(MenuCommand::FitVisible), "Fit Visible");
        assert_eq!(label(MenuCommand::DynamicZoom), "Dynamic Zoom");
    }

    /// The installed build answers the same way: this is the query the
    /// chrome runs, against the registry the app assembles.
    #[test]
    fn the_installed_registry_answers_the_menus_questions() {
        let facts = RegistryFacts::of(&crate::build_registry());

        assert_eq!(
            facts.commands.has(MenuCommand::SelectAll),
            cfg!(feature = "commands-core")
        );
        assert_eq!(facts.snapshot_tool, cfg!(feature = "tools-basic"));
        assert_eq!(facts.dynamic_zoom_tool, cfg!(feature = "tools-basic"));
        assert_eq!(
            facts.any_tool,
            cfg!(any(feature = "tools-basic", feature = "tools-comment"))
        );
        assert_eq!(facts.stamp_tool, cfg!(feature = "tools-comment"));
        assert_eq!(facts.sign_tool, cfg!(feature = "tools-fill-sign"));
        assert_eq!(facts.redact_tool, cfg!(feature = "redact"));
    }

    /// Open Recent is live when there is something to open, and says why
    /// when there is not.
    #[test]
    fn open_recent_follows_the_recents_list() {
        let empty = MenuState::new(
            0,
            None,
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            everything_installed(),
            0,
        );

        assert_eq!(
            entry(empty, MenuCommand::OpenRecent).availability,
            MenuAvailability::Disabled("No documents have been opened yet")
        );
        assert_eq!(
            entry(menu_state(0, None), MenuCommand::OpenRecent).availability,
            MenuAvailability::Enabled,
            "a recents list makes it live even with no document open"
        );
    }

    /// File > Open and the Help entries work with no document, which is the
    /// only way out of an empty window.
    #[test]
    fn the_commands_that_need_no_document_stay_live_without_one() {
        let state = menu_state(0, None);

        for command in [
            MenuCommand::Open,
            MenuCommand::Quit,
            MenuCommand::Preferences,
            MenuCommand::About,
            MenuCommand::KeyboardShortcuts,
        ] {
            assert_eq!(
                entry(state, command).availability,
                MenuAvailability::Enabled,
                "{} needs no document",
                command.id()
            );
        }
    }

    fn item_names(menu: &Menu) -> Vec<String> {
        menu.items
            .iter()
            .map(|item| match item {
                MenuItem::Action { name, .. } => name.to_string(),
                _ => panic!("the schema emits actions only"),
            })
            .collect()
    }

    #[test]
    fn native_menus_are_derived_from_the_same_five_section_schema() {
        let state = menu_state(2, None);
        let schema = main_menu_schema(state);
        let menus = native_menus(state);

        assert_eq!(menus.len(), 5);
        assert_eq!(menus[0].name.as_ref(), "File");
        assert_eq!(menus[1].name.as_ref(), "Edit");
        assert_eq!(menus[2].name.as_ref(), "View");
        assert_eq!(menus[3].name.as_ref(), "Window");
        assert_eq!(menus[4].name.as_ref(), "Help");
        for (section, menu) in schema.iter().zip(&menus) {
            assert_eq!(menu.items.len(), section.entries.len());
        }
    }

    /// A section whose entries are all deferred still renders its entries,
    /// disabled and carrying the reason. They used to be filtered out, which
    /// left Edit, Window and Help empty in the macOS menu bar: the app looked
    /// broken rather than unfinished.
    #[test]
    fn a_deferred_entry_is_present_and_disabled_rather_than_absent() {
        let menus = native_menus(menu_state(2, None));

        for section in [1usize, 3, 4] {
            assert!(
                !menus[section].items.is_empty(),
                "{} renders no items",
                menus[section].name
            );
        }
        assert_eq!(
            item_names(&menus[3]),
            vec!["Minimize", "Zoom", "Bring All to Front", "New Window"]
        );
        assert_eq!(
            item_names(&menus[1]),
            vec![
                // No history in this state: nothing to undo, said as such.
                "Undo (No document is open)",
                "Redo (No document is open)",
                // No tool answers the Edit verbs in this state.
                "Cut (No document is open)",
                "Copy (No document is open)",
                "Paste (No document is open)",
                "Delete (No document is open)",
                // Two tabs are open in this state, so the rest are live.
                "Select All",
                "Deselect All",
                "Take a Snapshot",
                "Find…",
                "Advanced Search…",
                "Organize Pages",
                "Crop Pages…",
                "Watermark…",
                "Background…",
                "Header & Footer…",
                "Bates Numbering…",
                "Create Links from URLs",
                "Remove Web Links",
                "Rotate Page Clockwise",
                "Rotate Page Counterclockwise",
                "Insert Blank Page",
                "Move Page Earlier",
                "Move Page Later",
                "Delete Page",
                "Number Pages From 1",
                "Stamps…",
                "Paste Clipboard Image as Stamp",
                "Add Signature…",
                "Add Initials…",
                "Clear Form",
                "Mark Pages for Redaction…",
                "Find Text & Redact…",
                "Redaction Properties…",
                "Apply Redactions…",
                "Remove Hidden Information…",
                "Summarize Comments…",
                "Preferences…",
            ]
        );
        assert_eq!(
            item_names(&menus[4]),
            vec!["About Onionskin", "Keyboard Shortcuts"]
        );
    }

    #[test]
    fn find_is_live_in_the_edit_menu_only_while_a_document_is_open() {
        let entry = |tabs| {
            main_menu_schema(menu_state(tabs, None))[1]
                .entries
                .iter()
                .find(|entry| entry.command == MenuCommand::Find)
                .copied()
                .expect("Edit carries Find")
        };

        assert_eq!(entry(1).availability, MenuAvailability::Enabled);
        assert_eq!(
            entry(0).availability,
            MenuAvailability::Disabled("No document is open")
        );
        // Enabled means the native item carries an action to raise, and there
        // is nothing to find in a window with no document.
        assert!(item_names(&native_menus(menu_state(1, None))[1]).contains(&"Find…".to_owned()));
        assert!(item_names(&native_menus(menu_state(0, None))[1])
            .contains(&"Find… (No document is open)".to_owned()));
    }

    #[test]
    fn close_others_is_disabled_consistently_for_one_tab() {
        let file = &main_menu_schema(menu_state(1, None))[0];
        let close_others = file
            .entries
            .iter()
            .find(|entry| entry.command == MenuCommand::CloseOtherTabs)
            .unwrap();

        assert_eq!(
            close_others.availability,
            MenuAvailability::Disabled("No other tabs are open")
        );
        assert!(item_names(&native_menus(menu_state(1, None))[0])
            .contains(&"Close Others (No other tabs are open)".to_owned()));
    }

    /// The five formats `codecs-common` registers reach the File menu, and
    /// the text entry says in its own label that it does not reorder into
    /// reading order, which is the limitation the parity scoreboard defers to
    /// M6. A user reads the menu, not the codec's doc comment.
    #[test]
    fn the_file_menu_offers_every_export_format_the_codecs_register() {
        let file = &main_menu_schema(menu_state(1, Some(view())))[0];

        let exports: Vec<_> = file
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, MenuCommand::Export(_)))
            .map(|entry| (entry.command, entry.label, entry.availability))
            .collect();

        assert_eq!(
            exports,
            vec![
                (
                    MenuCommand::Export(ExportTarget::Text),
                    "Export To Plain Text (Document Order)…",
                    MenuAvailability::Enabled,
                ),
                (
                    MenuCommand::Export(ExportTarget::Png),
                    "Export Pages To PNG…",
                    MenuAvailability::Enabled,
                ),
                (
                    MenuCommand::Export(ExportTarget::Svg),
                    "Export Pages To SVG…",
                    MenuAvailability::Enabled,
                ),
                (
                    MenuCommand::Export(ExportTarget::Jpeg),
                    "Export Pages To JPEG…",
                    MenuAvailability::Enabled,
                ),
                (
                    MenuCommand::Export(ExportTarget::Tiff),
                    "Export Pages To TIFF…",
                    MenuAvailability::Enabled,
                ),
            ]
        );
    }

    /// The ids the menu looks up are the ids `codecs-common` registers.
    /// Nothing else checks that: a rename on either side would otherwise ship
    /// as a permanently disabled entry rather than as a build failure.
    #[cfg(feature = "codecs-common")]
    #[test]
    fn every_export_entry_names_a_codec_the_default_build_installs() {
        let registry = crate::build_registry();

        for target in ExportTarget::ALL {
            let codec = registry
                .codec(target.codec())
                .unwrap_or_else(|| panic!("no codec is registered as {}", target.codec()));
            assert!(!codec.extension().is_empty());
        }
    }

    #[test]
    fn export_entries_wait_for_a_document_to_export() {
        let file = &main_menu_schema(menu_state(0, None))[0];

        let availability: Vec<_> = file
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, MenuCommand::Export(_)))
            .map(|entry| entry.availability)
            .collect();

        assert_eq!(
            availability,
            vec![MenuAvailability::Disabled("No document is open"); ExportTarget::ALL.len()]
        );
    }

    /// With the plugin compiled out the entries ship disabled saying so,
    /// rather than offering a format that would fail when it was picked.
    #[test]
    fn a_build_without_the_codecs_plugin_says_so_on_every_export_entry() {
        let state = MenuState::new(
            1,
            Some(view()),
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            RegistryFacts {
                codecs: ExportCodecs::default(),
                ..everything_installed()
            },
            0,
        );

        let availability: Vec<_> = main_menu_schema(state)[0]
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, MenuCommand::Export(_)))
            .map(|entry| entry.availability)
            .collect();

        assert_eq!(
            availability,
            vec![
                MenuAvailability::Disabled("The common codecs plugin is not installed");
                ExportTarget::ALL.len()
            ]
        );
        // The native menu shows them too, disabled and carrying the reason.
        assert!(
            item_names(&native_menus(state)[0])
                .iter()
                .filter(|name| name.contains("(The common codecs plugin is not installed)"))
                .count()
                == ExportTarget::ALL.len()
                    // Export All Images, which is the codecs plugin's too, and
                    // says so when this build left the plugin out.
                    + usize::from(!cfg!(feature = "codecs-common"))
        );
    }

    /// One codec missing disables its own entry and leaves the others
    /// alone, so the state is per format rather than all-or-nothing.
    #[test]
    fn a_missing_codec_disables_only_its_own_entry() {
        let state = MenuState::new(
            1,
            Some(view()),
            ShellViewState::new(WindowAppearance::Dark, ThemePreference::System),
            [true; QuickAction::ALL.len()],
            RegistryFacts {
                codecs: ExportCodecs::installed(|id| id != "svg"),
                ..everything_installed()
            },
            0,
        );

        let availability: Vec<_> = main_menu_schema(state)[0]
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, MenuCommand::Export(_)))
            .map(|entry| entry.availability)
            .collect();

        assert_eq!(
            availability,
            vec![
                MenuAvailability::Enabled,
                MenuAvailability::Enabled,
                MenuAvailability::Disabled("The common codecs plugin is not installed"),
                MenuAvailability::Enabled,
                MenuAvailability::Enabled,
            ]
        );
    }

    #[test]
    fn hamburger_and_native_view_entries_read_the_same_canvas_state() {
        let state = menu_state(1, Some(view()));
        let entries = &main_menu_schema(state)[2].entries;
        let expected_labels: Vec<_> = entries
            .iter()
            .map(|entry| {
                let label = if entry.selected {
                    format!("✓ {}", entry.label)
                } else {
                    entry.label.to_owned()
                };
                match entry.availability {
                    MenuAvailability::Enabled => label,
                    MenuAvailability::Disabled(reason) => format!("{label} ({reason})"),
                }
            })
            .collect();
        let native_labels = item_names(&native_menus(state)[2]);

        assert_eq!(native_labels, expected_labels);
        assert!(native_labels.contains(&"✓ Two Page".to_owned()));
        assert!(native_labels.contains(&"✓ Show Cover Page".to_owned()));
    }

    #[test]
    fn view_menu_commands_map_to_typed_canvas_actions() {
        let view = view();

        assert_eq!(
            MenuCommand::SinglePageContinuous.view_action(view),
            Some(ViewAction::SetLayout(PageLayoutMode::SinglePageContinuous))
        );
        assert_eq!(
            MenuCommand::TwoPageContinuous.view_action(view),
            Some(ViewAction::SetLayout(PageLayoutMode::TwoPageContinuous))
        );
        assert_eq!(
            MenuCommand::ToggleCover.view_action(view),
            Some(ViewAction::SetShowCover(false))
        );
        assert_eq!(MenuCommand::CloseTab.view_action(view), None);
    }

    #[test]
    fn zoom_policy_selects_the_same_actual_or_fit_choice_as_the_controls() {
        let mut actual = view();
        actual.zoom = 1.0;
        let actual_entries = view_entries(Some(actual));
        assert!(
            actual_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::ActualSize)
                .unwrap()
                .selected
        );

        let mut fitted = view();
        fitted.zoom_policy = ZoomPolicy::Fit(FitMode::Width);
        let fitted_entries = view_entries(Some(fitted));
        assert!(
            fitted_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::FitWidth)
                .unwrap()
                .selected
        );
        assert!(
            !fitted_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::ActualSize)
                .unwrap()
                .selected
        );

        // Fit Visible carries a rectangle, so its tick is a match on the
        // variant rather than on the whole value: comparing values would
        // leave the entry never ticked, whatever the viewport is fitting.
        let mut visible = view();
        visible.zoom_policy = ZoomPolicy::Fit(FitMode::Visible(
            onionskin_core::PageRenderRect::new(
                visible.current_page,
                onionskin_core::ViewPoint { x: 10.0, y: 20.0 },
                onionskin_core::ViewSize {
                    width: 30.0,
                    height: 40.0,
                },
                onionskin_core::ViewSize {
                    width: 200.0,
                    height: 100.0,
                },
            )
            .expect("the rectangle is inside the page"),
        ));
        let visible_entries = view_entries(Some(visible));
        assert!(
            visible_entries
                .iter()
                .find(|entry| entry.command == MenuCommand::FitVisible)
                .unwrap()
                .selected
        );
        for other in [
            MenuCommand::FitPage,
            MenuCommand::FitWidth,
            MenuCommand::FitHeight,
            MenuCommand::ActualSize,
        ] {
            assert!(
                !visible_entries
                    .iter()
                    .find(|entry| entry.command == other)
                    .unwrap()
                    .selected,
                "{other:?} is ticked while Fit Visible holds"
            );
        }
    }

    #[test]
    fn shell_view_selections_and_native_labels_share_one_schema() {
        let mut shell_view = ShellViewState::new(WindowAppearance::Dark, ThemePreference::System);
        shell_view.apply(ShellViewAction::ToggleNavigationPane);
        shell_view.apply(ShellViewAction::TogglePageControls);
        shell_view.apply(ShellViewAction::SetTheme(ThemePreference::Light));
        shell_view.apply(ShellViewAction::ToggleReadMode);
        shell_view.set_fullscreen(true);
        let mut quick_actions_visible = [true; QuickAction::ALL.len()];
        quick_actions_visible[QuickAction::Comment.index()] = false;
        let state = MenuState::new(
            1,
            Some(view()),
            shell_view,
            quick_actions_visible,
            everything_installed(),
            0,
        );
        let entries = &main_menu_schema(state)[2].entries;
        let native_labels: Vec<_> = native_menus(state)[2]
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();

        for command in [
            MenuCommand::ToggleNavigationPane,
            MenuCommand::ToggleQuickAction(QuickAction::Comment),
            MenuCommand::TogglePageControls,
        ] {
            assert!(
                !entries
                    .iter()
                    .find(|entry| entry.command == command)
                    .unwrap()
                    .selected
            );
        }
        for command in [
            MenuCommand::ThemeLight,
            MenuCommand::ReadMode,
            MenuCommand::FullScreen,
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry.command == command)
                .unwrap();
            assert!(entry.selected);
            assert!(native_labels.contains(&format!("✓ {}", entry.label)));
        }
    }

    /// Line Weights is a live, checked toggle: its check mark is the
    /// preference, and the native menu has a route that runs it.
    #[test]
    fn line_weights_is_a_checked_toggle_that_runs() {
        let entry_with = |on: bool| {
            view_menu_entries(
                Some(view()),
                ShellViewState::new(WindowAppearance::Dark, ThemePreference::System)
                    .with_line_weights(on),
                [true; QuickAction::ALL.len()],
                true,
                true,
            )
            .into_iter()
            .find(|entry| entry.command == MenuCommand::LineWeights)
            .expect("a Line Weights entry")
        };

        let on = entry_with(true);
        assert_eq!(on.availability, MenuAvailability::Enabled);
        assert!(on.selected, "on by default, as Acrobat's is");
        assert!(!entry_with(false).selected);
        assert!(native_action(MenuCommand::LineWeights).is_some());
        assert_eq!(on.command.view_action(view()), None);
        assert_eq!(on.command.shell_view_action(), None);
    }
}
