//! The page canvas and text-selection context menu.
//!
//! Acrobat parity row 225 counts this menu once and names every entry, so
//! every entry ships. What decides whether one is live is a query wherever
//! there is something to query: an entry that is a tool asks the registry
//! whether any plugin carries its capability, and an entry that acts on the
//! selection asks the document. Those go live on their own.
//!
//! The rest carry a static milestone reason, and that part is a list, so it
//! is worth being exact about why. `ToolCapability` names the three tool
//! entries today, so those are a real query. Nothing names the others yet:
//! `Command` ids are chosen by the plugin that registers them, and
//! `commands-core`, `tools-edit`, `redact` and `tools-organize` register
//! nothing, so there is no id to ask for. Guessing one would read as a live
//! query while being just as stale. When those plugins contribute commands,
//! `requirement()` is where the guesses become queries.

use onionskin_plugin_api::{PluginRegistry, ToolCapability};

use super::chrome::MenuAvailability;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum CanvasContextCommand {
    Copy,
    CopyWithFormatting,
    ExportSelectionAs,
    HighlightText,
    AddNoteToText,
    EditText,
    RedactText,
    CreateLink,
    TakeASnapshot,
    AddBookmark,
    RotateClockwise,
    Print,
    PageCommands,
}

/// What has to exist for an entry to be live, and what it says when that
/// thing is missing. The milestone reasons follow `ACROBAT-PARITY.md`:
/// rich-text copy and selection export are their own row at M3, and
/// File > Print is row 139, M3, waiting on `crates/print`.
///
/// `Milestone` is the variant to delete from as subsystems land; see the
/// module docs for why it is a list and not a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Requirement {
    /// The shell's own, so always live and never needing a reason.
    Shell,
    /// The document has to have text selected.
    TextSelection(&'static str),
    /// Some registered tool has to carry this capability.
    Tool(ToolCapability, &'static str),
    /// Nothing to query: the subsystem behind it is a later milestone.
    Milestone(&'static str),
}

impl CanvasContextCommand {
    /// Menu order, which is Acrobat's.
    pub(in crate::shell) const ALL: [Self; 13] = [
        Self::Copy,
        Self::CopyWithFormatting,
        Self::ExportSelectionAs,
        Self::HighlightText,
        Self::AddNoteToText,
        Self::EditText,
        Self::RedactText,
        Self::CreateLink,
        Self::TakeASnapshot,
        Self::AddBookmark,
        Self::RotateClockwise,
        Self::Print,
        Self::PageCommands,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::CopyWithFormatting => "Copy With Formatting",
            Self::ExportSelectionAs => "Export Selection As",
            Self::HighlightText => "Highlight Text",
            Self::AddNoteToText => "Add Note To Text",
            Self::EditText => "Edit Text",
            Self::RedactText => "Redact Text",
            Self::CreateLink => "Create Link",
            Self::TakeASnapshot => "Take A Snapshot",
            Self::AddBookmark => "Add Bookmark",
            Self::RotateClockwise => "Rotate Clockwise",
            Self::Print => "Print",
            Self::PageCommands => "Page Commands",
        }
    }

    /// The tool capability this entry activates, for the entries that are a
    /// tool rather than a command. The shell never names a tool by id: a
    /// plugin that arrives later carrying the capability makes the entry
    /// live with no change here.
    pub(in crate::shell) fn capability(self) -> Option<ToolCapability> {
        match self.requirement() {
            Requirement::Tool(capability, _) => Some(capability),
            Requirement::Shell | Requirement::TextSelection(_) | Requirement::Milestone(_) => None,
        }
    }

    fn requirement(self) -> Requirement {
        use ToolCapability::{Comment, Highlight, Snapshot};

        match self {
            Self::Copy => Requirement::TextSelection("Select text first"),
            Self::HighlightText => Requirement::Tool(Highlight, "Available in M3 tools-comment"),
            Self::AddNoteToText => Requirement::Tool(Comment, "Available in M3 tools-comment"),
            Self::TakeASnapshot => Requirement::Tool(Snapshot, "Available in P10 tools-basic"),
            Self::RotateClockwise => Requirement::Shell,
            Self::CopyWithFormatting | Self::ExportSelectionAs => {
                Requirement::Milestone("Available in M3 with rich-text export")
            }
            Self::EditText => Requirement::Milestone("Available in M5 tools-edit"),
            Self::RedactText => Requirement::Milestone("Available in M5 redact"),
            Self::CreateLink => Requirement::Milestone("Available in M5 commands-core"),
            Self::AddBookmark => Requirement::Milestone("Available in M3 commands-core"),
            Self::Print => Requirement::Milestone("Available in M3 with crates/print"),
            Self::PageCommands => Requirement::Milestone("Available in M3 tools-organize"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct CanvasContextEntry {
    pub(in crate::shell) command: CanvasContextCommand,
    pub(in crate::shell) label: &'static str,
    pub(in crate::shell) availability: MenuAvailability,
}

/// The first registered tool carrying `capability`. The one lookup rule,
/// shared by the entry that reports the tool missing and the click that
/// activates it, so the two cannot disagree.
pub(in crate::shell) fn tool_with(
    registry: &PluginRegistry,
    capability: ToolCapability,
) -> Option<usize> {
    registry
        .tools()
        .position(|tool| tool.capabilities().contains(&capability))
}

pub(in crate::shell) fn canvas_context_entries(
    registry: &PluginRegistry,
    has_text_selection: bool,
) -> Vec<CanvasContextEntry> {
    CanvasContextCommand::ALL
        .into_iter()
        .map(|command| CanvasContextEntry {
            command,
            label: command.label(),
            availability: match command.requirement() {
                Requirement::Shell => MenuAvailability::Enabled,
                Requirement::TextSelection(reason) => available(has_text_selection, reason),
                Requirement::Tool(capability, reason) => {
                    available(tool_with(registry, capability).is_some(), reason)
                }
                Requirement::Milestone(reason) => MenuAvailability::Disabled(reason),
            },
        })
        .collect()
}

fn available(live: bool, reason: &'static str) -> MenuAvailability {
    if live {
        MenuAvailability::Enabled
    } else {
        MenuAvailability::Disabled(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(registry: &PluginRegistry, has_text_selection: bool) -> Vec<CanvasContextCommand> {
        canvas_context_entries(registry, has_text_selection)
            .into_iter()
            .filter(|entry| entry.availability.is_enabled())
            .map(|entry| entry.command)
            .collect()
    }

    /// Parity row 225 counts the menu once and names every entry, so an
    /// entry that is not live has to be present and disabled rather than
    /// missing: absence reads as "Onionskin does not have this".
    #[test]
    fn every_named_entry_is_present_and_every_disabled_one_says_why() {
        let entries = canvas_context_entries(&crate::build_registry(), false);

        // Pinned to the count in the row itself, not to `ALL`, which would
        // agree with itself while quietly dropping an entry.
        assert_eq!(entries.len(), 13);
        for entry in entries {
            assert!(!entry.label.is_empty());
            if !entry.availability.is_enabled() {
                let reason = entry
                    .availability
                    .reason()
                    .expect("a disabled entry says why");
                assert!(!reason.is_empty(), "{} has an empty reason", entry.label);
            }
        }
    }

    /// The entries that exist at M2: view rotation is the shell's own, the
    /// snapshot tool is registered by `tools-basic`, and Copy waits for a
    /// selection to copy.
    #[test]
    fn the_registry_and_the_selection_decide_which_entries_are_live() {
        let registry = crate::build_registry();
        let with_snapshot_tool = cfg!(feature = "tools-basic");

        let mut expected = vec![CanvasContextCommand::RotateClockwise];
        if with_snapshot_tool {
            expected.insert(0, CanvasContextCommand::TakeASnapshot);
        }
        assert_eq!(live(&registry, false), expected);

        expected.insert(0, CanvasContextCommand::Copy);
        assert_eq!(live(&registry, true), expected);
        assert_eq!(
            tool_with(&registry, ToolCapability::Snapshot).is_some(),
            with_snapshot_tool
        );
    }

    /// Print and Add Bookmark are the two the plan calls out by name. Both
    /// wait on a milestone rather than on a capability, so both ship
    /// disabled and say which milestone.
    ///
    /// Asserted on the milestone the reason names rather than on the whole
    /// string: pinning the prose would keep passing once the subsystem
    /// lands, which is the state this test exists to catch.
    #[test]
    fn print_and_add_bookmark_are_disabled_with_their_milestone() {
        let entries = canvas_context_entries(&crate::build_registry(), true);
        let reason = |command| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .expect("the entry is present")
                .availability
                .reason()
                .expect("a disabled entry says why")
        };

        for command in [
            CanvasContextCommand::Print,
            CanvasContextCommand::AddBookmark,
        ] {
            assert!(
                reason(command).contains("M3"),
                "{} should name the milestone it waits on, said {:?}",
                command.label(),
                reason(command)
            );
        }
    }

    #[test]
    fn future_editing_entries_name_their_m5_milestone() {
        let entries = canvas_context_entries(&crate::build_registry(), true);
        let reason = |command| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .expect("the entry is present")
                .availability
                .reason()
                .expect("a disabled entry says why")
        };

        for command in [
            CanvasContextCommand::EditText,
            CanvasContextCommand::RedactText,
            CanvasContextCommand::CreateLink,
        ] {
            assert!(
                reason(command).contains("M5"),
                "{} should name the milestone it waits on, said {:?}",
                command.label(),
                reason(command)
            );
        }
    }

    /// The comment entries carry no hardcoded milestone gate: registering a
    /// tool with the capability makes them live with no change here, the
    /// way the Select quick action went live when `tools-basic` landed.
    #[test]
    fn a_tool_capability_makes_its_entry_live_with_no_application_change() {
        use onionskin_plugin_api::{PointerInput, ToolCtx, ToolPlugin};

        struct CommentTool;

        impl ToolPlugin for CommentTool {
            fn id(&self) -> &'static str {
                "comment"
            }

            fn name(&self) -> &'static str {
                "Comment"
            }

            fn icon(&self) -> &'static str {
                "comment"
            }

            fn capabilities(&self) -> &'static [ToolCapability] {
                &[ToolCapability::Comment]
            }

            fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

            fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

            fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
        }

        let mut registry = PluginRegistry::new();
        assert!(!live(&registry, false).contains(&CanvasContextCommand::AddNoteToText));

        registry.register_tool(Box::new(CommentTool));

        assert!(live(&registry, false).contains(&CanvasContextCommand::AddNoteToText));
        assert!(!live(&registry, false).contains(&CanvasContextCommand::HighlightText));
    }
}
