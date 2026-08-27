//! What an embedded font program can tell extraction that the PDF's own font
//! dictionary sometimes cannot: the vertical extent of a glyph, an advance
//! where `/Widths` is missing, and the reverse cmap that turns a CID-keyed
//! font with no `/ToUnicode` into readable text.
//!
//! Everything is copied out at load time. `ttf_parser::Face` borrows its
//! bytes, and holding both in one struct would be self-referential for no
//! gain: the tables below are small and are consulted once per glyph code,
//! not once per glyph. A glyph count is a `u16`, so they are bounded by the
//! format rather than by a limit of ours.

use std::collections::BTreeMap;

pub struct Embedded {
    units_per_em: f64,
    /// Typographic ascent and descent in text space units (em = 1.0).
    pub ascent: Option<f64>,
    pub descent: Option<f64>,
    advances: Vec<u16>,
    unicode_by_gid: BTreeMap<u16, String>,
    gid_by_unicode: BTreeMap<u32, u16>,
    /// A `(3, 0)` symbol subtable, keyed by the code it maps rather than by a
    /// Unicode scalar. Symbolic TrueType fonts address glyphs through it.
    gid_by_symbol_code: BTreeMap<u32, u16>,
}

impl Embedded {
    /// `None` when the bytes are not an sfnt this build can read: a bare Type 1
    /// program, a CFF blob outside an OpenType wrapper, or a damaged subset.
    pub fn parse(data: &[u8]) -> Option<Embedded> {
        let face = ttf_parser::Face::parse(data, 0).ok()?;
        let units_per_em = f64::from(face.units_per_em());
        if units_per_em <= 0.0 {
            return None;
        }

        let count = face.number_of_glyphs();
        let mut advances = Vec::with_capacity(usize::from(count));
        for gid in 0..count {
            advances.push(
                face.glyph_hor_advance(ttf_parser::GlyphId(gid))
                    .unwrap_or(0),
            );
        }

        let mut unicode_by_gid = BTreeMap::new();
        let mut gid_by_unicode = BTreeMap::new();
        let mut gid_by_symbol_code = BTreeMap::new();
        if let Some(cmap) = face.tables().cmap {
            for subtable in cmap.subtables {
                let symbol = subtable.platform_id == ttf_parser::PlatformId::Windows
                    && subtable.encoding_id == 0;
                if !symbol && !subtable.is_unicode() {
                    continue;
                }
                let mut pairs = Vec::new();
                subtable.codepoints(|cp| pairs.push(cp));
                for cp in pairs {
                    let Some(gid) = subtable.glyph_index(cp) else {
                        continue;
                    };
                    if symbol {
                        gid_by_symbol_code.entry(cp).or_insert(gid.0);
                        continue;
                    }
                    gid_by_unicode.entry(cp).or_insert(gid.0);
                    if let Some(ch) = char::from_u32(cp) {
                        // The lowest code pointing at a glyph is the one worth
                        // reporting: ligature and small-cap variants map back
                        // to the same glyph and would otherwise win by order.
                        unicode_by_gid
                            .entry(gid.0)
                            .or_insert_with(|| ch.to_string());
                    }
                }
            }
        }

        // A subset embedded for Identity-H usually ships no cmap at all, so
        // the only thing left saying what a glyph is is the `post` table's
        // glyph names. Consulted only when the cmap said nothing, because a
        // font with a cmap has already given a better answer.
        if unicode_by_gid.is_empty() {
            for gid in 0..count {
                let Some(name) = face.glyph_name(ttf_parser::GlyphId(gid)) else {
                    continue;
                };
                if let Some(text) = super::glyph_name_to_string(name) {
                    unicode_by_gid.insert(gid, text);
                }
            }
        }

        let scale = 1.0 / units_per_em;
        let ascent = f64::from(face.ascender()) * scale;
        let descent = f64::from(face.descender()) * scale;
        Some(Embedded {
            units_per_em,
            ascent: (ascent > 0.0).then_some(ascent),
            descent: (descent < 0.0).then_some(descent),
            advances,
            unicode_by_gid,
            gid_by_unicode,
            gid_by_symbol_code,
        })
    }

    /// Advance of a glyph in text space units (em = 1.0).
    pub fn advance(&self, gid: u16) -> Option<f64> {
        let raw = *self.advances.get(usize::from(gid))?;
        (raw > 0).then(|| f64::from(raw) / self.units_per_em)
    }

    pub fn unicode(&self, gid: u16) -> Option<&str> {
        self.unicode_by_gid.get(&gid).map(String::as_str)
    }

    pub fn gid_for_unicode(&self, ch: char) -> Option<u16> {
        self.gid_by_unicode.get(&(ch as u32)).copied()
    }

    /// Symbolic TrueType lookup: the code itself, then the `0xF0xx` private-use
    /// alias Windows subsetters write (ISO 32000-2 9.6.5.4).
    pub fn gid_for_symbol_code(&self, code: u32) -> Option<u16> {
        self.gid_by_symbol_code
            .get(&code)
            .or_else(|| self.gid_by_symbol_code.get(&(0xF000 + code)))
            .copied()
    }
}
