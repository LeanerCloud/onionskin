//! Rich Text Format for a text selection: Export Selection As.
//!
//! The selection arrives as spans of one face and size. Each span is written
//! as its own RTF group carrying its font, its size and, when the face's name
//! says so, bold and italic, so a word processor opening the file shows the
//! text in the faces the PDF drew it in. Colour, positions and columns are not
//! carried: the file is the selected text with its type, not a page layout,
//! which is also why whole-document RTF export stays the post-1.0 row.

use onionskin_core::TextSpan;

/// The selection as an RTF 1.x document (Microsoft's RTF 1.9.1
/// specification): a font table of the faces' families, then one group per
/// span.
pub fn selection_rtf(spans: &[TextSpan]) -> String {
    let families = font_table(spans);
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl");
    for (index, family) in families.iter().enumerate() {
        out.push_str(&format!("{{\\f{index}\\fnil {};}}", escape(family)));
    }
    out.push_str("}\n");
    for span in spans {
        let font = families
            .iter()
            .position(|family| *family == family_of(&span.font))
            .unwrap_or(0);
        out.push_str(&format!("{{\\f{font}\\fs{}", half_points(span.size)));
        let style = style_of(&span.font);
        if style.bold {
            out.push_str("\\b");
        }
        if style.italic {
            out.push_str("\\i");
        }
        out.push(' ');
        out.push_str(&escape(&span.text));
        out.push('}');
    }
    out.push_str("}\n");
    out
}

/// The families the spans use, in first-use order, without repeats.
fn font_table(spans: &[TextSpan]) -> Vec<String> {
    let mut families: Vec<String> = Vec::new();
    for span in spans {
        let family = family_of(&span.font);
        if !families.contains(&family) {
            families.push(family);
        }
    }
    if families.is_empty() {
        families.push("Helvetica".to_owned());
    }
    families
}

/// `Helvetica-BoldOblique` and `Arial,Bold` are Helvetica and Arial: the
/// style after the separator is written as `\b` and `\i`, not as a family.
fn family_of(font: &str) -> String {
    let family = font.split(['-', ',']).next().unwrap_or_default().trim();
    if family.is_empty() {
        "Helvetica".to_owned()
    } else {
        family.to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
}

/// What a PostScript name says about weight and slant.
fn style_of(font: &str) -> Style {
    let lower = font.to_ascii_lowercase();
    Style {
        bold: lower.contains("bold") || lower.contains("black") || lower.contains("heavy"),
        italic: lower.contains("italic") || lower.contains("oblique"),
    }
}

/// `\fs` counts half points. A size the text did not state (zero, or not a
/// number) is written as 12 points rather than as an invisible zero.
fn half_points(size: f64) -> u32 {
    if size.is_finite() && size > 0.0 {
        (size * 2.0).round().clamp(1.0, 3276.0) as u32
    } else {
        24
    }
}

/// Text as RTF: the three control characters escaped, line breaks as
/// paragraphs, tabs as `\tab`, and everything outside ASCII as `\uN?`, with
/// characters outside the Basic Multilingual Plane as their two UTF-16
/// halves, as the specification writes them.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(character);
            }
            '\n' => out.push_str("\\par\n"),
            '\r' => {}
            '\t' => out.push_str("\\tab "),
            ' '..='~' => out.push(character),
            _ => {
                let mut units = [0u16; 2];
                for unit in character.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{}?", *unit as i16));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, font: &str, size: f64) -> TextSpan {
        TextSpan {
            text: text.to_owned(),
            font: font.to_owned(),
            size,
        }
    }

    #[test]
    fn each_span_carries_its_family_size_and_style() {
        let rtf = selection_rtf(&[
            span("Title ", "Helvetica-Bold", 18.0),
            span("body text", "Times-Italic", 11.0),
            span(" more", "Helvetica", 11.0),
        ]);
        assert!(rtf.starts_with(
            "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\fnil Helvetica;}{\\f1\\fnil Times;}}"
        ));
        assert!(rtf.contains("{\\f0\\fs36\\b Title }"), "{rtf}");
        assert!(rtf.contains("{\\f1\\fs22\\i body text}"), "{rtf}");
        assert!(rtf.contains("{\\f0\\fs22  more}"), "{rtf}");
        assert!(rtf.trim_end().ends_with('}'));
    }

    #[test]
    fn control_characters_breaks_and_unicode_are_escaped() {
        assert_eq!(escape("a\\b{c}"), "a\\\\b\\{c\\}");
        assert_eq!(escape("one\ntwo\r\tx"), "one\\par\ntwo\\tab x");
        assert_eq!(escape("é"), "\\u233?");
        assert_eq!(escape("€"), "\\u8364?");
        // U+1F600 is two UTF-16 halves, each signed.
        assert_eq!(escape("😀"), "\\u-10179?\\u-8704?");
    }

    #[test]
    fn names_sizes_and_empty_selections_have_safe_defaults() {
        assert_eq!(family_of("Arial,BoldItalic"), "Arial");
        assert_eq!(family_of(""), "Helvetica");
        assert_eq!(
            style_of("Arial,BoldItalic"),
            Style {
                bold: true,
                italic: true
            }
        );
        assert_eq!(half_points(0.0), 24);
        assert_eq!(half_points(f64::NAN), 24);
        assert_eq!(half_points(10.25), 21);
        assert_eq!(
            selection_rtf(&[]),
            "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0\\fnil Helvetica;}}\n}\n"
        );
    }
}
