//! The normal appearance of a text or choice widget for a value: what
//! Acrobat draws after a field is filled, from the field's `/DA`, the
//! widget's `/MK` colours and its border.
//!
//! Text is set in a standard font: the `/DA` font when the form's resources
//! name one of the standard fourteen, Helvetica otherwise, in
//! WinAnsiEncoding. An embedded font's glyphs are not used, since writing
//! with them needs its own encoding.

use onionskin_content::{encode_win_ansi, standard_text_width};
use onionskin_cos::{Dict, Name, Object, Stream};

/// Space kept between the text and the widget's edge, in points.
const PADDING: f64 = 2.0;
/// The size text is fitted up to when `/DA` asks for automatic size.
const AUTO_MAX: f64 = 12.0;
const STANDARD: [&str; 14] = [
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
    "Symbol",
    "ZapfDingbats",
];

/// A field's `/DA`: its font resource, size and colour.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Da {
    pub(super) font: String,
    pub(super) size: f64,
    /// `g`, `rg` or `k` operands.
    pub(super) color: Vec<f64>,
}

pub(super) fn parse_da(da: Option<&str>) -> Da {
    let words: Vec<&str> = da.unwrap_or("").split_whitespace().collect();
    let number = |at: usize| words.get(at).and_then(|word| word.parse::<f64>().ok());
    let mut out = Da {
        font: "Helv".to_owned(),
        size: 0.0,
        color: vec![0.0],
    };
    for (at, word) in words.iter().enumerate() {
        match *word {
            "Tf" if at >= 2 => {
                out.size = number(at - 1).unwrap_or(0.0);
                if let Some(font) = words[at - 2].strip_prefix('/') {
                    out.font = font.to_owned();
                }
            }
            "g" if at >= 1 => out.color = vec![number(at - 1).unwrap_or(0.0)],
            "rg" if at >= 3 => out.color = (at - 3..at).map(|i| number(i).unwrap_or(0.0)).collect(),
            "k" if at >= 4 => out.color = (at - 4..at).map(|i| number(i).unwrap_or(0.0)).collect(),
            _ => {}
        }
    }
    out
}

/// The font an appearance is set in: the resource name used, the standard
/// font it is, and the dictionary to name it with.
pub(super) struct Font {
    name: String,
    base: String,
    dict: Object,
}

/// The `/DA` font when `resources` make it a standard font, Helvetica
/// otherwise.
pub(super) fn font(da: &Da, resources: Option<&Dict>, resolve: impl Fn(&Object) -> Object) -> Font {
    let named = resources
        .and_then(|resources| resources.get(b"Font"))
        .map(&resolve)
        .and_then(|fonts| {
            fonts
                .as_dict()
                .and_then(|fonts| fonts.get(da.font.as_bytes()).cloned())
        });
    let standard = named.as_ref().and_then(|entry| {
        let dict = resolve(entry);
        let base = dict
            .as_dict()?
            .get(b"BaseFont")?
            .as_name()?
            .as_bytes()
            .to_vec();
        let base = String::from_utf8_lossy(&base).into_owned();
        STANDARD.contains(&base.as_str()).then_some(base)
    });
    match (named, standard) {
        (Some(entry), Some(base)) => Font {
            name: da.font.clone(),
            base,
            dict: entry,
        },
        _ => {
            let mut dict = Dict::new();
            dict.set(Name::new("Type"), Object::name("Font"));
            dict.set(Name::new("Subtype"), Object::name("Type1"));
            dict.set(Name::new("BaseFont"), Object::name("Helvetica"));
            dict.set(Name::new("Encoding"), Object::name("WinAnsiEncoding"));
            Font {
                name: "Helv".to_owned(),
                base: "Helvetica".to_owned(),
                dict: Object::Dict(dict),
            }
        }
    }
}

/// How the text sits in the widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Layout {
    pub(super) multiline: bool,
    /// Characters each in one of this many cells.
    pub(super) comb: Option<usize>,
    pub(super) align: u8,
}

/// The widget's box, fill and border, from its `/Rect`, `/MK` and `/BS`.
pub(super) struct Frame {
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) background: Option<Vec<f64>>,
    pub(super) border: Option<Vec<f64>>,
    pub(super) border_width: f64,
}

impl Frame {
    pub(super) fn of(widget: &Dict, resolve: impl Fn(&Object) -> Object) -> Frame {
        let numbers = |value: Option<&Object>| -> Vec<f64> {
            value
                .map(&resolve)
                .and_then(|value| value.as_array().map(<[Object]>::to_vec))
                .unwrap_or_default()
                .iter()
                .filter_map(|value| match value {
                    Object::Integer(value) => Some(*value as f64),
                    Object::Real(value) => Some(*value),
                    _ => None,
                })
                .collect()
        };
        let rect = numbers(widget.get(b"Rect"));
        let (width, height) = match rect.as_slice() {
            [x0, y0, x1, y1] => ((x1 - x0).abs(), (y1 - y0).abs()),
            _ => (0.0, 0.0),
        };
        let mk = widget
            .get(b"MK")
            .map(&resolve)
            .and_then(|mk| mk.as_dict().cloned())
            .unwrap_or_default();
        let color = |key: &[u8]| Some(numbers(mk.get(key))).filter(|color| !color.is_empty());
        let border_width = widget
            .get(b"BS")
            .map(&resolve)
            .and_then(|bs| bs.as_dict().and_then(|bs| bs.get(b"W").cloned()))
            .and_then(|w| match w {
                Object::Integer(value) => Some(value as f64),
                Object::Real(value) => Some(value),
                _ => None,
            })
            .unwrap_or(1.0);
        Frame {
            width,
            height,
            background: color(b"BG"),
            border: color(b"BC"),
            border_width,
        }
    }
}

fn color_operator(color: &[f64], stroke: bool) -> String {
    let numbers: Vec<String> = color.iter().map(|value| format!("{value}")).collect();
    let operator = match (color.len(), stroke) {
        (1, false) => "g",
        (1, true) => "G",
        (4, false) => "k",
        (4, true) => "K",
        (_, false) => "rg",
        (_, true) => "RG",
    };
    format!("{} {operator}", numbers.join(" "))
}

/// The box and border every appearance starts with.
fn frame_content(frame: &Frame) -> String {
    let mut out = String::new();
    if let Some(background) = &frame.background {
        out.push_str(&format!(
            "{} 0 0 {} {} re f\n",
            color_operator(background, false),
            frame.width,
            frame.height
        ));
    }
    if let Some(border) = &frame.border {
        let inset = frame.border_width / 2.0;
        out.push_str(&format!(
            "{} {} w {inset} {inset} {} {} re S\n",
            color_operator(border, true),
            frame.border_width,
            frame.width - frame.border_width,
            frame.height - frame.border_width
        ));
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::from("<");
    for byte in bytes {
        out.push_str(&format!("{byte:02X}"));
    }
    out.push('>');
    out
}

/// Words wrapped to `width` at `size`.
fn wrap(text: &str, base: &str, size: f64, width: f64) -> Vec<String> {
    let measure = |line: &str| {
        standard_text_width(base, &encode_win_ansi(line)).unwrap_or(0.0) * size / 1000.0
    };
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if measure(&candidate) > width && !line.is_empty() {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// A text field's, or a dropdown's, normal appearance showing `text`.
pub(super) fn text_appearance(
    frame: &Frame,
    da: &Da,
    font: &Font,
    layout: Layout,
    text: &str,
) -> Stream {
    let inner = frame.width - 2.0 * PADDING;
    let measure = |line: &str, size: f64| {
        standard_text_width(&font.base, &encode_win_ansi(line)).unwrap_or(0.0) * size / 1000.0
    };
    let size = if da.size > 0.0 {
        da.size
    } else if layout.multiline {
        AUTO_MAX
    } else {
        let unit = measure(text, 1.0);
        let by_width = if unit > 0.0 { inner / unit } else { AUTO_MAX };
        by_width
            .min((frame.height - 2.0 * PADDING) * 0.8)
            .clamp(1.0, AUTO_MAX)
    };
    let mut body = format!(
        "BT /{} {size} Tf {}\n",
        font.name,
        color_operator(&da.color, false)
    );
    if let Some(cells) = layout.comb.filter(|cells| *cells > 0) {
        let cell = frame.width / cells as f64;
        let baseline = (frame.height - size * 0.7) / 2.0;
        for (index, character) in text.chars().take(cells).enumerate() {
            let glyph = character.to_string();
            let x = cell * index as f64 + (cell - measure(&glyph, size)) / 2.0;
            body.push_str(&format!(
                "1 0 0 1 {x} {baseline} Tm {} Tj\n",
                hex(&encode_win_ansi(&glyph))
            ));
        }
    } else {
        let lines = if layout.multiline {
            wrap(text, &font.base, size, inner)
        } else {
            vec![text.to_owned()]
        };
        let leading = size * 1.15;
        let first = if layout.multiline {
            frame.height - PADDING - size
        } else {
            (frame.height - size * 0.7) / 2.0
        };
        for (index, line) in lines.iter().enumerate() {
            let width = measure(line, size);
            let x = match layout.align {
                1 => (frame.width - width) / 2.0,
                2 => frame.width - PADDING - width,
                _ => PADDING,
            };
            let y = first - leading * index as f64;
            body.push_str(&format!(
                "1 0 0 1 {x} {y} Tm {} Tj\n",
                hex(&encode_win_ansi(line))
            ));
        }
    }
    body.push_str("ET\n");
    stream(frame, font, &body)
}

/// A list box's text size and row height.
pub(super) fn list_metrics(da: &Da) -> (f64, f64) {
    let size = if da.size > 0.0 { da.size } else { AUTO_MAX };
    (size, size * 1.15)
}

/// The list box row at height `y` in a widget whose top is `top`, counted
/// from the first, as [`list_appearance`] lays them out.
pub(super) fn list_row(da: &Da, top: f64, y: f64) -> Option<usize> {
    let (_, leading) = list_metrics(da);
    let row = ((top - PADDING - y) / leading).floor();
    (row >= 0.0).then_some(row as usize)
}

/// A list box's normal appearance: its entries from the top, the chosen
/// ones on a highlight.
pub(super) fn list_appearance(
    frame: &Frame,
    da: &Da,
    font: &Font,
    entries: &[(String, bool)],
) -> Stream {
    let (size, leading) = list_metrics(da);
    let mut body = String::new();
    for (index, (_, chosen)) in entries.iter().enumerate() {
        if *chosen {
            let top = frame.height - PADDING - leading * index as f64;
            body.push_str(&format!(
                "0.6 0.75 0.9 rg 0 {} {} {leading} re f\n",
                top - leading,
                frame.width
            ));
        }
    }
    body.push_str(&format!(
        "BT /{} {size} Tf {}\n",
        font.name,
        color_operator(&da.color, false)
    ));
    for (index, (display, _)) in entries.iter().enumerate() {
        let y = frame.height - PADDING - leading * index as f64 - size * 0.9;
        body.push_str(&format!(
            "1 0 0 1 {PADDING} {y} Tm {} Tj\n",
            hex(&encode_win_ansi(display))
        ));
    }
    body.push_str("ET\n");
    stream(frame, font, &body)
}

fn stream(frame: &Frame, font: &Font, body: &str) -> Stream {
    let content = format!(
        "/Tx BMC q\n{}{PADDING} {PADDING} {} {} re W n\n{body}Q EMC",
        frame_content(frame),
        (frame.width - 2.0 * PADDING).max(0.0),
        (frame.height - 2.0 * PADDING).max(0.0)
    );
    let mut fonts = Dict::new();
    fonts.set(Name::new(&font.name), font.dict.clone());
    let mut resources = Dict::new();
    resources.set(Name::new("Font"), Object::Dict(fonts));
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("XObject"));
    dict.set(Name::new("Subtype"), Object::name("Form"));
    dict.set(
        Name::new("BBox"),
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(frame.width),
            Object::Real(frame.height),
        ]),
    );
    dict.set(Name::new("Resources"), Object::Dict(resources));
    Stream {
        dict,
        raw: content.into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_da_gives_its_font_size_and_colour() {
        let da = parse_da(Some("/TiRo 9 Tf 0 0 1 rg"));
        assert_eq!(
            (da.font.as_str(), da.size, da.color.clone()),
            ("TiRo", 9.0, vec![0.0, 0.0, 1.0])
        );
        let grey = parse_da(Some("0.5 g /Helv 0 Tf"));
        assert_eq!((grey.size, grey.color.clone()), (0.0, vec![0.5]));
        let cmyk = parse_da(Some("/Helv 10 Tf 0 0 0 1 k"));
        assert_eq!(cmyk.color.len(), 4);
        assert_eq!(parse_da(None).font, "Helv");
    }

    #[test]
    fn colour_operators_follow_the_component_count() {
        assert_eq!(color_operator(&[0.5], false), "0.5 g");
        assert_eq!(color_operator(&[1.0, 0.0, 0.0], true), "1 0 0 RG");
        assert_eq!(color_operator(&[0.0, 0.0, 0.0, 1.0], false), "0 0 0 1 k");
        assert_eq!(color_operator(&[0.0, 0.0, 0.0, 1.0], true), "0 0 0 1 K");
    }

    #[test]
    fn words_wrap_to_the_width() {
        let lines = wrap("the quick brown fox\njumps", "Helvetica", 10.0, 50.0);
        assert_eq!(lines, ["the quick", "brown fox", "jumps"]);
    }
}
