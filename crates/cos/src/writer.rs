//! Object serialization and incremental-section assembly.
//!
//! Only objects the caller changed, or structures repair recovered, are ever
//! serialized. Untouched objects keep their original bytes because the section
//! is appended after them and its xref points back at them.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::object::{Dict, Name, ObjRef, Object};
use crate::parse;

pub(crate) fn write_object(out: &mut Vec<u8>, object: &Object) -> Result<()> {
    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Bool(true) => out.extend_from_slice(b"true"),
        Object::Bool(false) => out.extend_from_slice(b"false"),
        Object::Integer(v) => out.extend_from_slice(v.to_string().as_bytes()),
        Object::Real(v) => out.extend_from_slice(format_real(*v)?.as_bytes()),
        Object::String(bytes) => write_literal_string(out, bytes),
        Object::Name(name) => write_name(out, name),
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object(out, item)?;
            }
            out.push(b']');
        }
        Object::Dict(dict) => write_dict(out, dict)?,
        Object::Stream(stream) => {
            let mut dict = stream.dict.clone();
            dict.set("Length", Object::Integer(stream.raw.len() as i64));
            write_dict(out, &dict)?;
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&stream.raw);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Ref(r) => {
            out.extend_from_slice(format!("{} {} R", r.number, r.generation).as_bytes())
        }
    }
    Ok(())
}

pub(crate) fn write_dict(out: &mut Vec<u8>, dict: &Dict) -> Result<()> {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        write_name(out, key);
        out.push(b' ');
        write_object(out, value)?;
    }
    out.extend_from_slice(b">>");
    Ok(())
}

fn write_name(out: &mut Vec<u8>, name: &Name) {
    out.push(b'/');
    for &b in name.as_bytes() {
        if b == b'#' || b <= b' ' || b > b'~' || parse::is_delimiter(b) {
            out.extend_from_slice(format!("#{b:02X}").as_bytes());
        } else {
            out.push(b);
        }
    }
}

fn write_literal_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'(');
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7e => out.push(b),
            other => out.extend_from_slice(format!("\\{other:03o}").as_bytes()),
        }
    }
    out.push(b')');
}

/// PDF has no exponent notation, so `{}` is not safe for very small or very
/// large reals. It has no notation for infinity or NaN either, so those are an
/// error instead of a fabricated zero.
fn format_real(value: f64) -> Result<String> {
    if !value.is_finite() {
        return Err(Error::Unrecoverable {
            detail: format!("{value} cannot be written as a PDF real"),
        });
    }
    if value == value.trunc() && value.abs() < 1e15 {
        return Ok(format!("{}", value as i64));
    }
    let mut text = format!("{value:.6}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    // A value smaller than the six decimals above rounds to nothing; zero is
    // the correct rendering of it, not a stand-in for a lost value.
    Ok(match text.as_str() {
        "" | "-" | "-0" => "0".to_string(),
        _ => text,
    })
}

/// One entry of the cross-reference table an incremental section writes.
pub(crate) struct XrefRow {
    pub number: u32,
    pub generation: u16,
    pub offset: u64,
}

/// A byte offset an `xref` table's fixed ten-digit field cannot hold.
const MAX_TABLE_OFFSET: u64 = 9_999_999_999;

/// Keys that belong to a cross-reference stream's dictionary and are
/// meaningless, or actively wrong, in the classic `trailer` dictionary a new
/// section writes. `/Length`, `/W` and `/Index` in particular describe the old
/// stream's bytes, which the new section does not have.
const XREF_STREAM_ONLY_KEYS: [&[u8]; 8] = [
    b"Type",
    b"W",
    b"Index",
    b"Filter",
    b"DecodeParms",
    b"Length",
    b"Prev",
    b"XRefStm",
];

/// Strips a source trailer down to what a freshly written section may repeat.
///
/// The source of a trailer is either a `trailer` dictionary or, in a PDF 1.5+
/// file, the cross-reference stream's own dictionary. In the second case it
/// arrives carrying the stream's plumbing, and copying that into a classic
/// trailer produces a dictionary no conforming reader should accept.
pub(crate) fn trailer_for_new_section(source: &Dict) -> Dict {
    let mut trailer = source.clone();
    for key in XREF_STREAM_ONLY_KEYS {
        trailer.remove(key);
    }
    trailer
}

/// Serializes `objects` at `section_start`, then a cross-reference table and
/// trailer covering them plus any `extra_rows` that point back into bytes that
/// are already in the file (the repair case).
pub(crate) fn incremental_section(
    section_start: u64,
    objects: &[(ObjRef, Object)],
    extra_rows: &[XrefRow],
    trailer: Dict,
) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    // Keyed by object number, so a rewritten object supersedes the row that
    // pointed at its old bytes instead of both landing in the table.
    let mut rows: BTreeMap<u32, XrefRow> = extra_rows
        .iter()
        .map(|r| {
            (
                r.number,
                XrefRow {
                    number: r.number,
                    generation: r.generation,
                    offset: r.offset,
                },
            )
        })
        .collect();

    for (objref, object) in objects {
        let offset = section_start + body.len() as u64;
        body.extend_from_slice(format!("{} {} obj\n", objref.number, objref.generation).as_bytes());
        write_object(&mut body, object)?;
        body.extend_from_slice(b"\nendobj\n");
        rows.insert(
            objref.number,
            XrefRow {
                number: objref.number,
                generation: objref.generation,
                offset,
            },
        );
    }

    let rows: Vec<XrefRow> = rows.into_values().collect();
    if let Some(row) = rows.iter().find(|r| r.offset > MAX_TABLE_OFFSET) {
        return Err(Error::Unrecoverable {
            detail: format!(
                "object {} lives at byte {}, past what an xref table can address",
                row.number, row.offset
            ),
        });
    }

    let xref_offset = section_start + body.len() as u64;
    if xref_offset > MAX_TABLE_OFFSET {
        return Err(Error::Unrecoverable {
            detail: format!("cross-reference table would start at byte {xref_offset}"),
        });
    }
    body.extend_from_slice(b"xref\n");
    for group in contiguous_groups(&rows) {
        body.extend_from_slice(format!("{} {}\n", group[0].number, group.len()).as_bytes());
        for row in group {
            if row.number == 0 {
                body.extend_from_slice(b"0000000000 65535 f \n");
            } else {
                body.extend_from_slice(
                    format!("{:010} {:05} n \n", row.offset, row.generation).as_bytes(),
                );
            }
        }
    }

    body.extend_from_slice(b"trailer\n");
    write_dict(&mut body, &trailer)?;
    body.extend_from_slice(format!("\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
    Ok(body)
}

fn contiguous_groups(rows: &[XrefRow]) -> Vec<&[XrefRow]> {
    let mut groups = Vec::new();
    let mut start = 0usize;
    for i in 1..rows.len() {
        if Some(rows[i].number) != rows[i - 1].number.checked_add(1) {
            groups.push(&rows[start..i]);
            start = i;
        }
    }
    if start < rows.len() {
        groups.push(&rows[start..]);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Stream;

    fn text(object: &Object) -> String {
        let mut out = Vec::new();
        write_object(&mut out, object).expect("writable");
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn serializes_scalars_without_exponents() {
        assert_eq!(text(&Object::Real(0.000001)), "0.000001");
        assert_eq!(text(&Object::Real(3.0)), "3");
        assert_eq!(text(&Object::Integer(-7)), "-7");
        assert_eq!(text(&Object::name("A B")), "/A#20B");
        assert_eq!(text(&Object::String(b"a(b".to_vec())), "(a\\(b)");
    }

    #[test]
    fn stream_length_is_rewritten_to_match_the_bytes() {
        let mut dict = Dict::new();
        dict.set("Length", Object::Integer(999));
        let object = Object::Stream(Stream {
            dict,
            raw: b"abc".to_vec(),
        });
        assert!(text(&object).starts_with("<</Length 3>>\nstream\nabc\nendstream"));
    }
}
