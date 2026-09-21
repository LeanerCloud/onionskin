//! Setting text in the standard fonts: how wide it is, and how to write it.
//!
//! The widths are the ones `tools/stamps.py` centres the built-in stamps
//! with, carried in the generated catalog, so a line centred here at run time
//! sits the way the generated ones do.
//!
//! **Text is written in WinAnsiEncoding**, which the appearance's font
//! dictionaries name. A name with an accent - `José`, `Łukasz` - is written
//! as the one byte that encoding gives it, as an octal escape so the content
//! stream stays ASCII; a character the encoding has no byte for becomes `?`
//! rather than a byte that would draw some other glyph.

use crate::stamp::catalog::{FALLBACK, HELVETICA, HELVETICA_BOLD};

/// The width of `text` set in Helvetica, or Helvetica-Bold, at `size`.
pub(crate) fn measure(text: &str, bold: bool, size: f64) -> f64 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    let units: u32 = text
        .chars()
        .map(|character| {
            let code = character as u32;
            if (32..=126).contains(&code) {
                u32::from(table[(code - 32) as usize])
            } else {
                u32::from(FALLBACK)
            }
        })
        .sum();
    f64::from(units) * size / 1000.0
}

/// `text` as a PDF literal string, parentheses included, in WinAnsiEncoding.
pub(crate) fn literal(text: &str) -> String {
    let mut out = String::from("(");
    for character in text.chars() {
        match winansi(character) {
            Some(b'(') => out.push_str("\\("),
            Some(b')') => out.push_str("\\)"),
            Some(b'\\') => out.push_str("\\\\"),
            Some(byte @ 32..=126) => out.push(char::from(byte)),
            Some(byte) => out.push_str(&format!("\\{byte:03o}")),
            None => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// The WinAnsiEncoding byte for a character, if it has one.
fn winansi(character: char) -> Option<u8> {
    let code = character as u32;
    match code {
        32..=126 | 0xA0..=0xFF => Some(code as u8),
        _ => WINANSI_HIGH
            .iter()
            .find(|(_, unicode)| *unicode == character)
            .map(|(byte, _)| *byte),
    }
}

/// The bytes 0x80 to 0x9F, where WinAnsiEncoding differs from Latin-1.
const WINANSI_HIGH: [(u8, char); 27] = [
    (0x80, '€'),
    (0x82, '‚'),
    (0x83, 'ƒ'),
    (0x84, '„'),
    (0x85, '…'),
    (0x86, '†'),
    (0x87, '‡'),
    (0x88, 'ˆ'),
    (0x89, '‰'),
    (0x8A, 'Š'),
    (0x8B, '‹'),
    (0x8C, 'Œ'),
    (0x8E, 'Ž'),
    (0x91, '\u{2018}'),
    (0x92, '\u{2019}'),
    (0x93, '\u{201C}'),
    (0x94, '\u{201D}'),
    (0x95, '•'),
    (0x96, '–'),
    (0x97, '—'),
    (0x98, '˜'),
    (0x99, '™'),
    (0x9A, 'š'),
    (0x9B, '›'),
    (0x9C, 'œ'),
    (0x9E, 'ž'),
    (0x9F, 'Ÿ'),
];

/// `text` broken into lines no wider than `limit` at `size`, at spaces
/// where it can be and mid-word where one word alone is too wide. The
/// text's own newlines are kept.
pub(crate) fn wrap(text: &str, bold: bool, size: f64, limit: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if measure(&candidate, bold, size) <= limit {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            line = break_word(word, bold, size, limit, &mut lines);
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// A word too wide for a line, cut into lines that fit; returns the last
/// piece, which the next word may join.
fn break_word(word: &str, bold: bool, size: f64, limit: f64, lines: &mut Vec<String>) -> String {
    let mut piece = String::new();
    for character in word.chars() {
        piece.push(character);
        if measure(&piece, bold, size) > limit && piece.chars().count() > 1 {
            let last = piece.pop().expect("a character was just pushed");
            lines.push(std::mem::take(&mut piece));
            piece.push(last);
        }
    }
    piece
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_come_from_the_tables_and_scale_with_size() {
        assert_eq!(measure("A", false, 1000.0), 667.0);
        assert_eq!(measure("A", true, 1000.0), 722.0);
        assert_eq!(
            measure("é", false, 1000.0),
            556.0,
            "outside ASCII is the fallback"
        );
        assert_eq!(measure("ii", false, 10.0), 4.44);
    }

    #[test]
    fn literals_escape_what_ends_a_string_and_encode_the_rest_in_winansi() {
        assert_eq!(literal("a(b)c\\"), "(a\\(b\\)c\\\\)");
        assert_eq!(literal("José"), "(Jos\\351)");
        assert_eq!(literal("€5 — ok"), "(\\2005 \\227 ok)");
        assert_eq!(literal("日本"), "(??)");
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_inside_a_word_too_long_for_a_line() {
        let lines = wrap("the quick brown fox", false, 10.0, 50.0);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| measure(line, false, 10.0) <= 50.0));
        assert_eq!(lines.join(" "), "the quick brown fox");

        let long = wrap("abcdefghijklmnopqrstuvwxyz", false, 10.0, 30.0);
        assert!(long.len() > 3);
        assert_eq!(long.concat(), "abcdefghijklmnopqrstuvwxyz");
        assert!(long.iter().all(|line| measure(line, false, 10.0) <= 30.0));

        assert_eq!(wrap("one\n\ntwo", false, 10.0, 100.0), ["one", "", "two"]);
        assert_eq!(wrap("", false, 10.0, 100.0), [""]);
    }
}
