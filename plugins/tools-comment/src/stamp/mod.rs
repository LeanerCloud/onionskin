//! Stamps: the built-in sets, dynamic stamps, and the user's own.
//!
//! **Built-ins are generated.** `catalog.rs` is written by `tools/stamps.py`
//! from a table of constants, together with the SVGs the Stamps dialog
//! previews, and a test runs the script's `--check` so neither can be edited
//! by hand into something else.
//!
//! **Dynamic stamps are filled natively.** Acrobat fills name, date and time
//! through its `AF*` JavaScript; scripting is M5, so this tool writes them
//! into the appearance when the stamp is placed. The name is the one the user
//! chose to comment as, from the environment the shell hands the tool, and
//! is left out when they have chosen none: nothing here reads the operating
//! system's account name. The time is UTC and says so, because the timezone
//! database that would make it local is not something this crate carries.
//!
//! **The clock is injected.** A dynamic stamp is only dynamic if a different
//! instant draws different text, and a test can only show that with a clock
//! it controls.

// Generated, and compared byte for byte with a fresh generation.
#[rustfmt::skip]
pub(crate) mod catalog;
mod library;

use std::sync::Arc;

use onionskin_core::{
    add_annotation, Annotation, BaseFont, Color, PagePoint, Rect, StampArt, Subtype,
};
use onionskin_cos::{BytesSource, Document as CosDocument};
use onionskin_plugin_api::{
    PointerInput, ToolCapability, ToolChoice, ToolCtx, ToolEnvironment, ToolPlugin,
};

pub use library::{CustomStamp, LibraryError, StampLibrary};

use crate::place::page_object;
use crate::text::{literal, measure};
use library::CUSTOM_PREFIX;

/// A built-in stamp, as the generator writes it.
pub(crate) struct Builtin {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    /// `/Name`.
    pub name: &'static str,
    /// Points, at the size the stamp is placed.
    pub size: (f64, f64),
    pub color: (f64, f64, f64),
    /// The drawing, in a box of `size` with its origin at the lower left.
    pub content: &'static str,
    /// A dynamic stamp's second line: its size and baseline.
    pub dynamic_line: Option<(f64, f64)>,
}

/// How wide a custom stamp is placed, at most. A stamp made from a whole
/// page would otherwise cover the page it is put on.
const MAX_CUSTOM_WIDTH: f64 = 200.0;

/// Past this, in page units, press and release were not one click.
const SLIP: f64 = 4.0;

/// Where the library lives inside the tool's data directory.
const LIBRARY_DIR: &str = "stamps";

/// The custom stamp library a tool configured with `data_dir` uses: what the
/// shell's Stamps dialog manages, so both mean the same folder.
pub fn library_in(data_dir: &std::path::Path) -> StampLibrary {
    StampLibrary::new(data_dir.join(LIBRARY_DIR))
}

/// Seconds since the Unix epoch.
pub type Clock = Box<dyn Fn() -> i64 + Send>;

pub struct StampTool {
    chosen: String,
    author: Option<String>,
    library: Option<StampLibrary>,
    clock: Clock,
    pressed: Option<PagePoint>,
}

impl Default for StampTool {
    fn default() -> Self {
        StampTool::new()
    }
}

impl StampTool {
    pub fn new() -> Self {
        StampTool::with_clock(Box::new(crate::place::now))
    }

    /// A stamp tool reading the time from `clock`.
    pub fn with_clock(clock: Clock) -> Self {
        StampTool {
            chosen: catalog::BUILTINS[0].id.to_owned(),
            author: None,
            library: None,
            clock,
            pressed: None,
        }
    }

    /// The custom stamp library, once the shell has said where it is.
    pub fn library(&self) -> Option<&StampLibrary> {
        self.library.as_ref()
    }

    /// The annotation for the chosen stamp, centred on `at`.
    fn annotation(&self, at: PagePoint) -> Option<Annotation> {
        if let Some(builtin) = catalog::BUILTINS.iter().find(|b| b.id == self.chosen) {
            return Some(self.builtin_annotation(builtin, at));
        }
        let custom = self.library.as_ref()?.find(&self.chosen)?;
        custom_annotation(&custom, at).map(|annotation| self.stamped(annotation))
    }

    fn builtin_annotation(&self, builtin: &Builtin, at: PagePoint) -> Annotation {
        let (content, fonts) = match builtin.dynamic_line {
            Some((size, baseline)) => {
                let line = dynamic_line(self.author.as_deref(), (self.clock)());
                let x = (builtin.size.0 - measure(&line, false, size)) / 2.0;
                let (r, g, b) = builtin.color;
                (
                    format!(
                        "{}\nBT /Helv {size} Tf {r} {g} {b} rg {x} {baseline} Td {} Tj ET",
                        builtin.content,
                        literal(&line)
                    ),
                    vec![BaseFont::HelveticaBold, BaseFont::Helvetica],
                )
            }
            None => (builtin.content.to_owned(), vec![BaseFont::HelveticaBold]),
        };
        let mut annotation = Annotation::new(Subtype::Stamp, centred(at, builtin.size));
        annotation.stamp_art = Some(StampArt::Drawing {
            size: builtin.size,
            content,
            fonts,
        });
        annotation.icon = Some(builtin.name.to_owned());
        annotation.contents = Some(builtin.label.to_owned());
        let (r, g, b) = builtin.color;
        annotation.color = Some(Color::new(r, g, b));
        self.stamped(annotation)
    }

    fn stamped(&self, mut annotation: Annotation) -> Annotation {
        annotation.subject = Some("Stamp".to_owned());
        annotation.author = self.author.clone();
        annotation
    }
}

/// A custom stamp's annotation: its page, at its own size up to
/// [`MAX_CUSTOM_WIDTH`].
fn custom_annotation(custom: &CustomStamp, at: PagePoint) -> Option<Annotation> {
    let bytes = Arc::new(std::fs::read(&custom.path).ok()?);
    let (document, _) =
        CosDocument::open_repairing(Box::new(BytesSource::from_shared(bytes.clone()))).ok()?;
    let page = document.page(0).ok()?;
    let (width, height) = page_size(&document, &page.dict)?;
    let scale = (MAX_CUSTOM_WIDTH / width).min(1.0);
    let mut annotation =
        Annotation::new(Subtype::Stamp, centred(at, (width * scale, height * scale)));
    annotation.stamp_art = Some(StampArt::Page(bytes));
    annotation.icon = Some("Custom".to_owned());
    annotation.contents = Some(custom.name.clone());
    Some(annotation)
}

fn page_size(document: &CosDocument, page: &onionskin_cos::Dict) -> Option<(f64, f64)> {
    let boxed = page.get(b"CropBox").or_else(|| page.get(b"MediaBox"))?;
    let items = document.resolve(boxed).ok()?;
    let numbers: Vec<f64> = items
        .as_array()?
        .iter()
        .filter_map(|item| match document.resolve(item).ok()? {
            onionskin_cos::Object::Integer(value) => Some(value as f64),
            onionskin_cos::Object::Real(value) => Some(value),
            _ => None,
        })
        .collect();
    let [x0, y0, x1, y1] = numbers.as_slice().try_into().ok()?;
    let size = ((x1 - x0).abs(), (y1 - y0).abs());
    (size.0 > 0.0 && size.1 > 0.0).then_some(size)
}

fn centred(at: PagePoint, (width, height): (f64, f64)) -> Rect {
    Rect::new(
        at.x - width / 2.0,
        at.y - height / 2.0,
        at.x + width / 2.0,
        at.y + height / 2.0,
    )
}

/// A dynamic stamp's second line: who, and when, in UTC.
pub(crate) fn dynamic_line(author: Option<&str>, now: i64) -> String {
    // `D:YYYYMMDDHHmmSSZ00'00'`: the one date formatter this workspace has.
    let stamp = onionskin_core::pdf_date(now);
    let digits = &stamp[2..16];
    let when = format!(
        "{}-{}-{} {}:{} UTC",
        &digits[0..4],
        &digits[4..6],
        &digits[6..8],
        &digits[8..10],
        &digits[10..12]
    );
    match author.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => format!("{name}, {when}"),
        None => when,
    }
}

impl ToolPlugin for StampTool {
    fn id(&self) -> &'static str {
        "stamp"
    }

    fn name(&self) -> &'static str {
        "Stamp"
    }

    fn icon(&self) -> &'static str {
        "stamp"
    }

    fn group(&self) -> &'static str {
        "stamp"
    }

    fn capabilities(&self) -> &'static [ToolCapability] {
        &[ToolCapability::Comment, ToolCapability::Stamp]
    }

    fn configure(&mut self, environment: &ToolEnvironment) {
        self.author = environment.author.clone();
        self.library = environment.data_dir.as_deref().map(library_in);
    }

    fn choices(&self) -> Vec<ToolChoice> {
        let builtins = catalog::BUILTINS.iter().map(|builtin| ToolChoice {
            id: builtin.id.to_owned(),
            label: builtin.label.to_owned(),
            category: builtin.category.to_owned(),
        });
        let custom = self
            .library
            .iter()
            .flat_map(StampLibrary::list)
            .map(|stamp| ToolChoice {
                id: stamp.id(),
                label: stamp.name,
                category: stamp.category,
            });
        builtins.chain(custom).collect()
    }

    fn choose(&mut self, id: &str) -> bool {
        let known = catalog::BUILTINS.iter().any(|builtin| builtin.id == id)
            || (id.starts_with(CUSTOM_PREFIX)
                && self
                    .library
                    .as_ref()
                    .is_some_and(|library| library.find(id).is_some()));
        if known {
            self.chosen = id.to_owned();
        }
        known
    }

    fn chosen(&self) -> Option<String> {
        Some(self.chosen.clone())
    }

    fn on_pointer_down(&mut self, _ctx: &mut ToolCtx, input: PointerInput) {
        self.pressed = Some(input.at);
    }

    fn on_pointer_move(&mut self, _ctx: &mut ToolCtx, _input: PointerInput) {}

    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, input: PointerInput) {
        let Some(pressed) = self.pressed.take() else {
            return;
        };
        if pressed.page != input.at.page
            || (input.at.x - pressed.x).hypot(input.at.y - pressed.y) > SLIP
        {
            return;
        }
        let (Some(page), Some(annotation)) =
            (page_object(ctx.doc, pressed.page), self.annotation(pressed))
        else {
            return;
        };
        let now = (self.clock)();
        let _ = ctx.doc.edit_annotations("Stamp", |tx, structure| {
            add_annotation(tx, structure, page, &annotation, now).map(|_| ())
        });
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
    fn the_second_line_names_the_author_only_when_there_is_one() {
        // 2026-09-21 14:05:00 UTC.
        let now = 1_789_999_500;
        assert_eq!(dynamic_line(Some("Ana"), now), "Ana, 2026-09-21 14:05 UTC");
        assert_eq!(dynamic_line(None, now), "2026-09-21 14:05 UTC");
        assert_eq!(dynamic_line(Some("  "), now), "2026-09-21 14:05 UTC");
    }

    #[test]
    fn every_builtin_is_a_choice_and_nothing_else_is_without_a_library() {
        let mut tool = StampTool::new();
        let choices = tool.choices();
        assert_eq!(choices.len(), catalog::BUILTINS.len());
        assert!(tool.choose("sign-witness"));
        assert_eq!(tool.chosen().as_deref(), Some("sign-witness"));
        assert!(
            !tool.choose("custom:Mine/Receipt"),
            "no library, no custom stamp"
        );
        assert!(!tool.choose("nonsense"));
        assert_eq!(tool.chosen().as_deref(), Some("sign-witness"));
    }

    #[test]
    fn the_builtins_cover_the_three_sets_with_unique_ids() {
        let mut ids: Vec<_> = catalog::BUILTINS.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog::BUILTINS.len());
        for category in ["Standard Business", "Sign Here", "Dynamic"] {
            assert!(catalog::BUILTINS.iter().any(|b| b.category == category));
        }
        assert!(catalog::BUILTINS
            .iter()
            .all(|b| (b.category == "Dynamic") == b.dynamic_line.is_some()));
    }
}
