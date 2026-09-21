//! Hand-built PDF fixtures shared by the page suites.
//!
//! Each suite builds the minimal document carrying exactly the shape it tests,
//! so the writer they share is the one piece worth sharing.

#![allow(dead_code)]

/// A stream object body with a correct `/Length`.
pub fn stream(data: &str) -> Vec<u8> {
    stream_with(data, "")
}

/// A stream object body with extra dictionary entries.
pub fn stream_with(data: &str, entries: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data.as_bytes());
    out.extend_from_slice(b"\nendstream");
    out
}

/// A classic-xref PDF whose object `n` is `objects[n - 1]` and whose catalog
/// is object 1.
pub fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}
