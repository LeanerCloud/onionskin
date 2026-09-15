/// The three-object skeleton every fixture here needs: a catalog, a page tree
/// and one page. Further bodies become objects 4 and up.
pub(crate) fn skeleton() -> Vec<&'static [u8]> {
    vec![
        b"<</Type/Catalog/Pages 2 0 R>>",
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>",
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Resources<<>>>>",
    ]
}

pub(crate) fn with_junk_before_the_header(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::from(&b"junk\n"[..]);
    out.extend_from_slice(bytes);
    out
}

/// An xref-stream file whose objects 5 and 6 live inside object stream 4, with
/// junk before the header so the document opens repaired. Both halves matter:
/// a repaired document is the one whose section carries a table over every
/// object, and a compressed object is the one that table cannot point at, so
/// the section has to carry a copy of it.
pub(crate) fn repaired_with_compressed_objects(indirect_dependencies: bool, six: &[u8]) -> Vec<u8> {
    let six = if indirect_dependencies {
        b"<</Type/Referrer/Length 8 0 R/N 9 0 R/First 10 0 R/Filter 11 0 R/DecodeParms 12 0 R/Marker(endstream)>>"
    } else {
        six
    };
    let five: &[u8] = b"<</Type/Spare/Which 5>>";
    let header = format!("5 0 6 {} ", five.len() + 1);
    let first = header.len();
    let mut data = header.into_bytes();
    data.extend_from_slice(five);
    data.push(b' ');
    data.extend_from_slice(six);

    let mut bytes = Vec::from(&b"%PDF-1.5\n"[..]);
    let mut offsets = [0u64; 13];
    for (index, body) in skeleton().iter().enumerate() {
        offsets[index + 1] = bytes.len() as u64;
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    offsets[4] = bytes.len() as u64;
    let container_dictionary = if indirect_dependencies {
        "4 0 obj\n<</Type/ObjStm/Length 8 0 R/N 9 0 R/First 10 0 R/Filter 11 0 R/DecodeParms 12 0 R>>\nstream\n".to_string()
    } else {
        format!(
            "4 0 obj\n<</Type/ObjStm/N 2/First {first}/Length {}>>\nstream\n",
            data.len()
        )
    };
    bytes.extend_from_slice(container_dictionary.as_bytes());
    bytes.extend_from_slice(&data);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    if indirect_dependencies {
        let dependencies = [
            data.len().to_string().into_bytes(),
            b"2".to_vec(),
            first.to_string().into_bytes(),
            b"/Crypt".to_vec(),
            b"<</Name/Identity>>".to_vec(),
        ];
        for (index, body) in dependencies.iter().enumerate() {
            offsets[index + 8] = bytes.len() as u64;
            bytes.extend_from_slice(format!("{} 0 obj\n", index + 8).as_bytes());
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
    }

    offsets[7] = bytes.len() as u64;
    // /W [1 2 1]: type, a two-byte field, then one byte.
    let row = |kind: u8, field: u64, last: u8| [kind, (field >> 8) as u8, field as u8, last];
    let mut rows = Vec::new();
    rows.extend_from_slice(&row(0, 0, 255));
    for offset in &offsets[1..=4] {
        rows.extend_from_slice(&row(1, *offset, 0));
    }
    rows.extend_from_slice(&row(2, 4, 0));
    rows.extend_from_slice(&row(2, 4, 1));
    rows.extend_from_slice(&row(1, offsets[7], 0));
    if indirect_dependencies {
        for offset in &offsets[8..=12] {
            rows.extend_from_slice(&row(1, *offset, 0));
        }
    }

    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<</Type/XRef/Size {}/W[1 2 1]/Index[0 {}]/Root 1 0 R/Length {}>>\nstream\n",
            if indirect_dependencies { 13 } else { 8 },
            if indirect_dependencies { 13 } else { 8 },
            rows.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&rows);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(format!("startxref\n{}\n%%EOF\n", offsets[7]).as_bytes());
    with_junk_before_the_header(&bytes)
}
