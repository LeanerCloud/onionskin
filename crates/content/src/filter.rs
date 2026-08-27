//! Stream filters for content streams.
//!
//! `cos` decodes the filters the structural layer needs, but keeps that
//! decoder crate-private, so this is content's own. The filter set differs
//! anyway: content streams and embedded CMaps add LZW (older producers) and
//! RunLength, and never carry the image codecs.
//!
//! Anything else fails loud as [`Error::Filter`]. A content stream that
//! silently decoded to nothing would look exactly like a page with no text.

use std::io::Read;

use onionskin_cos::{Dict, Object};

use crate::error::{Error, Result};

/// Ceiling on one decoded stream. Larger than any real page description and
/// small enough that a decompression bomb is an error rather than the machine.
const MAX_DECODED: u64 = 128 * 1024 * 1024;

/// Decodes a stream's raw bytes through its `/Filter` chain.
pub fn decode(
    dict: &Dict,
    raw: &[u8],
    resolve: &dyn Fn(&Object) -> onionskin_cos::Result<Object>,
) -> Result<Vec<u8>> {
    let filters = names(dict, resolve)?;
    if filters.is_empty() {
        return Ok(raw.to_vec());
    }
    let parms = decode_parms(dict, resolve, filters.len())?;

    let mut data = raw.to_vec();
    for (i, name) in filters.iter().enumerate() {
        let parm = parms.get(i).cloned().flatten();
        data = match name.as_str() {
            "FlateDecode" | "Fl" => {
                let flat = inflate(&data)?;
                predict(flat, parm.as_ref(), resolve)?
            }
            "LZWDecode" | "LZW" => {
                let early = parm
                    .as_ref()
                    .and_then(|p| p.get(b"EarlyChange"))
                    .and_then(Object::as_integer)
                    .unwrap_or(1);
                let flat = lzw(&data, early != 0)?;
                predict(flat, parm.as_ref(), resolve)?
            }
            "ASCIIHexDecode" | "AHx" => ascii_hex(&data)?,
            "ASCII85Decode" | "A85" => ascii85(&data)?,
            "RunLengthDecode" | "RL" => run_length(&data)?,
            // Identity /Crypt is a no-op marker; a named crypt filter needs the
            // encryption handler this build refuses documents for anyway.
            "Crypt" => data,
            other => {
                return Err(Error::Filter {
                    filter: other.to_string(),
                    detail: "not a content stream filter this build decodes".into(),
                })
            }
        };
    }
    Ok(data)
}

fn names(
    dict: &Dict,
    resolve: &dyn Fn(&Object) -> onionskin_cos::Result<Object>,
) -> Result<Vec<String>> {
    let Some(entry) = dict.get(b"Filter").or_else(|| dict.get(b"F")) else {
        return Ok(Vec::new());
    };
    let entry = resolve(entry)?;
    // `/F` spells both the `/Filter` abbreviation and a file specification. A
    // string there means the data is in another file, which is not something to
    // paper over with an empty page.
    if matches!(entry, Object::String(_) | Object::Dict(_)) {
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
    resolve: &dyn Fn(&Object) -> onionskin_cos::Result<Object>,
    count: usize,
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
        // A malformed /DecodeParms costs the predictor, not the stream.
        _ => {}
    }
    Ok(out)
}

/// Zlib first, then raw deflate: producers ship both, and the wrapper is two
/// header bytes rather than a semantic difference.
///
/// A truncated stream keeps whatever inflated before the error. That is the
/// one place this module accepts a partial result, because a content stream
/// cut short by a broken `/Length` is common and its first operators are still
/// the page's real text.
fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let start = data
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(data.len());
    let trimmed = &data[start..];
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut zlib = Vec::new();
    let zlib_err = flate2::read::ZlibDecoder::new(trimmed)
        .take(MAX_DECODED)
        .read_to_end(&mut zlib)
        .err();
    if zlib_err.is_none() && !zlib.is_empty() {
        return Ok(zlib);
    }

    let mut raw = Vec::new();
    let raw_err = flate2::read::DeflateDecoder::new(trimmed)
        .take(MAX_DECODED)
        .read_to_end(&mut raw)
        .err();
    if raw_err.is_none() && !raw.is_empty() {
        return Ok(raw);
    }

    // Both paths errored. Keep the longer partial inflation if either produced
    // anything at all; report the failure only when neither did.
    let best = if zlib.len() >= raw.len() { zlib } else { raw };
    if best.is_empty() {
        return Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: match zlib_err {
                Some(e) => e.to_string(),
                None => "inflated to zero bytes".into(),
            },
        });
    }
    Ok(best)
}

fn lzw(data: &[u8], early_change: bool) -> Result<Vec<u8>> {
    // PDF's LZW is the TIFF variant: MSB-first, 8-bit symbols, and by default
    // it grows the code width one code early.
    let mut decoder = if early_change {
        weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
    } else {
        weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
    };
    let mut out = Vec::new();
    let result = decoder.into_stream(&mut out).decode_all(data);
    // Same rule as inflate: a truncated stream keeps what decoded.
    if let Err(e) = result.status {
        if out.is_empty() {
            return Err(Error::Filter {
                filter: "LZWDecode".into(),
                detail: e.to_string(),
            });
        }
    }
    Ok(out)
}

fn predict(
    data: Vec<u8>,
    parms: Option<&Dict>,
    resolve: &dyn Fn(&Object) -> onionskin_cos::Result<Object>,
) -> Result<Vec<u8>> {
    let Some(parms) = parms else {
        return Ok(data);
    };
    let int = |key: &[u8], default: i64| -> i64 {
        match parms.get(key) {
            Some(o) => resolve(o)
                .ok()
                .and_then(|o| o.as_integer())
                .unwrap_or(default),
            None => default,
        }
    };
    let predictor = int(b"Predictor", 1);
    if predictor <= 1 {
        return Ok(data);
    }
    // Every operand comes out of the file, so the row length is clamped before
    // it becomes an allocation.
    let colors = int(b"Colors", 1).clamp(1, 32) as usize;
    let bpc = int(b"BitsPerComponent", 8).clamp(1, 32) as usize;
    let columns = int(b"Columns", 1).max(1) as usize;
    let row_len = (colors * bpc * columns).div_ceil(8);
    if row_len == 0 || row_len > data.len() {
        return Err(Error::Filter {
            filter: "Predictor".into(),
            detail: format!(
                "row of {colors}x{bpc}x{columns} does not fit {} bytes",
                data.len()
            ),
        });
    }

    if predictor == 2 {
        return Ok(tiff(data, colors, bpc, columns));
    }

    let bpp = (colors * bpc).div_ceil(8).max(1);
    let stride = row_len + 1;
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0u8; row_len];
    for chunk in data.chunks(stride) {
        // A short final row is the tail of a truncated stream, not a row.
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
                        filter: "Predictor".into(),
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

fn tiff(mut data: Vec<u8>, colors: usize, bpc: usize, columns: usize) -> Vec<u8> {
    // Sub-byte components are only reachable through image data, which never
    // reaches this crate; leaving them untouched beats guessing at the packing.
    if bpc != 8 {
        return data;
    }
    let row_len = colors * columns;
    if row_len == 0 {
        return data;
    }
    for row in data.chunks_mut(row_len) {
        for i in colors..row.len() {
            row[i] = row[i].wrapping_add(row[i - colors]);
        }
    }
    data
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

fn ascii_hex(data: &[u8]) -> Result<Vec<u8>> {
    let mut nibbles = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        if b.is_ascii_whitespace() || b == 0 {
            continue;
        }
        let value = (b as char).to_digit(16).ok_or_else(|| Error::Filter {
            filter: "ASCIIHexDecode".into(),
            detail: format!("non-hex byte 0x{b:02x}"),
        })? as u8;
        nibbles.push(value);
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    Ok(nibbles.chunks(2).map(|p| p[0] * 16 + p[1]).collect())
}

fn ascii85(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut count = 0usize;
    let mut i = if data.starts_with(b"<~") { 2 } else { 0 };
    while i < data.len() {
        let b = data[i];
        i += 1;
        if b.is_ascii_whitespace() || b == 0 {
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
            push85(&mut out, &group, 5);
            count = 0;
        }
    }
    match count {
        // ISO 32000-1 7.4.3: a final group of one character encodes nothing.
        0 | 1 => {}
        _ => {
            for slot in group.iter_mut().skip(count) {
                *slot = 84;
            }
            push85(&mut out, &group, count);
        }
    }
    Ok(out)
}

/// A group encoding more than 32 bits is malformed. It wraps rather than
/// erroring, because losing four bytes of a page description costs one
/// operator and erroring costs the whole page's text.
fn push85(out: &mut Vec<u8>, group: &[u8; 5], count: usize) {
    let mut value: u32 = 0;
    for &digit in group.iter() {
        value = value.wrapping_mul(85).wrapping_add(u32::from(digit));
    }
    out.extend_from_slice(&value.to_be_bytes()[..count - 1]);
}

fn run_length(data: &[u8]) -> Result<Vec<u8>> {
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
        if out.len() as u64 > MAX_DECODED {
            return Err(Error::Filter {
                filter: "RunLengthDecode".into(),
                detail: format!("expanded past the {MAX_DECODED} byte ceiling"),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(o: &Object) -> onionskin_cos::Result<Object> {
        Ok(o.clone())
    }

    #[test]
    fn flate_round_trips() {
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"BT /F1 12 Tf ET").unwrap();
        let raw = encoder.finish().unwrap();

        let mut dict = Dict::new();
        dict.set("Filter", Object::name("FlateDecode"));
        assert_eq!(decode(&dict, &raw, &identity).unwrap(), b"BT /F1 12 Tf ET");
    }

    #[test]
    fn run_length_expands_both_run_kinds() {
        // 0x02 -> copy 3 literals; 0xFE -> repeat the next byte 3 times.
        let encoded = [2u8, b'a', b'b', b'c', 254, b'z', 128];
        assert_eq!(run_length(&encoded).unwrap(), b"abczzz");
    }

    #[test]
    fn ascii_armours_decode() {
        assert_eq!(ascii_hex(b"48 65 6C 6C 6F>").unwrap(), b"Hello");
        assert_eq!(ascii85(b"87cURD]i,\"Ebo80~>").unwrap(), b"Hello World!");
    }

    #[test]
    fn unknown_filter_fails_loud() {
        let mut dict = Dict::new();
        dict.set("Filter", Object::name("DCTDecode"));
        let err = decode(&dict, b"", &identity).unwrap_err();
        assert_eq!(err.category(), "filter");
    }

    #[test]
    fn file_specification_in_f_is_not_a_filter() {
        let mut dict = Dict::new();
        dict.set("F", Object::String(b"/tmp/elsewhere".to_vec()));
        let err = decode(&dict, b"", &identity).unwrap_err();
        assert_eq!(err.category(), "filter");
    }
}
