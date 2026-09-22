//! Edit > Cut, Copy, Paste and Delete on the canvas: whatever the active
//! tool says they mean, and disabled with a reason when it says nothing.

use onionskin_plugin_api::{EditVerb, ToolCtx};

use super::CanvasModel;

/// Why an Edit verb is not available, when it is not.
pub fn edit_verb_refusal(tool: Option<&'static str>, verb: EditVerb) -> &'static str {
    match (tool, verb) {
        (None, _) => "Choose a tool first",
        (Some(_), EditVerb::Cut) => "The active tool has nothing to cut",
        (Some(_), EditVerb::Copy) => "The active tool has nothing to copy",
        (Some(_), EditVerb::Paste) => "The active tool does not paste",
        (Some(_), EditVerb::Delete) => "The active tool has nothing to delete",
    }
}

impl CanvasModel {
    /// Whether the active tool answers `verb`, and its name for the reason
    /// when it does not.
    pub fn edit_verb_availability(&self, verb: EditVerb) -> Result<(), &'static str> {
        let tool = self.active_tool.and_then(|index| self.registry.tool(index));
        match tool {
            Some(tool) if tool.claims(verb) => Ok(()),
            Some(tool) => Err(edit_verb_refusal(Some(tool.name()), verb)),
            None => Err(edit_verb_refusal(None, verb)),
        }
    }

    /// Run `verb` on the active tool. Returns the text for the clipboard,
    /// when the verb produced any.
    pub fn run_edit_verb(&mut self, verb: EditVerb, pasted: Option<&str>) -> Option<String> {
        self.edit_verb_availability(verb).ok()?;
        let index = self.active_tool?;
        let mut file = self.document.borrow_mut();
        let document = file.document_mut();
        let viewport = &mut self.viewport;
        let tool = self.registry.tool_mut(index)?;
        tool.edit(
            &mut ToolCtx {
                doc: document,
                viewport,
            },
            verb,
            pasted,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_says_why_it_is_off() {
        for verb in EditVerb::ALL {
            assert_eq!(edit_verb_refusal(None, verb), "Choose a tool first");
            assert!(edit_verb_refusal(Some("Draw"), verb).starts_with("The active tool"));
        }
    }
}
