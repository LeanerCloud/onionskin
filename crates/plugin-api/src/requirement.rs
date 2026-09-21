//! What has to exist for a menu entry, toolbar action or tool to be live.
//!
//! One query, shared by every shell surface that shows something that might
//! be unavailable. It lived privately in the canvas context menu until M3;
//! it moves here because M3 adds two reasons an entry can be unavailable that
//! are not about the menu at all:
//!
//! - **a command some plugin registers**, [`Requirement::Command`]: the entry
//!   goes live when a plugin carrying that command id is compiled in, with no
//!   change to the shell - which is what turns the context menu's milestone
//!   "guesses" into queries;
//! - **a document that may not be edited**, [`Session::edit_refusal`]: an
//!   encrypted document at M3. An entry that edits reports itself disabled
//!   with the document's own reason, through this same query, rather than
//!   through a second flag someone could forget to consult.

use crate::{PluginRegistry, ToolCapability};

/// Whether an entry is live, and if not, what to tell the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Enabled,
    Disabled(&'static str),
}

impl Availability {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Enabled => None,
            Self::Disabled(reason) => Some(reason),
        }
    }

    fn when(live: bool, reason: &'static str) -> Self {
        if live {
            Self::Enabled
        } else {
            Self::Disabled(reason)
        }
    }
}

/// What has to exist for an entry to be live, and what it says when that
/// thing is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// The shell's own: always live and never needing a reason.
    Shell,
    /// The document has to have text selected.
    TextSelection(&'static str),
    /// Some registered tool has to carry this capability.
    Tool(ToolCapability, &'static str),
    /// Some registered plugin has to provide a command with this id.
    Command {
        id: &'static str,
        reason: &'static str,
    },
    /// Nothing to query: the subsystem behind it is a later milestone.
    Milestone(&'static str),
}

/// What the session knows that a requirement may ask about.
#[derive(Clone, Copy)]
pub struct Session<'a> {
    pub registry: &'a PluginRegistry,
    pub has_text_selection: bool,
    /// Why the open document may not be edited, or `None` when it may. Set from
    /// `core`'s protection query, which derives it from the document; the shell
    /// never sets it by hand.
    pub edit_refusal: Option<&'static str>,
}

impl Requirement {
    /// Whether this requirement is met in `session`.
    ///
    /// An entry that **edits** - a tool whose capability changes the
    /// document - is disabled on a document that may not be edited, with that
    /// document's reason rather than the entry's own. Checked first, because
    /// "this document is encrypted" is the true answer even when the tool is
    /// also missing.
    pub fn availability(self, session: &Session<'_>) -> Availability {
        if let (Some(refusal), true) = (session.edit_refusal, self.edits()) {
            return Availability::Disabled(refusal);
        }
        match self {
            Requirement::Shell => Availability::Enabled,
            Requirement::TextSelection(reason) => {
                Availability::when(session.has_text_selection, reason)
            }
            Requirement::Tool(capability, reason) => {
                Availability::when(tool_with(session.registry, capability).is_some(), reason)
            }
            Requirement::Command { id, reason } => Availability::when(
                session
                    .registry
                    .commands()
                    .iter()
                    .any(|command| command.id == id),
                reason,
            ),
            Requirement::Milestone(reason) => Availability::Disabled(reason),
        }
    }

    /// Whether meeting this requirement leads to an edit of the document.
    pub fn edits(self) -> bool {
        match self {
            Requirement::Tool(capability, _) => capability.edits_document(),
            _ => false,
        }
    }

    /// The tool capability this entry activates, for an entry that is a tool.
    pub fn capability(self) -> Option<ToolCapability> {
        match self {
            Requirement::Tool(capability, _) => Some(capability),
            _ => None,
        }
    }
}

/// The first registered tool carrying `capability`. The one lookup rule,
/// shared by the entry that reports the tool missing and the click that
/// activates it, so the two cannot disagree.
pub fn tool_with(registry: &PluginRegistry, capability: ToolCapability) -> Option<usize> {
    registry
        .tools()
        .position(|tool| tool.capabilities().contains(&capability))
}
