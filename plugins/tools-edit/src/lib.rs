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
mod image_tool;
pub mod images;
mod link_tool;
pub mod links;
pub mod marks;
pub mod text;
mod text_tool;

pub use crop::{crop_pages, crop_to_content, crop_to_rect, white_margins, CropPages};
pub use crop_tool::CropTool;
pub use image_tool::{AddImageTool, EditImageTool};
pub use link_tool::LinkTool;
pub use text_tool::{AddTextTool, EditTextTool};

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
        registry.register_tool(Box::new(EditImageTool::new()));
        registry.register_tool(Box::new(AddImageTool::new()));
        registry.register_tool(Box::new(EditTextTool::new()));
        registry.register_tool(Box::new(AddTextTool::new()));
    }
}

impl CommandPlugin for EditToolsPlugin {
    fn commands(&self) -> Vec<Command> {
        use onionskin_plugin_api::command_ids as ids;
        let image = |id, title, run: fn(&mut onionskin_core::Document) -> _| Command {
            id,
            title,
            keybind: None,
            effect: CommandEffect::Edits,
            run: Box::new(move |ctx| run(ctx.doc)),
        };
        vec![
            Command {
                id: ids::CROP_PAGES,
                title: "Crop Page to Its Content",
                keybind: None,
                effect: CommandEffect::Edits,
                run: Box::new(|ctx| crop_to_content(ctx.doc, &[ctx.page], PageBox::Crop)),
            },
            image(
                ids::ROTATE_IMAGE_CLOCKWISE,
                "Rotate Image Clockwise",
                |doc| images::rotate_selected(doc, true),
            ),
            image(
                ids::ROTATE_IMAGE_COUNTERCLOCKWISE,
                "Rotate Image Counterclockwise",
                |doc| images::rotate_selected(doc, false),
            ),
            image(ids::FLIP_IMAGE_HORIZONTAL, "Flip Image Horizontal", |doc| {
                images::flip_selected(doc, true)
            }),
            image(ids::FLIP_IMAGE_VERTICAL, "Flip Image Vertical", |doc| {
                images::flip_selected(doc, false)
            }),
        ]
    }
}
