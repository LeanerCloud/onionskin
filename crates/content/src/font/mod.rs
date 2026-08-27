//! Fonts, to the depth text extraction needs and no further.
//!
//! Three questions per character code, and this module answers exactly those:
//! how many bytes the code took, how far it advances the text position, and
//! what Unicode it stands for. Rendering questions - outlines, hinting,
//! shaping - belong to `render` and to M5's authoring path.
//!
//! The third answer is allowed to be "no idea". A code with no `/ToUnicode`,
//! no usable glyph name and no reverse cmap becomes [`crate::Mapping::Unmapped`]
//! and is counted in [`crate::error::Warning::UnmappedGlyphs`]. It never
//! becomes a question mark, and it is never dropped: the glyph is still
//! positioned, so selection and redaction still know it is there.

mod cmap;
mod embedded;
mod tables;

use std::collections::BTreeMap;

use onionskin_cos::{Dict, Document, ObjRef, Object};

use crate::error::Warning;
use crate::filter;
use crate::matrix::Matrix;

pub use cmap::CMap;
use embedded::Embedded;

/// Identifies a font across a document, so two runs in the same face compare
/// equal without comparing dictionaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontId {
    /// The usual case: the font dictionary is an indirect object.
    Object(ObjRef),
    /// A font dictionary written directly into a resource dictionary,
    /// numbered in the order the extraction met them.
    Direct(u32),
}

/// One character code lifted out of a shown string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code {
    /// The code as written, which for a composite font may be several bytes.
    pub value: u32,
    /// The CID the code selects. Equal to `value` for a simple font.
    pub cid: u32,
    /// How many bytes of the string the code consumed.
    pub len: u8,
}

impl Code {
    /// ISO 32000-2 9.3.3: word spacing applies to the single-byte code 32 and
    /// to nothing else, which is why a two-byte 0x0020 must not trigger it.
    pub fn is_word_space(&self) -> bool {
        self.len == 1 && self.value == 32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Simple,
    Type0,
    Type3,
}

enum Widths {
    /// `/FirstChar` plus `/Widths`, in glyph space.
    Simple {
        first: u32,
        widths: Vec<f64>,
    },
    /// `/W`, as singles and ranges, in glyph space.
    Cid {
        singles: BTreeMap<u32, f64>,
        ranges: Vec<(u32, u32, f64)>,
    },
    None,
}

pub struct Font {
    pub id: FontId,
    pub base_font: String,
    kind: Kind,
    /// Glyph names by code, for simple and Type 3 fonts.
    encoding: Vec<Option<String>>,
    /// The `/Encoding` CMap of a composite font.
    cmap: Option<CMap>,
    to_unicode: Option<CMap>,
    widths: Widths,
    /// Multiplies a glyph space width into text space: 1/1000 for every font
    /// except Type 3, which says so itself through `/FontMatrix`.
    width_scale: f64,
    missing_width: Option<f64>,
    default_width: f64,
    embedded: Option<Embedded>,
    base14: Option<&'static tables::Base14>,
    symbolic: bool,
    /// The font really is ZapfDingbats, so `a1` through `a191` mean what the
    /// dingbats glyph list says they mean rather than being ordinary names.
    dingbats: bool,
    /// A CIDFontType2 whose CIDs index glyphs directly, so the embedded
    /// font's reverse cmap can stand in for a missing `/ToUnicode`.
    cid_is_gid: bool,
    cid_to_gid: Option<Vec<u16>>,
    pub vertical: bool,
    ascent: f64,
    descent: f64,
}

/// Fallbacks for a font whose descriptor says nothing about its vertical
/// extent. Selection quads want the line box, not the ink box, so these are
/// the usual Latin text figures rather than anything derived from the glyph.
const DEFAULT_ASCENT: f64 = 0.75;
const DEFAULT_DESCENT: f64 = -0.25;

impl Font {
    /// Splits a shown string into codes.
    pub fn decode(&self, bytes: &[u8]) -> Vec<Code> {
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            match &self.cmap {
                Some(map) => {
                    let (value, len) = map.next_code(&bytes[i..]);
                    let len = usize::from(len).max(1);
                    out.push(Code {
                        value,
                        cid: map.cid(value).unwrap_or(value),
                        len: len as u8,
                    });
                    i += len;
                }
                None => {
                    let value = u32::from(bytes[i]);
                    out.push(Code {
                        value,
                        cid: value,
                        len: 1,
                    });
                    i += 1;
                }
            }
        }
        out
    }

    /// Horizontal displacement in text space units (1.0 is one em at the
    /// current font size). `None` when nothing in the font declares it, which
    /// the caller turns into a warning rather than a silent zero.
    pub fn displacement(&self, code: Code) -> Option<f64> {
        if let Some(glyph_space) = self.declared_width(code) {
            return Some(glyph_space * self.width_scale);
        }
        if let Some(base14) = self.base14 {
            if let Some(name) = self.glyph_name(code.value) {
                if let Some(width) = base14.width(name) {
                    return Some(f64::from(width) * self.width_scale);
                }
            }
        }
        if let Some(embedded) = &self.embedded {
            if let Some(gid) = self.gid(code) {
                if let Some(advance) = embedded.advance(gid) {
                    return Some(advance);
                }
            }
        }
        self.missing_width.map(|w| w * self.width_scale)
    }

    fn declared_width(&self, code: Code) -> Option<f64> {
        match &self.widths {
            Widths::Simple { first, widths } => {
                let index = code.value.checked_sub(*first)? as usize;
                widths.get(index).copied()
            }
            Widths::Cid { singles, ranges } => {
                if let Some(width) = singles.get(&code.cid) {
                    return Some(*width);
                }
                ranges
                    .iter()
                    .find(|(lo, hi, _)| code.cid >= *lo && code.cid <= *hi)
                    .map(|(_, _, w)| *w)
                    .or(Some(self.default_width))
            }
            Widths::None => None,
        }
    }

    /// The Unicode a code stands for, or `None` when the font does not say.
    ///
    /// In order: `/ToUnicode`; for a composite font the embedded program's
    /// reverse cmap or `post` names; the glyph name the encoding gives; the
    /// embedded program's symbol cmap. There is no step after those. Falling
    /// back to what the code would mean in Latin text would answer "A" for a
    /// glyph the font said was its fifth, and a caller cannot tell an invented
    /// character from a read one.
    pub fn unicode(&self, code: Code) -> Option<String> {
        if let Some(map) = &self.to_unicode {
            if let Some(text) = map.text(code.value) {
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        if self.kind == Kind::Type0 {
            let gid = self.gid(code)?;
            return self.embedded.as_ref()?.unicode(gid).map(str::to_owned);
        }
        if let Some(name) = self.glyph_name(code.value) {
            if let Some(text) = glyph_name_to_string_in(name, self.dingbats) {
                return Some(text);
            }
        }
        let embedded = self.embedded.as_ref()?;
        let gid = embedded.gid_for_symbol_code(code.value)?;
        embedded.unicode(gid).map(str::to_owned)
    }

    fn glyph_name(&self, code: u32) -> Option<&str> {
        self.encoding.get(code as usize)?.as_deref()
    }

    fn gid(&self, code: Code) -> Option<u16> {
        match self.kind {
            Kind::Type0 => {
                if let Some(map) = &self.cid_to_gid {
                    return map.get(code.cid as usize).copied();
                }
                self.cid_is_gid.then(|| code.cid.min(0xFFFF) as u16)
            }
            _ => {
                let embedded = self.embedded.as_ref()?;
                if self.symbolic || self.glyph_name(code.value).is_none() {
                    if let Some(gid) = embedded.gid_for_symbol_code(code.value) {
                        return Some(gid);
                    }
                }
                let name = self.glyph_name(code.value)?;
                let text = glyph_name_to_string_in(name, self.dingbats)?;
                embedded.gid_for_unicode(text.chars().next()?)
            }
        }
    }

    /// Top and bottom of the selection box in text space units.
    pub fn extents(&self) -> (f64, f64) {
        (self.ascent, self.descent)
    }

    /// A predefined CMap this build does not carry, so codes came out of an
    /// assumed two-byte codespace.
    pub fn unsupported_cmap(&self) -> Option<&str> {
        self.cmap.as_ref()?.unresolved_parent.as_deref()
    }
}

// ---- loading ----------------------------------------------------------------

/// Builds a font from its dictionary. Never fails: a font that gives up
/// nothing still positions its glyphs at the sizes the content stream asks
/// for, and every gap is visible through the warnings and through
/// [`crate::Mapping::Unmapped`].
pub fn load(doc: &Document, dict: &Dict, id: FontId, warnings: &mut Vec<Warning>) -> Font {
    let subtype = name_of(doc, dict, b"Subtype").unwrap_or_default();
    let base_font = strip_subset_prefix(&name_of(doc, dict, b"BaseFont").unwrap_or_default());

    let kind = match subtype.as_str() {
        "Type0" => Kind::Type0,
        "Type3" => Kind::Type3,
        _ => Kind::Simple,
    };

    // A composite font keeps its metrics and its font program on the
    // descendant; everything else keeps them on itself.
    let descendant = (kind == Kind::Type0)
        .then(|| descendant_font(doc, dict))
        .flatten();
    let metrics_dict = descendant.as_ref().unwrap_or(dict);
    let descriptor = resolve_dict(doc, metrics_dict, b"FontDescriptor");

    let flags = descriptor
        .as_ref()
        .and_then(|d| integer(doc, d, b"Flags"))
        .unwrap_or(0);
    // ISO 32000-2 table 121: bit position 3, so the mask is 4.
    let symbolic = flags & 4 != 0 && flags & 32 == 0;

    let embedded = descriptor
        .as_ref()
        .and_then(|d| font_program(doc, d))
        .and_then(|data| Embedded::parse(&data));

    let base14 = tables::base14_metrics(&base_font);
    let dingbats =
        base_font.eq_ignore_ascii_case("ZapfDingbats") || base_font.eq_ignore_ascii_case("ZaDb");

    let cmap = (kind == Kind::Type0).then(|| encoding_cmap(doc, dict, warnings));
    let vertical = cmap.as_ref().is_some_and(|c| c.vertical);
    let to_unicode = stream_data(doc, dict, b"ToUnicode").map(|d| cmap::parse(&d));

    let font_matrix = (kind == Kind::Type3)
        .then(|| {
            resolve(doc, dict, b"FontMatrix")
                .as_ref()
                .and_then(|o| o.as_array().map(Matrix::from_operands))
                .flatten()
        })
        .flatten();
    let width_scale = match font_matrix {
        Some(m) => m.a,
        None => 0.001,
    };

    let encoding = match kind {
        Kind::Type0 => Vec::new(),
        _ => simple_encoding(doc, dict, &base_font, symbolic, embedded.is_some()),
    };

    let widths = match kind {
        Kind::Type0 => cid_widths(doc, metrics_dict),
        _ => simple_widths(doc, dict),
    };

    let (cid_is_gid, cid_to_gid) = match kind {
        Kind::Type0 => cid_to_gid_map(doc, metrics_dict, descendant.as_ref()),
        _ => (false, None),
    };

    let scale = 0.001;
    let ascent = descriptor
        .as_ref()
        .and_then(|d| number(doc, d, b"Ascent"))
        .map(|v| v * scale)
        .filter(|v| *v > 0.0)
        .or_else(|| embedded.as_ref().and_then(|e| e.ascent))
        .unwrap_or(DEFAULT_ASCENT);
    let descent = descriptor
        .as_ref()
        .and_then(|d| number(doc, d, b"Descent"))
        .map(|v| v * scale)
        .filter(|v| *v < 0.0)
        .or_else(|| embedded.as_ref().and_then(|e| e.descent))
        .unwrap_or(DEFAULT_DESCENT);

    Font {
        id,
        base_font,
        kind,
        encoding,
        cmap,
        to_unicode,
        widths,
        width_scale,
        missing_width: descriptor
            .as_ref()
            .and_then(|d| number(doc, d, b"MissingWidth")),
        default_width: metrics_dict
            .get(b"DW")
            .and_then(|o| doc.resolve(o).ok())
            .as_ref()
            .and_then(crate::tokenizer::number)
            .unwrap_or(1000.0),
        embedded,
        base14,
        symbolic,
        dingbats,
        cid_is_gid,
        cid_to_gid,
        vertical,
        ascent,
        descent,
    }
}

fn descendant_font(doc: &Document, dict: &Dict) -> Option<Dict> {
    let entry = doc.resolve(dict.get(b"DescendantFonts")?).ok()?;
    let first = entry.as_array()?.first()?;
    doc.resolve(first).ok()?.as_dict().cloned()
}

fn encoding_cmap(doc: &Document, dict: &Dict, warnings: &mut Vec<Warning>) -> CMap {
    match dict.get(b"Encoding").map(|o| doc.resolve(o)) {
        Some(Ok(Object::Name(name))) => {
            let name = String::from_utf8_lossy(name.as_bytes()).into_owned();
            match cmap::predefined(&name) {
                Some(map) => map,
                None => {
                    warnings.push(Warning::UnsupportedCMap { name: name.clone() });
                    let mut map = CMap::opaque(2, name.ends_with("-V"));
                    map.unresolved_parent = Some(name);
                    map
                }
            }
        }
        _ => match stream_data(doc, dict, b"Encoding") {
            Some(data) => {
                let map = cmap::parse(&data);
                if let Some(parent) = &map.unresolved_parent {
                    warnings.push(Warning::UnsupportedCMap {
                        name: parent.clone(),
                    });
                }
                if map.is_empty() && map.unresolved_parent.is_none() {
                    return CMap::identity(false);
                }
                map
            }
            // No /Encoding on a Type0 font is malformed. Identity-H is what
            // every reader assumes, and it at least splits the codes right.
            None => CMap::identity(false),
        },
    }
}

fn simple_encoding(
    doc: &Document,
    dict: &Dict,
    base_font: &str,
    symbolic: bool,
    has_embedded: bool,
) -> Vec<Option<String>> {
    let builtin: &[Option<&'static str>; 256] = if base_font.eq_ignore_ascii_case("Symbol") {
        &tables::SYMBOL_ENCODING
    } else if base_font.eq_ignore_ascii_case("ZapfDingbats") {
        &tables::ZAPF_DINGBATS_ENCODING
    } else {
        &tables::STANDARD_ENCODING
    };

    let encoding = dict.get(b"Encoding").and_then(|o| doc.resolve(o).ok());
    let named = |name: &[u8]| -> Option<&'static [Option<&'static str>; 256]> {
        match name {
            b"WinAnsiEncoding" => Some(&tables::WIN_ANSI_ENCODING),
            b"MacRomanEncoding" => Some(&tables::MAC_ROMAN_ENCODING),
            b"StandardEncoding" => Some(&tables::STANDARD_ENCODING),
            b"PDFDocEncoding" => Some(&tables::PDF_DOC_ENCODING),
            // MacExpertEncoding is an expert glyph set this build has no table
            // for; its codes fall through to the built-in encoding.
            _ => None,
        }
    };

    let (base, differences) = match &encoding {
        Some(Object::Name(n)) => (named(n.as_bytes()).unwrap_or(builtin), None),
        Some(Object::Dict(d)) => {
            let base = match d.get(b"BaseEncoding").and_then(|o| doc.resolve(o).ok()) {
                Some(Object::Name(n)) => named(n.as_bytes()).unwrap_or(builtin),
                _ => builtin,
            };
            (
                base,
                d.get(b"Differences").and_then(|o| doc.resolve(o).ok()),
            )
        }
        _ => (builtin, None),
    };

    let mut out: Vec<Option<String>> = base
        .iter()
        .map(|n| n.map(|s| s.to_string()))
        .collect::<Vec<_>>();
    if symbolic
        && has_embedded
        && !matches!(encoding, Some(Object::Name(_)))
        && differences.is_none()
    {
        // Nothing named an encoding, the font is symbolic and carries its own
        // program: its glyph names are its own business.
        out = vec![None; 256];
    }

    if let Some(Object::Array(items)) = differences {
        let mut code = 0usize;
        for item in items {
            match doc.resolve(&item) {
                Ok(Object::Integer(i)) if (0..256).contains(&i) => code = i as usize,
                Ok(Object::Real(r)) if (0.0..256.0).contains(&r) => code = r as usize,
                Ok(Object::Name(n)) => {
                    if code < 256 {
                        out[code] = Some(String::from_utf8_lossy(n.as_bytes()).into_owned());
                    }
                    code += 1;
                }
                _ => {}
            }
        }
    }
    out
}

fn simple_widths(doc: &Document, dict: &Dict) -> Widths {
    let Some(Object::Array(items)) = resolve(doc, dict, b"Widths") else {
        return Widths::None;
    };
    let first = integer(doc, dict, b"FirstChar").unwrap_or(0).max(0) as u32;
    let widths: Vec<f64> = items
        .iter()
        .map(|o| {
            doc.resolve(o)
                .ok()
                .as_ref()
                .and_then(crate::tokenizer::number)
                .unwrap_or(0.0)
        })
        .collect();
    if widths.is_empty() {
        return Widths::None;
    }
    Widths::Simple { first, widths }
}

/// `/W` is `[ c [w1 w2 ...] cfirst clast w ... ]` (ISO 32000-2 9.7.4.3).
fn cid_widths(doc: &Document, dict: &Dict) -> Widths {
    let mut singles = BTreeMap::new();
    let mut ranges = Vec::new();
    if let Some(Object::Array(items)) = resolve(doc, dict, b"W") {
        let resolved: Vec<Object> = items
            .iter()
            .map(|o| doc.resolve(o).unwrap_or(Object::Null))
            .collect();
        let mut i = 0usize;
        while i < resolved.len() {
            let Some(first) = crate::tokenizer::number(&resolved[i]) else {
                i += 1;
                continue;
            };
            let first = first.max(0.0) as u32;
            match resolved.get(i + 1) {
                Some(Object::Array(list)) => {
                    for (k, item) in list.iter().enumerate() {
                        if let Some(width) = doc
                            .resolve(item)
                            .ok()
                            .as_ref()
                            .and_then(crate::tokenizer::number)
                        {
                            singles.insert(first + k as u32, width);
                        }
                    }
                    i += 2;
                }
                Some(second) => {
                    let (Some(last), Some(width)) = (
                        crate::tokenizer::number(second),
                        resolved.get(i + 2).and_then(crate::tokenizer::number),
                    ) else {
                        i += 2;
                        continue;
                    };
                    ranges.push((first, last.max(0.0) as u32, width));
                    i += 3;
                }
                None => break,
            }
        }
    }
    Widths::Cid { singles, ranges }
}

fn cid_to_gid_map(
    doc: &Document,
    metrics: &Dict,
    descendant: Option<&Dict>,
) -> (bool, Option<Vec<u16>>) {
    let is_type2 = descendant
        .and_then(|d| name_of(doc, d, b"Subtype"))
        .is_some_and(|s| s == "CIDFontType2");
    if !is_type2 {
        // A CIDFontType0's CID-to-glyph mapping lives in the CFF charset,
        // which this build does not read. Guessing identity there would put
        // wrong characters in the extracted text.
        return (false, None);
    }
    match stream_data(doc, metrics, b"CIDToGIDMap") {
        Some(data) => {
            let map = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes(*p))
                .collect();
            (true, Some(map))
        }
        None => (true, None),
    }
}

fn font_program(doc: &Document, descriptor: &Dict) -> Option<Vec<u8>> {
    for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
        if let Some(data) = stream_data(doc, descriptor, key) {
            return Some(data);
        }
    }
    None
}

fn stream_data(doc: &Document, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    let object = doc.resolve(dict.get(key)?).ok()?;
    let stream = object.as_stream()?;
    filter::decode(&stream.dict, &stream.raw, &|o| doc.resolve(o)).ok()
}

fn resolve(doc: &Document, dict: &Dict, key: &[u8]) -> Option<Object> {
    doc.resolve(dict.get(key)?).ok()
}

fn resolve_dict(doc: &Document, dict: &Dict, key: &[u8]) -> Option<Dict> {
    resolve(doc, dict, key)?.as_dict().cloned()
}

fn name_of(doc: &Document, dict: &Dict, key: &[u8]) -> Option<String> {
    match resolve(doc, dict, key)? {
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

fn integer(doc: &Document, dict: &Dict, key: &[u8]) -> Option<i64> {
    resolve(doc, dict, key)?.as_integer()
}

fn number(doc: &Document, dict: &Dict, key: &[u8]) -> Option<f64> {
    crate::tokenizer::number(&resolve(doc, dict, key)?)
}

/// Drops the `ABCDEF+` tag a subsetter puts in front of `/BaseFont`.
fn strip_subset_prefix(name: &str) -> String {
    let bytes = name.as_bytes();
    if bytes.len() > 7 && bytes[6] == b'+' && bytes[..6].iter().all(|b| b.is_ascii_uppercase()) {
        return name[7..].to_string();
    }
    name.to_string()
}

/// A PDF text string (ISO 32000-2 7.9.2.2): UTF-8 or UTF-16 behind a byte
/// order mark, PDFDocEncoding otherwise. A PDFDocEncoding byte with no glyph
/// name is taken as Latin-1, which the encoding already agrees with over most
/// of its range.
pub fn pdf_text_string(bytes: &[u8]) -> String {
    match bytes {
        [0xFE, 0xFF, rest @ ..] => cmap::utf16be(rest),
        // PDF 2.0 added UTF-8, and the little-endian mark is not legal but is
        // written; both are unambiguous, so reading them costs nothing.
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, rest @ ..] => char::decode_utf16(
            rest.as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair)),
        )
        .flatten()
        .collect(),
        _ => bytes
            .iter()
            .map(|b| {
                tables::PDF_DOC_ENCODING[usize::from(*b)]
                    .and_then(tables::glyph_name_to_unicode)
                    .unwrap_or(char::from(*b))
            })
            .collect(),
    }
}

/// Glyph name to text, covering the Adobe Glyph List plus the algorithmic
/// names of the AGL specification.
///
/// Names outside all of those return `None` rather than a guess. `g12`,
/// `index7` and `cid41` name a glyph by position in a font program and say
/// nothing at all about the character it draws.
///
/// ZapfDingbats names are excluded here. Adobe publishes them in a separate
/// list for a reason: `a1` through `a191` are perfectly ordinary names for a
/// Type 3 glyph or a subsetter's `post` table, and resolving them against the
/// dingbats list turns a page of letters into a page of scissors and pencils.
/// [`glyph_name_to_string_in`] opts back in for a font that really is
/// ZapfDingbats.
pub fn glyph_name_to_string(name: &str) -> Option<String> {
    glyph_name_to_string_in(name, false)
}

pub fn glyph_name_to_string_in(name: &str, dingbats: bool) -> Option<String> {
    // A suffix after a period is a variant of the base glyph: `a.sc`, `one.oldstyle`.
    let name = name.split('.').next().filter(|s| !s.is_empty())?;

    // AGL joins multiple characters with underscores.
    if name.contains('_') {
        let mut out = String::new();
        for part in name.split('_') {
            out.push_str(&single_glyph_name(part, dingbats)?);
        }
        return (!out.is_empty()).then_some(out);
    }
    single_glyph_name(name, dingbats)
}

/// `a` followed by digits: the shape every ZapfDingbats glyph name has, and no
/// Latin AGL name has.
fn is_dingbats_name(name: &str) -> bool {
    match name.strip_prefix('a') {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

fn single_glyph_name(name: &str, dingbats: bool) -> Option<String> {
    if dingbats || !is_dingbats_name(name) {
        if let Some(ch) = tables::glyph_name_to_unicode(name) {
            return Some(String::from(ch));
        }
    }
    if let Some(hex) = name.strip_prefix("uni") {
        // uniXXXX, and the sequences uniXXXXXXXX the AGL allows.
        if hex.len() >= 4 && hex.len() % 4 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let units: Vec<u16> = hex
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                .collect();
            let text: String = char::decode_utf16(units).flatten().collect();
            return (!text.is_empty()).then_some(text);
        }
        return None;
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .map(String::from);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_prefixes_are_stripped_only_when_well_formed() {
        assert_eq!(strip_subset_prefix("ABCDEF+Arial"), "Arial");
        assert_eq!(strip_subset_prefix("Abcdef+Arial"), "Abcdef+Arial");
        assert_eq!(strip_subset_prefix("Helvetica"), "Helvetica");
        assert_eq!(strip_subset_prefix("ABCDEF+"), "ABCDEF+");
    }

    #[test]
    fn algorithmic_glyph_names_resolve() {
        assert_eq!(glyph_name_to_string("uni0041").as_deref(), Some("A"));
        assert_eq!(glyph_name_to_string("u1F600").as_deref(), Some("\u{1F600}"));
        assert_eq!(glyph_name_to_string("f_f_i").as_deref(), Some("ffi"));
        assert_eq!(glyph_name_to_string("a.sc").as_deref(), Some("a"));
    }

    #[test]
    fn pdf_text_strings_follow_their_byte_order_mark() {
        assert_eq!(pdf_text_string(&[0xFE, 0xFF, 0x00, 0x41, 0x00, 0x42]), "AB");
        assert_eq!(pdf_text_string(b"AB"), "AB");
        assert_eq!(pdf_text_string(&[0xEF, 0xBB, 0xBF, b'A', b'B']), "AB");
        assert_eq!(pdf_text_string(&[0xFF, 0xFE, 0x41, 0x00, 0x42, 0x00]), "AB");
        // Without a mark the bytes are PDFDocEncoding, which is neither
        // Latin-1 (0x92 is a C1 control there) nor WinAnsi (a right single
        // quote there).
        assert_eq!(pdf_text_string(&[0x92]), "\u{2122}");
    }

    #[test]
    fn dingbats_names_need_a_dingbats_font() {
        // `a1` is a Type 3 glyph name far more often than it is scissors.
        assert_eq!(glyph_name_to_string("a1"), None);
        assert_eq!(
            glyph_name_to_string_in("a1", true).as_deref(),
            Some("\u{2701}")
        );
        // A real letter keeps working either way.
        assert_eq!(glyph_name_to_string("a").as_deref(), Some("a"));
        assert_eq!(glyph_name_to_string_in("a", true).as_deref(), Some("a"));
    }

    #[test]
    fn position_only_glyph_names_stay_unmapped() {
        assert_eq!(glyph_name_to_string("g12"), None);
        assert_eq!(glyph_name_to_string("index7"), None);
        assert_eq!(glyph_name_to_string("cid41"), None);
        assert_eq!(glyph_name_to_string(""), None);
    }

    #[test]
    fn word_spacing_is_single_byte_code_32_only() {
        assert!(Code {
            value: 32,
            cid: 32,
            len: 1
        }
        .is_word_space());
        assert!(!Code {
            value: 32,
            cid: 32,
            len: 2
        }
        .is_word_space());
    }
}
