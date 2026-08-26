//! The plugin registry: the kernel's catalog of everything installed.

use crate::{Command, CommandPlugin, ToolPlugin};

/// One plugin crate's entry point.
pub trait PluginManifest {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn register(&self, registry: &mut PluginRegistry);
}

/// What the registry keeps about an installed plugin, so the app, the CLI
/// and MCP's `describe` can enumerate the set without holding manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginEntry {
    pub id: &'static str,
    pub name: &'static str,
}

/// Everything registered, assembled at startup from each enabled
/// `PluginManifest`. Empty is a valid state: with no plugins the kernel
/// still boots, to a workspace that can do nothing.
#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<PluginEntry>,
    tools: Vec<Box<dyn ToolPlugin>>,
    commands: Vec<Command>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a manifest and let it register its contributions.
    ///
    /// Panics on an empty or duplicate id. The set is assembled from
    /// compiled-in crates, so either is a build mistake to surface at once
    /// rather than a runtime condition to degrade around. When third-party
    /// WASM plugins arrive they load at runtime from untrusted files, so
    /// that path needs a `Result` rather than these asserts.
    pub fn install(&mut self, manifest: &dyn PluginManifest) {
        assert!(!manifest.id().is_empty(), "plugin has an empty id");
        assert!(
            !self.plugins.iter().any(|p| p.id == manifest.id()),
            "duplicate plugin id {}",
            manifest.id()
        );
        self.plugins.push(PluginEntry {
            id: manifest.id(),
            name: manifest.name(),
        });
        manifest.register(self);
    }

    pub fn register_tool(&mut self, tool: Box<dyn ToolPlugin>) {
        assert!(!tool.id().is_empty(), "{} has an empty id", tool.name());
        assert!(
            !self.tools.iter().any(|t| t.id() == tool.id()),
            "duplicate tool id {}",
            tool.id()
        );
        self.tools.push(tool);
    }

    pub fn register_commands(&mut self, plugin: &dyn CommandPlugin) {
        for command in plugin.commands() {
            assert!(!command.id.is_empty(), "{} has an empty id", command.title);
            assert!(
                !self.commands.iter().any(|c| c.id == command.id),
                "duplicate command id {}",
                command.id
            );
            self.commands.push(command);
        }
    }

    pub fn plugins(&self) -> &[PluginEntry] {
        &self.plugins
    }

    pub fn tools(&self) -> impl Iterator<Item = &dyn ToolPlugin> {
        self.tools.iter().map(|t| t.as_ref())
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
}
