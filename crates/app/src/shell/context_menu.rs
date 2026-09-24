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

use onionskin_plugin_api::command_ids;
use onionskin_plugin_api::{PluginRegistry, Requirement, Session, ToolCapability};

use super::chrome::MenuAvailability;

pub(in crate::shell) use onionskin_plugin_api::tool_with;

/// Why Copy With Formatting stays greyed out.
pub(in crate::shell) const COPY_WITH_FORMATTING_REASON: &str =
    "After 1.0: the clipboard has no rich-text format yet; use Export Selection As";

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
    RotatePage,
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
        Self::RotatePage,
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
            Self::RotatePage => "Rotate Page",
        }
    }

    /// The tool capability this entry activates, for the entries that are a
    /// tool rather than a command. The shell never names a tool by id: a
    /// plugin that arrives later carrying the capability makes the entry
    /// live with no change here.
    pub(in crate::shell) fn capability(self) -> Option<ToolCapability> {
        self.requirement().capability()
    }

    /// The registered command this entry runs, for the entries that are a
    /// command rather than a tool: the id its availability asked about, so the
    /// entry runs exactly what made it live.
    pub(in crate::shell) fn command_id(self) -> Option<&'static str> {
        self.requirement().command_id()
    }

    fn requirement(self) -> Requirement {
        use ToolCapability::{Comment, Highlight, Snapshot};

        match self {
            Self::Copy => Requirement::TextSelection("Select text first"),
            Self::HighlightText => Requirement::Tool(Highlight, "Available in M3 tools-comment"),
            Self::AddNoteToText => Requirement::Tool(Comment, "Available in M3 tools-comment"),
            Self::TakeASnapshot => Requirement::Tool(Snapshot, "Available in P10 tools-basic"),
            Self::RotateClockwise => Requirement::Shell,
            // The four that named M3 as a guess are registry queries now: each
            // goes live when the package that lands its command is compiled in.
            // The clipboard half of row 31: GPUI's clipboard carries text
            // and images only, so there is no rich-text flavour to put this
            // on until a fork adds one. Export Selection As writes the RTF.
            Self::CopyWithFormatting => Requirement::Milestone(COPY_WITH_FORMATTING_REASON),
            // The shell's own save prompt and writer, like Print.
            Self::ExportSelectionAs => Requirement::TextSelection("Select text first"),
            Self::AddBookmark => Requirement::Command {
                id: command_ids::ADD_BOOKMARK,
                reason: "Available with bookmark authoring",
            },
            // The shell's own dialog, as File > Print, not a registry command.
            Self::Print => Requirement::Shell,
            // The page itself, where Rotate Clockwise above turns the view:
            // this one writes `/Rotate` and is saved with the document.
            Self::RotatePage => Requirement::Command {
                id: command_ids::ROTATE_PAGE_CLOCKWISE,
                reason: "Available with page organization",
            },
            // Chooses the Edit Text tool, whose click edits a line.
            Self::EditText => Requirement::Tool(
                ToolCapability::EditText,
                "Available with the Edit Text tool",
            ),
            // The selection marked for redaction; with nothing selected, the
            // Redact tool itself.
            Self::RedactText => {
                Requirement::Tool(ToolCapability::Redact, "Available with the Redact tool")
            }
            // The selection's own link, through the Link tool's dialog; with
            // nothing selected, the Link tool itself.
            Self::CreateLink => {
                Requirement::Tool(ToolCapability::Link, "Available with the Link tool")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct CanvasContextEntry {
    pub(in crate::shell) command: CanvasContextCommand,
    pub(in crate::shell) label: &'static str,
    pub(in crate::shell) availability: MenuAvailability,
}

/// Why the open document refuses what an entry would do, from `core`'s
/// protection queries. Default is a document that refuses nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::shell) struct Refusals {
    pub(in crate::shell) edit: Option<&'static str>,
    /// Why comments are refused, which a document can allow where it
    /// refuses every other change.
    pub(in crate::shell) comment: Option<&'static str>,
    pub(in crate::shell) read_out: Option<&'static str>,
}

impl Refusals {
    /// Why a tool with `capability` is refused, if it is.
    pub(in crate::shell) fn for_capability(
        self,
        capability: onionskin_plugin_api::ToolCapability,
    ) -> Option<&'static str> {
        match capability.edit_kind() {
            Some(onionskin_core::protection::EditKind::Comments) => self.comment,
            Some(_) => self.edit,
            None => None,
        }
    }
}

pub(in crate::shell) fn canvas_context_entries(
    registry: &PluginRegistry,
    has_text_selection: bool,
    refusals: Refusals,
) -> Vec<CanvasContextEntry> {
    let session = Session {
        registry,
        has_text_selection,
        edit_refusal: refusals.edit,
        comment_refusal: refusals.comment,
        read_out_refusal: refusals.read_out,
    };
    CanvasContextCommand::ALL
        .into_iter()
        .map(|command| CanvasContextEntry {
            command,
            label: command.label(),
            availability: command.requirement().availability(&session),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(registry: &PluginRegistry, has_text_selection: bool) -> Vec<CanvasContextCommand> {
        canvas_context_entries(registry, has_text_selection, Refusals::default())
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
        let entries = canvas_context_entries(&crate::build_registry(), false, Refusals::default());

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

    /// What is live is decided by the registry and the selection, never by a
    /// list kept here: view rotation is the shell's own, everything else needs
    /// a tool that declares the capability, and Copy needs something selected.
    ///
    /// Derived rather than written out, because a fixed list has to be edited
    /// by every tool package that lands a capability - and the edit that
    /// matters, a capability that stopped being reachable, looks exactly like
    /// the edit that does not. What is pinned here is the rule.
    #[test]
    fn the_registry_and_the_selection_decide_which_entries_are_live() {
        let registry = crate::build_registry();

        let expected: Vec<CanvasContextCommand> = CanvasContextCommand::ALL
            .into_iter()
            .filter(|command| match command.requirement() {
                Requirement::Shell => true,
                Requirement::TextSelection(..) => false,
                Requirement::Tool(capability, _) => tool_with(&registry, capability).is_some(),
                Requirement::Command { id, .. } => {
                    registry.commands().iter().any(|command| command.id == id)
                }
                Requirement::Milestone(..) => false,
            })
            .collect();
        assert_eq!(live(&registry, false), expected);

        let with_selection: Vec<CanvasContextCommand> = CanvasContextCommand::ALL
            .into_iter()
            .filter(|command| {
                expected.contains(command)
                    || matches!(command.requirement(), Requirement::TextSelection(..))
            })
            .collect();
        assert!(with_selection.contains(&CanvasContextCommand::Copy));
        assert!(with_selection.contains(&CanvasContextCommand::ExportSelectionAs));
        assert_eq!(live(&registry, true), with_selection);

        // The rule is only worth anything if it separates the two cases, so
        // both sides of it have to be non-empty in this build.
        assert!(
            expected.contains(&CanvasContextCommand::RotateClockwise),
            "an entry needing no tool is live"
        );
        assert_eq!(
            tool_with(&registry, ToolCapability::Snapshot).is_some(),
            cfg!(feature = "tools-basic"),
            "tools-basic is what registers the snapshot tool"
        );
    }

    /// No entry still waits on "M3" by milestone: every one M3 lands is a
    /// registry query now, and goes live when its package's command is
    /// compiled in. A reason naming M3 is a guess that outlived its milestone.
    #[test]
    fn no_milestone_reason_names_m3() {
        for command in CanvasContextCommand::ALL {
            if let Requirement::Milestone(reason) = command.requirement() {
                assert!(
                    !reason.contains("M3"),
                    "{} still waits on M3 by milestone rather than by query: {reason:?}",
                    command.label()
                );
            }
        }
    }

    /// Print and Add Bookmark are the two the plan calls out by name. Print
    /// is the shell's own dialog now (P17), live on any open document; Add
    /// Bookmark is still a query, live once a command registers its id.
    #[test]
    fn print_is_live_and_add_bookmark_goes_live_when_its_command_is_registered() {
        let mut registry = PluginRegistry::new();
        let entry = |entries: &[CanvasContextEntry], command| {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .expect("present")
                .availability
        };
        let before = canvas_context_entries(&registry, true, Refusals::default());
        assert!(entry(&before, CanvasContextCommand::Print).is_enabled());
        assert_eq!(
            entry(&before, CanvasContextCommand::AddBookmark).reason(),
            Some("Available with bookmark authoring")
        );

        struct Stub;
        impl onionskin_plugin_api::CommandPlugin for Stub {
            fn commands(&self) -> Vec<onionskin_plugin_api::Command> {
                vec![onionskin_plugin_api::Command {
                    id: command_ids::ADD_BOOKMARK,
                    title: command_ids::ADD_BOOKMARK,
                    keybind: None,
                    effect: onionskin_plugin_api::CommandEffect::Reads,
                    run: Box::new(|_| Ok(())),
                }]
            }
        }
        registry.register_commands(&Stub);
        let after = canvas_context_entries(&registry, true, Refusals::default());
        assert!(entry(&after, CanvasContextCommand::AddBookmark).is_enabled());
    }

    /// P1b's editing gate, reached through the same query: on a document that
    /// may not be edited, every entry that edits is disabled with the
    /// document's reason, and every entry that does not is untouched.
    #[test]
    fn a_document_that_may_not_be_edited_disables_exactly_the_entries_that_edit() {
        let registry = crate::build_registry();
        let refusal = "Security: changes are not allowed";
        let open = canvas_context_entries(&registry, true, Refusals::default());
        let locked = canvas_context_entries(
            &registry,
            true,
            Refusals {
                edit: Some(refusal),
                comment: Some(refusal),
                read_out: None,
            },
        );

        for (free, gated) in open.iter().zip(&locked) {
            if free.command.requirement().edits(&registry) {
                assert_eq!(
                    gated.availability.reason(),
                    Some(refusal),
                    "{} edits and is not gated",
                    free.command.label()
                );
            } else {
                assert_eq!(
                    gated.availability,
                    free.availability,
                    "{} does not edit and changed",
                    free.command.label()
                );
            }
        }
        assert!(
            open.iter()
                .any(|entry| entry.command.requirement().edits(&registry)),
            "the menu has to contain an editing entry, or this proves nothing"
        );
    }

    /// Edit Text chooses the Edit Text tool, so it is live where that tool
    /// is installed and says which tool it waits on where it is not.
    #[test]
    fn edit_text_is_live_with_the_edit_text_tool() {
        let entries = canvas_context_entries(&crate::build_registry(), true, Refusals::default());
        let entry = entries
            .iter()
            .find(|entry| entry.command == CanvasContextCommand::EditText)
            .expect("the entry is present");
        if cfg!(feature = "tools-edit") {
            assert!(entry.availability.is_enabled());
        } else {
            assert_eq!(
                entry.availability.reason(),
                Some("Available with the Edit Text tool")
            );
        }
        assert_eq!(
            CanvasContextCommand::EditText.capability(),
            Some(ToolCapability::EditText)
        );
    }

    /// Create Link is live wherever a tool makes links, with or without a
    /// selection: with none it chooses the Link tool.
    #[cfg(feature = "tools-edit")]
    #[test]
    fn create_link_is_live_with_the_link_tool() {
        for selected in [true, false] {
            let entries =
                canvas_context_entries(&crate::build_registry(), selected, Refusals::default());
            let entry = entries
                .iter()
                .find(|entry| entry.command == CanvasContextCommand::CreateLink)
                .expect("present");
            assert!(entry.availability.is_enabled(), "{selected}");
        }
    }

    /// Redact Text is live wherever a tool marks for redaction.
    #[cfg(feature = "redact")]
    #[test]
    fn redact_text_is_live_with_the_redact_tool() {
        for selected in [true, false] {
            let entries =
                canvas_context_entries(&crate::build_registry(), selected, Refusals::default());
            let entry = entries
                .iter()
                .find(|entry| entry.command == CanvasContextCommand::RedactText)
                .expect("present");
            assert!(entry.availability.is_enabled(), "{selected}");
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
