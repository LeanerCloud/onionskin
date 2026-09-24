//! Removing glyphs from a text-showing operator without moving the ones
//! that stay.
//!
//! The operator is rewritten as a `TJ` array: the kept glyphs' codes as they
//! were written, and in place of each run of removed glyphs a number that
//! moves the pen exactly as far as they would have. The text matrix ends
//! where the original left it, so a later operator in the same text object
//! draws where it always did.

use onionskin_cos::Object;

use super::output::{number, Emit};
use crate::font::Code;

/// A piece of a shown string: text, or a `TJ` adjustment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Show {
    Text(Vec<u8>),
    Adjust(f64),
}

/// One glyph as the interpreter placed it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    /// Which [`Show`] the glyph came from.
    pub(crate) part: usize,
    pub(crate) code: Code,
    /// The font's displacement, or `None` when it declares none.
    pub(crate) advance: Option<f64>,
    pub(crate) removed: bool,
}

/// The text state a displacement is worked out from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spacing {
    pub(crate) size: f64,
    pub(crate) char_spacing: f64,
    pub(crate) word_spacing: f64,
    pub(crate) horizontal_scale: f64,
    /// `Some(advance per em)` for vertical writing.
    pub(crate) vertical: Option<f64>,
}

impl Spacing {
    /// The `TJ` number that moves the pen as far as `glyph` did, or `None`
    /// when no number can: a zero size or scale.
    fn skip(&self, glyph: &Placed) -> Option<f64> {
        let word = if glyph.code.is_word_space() {
            self.word_spacing
        } else {
            0.0
        };
        match self.vertical {
            Some(advance) if self.size != 0.0 => {
                let ty = advance * self.size + self.char_spacing + word;
                Some(-ty * 1000.0 / self.size)
            }
            None if self.size != 0.0 && self.horizontal_scale != 0.0 => {
                let em = glyph.advance.unwrap_or(0.0);
                Some(-(em * self.size + self.char_spacing + word) * 1000.0 / self.size)
            }
            _ => None,
        }
    }
}

enum Item {
    Text(Vec<u8>),
    Adjust(f64),
}

/// What a showing operator becomes once `placed` says which glyphs go.
///
/// A run where a removed glyph cannot be stepped over exactly, because its
/// font gives no width or the size is zero, loses every glyph: what the
/// renderer would draw after the gap cannot be placed, so nothing of it is
/// kept.
pub(crate) fn rewrite(
    operator: &[u8],
    operands: &[Object],
    parts: &[Show],
    placed: &[Placed],
    spacing: &Spacing,
) -> Emit {
    if !placed.iter().any(|glyph| glyph.removed) {
        return Emit::Copy;
    }
    let exact = placed.iter().all(|glyph| glyph.advance.is_some())
        && placed.iter().all(|glyph| spacing.skip(glyph).is_some());
    let mut items: Vec<Item> = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        match part {
            Show::Adjust(amount) => push_adjust(&mut items, *amount),
            Show::Text(bytes) => {
                let mut at = 0usize;
                for glyph in placed.iter().filter(|glyph| glyph.part == index) {
                    let end = (at + usize::from(glyph.code.len)).min(bytes.len());
                    if glyph.removed || !exact {
                        push_adjust(&mut items, spacing.skip(glyph).unwrap_or(0.0));
                    } else {
                        push_text(&mut items, &bytes[at..end]);
                    }
                    at = end;
                }
            }
        }
    }
    let mut out = prefix(operator, operands);
    out.push(b'[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        match item {
            Item::Text(bytes) => out.extend_from_slice(&hex(bytes)),
            Item::Adjust(amount) => out.extend_from_slice(number(*amount).as_bytes()),
        }
    }
    out.extend_from_slice(b"] TJ");
    Emit::Replace(out)
}

/// A showing operator removed whole: `'` and `"` keep their line move.
pub(crate) fn dropped(operator: &[u8], operands: &[Object]) -> Emit {
    let mut kept = prefix(operator, operands);
    if kept.is_empty() {
        return Emit::Drop;
    }
    kept.pop();
    Emit::Replace(kept)
}

/// What `'` and `"` do before they show: the line move and, for `"`, the
/// spacing they set.
fn prefix(operator: &[u8], operands: &[Object]) -> Vec<u8> {
    match operator {
        b"'" => b"T*\n".to_vec(),
        b"\"" => {
            let spacing: Vec<String> = operands
                .iter()
                .rev()
                .skip(1)
                .take(2)
                .filter_map(crate::tokenizer::number)
                .map(number)
                .collect();
            match spacing.as_slice() {
                [char_spacing, word_spacing] => {
                    format!("{word_spacing} Tw {char_spacing} Tc T*\n").into_bytes()
                }
                _ => b"T*\n".to_vec(),
            }
        }
        _ => Vec::new(),
    }
}

fn push_adjust(items: &mut Vec<Item>, amount: f64) {
    if let Some(Item::Adjust(last)) = items.last_mut() {
        *last += amount;
    } else {
        items.push(Item::Adjust(amount));
    }
}

fn push_text(items: &mut Vec<Item>, bytes: &[u8]) {
    if let Some(Item::Text(last)) = items.last_mut() {
        last.extend_from_slice(bytes);
    } else {
        items.push(Item::Text(bytes.to_vec()));
    }
}

/// A string operand in hexadecimal, which no byte can break out of.
fn hex(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2 + 2);
    out.push(b'<');
    for byte in bytes {
        out.extend_from_slice(format!("{byte:02X}").as_bytes());
    }
    out.push(b'>');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(value: u8) -> Code {
        Code {
            value: u32::from(value),
            cid: u32::from(value),
            len: 1,
        }
    }

    fn placed(part: usize, bytes: &[u8], removed: &[bool]) -> Vec<Placed> {
        bytes
            .iter()
            .zip(removed)
            .map(|(byte, removed)| Placed {
                part,
                code: code(*byte),
                advance: Some(0.5),
                removed: *removed,
            })
            .collect()
    }

    const SPACING: Spacing = Spacing {
        size: 10.0,
        char_spacing: 1.0,
        word_spacing: 2.0,
        horizontal_scale: 1.0,
        vertical: None,
    };

    fn text(emit: Emit) -> String {
        match emit {
            Emit::Replace(bytes) => String::from_utf8(bytes).expect("ascii"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_removed_is_kept_as_written() {
        let parts = [Show::Text(b"ab".to_vec())];
        let glyphs = placed(0, b"ab", &[false, false]);
        assert_eq!(rewrite(b"Tj", &[], &parts, &glyphs, &SPACING), Emit::Copy);
    }

    #[test]
    fn removed_glyphs_become_the_distance_they_covered() {
        // "a b": 0.5 em each at 10pt with 1pt character spacing is 6pt, or
        // 600 thousandths; the space adds 2pt of word spacing.
        let parts = [Show::Text(b"a b".to_vec())];
        let glyphs = placed(0, b"a b", &[false, true, true]);
        assert_eq!(
            text(rewrite(b"Tj", &[], &parts, &glyphs, &SPACING)),
            "[<61> -1400] TJ"
        );
    }

    #[test]
    fn a_tj_array_keeps_its_numbers_and_merges_with_the_gaps() {
        let parts = [
            Show::Text(b"ab".to_vec()),
            Show::Adjust(-100.0),
            Show::Text(b"cd".to_vec()),
        ];
        let mut glyphs = placed(0, b"ab", &[false, true]);
        glyphs.extend(placed(2, b"cd", &[true, false]));
        assert_eq!(
            text(rewrite(b"TJ", &[], &parts, &glyphs, &SPACING)),
            "[<61> -1300 <64>] TJ"
        );
    }

    #[test]
    fn quote_operators_keep_their_line_move_and_spacing() {
        let parts = [Show::Text(b"a".to_vec())];
        let glyphs = placed(0, b"a", &[true]);
        assert_eq!(
            text(rewrite(b"'", &[], &parts, &glyphs, &SPACING)),
            "T*\n[-600] TJ"
        );
        let operands = [
            Object::Integer(3),
            Object::Real(0.5),
            Object::String(b"a".to_vec()),
        ];
        assert_eq!(
            text(rewrite(b"\"", &operands, &parts, &glyphs, &SPACING)),
            "3 Tw 0.5 Tc T*\n[-600] TJ"
        );
        assert_eq!(
            text(rewrite(b"\"", &[], &parts, &glyphs, &SPACING)),
            "T*\n[-600] TJ"
        );
    }

    #[test]
    fn a_glyph_without_a_width_takes_the_whole_run_with_it() {
        let parts = [Show::Text(b"ab".to_vec())];
        let mut glyphs = placed(0, b"ab", &[true, false]);
        glyphs[1].advance = None;
        assert_eq!(
            text(rewrite(b"Tj", &[], &parts, &glyphs, &SPACING)),
            "[-700] TJ"
        );
        let zero = Spacing {
            size: 0.0,
            ..SPACING
        };
        let glyphs = placed(0, b"ab", &[true, false]);
        assert_eq!(text(rewrite(b"Tj", &[], &parts, &glyphs, &zero)), "[0] TJ");
    }

    #[test]
    fn vertical_writing_steps_down_the_column() {
        let vertical = Spacing {
            vertical: Some(-1.0),
            ..SPACING
        };
        let parts = [Show::Text(b"ab".to_vec())];
        let glyphs = placed(0, b"ab", &[true, false]);
        // ty = -1 * 10 + 1 = -9, so the number is 900.
        assert_eq!(
            text(rewrite(b"Tj", &[], &parts, &glyphs, &vertical)),
            "[900 <62>] TJ"
        );
    }
}
