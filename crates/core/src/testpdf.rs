//! Hand-assembled PDFs for the navigation-pane reader tests.
//!
//! The seed corpus carries no outline, no embedded files, no optional content
//! and no signature fields, and `corpus/external/` is fetched rather than
//! committed, so a reader test that only ran against real files would not run
//! at all on a fresh clone. These builders make the structure under test the
//! only variable: same assembler as `corpus/make-seeds.py`, a correct classic
//! cross-reference table, and object numbers the test writes by hand so it can
//! point a destination at one.

/// Assemble numbered objects, in order, into a document whose `/Root` is
/// object 1 and whose cross-reference table is correct.
pub(crate) fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
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

/// A dictionary object, written from its source text.
pub(crate) fn dict(body: &str) -> Vec<u8> {
    body.as_bytes().to_vec()
}

/// A stream object carrying `data`, with a correct `/Length`.
pub(crate) fn stream(dict_body: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict_body} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// `count` page objects under one `/Pages` node, ready to be appended after
/// the catalog and the page-tree root.
///
/// Returned as (page tree root body, page bodies): the caller writes them at
/// the object numbers it chose, which is what lets a destination reference a
/// page by number.
pub(crate) fn pages(first_object: usize, count: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let kids: Vec<String> = (0..count)
        .map(|page| format!("{} 0 R", first_object + page))
        .collect();
    let root = format!(
        "<< /Type /Pages /Kids [{}] /Count {count} >>",
        kids.join(" ")
    )
    .into_bytes();
    let bodies = (0..count)
        .map(|_| {
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec()
        })
        .collect();
    (root, bodies)
}
