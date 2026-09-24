//! Acrobat's Edit PDF toolset: line-level text editing backed by
//! `text-engine` font matching, image replace and transform, links,
//! headers and footers, watermarks, Bates numbering and crop. Reflowing
//! text edit is out of scope pre-1.0.
//!
//! What is here so far is crop: [`crop_pages`] sets a page box from margins
//! for the Crop Pages dialog, [`white_margins`] measures the margins that
//! fit a page to what it draws, and [`CropTool`] crops a page to a rectangle
//! drawn on it.

use onionskin_core::pages::PageBox;
use onionskin_plugin_api::{Command, CommandEffect, CommandPlugin, PluginManifest, PluginRegistry};

mod crop;
mod crop_tool;
mod link_tool;
pub mod links;
pub mod marks;
mod marquee;

pub use crop::{crop_pages, crop_to_content, crop_to_rect, white_margins, CropPages};
pub use crop_tool::CropTool;
pub use link_tool::LinkTool;

pub struct EditToolsPlugin;

impl PluginManifest for EditToolsPlugin {
    fn id(&self) -> &'static str {
        "onionskin.tools-edit"
    }

    fn name(&self) -> &'static str {
        "Edit PDF"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_commands(self);
        registry.register_tool(Box::new(CropTool::new()));
        registry.register_tool(Box::new(LinkTool::new()));
    }
}

impl CommandPlugin for EditToolsPlugin {
    fn commands(&self) -> Vec<Command> {
        vec![Command {
            id: onionskin_plugin_api::command_ids::CROP_PAGES,
            title: "Crop Page to Its Content",
            keybind: None,
            effect: CommandEffect::Edits,
            run: Box::new(|ctx| crop_to_content(ctx.doc, &[ctx.page], PageBox::Crop)),
        }]
    }
}
