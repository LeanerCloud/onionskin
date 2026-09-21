//! The plugin registry: the kernel's catalog of everything installed.

use std::sync::Arc;

use crate::{CodecPlugin, Command, CommandPlugin, ToolPlugin};

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
    codecs: Vec<Arc<dyn CodecPlugin + Send + Sync>>,
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

    /// Record an export format.
    ///
    /// This is the surface a menu or a context menu binds an export entry to:
    /// a `Command` cannot carry a destination or report which page failed, so
    /// exports are looked up here and run through [`PluginRegistry::codec`].
    pub fn register_codec(&mut self, codec: Box<dyn CodecPlugin + Send + Sync>) {
        assert!(!codec.id().is_empty(), "{} has an empty id", codec.name());
        assert!(
            !codec.extension().is_empty(),
            "{} has no filename extension",
            codec.id()
        );
        assert!(
            !self.codecs.iter().any(|c| c.id() == codec.id()),
            "duplicate codec id {}",
            codec.id()
        );
        self.codecs.push(Arc::from(codec));
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

    /// Hand every tool the shell's environment.
    pub fn configure_tools(&mut self, environment: &crate::ToolEnvironment) {
        for tool in &mut self.tools {
            tool.configure(environment);
        }
    }

    pub fn tool(&self, index: usize) -> Option<&dyn ToolPlugin> {
        self.tools.get(index).map(|tool| tool.as_ref())
    }

    pub fn tool_mut(&mut self, index: usize) -> Option<&mut (dyn ToolPlugin + '_)> {
        match self.tools.get_mut(index) {
            Some(tool) => Some(tool.as_mut()),
            None => None,
        }
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn codecs(&self) -> impl Iterator<Item = &(dyn CodecPlugin + Send + Sync)> {
        self.codecs.iter().map(|c| c.as_ref())
    }

    pub fn codec(&self, id: &str) -> Option<Arc<dyn CodecPlugin + Send + Sync>> {
        self.codecs
            .iter()
            .find(|codec| codec.id() == id)
            .map(Arc::clone)
    }

    /// The codec that imports `bytes`, recognised by their signature.
    pub fn importer(&self, bytes: &[u8]) -> Option<Arc<dyn CodecPlugin + Send + Sync>> {
        self.codecs
            .iter()
            .find(|codec| codec.reads(bytes))
            .map(Arc::clone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Document, ExportError, ExportOutputKind, ExportRequest, PageIndex, PointerInput, ToolCtx,
    };

    struct TestTool;

    impl ToolPlugin for TestTool {
        fn id(&self) -> &'static str {
            "test"
        }

        fn name(&self) -> &'static str {
            "Test"
        }

        fn icon(&self) -> &'static str {
            "test"
        }

        fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

        fn on_pointer_up(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}
    }

    struct TestCodec;

    impl CodecPlugin for TestCodec {
        fn id(&self) -> &'static str {
            "test"
        }

        fn name(&self) -> &'static str {
            "Test"
        }

        fn extension(&self) -> &'static str {
            "test"
        }

        fn output_kind(&self) -> ExportOutputKind {
            ExportOutputKind::Single
        }

        fn export_page(
            &self,
            _doc: &mut Document,
            _request: &ExportRequest,
            _page: PageIndex,
            _first_in_request: bool,
        ) -> Result<Vec<u8>, ExportError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn a_registered_codec_is_found_by_its_id() {
        let mut registry = PluginRegistry::new();
        registry.register_codec(Box::new(TestCodec));

        assert_eq!(registry.codecs().count(), 1);
        assert_eq!(
            registry.codec("test").expect("codec is installed").id(),
            "test"
        );
        assert!(registry.codec("png").is_none());
    }

    struct Importing;

    impl CodecPlugin for Importing {
        fn id(&self) -> &'static str {
            "importing"
        }
        fn name(&self) -> &'static str {
            "Importing"
        }
        fn extension(&self) -> &'static str {
            "imp"
        }
        fn output_kind(&self) -> ExportOutputKind {
            ExportOutputKind::Single
        }
        fn export_page(
            &self,
            _doc: &mut Document,
            _request: &ExportRequest,
            _page: PageIndex,
            _first_in_request: bool,
        ) -> Result<Vec<u8>, ExportError> {
            Ok(Vec::new())
        }
        fn imports(&self) -> bool {
            true
        }
        fn reads(&self, bytes: &[u8]) -> bool {
            bytes.starts_with(b"IMP")
        }
    }

    #[test]
    fn an_importer_is_found_by_signature_and_an_export_only_codec_imports_nothing() {
        let mut registry = PluginRegistry::new();
        registry.register_codec(Box::new(TestCodec));
        registry.register_codec(Box::new(Importing));

        assert_eq!(
            registry.importer(b"IMP...").expect("recognised").id(),
            "importing"
        );
        assert!(registry.importer(b"%PDF-").is_none());
        assert!(matches!(
            TestCodec.import(b"anything"),
            Err(crate::ImportError::NotImported)
        ));
        assert!(!TestCodec.reads(b"IMP"));
        assert!(!TestCodec.imports());
        assert!(registry.codecs().any(|codec| codec.imports()));
    }

    #[test]
    fn codec_lookups_share_the_exact_registered_handle() {
        let mut registry = PluginRegistry::new();
        registry.register_codec(Box::new(TestCodec));

        let first = registry.codec("test").expect("codec is installed");
        let second = registry.codec("test").expect("codec is installed");

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    #[should_panic(expected = "duplicate codec id test")]
    fn two_codecs_cannot_claim_one_format() {
        let mut registry = PluginRegistry::new();
        registry.register_codec(Box::new(TestCodec));
        registry.register_codec(Box::new(TestCodec));
    }

    #[test]
    fn a_registered_tool_can_be_borrowed_mutably_by_index() {
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(TestTool));

        assert_eq!(registry.tool_mut(0).unwrap().id(), "test");
        assert!(registry.tool_mut(1).is_none());
    }
}
