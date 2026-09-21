//! Attach File as a comment: a `/FileAttachment` annotation carrying the file.
//!
//! **The file is chosen before the click.** A tool cannot open a file dialog,
//! so this one declares [`ToolCapability::ChoosesFile`]: the shell asks for a
//! file when the tool is chosen and hands its path to [`ToolPlugin::choose`],
//! and each click then attaches that file where it lands. Acrobat asks after
//! the click; the result - a paperclip where the user clicked, carrying the
//! file they picked - is the same.
//!
//! The file is read when it is attached, not when it is chosen, so what goes
//! into the document is the file as it is at that moment. Its bytes go in
//! whole through `core::embedded`, which refuses a name that is a path.

use std::path::PathBuf;

use onionskin_core::embedded::{embed_file, mime_for, NewAttachment};
use onionskin_core::{add_annotation, Annotation, Color, PagePoint, Rect, Subtype};
use onionskin_plugin_api::{PointerInput, ToolCapability, ToolCtx, ToolEnvironment, ToolPlugin};

use crate::place::{now, page_object};

/// The paperclip's box, in page units, hanging from the click.
const ICON: (f64, f64) = (14.0, 24.0);
const SLIP: f64 = 4.0;

pub struct AttachFileTool {
    file: Option<PathBuf>,
    author: Option<String>,
    pressed: Option<PagePoint>,
}

impl Default for AttachFileTool {
    fn default() -> Self {
        AttachFileTool::new()
    }
}

impl AttachFileTool {
    pub fn new() -> Self {
        AttachFileTool {
            file: None,
            author: None,
            pressed: None,
        }
    }

    fn attach(&self, ctx: &mut ToolCtx, at: PagePoint) {
        let Some(path) = &self.file else {
            return;
        };
        let (Ok(data), Some(name), Some(page)) = (
            std::fs::read(path),
            path.file_name().and_then(|name| name.to_str()),
            page_object(ctx.doc, at.page),
        ) else {
            return;
        };
        let mut annotation = Annotation::new(
            Subtype::FileAttachment,
            Rect::new(at.x, at.y - ICON.1, at.x + ICON.0, at.y),
        );
        annotation.icon = Some("Paperclip".to_owned());
        annotation.contents = Some(name.to_owned());
        annotation.author = self.author.clone();
        annotation.color = Some(Color::new(0.2, 0.3, 0.6));
        let file = NewAttachment {
            name,
            data: &data,
            mime: mime_for(name),
            description: None,
        };
        let when = now();
        let _ = ctx.doc.edit_annotations("Attach File", |tx, structure| {
            annotation.file = Some(embed_file(tx, &file, when)?);
            add_annotation(tx, structure, page, &annotation, when).map(|_| ())
        });
    }
}

impl ToolPlugin for AttachFileTool {
    fn id(&self) -> &'static str {
        "attach-file"
    }

    fn name(&self) -> &'static str {
        "Attach File"
    }

    fn icon(&self) -> &'static str {
        "attach-file"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Comment, ToolCapability::ChoosesFile]
    }

    fn configure(&mut self, environment: &ToolEnvironment) {
        self.author = environment.author.clone();
    }

    fn choose(&mut self, path: &str) -> bool {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return false;
        }
        self.file = Some(path);
        true
    }

    fn chosen(&self) -> Option<String> {
        self.file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.pressed = Some(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(pressed) = self.pressed.take() else {
            return;
        };
        if pressed.page == input.at.page
            && (input.at.x - pressed.x).hypot(input.at.y - pressed.y) <= SLIP
        {
            self.attach(ctx, pressed);
        }
    }

    fn on_cancel(&mut self, _ctx: &mut ToolCtx) {
        self.pressed = None;
    }

    fn on_deactivate(&mut self, ctx: &mut ToolCtx) {
        self.on_cancel(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_existing_file_can_be_chosen() {
        let mut tool = AttachFileTool::new();
        assert!(!tool.choose("/nonexistent/onionskin/file.txt"));
        assert_eq!(tool.chosen(), None);
        assert!(tool.choose(env!("CARGO_MANIFEST_PATH")));
        assert!(tool.chosen().is_some());
    }
}
