//! Writing text in a standard (Base 14) font: the bytes a WinAnsiEncoding
//! string holds for some text, and how wide they set.
//!
//! What page marks and form appearances need to lay a line out without
//! embedding a font: the widths are the fonts' published metrics, the ones
//! every reader uses for these fonts.

use super::tables::{base14_metrics, glyph_name_to_unicode, WIN_ANSI_ENCODING};

/// What a character with no WinAnsiEncoding code is written as.
const MISSING: u8 = b'?';

/// `text` as WinAnsiEncoding bytes. A character the encoding has no code for
/// becomes `?`, so what is drawn is visibly not the character asked for
/// rather than a different one.
pub fn encode_win_ansi(text: &str) -> Vec<u8> {
    text.chars().map(win_ansi_code).collect()
}

fn win_ansi_code(character: char) -> u8 {
    if character.is_ascii() && !character.is_ascii_control() {
        return character as u8;
    }
    WIN_ANSI_ENCODING
        .iter()
        .position(|name| name.and_then(glyph_name_to_unicode) == Some(character))
        .map_or(MISSING, |code| code as u8)
}

/// How wide `bytes`, WinAnsiEncoding codes, set in `base_font`, in
/// thousandths of the font size. `None` when `base_font` is not a standard
/// font. A code with no glyph counts as nothing.
pub fn standard_text_width(base_font: &str, bytes: &[u8]) -> Option<f64> {
    let metrics = base14_metrics(base_font)?;
    Some(
        bytes
            .iter()
            .filter_map(|&code| WIN_ANSI_ENCODING[usize::from(code)])
            .filter_map(|name| metrics.width(name))
            .map(f64::from)
            .sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through_and_the_rest_is_looked_up() {
        assert_eq!(encode_win_ansi("Page 1"), b"Page 1");
        assert_eq!(encode_win_ansi("café"), b"caf\xe9");
        assert_eq!(encode_win_ansi("€"), [0x80]);
        assert_eq!(encode_win_ansi("日\t"), b"??");
    }

    #[test]
    fn widths_are_the_published_metrics() {
        // Helvetica: H 722, i 222; Courier is 600 throughout.
        assert_eq!(standard_text_width("Helvetica", b"Hi"), Some(944.0));
        assert_eq!(standard_text_width("Courier", b"abc"), Some(1800.0));
        assert_eq!(standard_text_width("Times-Roman", b""), Some(0.0));
        assert_eq!(standard_text_width("NotAFont", b"a"), None);
        assert_eq!(standard_text_width("Helvetica", &[0x00]), Some(0.0));
    }
}
