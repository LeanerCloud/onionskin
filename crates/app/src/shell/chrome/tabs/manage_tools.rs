//! The frame's half of Manage Tools: opening the dialog and applying a
//! checkbox to the preferences and the rail.

use gpui::{Context, Window};

use super::ShellFrame;
use crate::shell::chrome::manage_tools::{self, ManagedTool};
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    /// List the rail tools of the document in front, or of a fresh registry
    /// on Home, and show the dialog.
    pub(super) fn open_manage_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.managed_tools = match self.active_canvas() {
            Some(canvas) => manage_tools::managed_tools(canvas.read(cx).model.registry()),
            None => manage_tools::managed_tools(&crate::build_registry()),
        };
        self.show_dialog(ShellDialog::ManageTools, window, cx);
    }

    pub(in crate::shell) fn managed_tools(&self) -> &[ManagedTool] {
        &self.managed_tools
    }

    /// Show or hide one tool's rail button, and keep the choice.
    pub(super) fn toggle_tool_shown(&mut self, id: &str, cx: &mut Context<Self>) {
        manage_tools::toggle(&mut self.settings.preferences.hidden_tools, id);
        self.save_preferences();
        cx.notify();
    }
}
