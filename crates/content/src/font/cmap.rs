//! CMaps: the byte-to-code, code-to-CID and code-to-Unicode tables of ISO
//! 32000-2 9.7.5 and 9.10.3.
//!
//! One parser serves all three because they are the same PostScript-ish
//! syntax over the same lexer the content streams use; only the operators
//! differ, and a given CMap uses either the CID set or the `bf` set, never
//! both.

use std::collections::BTreeMap;

use onionskin_cos::Object;

use crate::tokenizer::Tokenizer;

/// Operand ceiling for CMap parsing. A section is supposed to hold at most 100
/// entries (three operands each for `bfrange`), but producers exceed that, and
/// every entry dropped here is text silently lost.
const CMAP_OPERANDS: usize = 65536;

#[derive(Clone, Copy, Debug)]
struct Codespace {
    low: u32,
    high: u32,
    bytes: u8,
}

#[derive(Clone, Debug, Default)]
pub struct CMap {
    codespace: Vec<Codespace>,
    cid_single: BTreeMap<u32, u32>,
    cid_range: Vec<(u32, u32, u32)>,
    text_single: BTreeMap<u32, String>,
    text_range: Vec<(u32, u32, String)>,
    pub vertical: bool,
    /// A `usecmap` naming something this build does not carry.
    pub unresolved_parent: Option<String>,
}

impl CMap {
    /// The two-byte identity mapping, which is what `Identity-H` and
    /// `Identity-V` mean: every code is its own CID.
    pub fn identity(vertical: bool) -> CMap {
        CMap {
            codespace: vec![Codespace {
                low: 0,
                high: 0xFFFF,
                bytes: 2,
            }],
            cid_range: vec![(0, 0xFFFF, 0)],
            vertical,
            ..Default::default()
        }
    }

    /// A codespace of `bytes`-wide codes covering the whole range. Used when a
    /// predefined CMap is named that this build does not carry: the codes come
    /// out right for the CJK CMaps, which are all two-byte, and the CIDs are
    /// left unmapped rather than invented.
    pub fn opaque(bytes: u8, vertical: bool) -> CMap {
        let high = match bytes {
            1 => 0xFF,
            2 => 0xFFFF,
            3 => 0x00FF_FFFF,
            _ => 0xFFFF_FFFF,
        };
        CMap {
            codespace: vec![Codespace {
                low: 0,
                high,
                bytes,
            }],
            vertical,
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cid_single.is_empty()
            && self.cid_range.is_empty()
            && self.text_single.is_empty()
            && self.text_range.is_empty()
    }

    /// Splits the next code off the front of a string.
    ///
    /// ISO 32000-2 9.7.6.3 matches the input against the codespace ranges by
    /// byte width. A byte sequence in no range takes the width of the range
    /// its first byte fits, and one byte when nothing fits at all, which is
    /// what keeps a damaged string from swallowing the rest of the line.
    pub fn next_code(&self, bytes: &[u8]) -> (u32, u8) {
        if self.codespace.is_empty() {
            return (u32::from(bytes[0]), 1);
        }
        for width in 1u8..=4 {
            let width_usize = usize::from(width);
            if bytes.len() < width_usize {
                break;
            }
            let value = be(&bytes[..width_usize]);
            if self
                .codespace
                .iter()
                .any(|c| c.bytes == width && value >= c.low && value <= c.high)
            {
                return (value, width);
            }
        }
        let first = bytes[0];
        let width = self
            .codespace
            .iter()
            .find(|c| {
                let shift = 8 * (u32::from(c.bytes) - 1);
                let lo = (c.low >> shift) as u8;
                let hi = (c.high >> shift) as u8;
                first >= lo && first <= hi
            })
            .map(|c| c.bytes)
            .unwrap_or(1);
        let width = width.min(bytes.len() as u8).max(1);
        (be(&bytes[..usize::from(width)]), width)
    }

    pub fn cid(&self, code: u32) -> Option<u32> {
        if let Some(cid) = self.cid_single.get(&code) {
            return Some(*cid);
        }
        self.cid_range
            .iter()
            .find(|(lo, hi, _)| code >= *lo && code <= *hi)
            .map(|(lo, _, base)| base + (code - lo))
    }

    pub fn text(&self, code: u32) -> Option<String> {
        if let Some(text) = self.text_single.get(&code) {
            return Some(text.clone());
        }
        let (lo, _, base) = self
            .text_range
            .iter()
            .find(|(lo, hi, _)| code >= *lo && code <= *hi)?;
        Some(offset_last(base, code - lo))
    }

    fn merge_from(&mut self, other: &CMap) {
        if self.codespace.is_empty() {
            self.codespace = other.codespace.clone();
        }
        for (code, cid) in &other.cid_single {
            self.cid_single.entry(*code).or_insert(*cid);
        }
        self.cid_range.extend_from_slice(&other.cid_range);
        for (code, text) in &other.text_single {
            self.text_single
                .entry(*code)
                .or_insert_with(|| text.clone());
        }
        self.text_range.extend_from_slice(&other.text_range);
    }
}

/// Parses a CMap program. Never fails: a CMap that half-parses still maps the
/// codes it got through, and an empty one is visible as [`CMap::is_empty`].
pub fn parse(data: &[u8]) -> CMap {
    let mut map = CMap::default();
    let mut lexer = Tokenizer::with_operand_limit(data, CMAP_OPERANDS);
    while let Some(op) = lexer.next_operation() {
        let operands = &op.operands;
        match op.operator.as_bytes() {
            b"endcodespacerange" => {
                for pair in operands.chunks(2) {
                    let (Some(low), Some(high)) = (string(pair.first()), string(pair.get(1)))
                    else {
                        continue;
                    };
                    if low.is_empty() || low.len() > 4 || low.len() != high.len() {
                        continue;
                    }
                    map.codespace.push(Codespace {
                        low: be(low),
                        high: be(high),
                        bytes: low.len() as u8,
                    });
                }
            }
            b"endcidchar" => {
                for pair in operands.chunks(2) {
                    let (Some(code), Some(Object::Integer(cid))) =
                        (string(pair.first()), pair.get(1))
                    else {
                        continue;
                    };
                    if let Ok(cid) = u32::try_from(*cid) {
                        map.cid_single.insert(be(code), cid);
                    }
                }
            }
            b"endcidrange" => {
                for triple in operands.chunks(3) {
                    let (Some(low), Some(high), Some(Object::Integer(cid))) =
                        (string(triple.first()), string(triple.get(1)), triple.get(2))
                    else {
                        continue;
                    };
                    if let Ok(cid) = u32::try_from(*cid) {
                        map.cid_range.push((be(low), be(high), cid));
                    }
                }
            }
            b"endbfchar" => {
                for pair in operands.chunks(2) {
                    let Some(code) = string(pair.first()) else {
                        continue;
                    };
                    let Some(text) = destination(pair.get(1)) else {
                        continue;
                    };
                    map.text_single.insert(be(code), text);
                }
            }
            b"endbfrange" => {
                for triple in operands.chunks(3) {
                    let (Some(low), Some(high)) = (string(triple.first()), string(triple.get(1)))
                    else {
                        continue;
                    };
                    let (low, high) = (be(low), be(high));
                    match triple.get(2) {
                        Some(Object::Array(items)) => {
                            for (i, item) in items.iter().enumerate() {
                                let Some(text) = destination(Some(item)) else {
                                    continue;
                                };
                                map.text_single.insert(low + i as u32, text);
                            }
                        }
                        other => {
                            if let Some(text) = destination(other) {
                                map.text_range.push((low, high, text));
                            }
                        }
                    }
                }
            }
            b"def" => {
                // /WMode 1 def
                if let (Some(Object::Name(key)), Some(Object::Integer(value))) =
                    (operands.first(), operands.get(1))
                {
                    if key.as_bytes() == b"WMode" {
                        map.vertical = *value == 1;
                    }
                }
            }
            b"usecmap" => {
                let Some(Object::Name(name)) = operands.last() else {
                    continue;
                };
                let name = String::from_utf8_lossy(name.as_bytes()).into_owned();
                match predefined(&name) {
                    Some(parent) => map.merge_from(&parent),
                    None => map.unresolved_parent = Some(name),
                }
            }
            _ => {}
        }
    }
    map
}

/// The predefined CMaps this build carries, which is the Identity pair. Every
/// other name is a registered CJK CMap whose tables are not shipped.
pub fn predefined(name: &str) -> Option<CMap> {
    match name {
        "Identity-H" => Some(CMap::identity(false)),
        "Identity-V" => Some(CMap::identity(true)),
        _ => None,
    }
}

fn string(object: Option<&Object>) -> Option<&[u8]> {
    match object? {
        Object::String(s) => Some(s),
        _ => None,
    }
}

/// A `bf` destination: a UTF-16BE string, or a glyph name.
fn destination(object: Option<&Object>) -> Option<String> {
    match object? {
        Object::String(s) => Some(utf16be(s)),
        Object::Name(n) => {
            let name = String::from_utf8_lossy(n.as_bytes());
            super::glyph_name_to_string(&name)
        }
        _ => None,
    }
}

fn be(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |acc, b| (acc << 8) | u32::from(*b))
}

/// UTF-16BE with surrogate pairs. An unpaired surrogate is dropped rather than
/// turned into a replacement character, so a caller can tell "no text" from
/// "text that happens to be U+FFFD".
pub fn utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_be_bytes(*p))
        .collect();
    // A one-byte destination is a Latin-1 code, which several producers emit.
    if units.is_empty() {
        return bytes.iter().map(|b| char::from(*b)).collect();
    }
    char::decode_utf16(units).flatten().collect()
}

/// A `bfrange` string destination advances its last UTF-16 code unit once per
/// code in the range (ISO 32000-2 9.10.3).
fn offset_last(base: &str, offset: u32) -> String {
    if offset == 0 {
        return base.to_string();
    }
    let mut chars: Vec<char> = base.chars().collect();
    let Some(last) = chars.pop() else {
        return String::new();
    };
    match char::from_u32(last as u32 + offset) {
        Some(shifted) => {
            chars.push(shifted);
            chars.into_iter().collect()
        }
        // Ran off the end of the code space; the range is lying.
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_h_splits_two_byte_codes() {
        let map = CMap::identity(false);
        assert_eq!(map.next_code(&[0x00, 0x41, 0x00]), (0x0041, 2));
        assert_eq!(map.cid(0x0041), Some(0x0041));
    }

    #[test]
    fn mixed_codespace_widths_split_correctly() {
        let map = parse(
            b"3 begincodespacerange\n\
              <00> <80>\n<8140> <9ffc>\n<a0> <de>\n\
              endcodespacerange",
        );
        assert_eq!(map.next_code(b"\x41\x42"), (0x41, 1));
        assert_eq!(map.next_code(b"\x81\x50zz"), (0x8150, 2));
        assert_eq!(map.next_code(b"\xa5z"), (0xa5, 1));
    }

    #[test]
    fn bfchar_and_bfrange_map_to_text() {
        let map = parse(
            b"1 beginbfchar <0003> <0020> endbfchar\n\
              2 beginbfrange\n\
              <0024> <0026> <0041>\n\
              <0030> <0031> [<0061> <00660066>]\n\
              endbfrange",
        );
        assert_eq!(map.text(0x0003).as_deref(), Some(" "));
        assert_eq!(map.text(0x0024).as_deref(), Some("A"));
        assert_eq!(map.text(0x0026).as_deref(), Some("C"));
        assert_eq!(map.text(0x0030).as_deref(), Some("a"));
        assert_eq!(map.text(0x0031).as_deref(), Some("ff"));
        assert_eq!(map.text(0x0099), None);
    }

    #[test]
    fn cidrange_offsets_from_the_range_start() {
        let map = parse(b"1 begincidrange <0020> <007e> 3 endcidrange");
        assert_eq!(map.cid(0x20), Some(3));
        assert_eq!(map.cid(0x21), Some(4));
        assert_eq!(map.cid(0x7f), None);
    }

    #[test]
    fn wmode_is_read_from_the_program() {
        assert!(parse(b"/WMode 1 def").vertical);
        assert!(!parse(b"/WMode 0 def").vertical);
    }

    #[test]
    fn surrogate_pairs_decode() {
        assert_eq!(utf16be(&[0xD8, 0x3D, 0xDE, 0x00]), "\u{1F600}");
    }

    #[test]
    fn an_unknown_usecmap_is_recorded_not_guessed() {
        let map = parse(b"/UniJIS-UCS2-H usecmap");
        assert_eq!(map.unresolved_parent.as_deref(), Some("UniJIS-UCS2-H"));
        assert!(map.is_empty());
    }
}
