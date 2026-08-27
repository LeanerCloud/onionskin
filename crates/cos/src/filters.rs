//! Stream filters. The spike decodes only what the structural layer needs:
//! xref streams and object streams, which in practice means FlateDecode with
//! the PNG/TIFF predictors, plus the two ASCII armours that sometimes wrap it.
//! Anything else fails loud as `Error::UnsupportedFilter`.

use std::io::Read;

use crate::error::{Error, Result};
use crate::object::{Dict, Object};

/// Decodes a stream's raw bytes through its `/Filter` chain.
///
/// `resolve` follows indirect references found in `/Filter` or `/DecodeParms`.
pub(crate) fn decode(
    dict: &Dict,
    raw: &[u8],
    resolve: &dyn Fn(&Object) -> Result<Object>,
) -> Result<Vec<u8>> {
    let filters = filter_names(dict, resolve)?;
    if filters.is_empty() {
        return Ok(raw.to_vec());
    }
    let parms = decode_parms(dict, resolve, filters.len())?;

    let mut data = raw.to_vec();
    for (i, name) in filters.iter().enumerate() {
        let parm = parms.get(i).cloned().flatten();
        data = match name.as_str() {
            "FlateDecode" | "Fl" => {
                let inflated = inflate(&data)?;
                apply_predictor(inflated, parm.as_ref(), resolve)?
            }
            "ASCIIHexDecode" | "AHx" => ascii_hex_decode(&data)?,
            "ASCII85Decode" | "A85" => ascii85_decode(&data)?,
            other => return Err(Error::UnsupportedFilter(other.to_string())),
        };
    }
    Ok(data)
}

fn filter_names(dict: &Dict, resolve: &dyn Fn(&Object) -> Result<Object>) -> Result<Vec<String>> {
    let Some(entry) = dict.get(b"Filter") else {
        return Ok(Vec::new());
    };
    let entry = resolve(entry)?;
    match entry {
        Object::Null => Ok(Vec::new()),
        Object::Name(n) => Ok(vec![String::from_utf8_lossy(n.as_bytes()).into_owned()]),
        Object::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in &items {
                let item = resolve(item)?;
                match item {
                    Object::Name(n) => out.push(String::from_utf8_lossy(n.as_bytes()).into_owned()),
                    Object::Null => {}
                    _ => {
                        return Err(Error::Filter {
                            filter: "Filter".into(),
                            detail: "filter array holds a non-name".into(),
                        })
                    }
                }
            }
            Ok(out)
        }
        _ => Err(Error::Filter {
            filter: "Filter".into(),
            detail: "/Filter is neither a name nor an array".into(),
        }),
    }
}

fn decode_parms(
    dict: &Dict,
    resolve: &dyn Fn(&Object) -> Result<Object>,
    count: usize,
) -> Result<Vec<Option<Dict>>> {
    let entry = dict.get(b"DecodeParms").or_else(|| dict.get(b"DP"));
    let Some(entry) = entry else {
        return Ok(vec![None; count]);
    };
    let entry = resolve(entry)?;
    let mut out = vec![None; count];
    match entry {
        Object::Dict(d) => {
            if let Some(slot) = out.first_mut() {
                *slot = Some(d);
            }
        }
        Object::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                if i >= count {
                    break;
                }
                if let Object::Dict(d) = resolve(item)? {
                    out[i] = Some(d);
                }
            }
        }
        Object::Null => {}
        _ => {
            return Err(Error::Filter {
                filter: "DecodeParms".into(),
                detail: "/DecodeParms is neither a dictionary nor an array".into(),
            })
        }
    }
    Ok(out)
}

/// Ceiling on one stream's inflated size. `cos` only inflates structural
/// streams (xref and object streams), so this is generous for anything
/// legitimate and closes the decompression-bomb hole.
const MAX_INFLATED: u64 = 256 * 1024 * 1024;

/// Zlib first, then raw deflate: producers ship both, and the wrapper is a
/// two-byte header rather than a semantic difference. A payload that decodes
/// as neither is an error, never a partial result passed off as the whole.
fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let trimmed = {
        let start = data
            .iter()
            .position(|b| !crate::parse::is_whitespace(*b))
            .unwrap_or(data.len());
        &data[start..]
    };
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let zlib = flate2::read::ZlibDecoder::new(trimmed)
        .take(MAX_INFLATED)
        .read_to_end(&mut out);
    if zlib.is_ok() && !out.is_empty() {
        return bounded(out);
    }

    let mut raw = Vec::new();
    match flate2::read::DeflateDecoder::new(trimmed)
        .take(MAX_INFLATED)
        .read_to_end(&mut raw)
    {
        Ok(_) if !raw.is_empty() => bounded(raw),
        _ => Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: match zlib {
                Err(e) => e.to_string(),
                Ok(_) => "inflated to zero bytes".to_string(),
            },
        }),
    }
}

fn bounded(data: Vec<u8>) -> Result<Vec<u8>> {
    if data.len() as u64 >= MAX_INFLATED {
        return Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: format!("inflated past the {MAX_INFLATED} byte ceiling"),
        });
    }
    Ok(data)
}

fn apply_predictor(
    data: Vec<u8>,
    parms: Option<&Dict>,
    resolve: &dyn Fn(&Object) -> Result<Object>,
) -> Result<Vec<u8>> {
    let Some(parms) = parms else {
        return Ok(data);
    };
    let int = |key: &[u8], default: i64| -> Result<i64> {
        match parms.get(key) {
            Some(o) => Ok(resolve(o)?.as_integer().unwrap_or(default)),
            None => Ok(default),
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
            filter: "FlateDecode".into(),
            detail: format!(
                "predictor row of {colors}x{bpc}x{columns} does not fit {} bytes of data",
                data.len()
            ),
        })?;

    if predictor == 2 {
        return tiff_predictor(data, colors, bpc, columns);
    }

    // PNG predictors: each row is prefixed with its filter type byte.
    let stride = row_len + 1;
    if stride == 0 || data.len() < stride {
        return Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: format!(
                "predictor {predictor} needs at least {stride} bytes, got {}",
                data.len()
            ),
        });
    }
    // A partial final row would be zero-padded into a plausible-looking row,
    // which for an xref stream means a fabricated cross-reference entry.
    if !data.len().is_multiple_of(stride) {
        return Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: format!(
                "{} bytes is not a whole number of {stride} byte predictor rows",
                data.len()
            ),
        });
    }
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0u8; row_len];
    for chunk in data.chunks(stride) {
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
                        filter: "FlateDecode".into(),
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

fn tiff_predictor(mut data: Vec<u8>, colors: usize, bpc: usize, columns: usize) -> Result<Vec<u8>> {
    if bpc != 8 {
        return Err(Error::Filter {
            filter: "FlateDecode".into(),
            detail: format!("TIFF predictor with {bpc} bits per component is not implemented"),
        });
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

fn ascii85_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut count = 0usize;
    let mut i = 0usize;
    if data.starts_with(b"<~") {
        i = 2;
    }
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
            push_base85(&mut out, &group, 5)?;
            count = 0;
        }
    }
    match count {
        0 => {}
        // A final group of one digit encodes nothing; ISO 32000-1 7.4.3 calls
        // it an error rather than something to round down.
        1 => {
            return Err(Error::Filter {
                filter: "ASCII85Decode".into(),
                detail: "final group holds a single character".into(),
            })
        }
        _ => {
            for slot in group.iter_mut().skip(count) {
                *slot = 84;
            }
            push_base85(&mut out, &group, count)?;
        }
    }
    Ok(out)
}

fn push_base85(out: &mut Vec<u8>, group: &[u8; 5], count: usize) -> Result<()> {
    let mut value: u32 = 0;
    for &digit in group.iter() {
        value = value
            .checked_mul(85)
            .and_then(|v| v.checked_add(u32::from(digit)))
            .ok_or_else(|| Error::Filter {
                filter: "ASCII85Decode".into(),
                detail: "group encodes a value larger than 32 bits".into(),
            })?;
    }
    out.extend_from_slice(&value.to_be_bytes()[..count - 1]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(o: &Object) -> Result<Object> {
        Ok(o.clone())
    }

    #[test]
    fn flate_round_trips() {
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"the quick brown fox").unwrap();
        let raw = encoder.finish().unwrap();

        let mut dict = Dict::new();
        dict.set("Filter", Object::name("FlateDecode"));
        assert_eq!(
            decode(&dict, &raw, &identity).unwrap(),
            b"the quick brown fox"
        );
    }

    #[test]
    fn unsupported_filter_fails_loud() {
        let mut dict = Dict::new();
        dict.set("Filter", Object::name("JBIG2Decode"));
        let err = decode(&dict, b"", &identity).unwrap_err();
        assert_eq!(err.category(), "unsupported-filter");
    }

    #[test]
    fn ascii_armours_decode() {
        assert_eq!(ascii_hex_decode(b"48 65 6C 6C 6F>").unwrap(), b"Hello");
        assert_eq!(
            ascii85_decode(b"87cURD]i,\"Ebo80~>").unwrap(),
            b"Hello World!"
        );
    }
}
