//! Watermarks and backgrounds: text, a page of another PDF, or (for a
//! background) a colour, placed on each page with a rotation and an
//! opacity.

use std::sync::Arc;

use onionskin_core::pages::{import_page_as_form, MarkKind, PageMark};
use onionskin_core::{Document, PageIndex, Transaction};
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, ObjRef, Object};
use onionskin_plugin_api::CommandError;

use super::text::TextStyle;
use super::{opacity_state, shown_sizes, write};

/// What a watermark or background shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Art {
    /// Lines of text, centred on one another.
    Text { text: String, style: TextStyle },
    /// Page `page` of a PDF, drawn at `scale` times its own size.
    Page {
        pdf: Arc<Vec<u8>>,
        page: usize,
        scale: f64,
    },
    /// The whole page filled with one colour: a background's.
    Color([f64; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

/// Where and how the art sits on each page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    /// Degrees, counterclockwise, about the art's centre.
    pub rotation: f64,
    /// 0 to 1.
    pub opacity: f64,
    pub horizontal: HAlign,
    pub vertical: VAlign,
    /// Points right and up from where the alignment put it.
    pub offset: (f64, f64),
    /// A watermark behind the page's content rather than over it.
    pub behind: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            rotation: 0.0,
            opacity: 1.0,
            horizontal: HAlign::Center,
            vertical: VAlign::Center,
            offset: (0.0, 0.0),
            behind: false,
        }
    }
}

/// Put `art` on `pages` as a watermark; with `replace`, instead of the
/// watermark they have.
pub fn add_watermark(
    doc: &mut Document,
    pages: &[PageIndex],
    art: &Art,
    appearance: Appearance,
    replace: bool,
    settings: &str,
) -> Result<(), CommandError> {
    let how = How {
        appearance,
        replace,
        settings,
    };
    add(doc, MarkKind::Watermark, pages, art, how)
}

/// Put `art` behind `pages` as a background; with `replace`, instead of the
/// background they have.
pub fn add_background(
    doc: &mut Document,
    pages: &[PageIndex],
    art: &Art,
    appearance: Appearance,
    replace: bool,
    settings: &str,
) -> Result<(), CommandError> {
    let appearance = Appearance {
        behind: true,
        ..appearance
    };
    let how = How {
        appearance,
        replace,
        settings,
    };
    add(doc, MarkKind::Background, pages, art, how)
}

/// How art is added: where it sits, whether it replaces what is there, and
/// the settings kept with it.
struct How<'s> {
    appearance: Appearance,
    replace: bool,
    settings: &'s str,
}

fn add(
    doc: &mut Document,
    kind: MarkKind,
    pages: &[PageIndex],
    art: &Art,
    how: How<'_>,
) -> Result<(), CommandError> {
    let sizes = shown_sizes(doc, pages)?;
    write(doc, kind, how.replace, |tx| {
        let drawing = Drawing::of(tx, art)?;
        Ok(sizes
            .into_iter()
            .map(|(page, size)| {
                let mut mark = drawing.mark(size, how.appearance);
                mark.settings = how.settings.as_bytes().to_vec();
                (page, mark)
            })
            .collect())
    })
}

/// The art made ready to place: its size, its operators at its own origin,
/// and what they name.
struct Drawing {
    size: Option<(f64, f64)>,
    content: Vec<u8>,
    resources: Dict,
}

impl Drawing {
    fn of(tx: &mut Transaction<'_>, art: &Art) -> onionskin_core::Result<Drawing> {
        Ok(match art {
            Art::Text { text, style } => text_drawing(text, style),
            Art::Page { pdf, page, scale } => {
                let source = CosDocument::open(Box::new(BytesSource::from_shared(pdf.clone())))?;
                let (form, bbox) = import_page_as_form(tx, &source, *page)?;
                page_drawing(form, bbox, *scale)
            }
            Art::Color([r, g, b]) => Drawing {
                size: None,
                content: format!("{r} {g} {b} rg\n").into_bytes(),
                resources: Dict::new(),
            },
        })
    }

    /// This drawing on a shown page `page` wide and high.
    fn mark(&self, page: (f64, f64), appearance: Appearance) -> PageMark {
        let mut resources = self.resources.clone();
        let state = opacity_state(appearance.opacity, &mut resources);
        let mut content = format!("q\n{state}").into_bytes();
        match self.size {
            // A colour fills the page, and has nothing to place or turn.
            None => {
                content.extend_from_slice(&self.content);
                content.extend_from_slice(format!("0 0 {} {} re f\n", page.0, page.1).as_bytes());
            }
            Some(size) => {
                content.extend_from_slice(placement(page, size, appearance).as_bytes());
                content.extend_from_slice(&self.content);
            }
        }
        content.extend_from_slice(b"Q\n");
        PageMark {
            content,
            resources,
            behind: appearance.behind,
            settings: Vec::new(),
        }
    }
}

fn text_drawing(text: &str, style: &TextStyle) -> Drawing {
    let lines: Vec<&str> = text.lines().collect();
    let leading = style.size * 1.2;
    let width = lines
        .iter()
        .map(|line| style.width(line))
        .fold(0.0, f64::max);
    let descent = style.size * 0.22;
    let height = style.ascent() + descent + leading * lines.len().saturating_sub(1) as f64;
    let mut content = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let x = (width - style.width(line)) / 2.0;
        let y = height - style.ascent() - leading * index as f64;
        style.line(&mut content, line, (x, y));
    }
    let mut resources = Dict::new();
    style.resources(&mut resources);
    Drawing {
        size: Some((width, height)),
        content,
        resources,
    }
}

fn page_drawing(form: ObjRef, [x0, y0, x1, y1]: [f64; 4], scale: f64) -> Drawing {
    let mut forms = Dict::new();
    forms.set(Name::new("Art"), Object::Ref(form));
    let mut resources = Dict::new();
    resources.set(Name::new("XObject"), Object::Dict(forms));
    Drawing {
        size: Some(((x1 - x0) * scale, (y1 - y0) * scale)),
        content: format!(
            "{scale} 0 0 {scale} {} {} cm\n/Art Do\n",
            -x0 * scale,
            -y0 * scale
        )
        .into_bytes(),
        resources,
    }
}

/// `cm` operators taking art `size` from its own origin to its place on a
/// shown page, turned about its centre.
fn placement(
    (page_width, page_height): (f64, f64),
    (width, height): (f64, f64),
    appearance: Appearance,
) -> String {
    let x = match appearance.horizontal {
        HAlign::Left => 0.0,
        HAlign::Center => (page_width - width) / 2.0,
        HAlign::Right => page_width - width,
    } + appearance.offset.0;
    let y = match appearance.vertical {
        VAlign::Bottom => 0.0,
        VAlign::Center => (page_height - height) / 2.0,
        VAlign::Top => page_height - height,
    } + appearance.offset.1;
    let (sin, cos) = appearance.rotation.to_radians().sin_cos();
    let (sin, cos) = (tidy(sin), tidy(cos));
    format!(
        "1 0 0 1 {} {} cm\n{cos} {sin} {} {cos} 0 0 cm\n1 0 0 1 {} {} cm\n",
        x + width / 2.0,
        y + height / 2.0,
        -sin,
        -width / 2.0,
        -height / 2.0
    )
}

/// A sine or cosine within rounding of 0 or 1 as exactly that, so a
/// quarter turn writes `0 1 -1 0` rather than `6e-17 1 -1 6e-17`.
fn tidy(value: f64) -> f64 {
    let rounded = value.round();
    if (value - rounded).abs() < 1e-12 {
        rounded
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marks::text::Font;

    #[test]
    fn the_art_is_aligned_then_turned_about_its_centre() {
        let centred = placement((600.0, 800.0), (100.0, 50.0), Appearance::default());
        assert_eq!(
            centred,
            "1 0 0 1 300 400 cm\n1 0 -0 1 0 0 cm\n1 0 0 1 -50 -25 cm\n"
        );
        let corner = Appearance {
            horizontal: HAlign::Right,
            vertical: VAlign::Top,
            offset: (-10.0, -20.0),
            rotation: 90.0,
            ..Appearance::default()
        };
        assert_eq!(
            placement((600.0, 800.0), (100.0, 50.0), corner),
            "1 0 0 1 540 755 cm\n0 1 -1 0 0 0 cm\n1 0 0 1 -50 -25 cm\n"
        );
        let low = Appearance {
            horizontal: HAlign::Left,
            vertical: VAlign::Bottom,
            ..Appearance::default()
        };
        assert!(placement((600.0, 800.0), (100.0, 50.0), low).starts_with("1 0 0 1 50 25 cm"));
    }

    #[test]
    fn text_lines_are_centred_on_the_widest() {
        let style = TextStyle {
            font: Font::Courier,
            size: 10.0,
            color: [0.5; 3],
        };
        let drawing = text_drawing("abcd\nab", &style);
        let (width, height) = drawing.size.expect("sized");
        assert_eq!(width, 24.0);
        assert!((height - (7.2 + 2.2 + 12.0)).abs() < 1e-9);
        let text = String::from_utf8_lossy(&drawing.content);
        assert!(text.contains("6 "), "the short line is indented: {text}");
    }

    #[test]
    fn a_colour_fills_the_page_and_opacity_is_a_graphics_state() {
        let drawing = Drawing {
            size: None,
            content: b"1 0 0 rg\n".to_vec(),
            resources: Dict::new(),
        };
        let mark = drawing.mark(
            (200.0, 100.0),
            Appearance {
                opacity: 0.5,
                behind: true,
                ..Appearance::default()
            },
        );
        let text = String::from_utf8_lossy(&mark.content);
        assert!(text.contains("/GS0 gs"));
        assert!(text.contains("0 0 200 100 re f"));
        assert!(mark.behind);
        assert!(mark.resources.get(b"ExtGState").is_some());
    }
}
