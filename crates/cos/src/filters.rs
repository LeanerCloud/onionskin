//! Stream filters.
//!
//! One decoder serves both callers. The structural layer decodes
//! cross-reference streams and object streams; [`crate::Document::decode_stream`]
//! decodes everything above it - page descriptions, embedded font programs,
//! CMaps, attachments. The filter set is the union of what those need: Flate
//! and LZW with the PNG and TIFF predictors, RunLength, and the two ASCII
//! armours. Image codecs are not here and fail loud as
//! [`Error::UnsupportedFilter`].
//!
//! What the two callers cannot share is what a payload that only half decodes
//! means, which is what [`Damaged`] selects.

use std::io::{Read, Write};

use crate::error::{Error, Result};
use crate::object::{Dict, Object};

/// What a decode does with a payload that does not decode all the way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Damaged {
    /// Refuse the stream. Half a cross-reference stream is a fabricated
    /// cross-reference entry and half an object stream is an object graph that
    /// disagrees with the file, so the structural layer takes nothing partial.
    Refuse,
    /// Keep whatever decoded. A page description cut short by a wrong
    /// `/Length` is common, its first operators are still the page's real
    /// text, and refusing would cost the whole page to save nothing.
    Salvage,
}

impl Damaged {
    /// Ceiling on one decoded stream, past which a decompression bomb is an
    /// error rather than the machine.
    ///
    /// The two callers decode different things, so they get different room. A
    /// cross-reference stream covering ten million objects is tens of
    /// megabytes and legitimate; a page description, a font program or a CMap
    /// is orders of magnitude smaller than either figure.
    ///
    /// Exceeding it is an error under both, unlike a stream that merely ends
    /// early. A payload cut at the ceiling is indistinguishable from one that
    /// ended there, so handing it back would be the one silent degradation
    /// `Salvage` is not allowed: partial output is worth having only when the
    /// caller can tell it apart from the whole.
    fn ceiling(self) -> u64 {
        match self {
            Damaged::Refuse => 256 * 1024 * 1024,
            Damaged::Salvage => 128 * 1024 * 1024,
        }
    }
}

/// Decodes a stream's raw bytes through its `/Filter` chain.
///
/// `resolve` follows indirect references found in `/Filter` or `/DecodeParms`.
pub(crate) fn decode(
    dict: &Dict,
    raw: &[u8],
    resolve: &dyn Fn(&Object) -> Result<Object>,
    damaged: Damaged,
) -> Result<Vec<u8>> {
    let filters = filter_names(dict, resolve)?;
    if filters.is_empty() {
        return Ok(raw.to_vec());
    }
    let parms = decode_parms(dict, resolve, filters.len(), damaged)?;

    let mut data = raw.to_vec();
    for (i, name) in filters.iter().enumerate() {
        let parm = parms.get(i).cloned().flatten();
        data = match name.as_str() {
            "FlateDecode" | "Fl" => {
                let flat = inflate(&data, damaged)?;
                apply_predictor(flat, "FlateDecode", parm.as_ref(), resolve, damaged)?
            }
            "LZWDecode" | "LZW" => {
                let early = parm
                    .as_ref()
                    .and_then(|p| p.get(b"EarlyChange"))
                    .and_then(Object::as_integer)
                    .unwrap_or(1);
                let flat = lzw(&data, early != 0, damaged)?;
                apply_predictor(flat, "LZWDecode", parm.as_ref(), resolve, damaged)?
            }
            "ASCIIHexDecode" | "AHx" => ascii_hex_decode(&data)?,
            "ASCII85Decode" | "A85" => ascii85_decode(&data, damaged)?,
            "RunLengthDecode" | "RL" => run_length_decode(&data, damaged)?,
            // Identity /Crypt is a no-op marker and the default when
            // `/DecodeParms /Name` is absent. A named handler is a key into
            // the `/CF` dictionary of the trailer's `/Encrypt`, and every open
            // path refuses a trailer that has one, so a document that got this
            // far defines no such handler: the name resolves to nothing and
            // the file does not say what, if anything, was applied to these
            // bytes. Reading it as Identity is a guess about a file that
            // contradicts itself, so it is refused by name instead.
            "Crypt" => {
                let named = parm
                    .as_ref()
                    .and_then(|p| p.get(b"Name"))
                    .and_then(Object::as_name)
                    .map(|n| n.as_bytes().to_vec());
                match named.as_deref() {
                    None | Some(b"Identity") => data,
                    Some(other) => {
                        return Err(Error::UnsupportedFilter(format!(
                            "Crypt /{}",
                            String::from_utf8_lossy(other)
                        )))
                    }
                }
            }
            other => return Err(Error::UnsupportedFilter(other.to_string())),
        };
    }
    Ok(data)
}

fn filter_names(dict: &Dict, resolve: &dyn Fn(&Object) -> Result<Object>) -> Result<Vec<String>> {
    let (abbreviated, entry) = match dict.get(b"Filter") {
        Some(entry) => (false, entry),
        None => match dict.get(b"F") {
            Some(entry) => (true, entry),
            None => return Ok(Vec::new()),
        },
    };
    let entry = resolve(entry)?;
    // `/F` spells both the `/Filter` abbreviation and a file specification. A
    // string or dictionary there means the data is in another file, which is
    // not something to paper over with an empty decode.
    if abbreviated && matches!(entry, Object::String(_) | Object::Dict(_)) {
        return Err(Error::Filter {
            filter: "F".into(),
            detail: "stream data lives in an external file".into(),
        });
    }
    let one = |o: &Object| -> Result<Option<String>> {
        match o {
            Object::Name(n) => Ok(Some(String::from_utf8_lossy(n.as_bytes()).into_owned())),
            Object::Null => Ok(None),
            _ => Err(Error::Filter {
                filter: "Filter".into(),
                detail: "filter entry is not a name".into(),
            }),
        }
    };
    match entry {
        Object::Null => Ok(Vec::new()),
        Object::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in &items {
                if let Some(name) = one(&resolve(item)?)? {
                    out.push(name);
                }
            }
            Ok(out)
        }
        other => Ok(one(&other)?.into_iter().collect()),
    }
}

fn decode_parms(
    dict: &Dict,
    resolve: &dyn Fn(&Object) -> Result<Object>,
    count: usize,
    damaged: Damaged,
) -> Result<Vec<Option<Dict>>> {
    let entry = dict.get(b"DecodeParms").or_else(|| dict.get(b"DP"));
    let Some(entry) = entry else {
        return Ok(vec![None; count]);
    };
    let mut out = vec![None; count];
    match resolve(entry)? {
        Object::Dict(d) => {
            if let Some(slot) = out.first_mut() {
                *slot = Some(d);
            }
        }
        Object::Array(items) => {
            for (i, item) in items.iter().take(count).enumerate() {
                if let Object::Dict(d) = resolve(item)? {
                    out[i] = Some(d);
                }
            }
        }
        Object::Null => {}
        // A malformed /DecodeParms costs a content stream its predictor and
        // costs a cross-reference stream its meaning.
        _ => {
            if damaged == Damaged::Refuse {
                return Err(Error::Filter {
                    filter: "DecodeParms".into(),
                    detail: "/DecodeParms is neither a dictionary nor an array".into(),
                });
            }
        }
    }
    Ok(out)
}

/// Zlib first, then raw deflate: producers ship both, and the wrapper is a
/// two-byte header rather than a semantic difference.
fn inflate(data: &[u8], damaged: Damaged) -> Result<Vec<u8>> {
    let start = data
        .iter()
        .position(|b| !crate::parse::is_whitespace(*b))
        .unwrap_or(data.len());
    let trimmed = &data[start..];
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    // One byte past the ceiling, so a payload that stops exactly at it is a
    // stream that ended rather than one that was cut.
    let mut zlib = Vec::new();
    let zlib_err = flate2::read::ZlibDecoder::new(trimmed)
        .take(damaged.ceiling() + 1)
        .read_to_end(&mut zlib)
        .err();
    if zlib_err.is_none() && !zlib.is_empty() {
        return bounded(zlib, damaged);
    }

    let mut raw = Vec::new();
    let raw_err = flate2::read::DeflateDecoder::new(trimmed)
        .take(damaged.ceiling() + 1)
        .read_to_end(&mut raw)
        .err();
    if raw_err.is_none() && !raw.is_empty() {
        return bounded(raw, damaged);
    }

    // Neither wrapper produced bytes. A decode that reached a clean end of
    // stream produced none because there were none: a blank compressed content
    // stream is exactly that, and calling it a filter failure warned about
    // every empty page.
    if zlib_err.is_none() || raw_err.is_none() {
        return Ok(Vec::new());
    }

    let failure = || Error::Filter {
        filter: "FlateDecode".into(),
        detail: match &zlib_err {
            Some(e) => e.to_string(),
            None => "inflated to zero bytes".to_string(),
        },
    };
    match damaged {
        Damaged::Refuse => Err(failure()),
        // Both paths errored. Keep the longer partial inflation if either
        // produced anything; report the failure only when neither did.
        Damaged::Salvage => {
            let best = if zlib.len() >= raw.len() { zlib } else { raw };
            if best.is_empty() {
                Err(failure())
            } else {
                Ok(best)
            }
        }
    }
}

fn bounded(data: Vec<u8>, damaged: Damaged) -> Result<Vec<u8>> {
    let ceiling = damaged.ceiling();
    if data.len() as u64 > ceiling {
        return Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: format!("inflated past the {ceiling} byte ceiling"),
        });
    }
    Ok(data)
}

/// A sink that refuses to grow past `limit`. LZW expands without bound - a few
/// kilobytes of codes reach gigabytes - so the decoder needs the ceiling that
/// `inflate` gets from `Read::take`, and like that one it is set a byte over so
/// output stopping exactly at the ceiling is not mistaken for output that fits.
struct Bounded<'a> {
    out: &'a mut Vec<u8>,
    limit: usize,
}

impl Write for Bounded<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let room = self.limit.saturating_sub(self.out.len());
        if room == 0 {
            return Err(std::io::Error::other("output ceiling reached"));
        }
        let take = room.min(buf.len());
        self.out.extend_from_slice(&buf[..take]);
        Ok(take)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn lzw(data: &[u8], early_change: bool, damaged: Damaged) -> Result<Vec<u8>> {
    // PDF's LZW is the TIFF variant: MSB-first, 8-bit symbols, and by default
    // it grows the code width one code early.
    let mut decoder = if early_change {
        weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
    } else {
        weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
    };
    let ceiling = damaged.ceiling();
    let mut out = Vec::new();
    let mut sink = Bounded {
        out: &mut out,
        limit: ceiling as usize + 1,
    };
    let result = decoder.into_stream(&mut sink).decode_all(data);
    if out.len() as u64 > ceiling {
        return Err(Error::Filter {
            filter: "LZWDecode".into(),
            detail: format!("expanded past the {ceiling} byte ceiling"),
        });
    }
    if let Err(e) = result.status {
        // Same rule as inflate: a stream that merely ends early keeps what
        // decoded when the caller can use a partial page.
        if damaged == Damaged::Refuse || out.is_empty() {
            return Err(Error::Filter {
                filter: "LZWDecode".into(),
                detail: e.to_string(),
            });
        }
    }
    Ok(out)
}

/// Undoes the `/DecodeParms` predictor a Flate or LZW stream was encoded with.
/// `filter` is the filter the parameters belong to, so an error names the
/// entry a reader would go looking for rather than the predictor step.
fn apply_predictor(
    data: Vec<u8>,
    filter: &str,
    parms: Option<&Dict>,
    resolve: &dyn Fn(&Object) -> Result<Object>,
    damaged: Damaged,
) -> Result<Vec<u8>> {
    let Some(parms) = parms else {
        return Ok(data);
    };
    let int = |key: &[u8], default: i64| -> Result<i64> {
        let Some(entry) = parms.get(key) else {
            return Ok(default);
        };
        match resolve(entry) {
            Ok(value) => Ok(value.as_integer().unwrap_or(default)),
            Err(e) if damaged == Damaged::Refuse => Err(e),
            Err(_) => Ok(default),
        }
    };
    let predictor = int(b"Predictor", 1)?;
    if predictor <= 1 {
        return Ok(data);
    }
    // These three come straight out of the file, so their product is bounded
    // before it becomes an allocation.
    let colors = int(b"Colors", 1)?.clamp(1, 32) as usize;
    let bpc = int(b"BitsPerComponent", 8)?.clamp(1, 32) as usize;
    let columns = int(b"Columns", 1)?.max(1) as usize;
    let bpp = (colors * bpc).div_ceil(8).max(1);
    let row_len = colors
        .checked_mul(bpc)
        .and_then(|bits| bits.checked_mul(columns))
        .map(|bits| bits.div_ceil(8))
        .filter(|len| *len <= data.len())
        .ok_or_else(|| Error::Filter {
            filter: filter.into(),
            detail: format!(
                "row of {colors}x{bpc}x{columns} does not fit {} bytes of data",
                data.len()
            ),
        })?;

    if predictor == 2 {
        return tiff_predictor(data, filter, colors, bpc, columns, damaged);
    }

    // PNG predictors: each row is prefixed with its filter type byte.
    let stride = row_len + 1;
    if damaged == Damaged::Refuse {
        if data.len() < stride {
            return Err(Error::Filter {
                filter: filter.into(),
                detail: format!(
                    "predictor {predictor} needs at least {stride} bytes, got {}",
                    data.len()
                ),
            });
        }
        // A partial final row would be zero-padded into a plausible-looking
        // row, which for a cross-reference stream means a fabricated entry.
        if !data.len().is_multiple_of(stride) {
            return Err(Error::Filter {
                filter: filter.into(),
                detail: format!(
                    "{} bytes is not a whole number of {stride} byte predictor rows",
                    data.len()
                ),
            });
        }
    }
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0u8; row_len];
    for chunk in data.chunks(stride) {
        // A short final row is the tail of a truncated stream, not a row.
        // Unreachable under `Refuse`, which rejected such data above.
        if chunk.len() < stride {
            break;
        }
        let tag = chunk[0];
        let mut row = chunk[1..].to_vec();
        for i in 0..row_len {
            let left = if i >= bpp { row[i - bpp] } else { 0 };
            let up = previous[i];
            let up_left = if i >= bpp { previous[i - bpp] } else { 0 };
            row[i] = match tag {
                0 => row[i],
                1 => row[i].wrapping_add(left),
                2 => row[i].wrapping_add(up),
                3 => row[i].wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                4 => row[i].wrapping_add(paeth(left, up, up_left)),
                other => {
                    return Err(Error::Filter {
                        filter: filter.into(),
                        detail: format!("unknown PNG predictor tag {other}"),
                    })
                }
            };
        }
        out.extend_from_slice(&row);
        previous = row;
    }
    Ok(out)
}

fn tiff_predictor(
    mut data: Vec<u8>,
    filter: &str,
    colors: usize,
    bpc: usize,
    columns: usize,
    damaged: Damaged,
) -> Result<Vec<u8>> {
    if bpc != 8 {
        // Sub-byte components are only reachable through image data, which
        // never reaches a caller that can use a partial result; leaving the
        // bytes untouched beats guessing at the packing.
        return match damaged {
            Damaged::Refuse => Err(Error::Filter {
                filter: filter.into(),
                detail: format!("TIFF predictor with {bpc} bits per component is not implemented"),
            }),
            Damaged::Salvage => Ok(data),
        };
    }
    let row_len = colors * columns;
    if row_len == 0 {
        return Ok(data);
    }
    for row in data.chunks_mut(row_len) {
        for i in colors..row.len() {
            row[i] = row[i].wrapping_add(row[i - colors]);
        }
    }
    Ok(data)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn ascii_hex_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut nibbles = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        if crate::parse::is_whitespace(b) {
            continue;
        }
        let value = crate::parse::hex_value(b).ok_or_else(|| Error::Filter {
            filter: "ASCIIHexDecode".into(),
            detail: format!("non-hex byte 0x{b:02x}"),
        })?;
        nibbles.push(value);
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    Ok(nibbles.chunks(2).map(|p| p[0] * 16 + p[1]).collect())
}

fn ascii85_decode(data: &[u8], damaged: Damaged) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut count = 0usize;
    let mut i = if data.starts_with(b"<~") { 2 } else { 0 };
    while i < data.len() {
        let b = data[i];
        i += 1;
        if crate::parse::is_whitespace(b) {
            continue;
        }
        if b == b'~' {
            break;
        }
        if b == b'z' && count == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            return Err(Error::Filter {
                filter: "ASCII85Decode".into(),
                detail: format!("byte 0x{b:02x} outside the base-85 alphabet"),
            });
        }
        group[count] = b - b'!';
        count += 1;
        if count == 5 {
            push_base85(&mut out, &group, 5, damaged)?;
            count = 0;
        }
    }
    match count {
        0 => {}
        // A final group of one digit encodes nothing; ISO 32000-1 7.4.3 calls
        // it an error rather than something to round down.
        1 => {
            if damaged == Damaged::Refuse {
                return Err(Error::Filter {
                    filter: "ASCII85Decode".into(),
                    detail: "final group holds a single character".into(),
                });
            }
        }
        _ => {
            for slot in group.iter_mut().skip(count) {
                *slot = 84;
            }
            push_base85(&mut out, &group, count, damaged)?;
        }
    }
    Ok(out)
}

/// A group encoding more than 32 bits is malformed. Under `Salvage` it wraps
/// rather than erroring, because losing four bytes of a page description costs
/// one operator and erroring costs the whole page's text.
fn push_base85(out: &mut Vec<u8>, group: &[u8; 5], count: usize, damaged: Damaged) -> Result<()> {
    let mut value: u32 = 0;
    for &digit in group.iter() {
        value = match value
            .checked_mul(85)
            .and_then(|v| v.checked_add(u32::from(digit)))
        {
            Some(v) => v,
            None if damaged == Damaged::Refuse => {
                return Err(Error::Filter {
                    filter: "ASCII85Decode".into(),
                    detail: "group encodes a value larger than 32 bits".into(),
                })
            }
            None => value.wrapping_mul(85).wrapping_add(u32::from(digit)),
        };
    }
    out.extend_from_slice(&value.to_be_bytes()[..count - 1]);
    Ok(())
}

fn run_length_decode(data: &[u8], damaged: Damaged) -> Result<Vec<u8>> {
    let ceiling = damaged.ceiling();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let length = data[i];
        i += 1;
        match length {
            128 => break,
            0..=127 => {
                let take = usize::from(length) + 1;
                let end = (i + take).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                let Some(&byte) = data.get(i) else { break };
                i += 1;
                out.extend(std::iter::repeat_n(byte, 257 - usize::from(length)));
            }
        }
        if out.len() as u64 > ceiling {
            return Err(Error::Filter {
                filter: "RunLengthDecode".into(),
                detail: format!("expanded past the {ceiling} byte ceiling"),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(o: &Object) -> Result<Object> {
        Ok(o.clone())
    }

    fn filter_dict(name: &str) -> Dict {
        let mut dict = Dict::new();
        dict.set("Filter", Object::name(name));
        dict
    }

    #[test]
    fn flate_round_trips() {
        use flate2::write::ZlibEncoder;

        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"the quick brown fox").unwrap();
        let raw = encoder.finish().unwrap();

        for damaged in [Damaged::Refuse, Damaged::Salvage] {
            assert_eq!(
                decode(&filter_dict("FlateDecode"), &raw, &identity, damaged).unwrap(),
                b"the quick brown fox"
            );
        }
    }

    /// A stream cut short mid-deflate decodes to its prefix, under both
    /// callers. Worth pinning because it is not obvious: `flate2` reports a
    /// stream that simply ends as end-of-input rather than as corruption, so
    /// what [`Damaged::Refuse`] rejects is a payload the decoder actively
    /// rejects, a predictor that does not tile, or the size ceiling - not the
    /// common case of a wrong `/Length`.
    #[test]
    fn a_truncated_flate_stream_decodes_to_its_prefix() {
        use flate2::write::ZlibEncoder;

        let source = b"BT /F1 12 Tf (the page's real text) Tj ET\n".repeat(100);
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&source).unwrap();
        let full = encoder.finish().unwrap();
        let cut = &full[..full.len() * 3 / 4];

        let dict = filter_dict("FlateDecode");
        for damaged in [Damaged::Refuse, Damaged::Salvage] {
            let out = decode(&dict, cut, &identity, damaged).expect("the prefix decodes");
            assert!(out.starts_with(b"BT /F1 12 Tf (the page's real text) Tj ET"));
            assert!(out.len() < source.len(), "the whole stream cannot decode");
        }
    }

    /// The split that a cross-reference stream depends on: a predictor whose
    /// last row is short. Zero-padding it would fabricate a cross-reference
    /// entry, so the structural layer refuses; a content stream drops the
    /// partial row and keeps the rest.
    #[test]
    fn a_partial_predictor_row_is_refused_structurally_and_dropped_for_content() {
        use flate2::write::ZlibEncoder;

        // Two whole PNG-predictor rows of four "up" bytes each, then one byte
        // of a third row.
        let rows = [0u8, 1, 2, 3, 4, 0, 5, 6, 7, 8, 0];
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&rows).unwrap();
        let raw = encoder.finish().unwrap();

        let mut parms = Dict::new();
        parms.set("Predictor", Object::Integer(12));
        parms.set("Columns", Object::Integer(4));
        let mut dict = filter_dict("FlateDecode");
        dict.set("DecodeParms", Object::Dict(parms));

        assert_eq!(
            decode(&dict, &raw, &identity, Damaged::Salvage).unwrap(),
            vec![1u8, 2, 3, 4, 5, 6, 7, 8]
        );
        let err = decode(&dict, &raw, &identity, Damaged::Refuse).unwrap_err();
        assert_eq!(err.category(), "filter-failed");
        assert!(
            err.to_string().contains("whole number"),
            "{err} does not say why the rows do not fit"
        );
    }

    /// A zlib stream of nothing is a stream of nothing, not a broken stream.
    /// Producers emit one for a blank content stream, and reporting it as a
    /// filter failure turned every blank page into a warning.
    #[test]
    fn an_empty_flate_stream_decodes_to_nothing() {
        use flate2::write::ZlibEncoder;

        let raw = ZlibEncoder::new(Vec::new(), flate2::Compression::default())
            .finish()
            .unwrap();

        for damaged in [Damaged::Refuse, Damaged::Salvage] {
            assert_eq!(
                decode(&filter_dict("FlateDecode"), &raw, &identity, damaged).unwrap(),
                Vec::<u8>::new(),
                "{damaged:?} rejected a valid zlib stream of empty input"
            );
        }
    }

    /// The ceiling is the same answer for every filter and both callers: an
    /// error naming the ceiling, never a silently truncated payload. A caller
    /// handed exactly the ceiling could not tell a stream that ended from one
    /// that was cut off, which is the silent degradation this crate does not
    /// do.
    ///
    /// The fixtures are sized past the larger of the two ceilings so one set
    /// clears both.
    #[test]
    fn every_ceiling_is_an_error_that_names_itself() {
        let over = Damaged::Refuse.ceiling().max(Damaged::Salvage.ceiling()) + 4096;

        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::copy(&mut std::io::repeat(b'A').take(over), &mut encoder)
            .expect("the fixture encodes");
        let flate = encoder.finish().expect("the fixture encodes");

        let mut lzw_bytes = Vec::new();
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .into_stream(&mut lzw_bytes)
            .encode_all(std::io::BufReader::new(std::io::repeat(b'A').take(over)))
            .status
            .expect("the fixture encodes");

        // 0x81 repeats the next byte 128 times, so clearing the ceiling costs
        // a couple of megabytes of input.
        let runs = (over as usize / 128) + 64;
        let mut run_length = Vec::with_capacity(runs * 2);
        for _ in 0..runs {
            run_length.extend_from_slice(&[0x81, b'A']);
        }

        // Both bombs are small on disk, which is what makes them bombs: the
        // source is a reader rather than a buffer, so only the output is ever
        // allocated.
        for (name, bomb) in [("flate", &flate), ("lzw", &lzw_bytes)] {
            assert!(
                (bomb.len() as u64) < over / 100,
                "the {name} bomb is {} bytes of input for {over} of output, which is no bomb",
                bomb.len()
            );
        }

        for damaged in [Damaged::Refuse, Damaged::Salvage] {
            for (name, raw) in [
                ("FlateDecode", &flate),
                ("LZWDecode", &lzw_bytes),
                ("RunLengthDecode", &run_length),
            ] {
                let err = decode(&filter_dict(name), raw, &identity, damaged)
                    .map(|out| out.len())
                    .expect_err(&format!(
                        "/{name} under {damaged:?} truncated instead of failing"
                    ));
                assert_eq!(err.category(), "filter-failed", "{err}");
                assert!(
                    err.to_string().contains("ceiling"),
                    "/{name} under {damaged:?} does not name the ceiling: {err}"
                );
            }
        }
    }

    #[test]
    fn unsupported_filter_fails_loud() {
        let err = decode(
            &filter_dict("JBIG2Decode"),
            b"",
            &identity,
            Damaged::Salvage,
        )
        .unwrap_err();
        assert_eq!(err.category(), "unsupported-filter");
    }

    #[test]
    fn ascii_armours_decode() {
        assert_eq!(ascii_hex_decode(b"48 65 6C 6C 6F>").unwrap(), b"Hello");
        assert_eq!(
            ascii85_decode(b"87cURD]i,\"Ebo80~>", Damaged::Refuse).unwrap(),
            b"Hello World!"
        );
    }

    #[test]
    fn run_length_expands_both_run_kinds() {
        // 0x02 -> copy 3 literals; 0xFE -> repeat the next byte 3 times.
        let encoded = [2u8, b'a', b'b', b'c', 254, b'z', 128];
        assert_eq!(
            run_length_decode(&encoded, Damaged::Salvage).unwrap(),
            b"abczzz"
        );
    }

    #[test]
    fn lzw_round_trips_through_the_filter_chain() {
        let mut compressed = Vec::new();
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .into_stream(&mut compressed)
            .encode_all(&b"BT /F1 12 Tf (lzw) Tj ET"[..])
            .status
            .expect("the fixture encodes");
        assert_eq!(
            decode(
                &filter_dict("LZWDecode"),
                &compressed,
                &identity,
                Damaged::Salvage
            )
            .unwrap(),
            b"BT /F1 12 Tf (lzw) Tj ET"
        );
    }

    /// The `Damaged` split again, on LZW: the decoder stops mid-stream, and
    /// what that means depends on who is asking.
    #[test]
    fn a_truncated_lzw_stream_is_refused_structurally_and_salvaged_for_content() {
        let source = b"BT /F1 12 Tf (the page's real text) Tj ET\n".repeat(50);
        let mut compressed = Vec::new();
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .into_stream(&mut compressed)
            .encode_all(&source[..])
            .status
            .expect("the fixture encodes");
        let cut = &compressed[..compressed.len() * 3 / 4];

        let salvaged = lzw(cut, true, Damaged::Salvage).expect("the prefix decodes");
        assert!(salvaged.starts_with(b"BT /F1 12 Tf (the page's real text)"));
        assert!(
            salvaged.len() < source.len(),
            "the whole stream cannot decode"
        );
        assert_eq!(
            lzw(cut, true, Damaged::Refuse)
                .map(|out| out.len())
                .unwrap_err()
                .category(),
            "filter-failed"
        );
    }

    /// `/Crypt` names the identity handler by default, which is a marker to
    /// step over. Any other name is a key into a `/CF` dictionary the document
    /// does not have, since a file with `/Encrypt` never opens, so the file
    /// does not say what these bytes are; guessing Identity is refused.
    #[test]
    fn an_identity_crypt_filter_passes_through_and_a_named_one_does_not() {
        let dict = filter_dict("Crypt");
        assert_eq!(
            decode(&dict, b"already plain", &identity, Damaged::Salvage).unwrap(),
            b"already plain"
        );

        let mut named = Dict::new();
        named.set("Name", Object::name("StdCF"));
        let mut dict = filter_dict("Crypt");
        dict.set("DecodeParms", Object::Dict(named));
        let err = decode(&dict, b"ciphertext", &identity, Damaged::Salvage).unwrap_err();
        assert_eq!(err.category(), "unsupported-filter");
        assert!(err.to_string().contains("StdCF"), "{err}");
    }

    #[test]
    fn file_specification_in_f_is_not_a_filter() {
        let mut dict = Dict::new();
        dict.set("F", Object::String(b"/tmp/elsewhere".to_vec()));
        let err = decode(&dict, b"", &identity, Damaged::Salvage).unwrap_err();
        assert_eq!(err.category(), "filter-failed");
    }

    /// `/Filter` is never a file specification, so a dictionary there is a
    /// malformed filter entry rather than external data.
    #[test]
    fn a_dictionary_in_filter_is_a_malformed_filter_entry() {
        let mut dict = Dict::new();
        dict.set("Filter", Object::Dict(Dict::new()));
        let err = decode(&dict, b"", &identity, Damaged::Salvage).unwrap_err();
        match err {
            Error::Filter { filter, detail } => {
                assert_eq!(filter, "Filter");
                assert!(detail.contains("is not a name"), "{detail}");
            }
            other => panic!("expected a filter error, got {other:?}"),
        }
    }
}
