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

/// A page's content: "Page n" in Helvetica, which text extraction reads back.
pub fn page_text(index: usize) -> Vec<u8> {
    stream(&format!("BT /F1 24 Tf 72 700 Td (Page {index}) Tj ET"))
}

pub const FONT: &[u8] =
    b"<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>";

/// `count` pages under one `/Pages` node, sharing one resource dictionary, each
/// saying "Page n".
pub fn flat(count: usize) -> Vec<u8> {
    let first_page = 3;
    let resources = first_page + count;
    let first_content = resources + 1;
    let kids: Vec<String> = (0..count)
        .map(|index| format!("{} 0 R", first_page + index))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} /MediaBox [0 0 612 792] /Resources {resources} 0 R >>",
            kids.join(" ")
        )
        .into_bytes(),
    ];
    for index in 0..count {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>",
                first_content + index
            )
            .into_bytes(),
        );
    }
    objects.push(FONT.to_vec());
    for index in 0..count {
        objects.push(page_text(index + 1));
    }
    pdf(&objects)
}

/// Four pages two levels down: the first two inherit `/Rotate 270` from their
/// node, the last two a different `/MediaBox` from theirs, and all four their
/// resources from the root.
pub fn deep() -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 4 /MediaBox [0 0 612 792] /Resources 9 0 R >>"
            .to_vec(),
        b"<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 6 0 R] /Count 2 /Rotate 270 >>".to_vec(),
        b"<< /Type /Pages /Parent 2 0 R /Kids [7 0 R 8 0 R] /Count 2 /MediaBox [0 0 400 600] >>"
            .to_vec(),
        b"<< /Type /Page /Parent 3 0 R /Contents 10 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 3 0 R /Contents 11 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 4 0 R /Contents 12 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 4 0 R /Contents 13 0 R >>".to_vec(),
        FONT.to_vec(),
    ];
    objects.extend((1..=4).map(page_text));
    pdf(&objects)
}

/// Three tagged pages, each one paragraph with its text marked as MCID 0.
pub fn tagged() -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 10 0 R /MarkInfo << /Marked true >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 612 792] /Resources 6 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /StructParents 0 /Contents 7 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /StructParents 1 /Contents 8 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /StructParents 2 /Contents 9 0 R >>".to_vec(),
        FONT.to_vec(),
    ];
    objects.extend((1..=3).map(|index| {
        stream(&format!(
            "/P << /MCID 0 >> BDC BT /F1 24 Tf 72 700 Td (Page {index}) Tj ET EMC"
        ))
    }));
    objects.push(
        b"<< /Type /StructTreeRoot /K [11 0 R 12 0 R 13 0 R] /ParentTree << /Nums [0 [11 0 R] 1 [12 0 R] 2 [13 0 R]] >> /ParentTreeNextKey 3 >>".to_vec(),
    );
    for page in 3..=5 {
        objects.push(
            format!("<< /Type /StructElem /S /P /P 10 0 R /Pg {page} 0 R /K 0 >>").into_bytes(),
        );
    }
    pdf(&objects)
}

/// Parse `bytes` as they are, refusing anything that needs repair.
pub fn open(bytes: &[u8]) -> onionskin_cos::Document {
    onionskin_cos::Document::open(Box::new(onionskin_cos::BytesSource::new(bytes.to_vec())))
        .expect("the document opens")
}

/// `original` with the edit `body` makes appended as an incremental
/// section, and what `body` returned.
pub fn try_apply<T>(
    original: &[u8],
    body: impl FnOnce(
        &mut onionskin_core::Transaction<'_>,
        &onionskin_core::Structure,
    ) -> onionskin_core::Result<T>,
) -> onionskin_core::Result<(Vec<u8>, T)> {
    let base = open(original);
    let structure = onionskin_core::read_structure(&base).expect("the structure reads");
    let mut edit = onionskin_core::EditSession::for_base(&base);
    let value = edit.transact(&base, "Edit", |tx| body(tx, &structure))?;
    let mut bytes = original.to_vec();
    if let Some(section) = base
        .section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
    {
        bytes.extend_from_slice(&section);
    }
    Ok((bytes, value))
}

/// [`try_apply`] for an edit that must run.
pub fn apply<T>(
    original: &[u8],
    body: impl FnOnce(
        &mut onionskin_core::Transaction<'_>,
        &onionskin_core::Structure,
    ) -> onionskin_core::Result<T>,
) -> Vec<u8> {
    try_apply(original, body).expect("the edit runs").0
}
