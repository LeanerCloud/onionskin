//! A line of text in a standard font: its width, and the operators and
//! resources that draw it.

use onionskin_content::{encode_win_ansi, standard_text_width};
use onionskin_cos::{Dict, Name, Object};

/// The standard fonts a mark can be set in; nothing is embedded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Helvetica,
    HelveticaBold,
    TimesRoman,
    TimesBold,
    Courier,
}

impl Font {
    pub const ALL: [Font; 5] = [
        Font::Helvetica,
        Font::HelveticaBold,
        Font::TimesRoman,
        Font::TimesBold,
        Font::Courier,
    ];

    /// The `/BaseFont` name, which is also what a menu calls it.
    pub fn base_font(self) -> &'static str {
        match self {
            Font::Helvetica => "Helvetica",
            Font::HelveticaBold => "Helvetica-Bold",
            Font::TimesRoman => "Times-Roman",
            Font::TimesBold => "Times-Bold",
            Font::Courier => "Courier",
        }
    }
}

/// How a mark's text is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: Font,
    /// Points.
    pub size: f64,
    /// Red, green and blue, each 0 to 1.
    pub color: [f64; 3],
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: Font::Helvetica,
            size: 12.0,
            color: [0.0, 0.0, 0.0],
        }
    }
}

impl TextStyle {
    /// How wide `text` sets, in points.
    pub fn width(&self, text: &str) -> f64 {
        let thousandths = standard_text_width(self.font.base_font(), &encode_win_ansi(text))
            .expect("a standard font");
        thousandths * self.size / 1000.0
    }

    /// The part of the size above the baseline a capital reaches, near
    /// enough for placing a line by its top.
    pub fn ascent(&self) -> f64 {
        self.size * 72.0 / 100.0
    }

    /// `/Font << /F0 ... >>`: the one font the operators name.
    pub(super) fn resources(&self, resources: &mut Dict) {
        let mut font = Dict::new();
        font.set(Name::new("Type"), Object::name("Font"));
        font.set(Name::new("Subtype"), Object::name("Type1"));
        font.set(Name::new("BaseFont"), Object::name(self.font.base_font()));
        font.set(Name::new("Encoding"), Object::name("WinAnsiEncoding"));
        let mut fonts = Dict::new();
        fonts.set(Name::new("F0"), Object::Dict(font));
        resources.set(Name::new("Font"), Object::Dict(fonts));
    }

    /// Operators drawing `text` with its baseline starting at `(x, y)`.
    pub(super) fn line(&self, out: &mut Vec<u8>, text: &str, (x, y): (f64, f64)) {
        let [r, g, b] = self.color;
        out.extend_from_slice(
            format!("BT\n/F0 {} Tf\n{r} {g} {b} rg\n{x} {y} Td\n(", self.size).as_bytes(),
        );
        for byte in encode_win_ansi(text) {
            if matches!(byte, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(byte);
        }
        out.extend_from_slice(b") Tj\nET\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_escaped_and_encoded() {
        let style = TextStyle {
            font: Font::Courier,
            size: 10.0,
            color: [1.0, 0.0, 0.0],
        };
        let mut out = Vec::new();
        style.line(&mut out, "a(b)é\\", (5.0, 6.0));
        let drawn = String::from_utf8_lossy(&out);
        assert!(drawn.contains("/F0 10 Tf"), "{drawn}");
        assert!(drawn.contains("1 0 0 rg"));
        assert!(drawn.contains("5 6 Td"));
        assert!(out.windows(8).any(|w| w == b"a\\(b\\)\xe9\\"));
        assert_eq!(style.width("abc"), 18.0, "Courier is 600 wide");
        assert_eq!(style.ascent(), 7.2);
    }

    #[test]
    fn every_font_is_a_standard_one() {
        for font in Font::ALL {
            let style = TextStyle {
                font,
                ..TextStyle::default()
            };
            assert!(style.width("W") > 0.0, "{}", font.base_font());
            let mut resources = Dict::new();
            style.resources(&mut resources);
            assert!(resources.get(b"Font").is_some());
        }
    }
}
