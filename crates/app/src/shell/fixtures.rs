//! Hand-assembled documents for the shell's own tests.
//!
//! The seed corpus carries no outline and no optional content, and
//! `corpus/external/` is fetched rather than committed, so a shell test that
//! only ran against real files would not run on a fresh clone. `core` builds
//! the same shapes for its reader tests, but its builder is private to that
//! crate, which is a crate boundary rather than a duplication worth
//! removing: these exist to drive the panes, not to test the readers.

/// Assemble numbered objects, in order, into a document whose `/Root` is
/// object 1 and whose cross-reference table is correct.
pub(in crate::shell) fn pdf(objects: &[&[u8]]) -> Vec<u8> {
    pdf_with_trailer(objects, "")
}

/// [`pdf`], with `extra` added to the trailer dictionary (an `/Info`).
fn pdf_with_trailer(objects: &[&[u8]], extra: &str) -> Vec<u8> {
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
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R {extra}>>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// One page whose only mark is inside an optional content group, so hiding
/// the group is a visible change. Object 4 is the group.
pub(in crate::shell) fn optional_content_pdf() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R] /D << >> >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
           /Resources << /Properties << /MC0 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /OCG /Name (Stamp) >>",
        b"<< /Length 44 >>\nstream\n/OC /MC0 BDC\n0 0 0 rg\n20 20 100 50 re f\nEMC\nendstream",
    ])
}

/// Three pages and an outline that names the third, so a click on a bookmark
/// has somewhere to go that is not where the view already is. Objects 3, 4
/// and 5 are the pages.
///
/// US Letter rather than the tiny box the other fixtures use: three small
/// pages fit a test window at once, and a jump to the third would then move
/// nothing and prove nothing.
pub(in crate::shell) fn outline_pdf() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
        b"<< /Type /Outlines /First 7 0 R /Last 8 0 R /Count 2 >>",
        b"<< /Title (Front matter) /Parent 6 0 R /Next 8 0 R /Dest [3 0 R /Fit] >>",
        b"<< /Title (The last page) /Parent 6 0 R /Prev 7 0 R /Dest [5 0 R /Fit] >>",
    ])
}

/// One page plus one embedded text file, for the Attachments pane.
pub(in crate::shell) fn attachment_pdf() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 4 0 R >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
        b"<< /Names [(notes.txt) 5 0 R] >>",
        b"<< /Type /Filespec /F (notes.txt) /UF (notes.txt) \
          /Desc (Reviewer notes) /EF << /F 6 0 R >> >>",
        b"<< /Type /EmbeddedFile /Subtype /text#2Fplain /Params << /Size 11 >> \
          /Length 11 >>\nstream\nhello world\nendstream",
    ])
}

/// A page saying "cover", written by Ana Pop, with `annex.pdf` attached: a
/// PDF whose one page says "heron". For Advanced Search, which finds the
/// word only when it searches attachments.
pub(in crate::shell) fn attached_pdf_pdf() -> Vec<u8> {
    attached_pdf_with_word("heron")
}

/// One page saying "cover", with a PDF attachment containing `word`.
pub(in crate::shell) fn attached_pdf_with_word(word: &str) -> Vec<u8> {
    let annex = text_pdf(word);
    attached_pdf_with_payload("annex.pdf", &annex)
}

/// One page saying "cover", with an arbitrary embedded PDF payload.
pub(in crate::shell) fn attached_pdf_with_payload(name: &str, annex: &[u8]) -> Vec<u8> {
    let mut embedded = format!(
        "<< /Type /EmbeddedFile /Subtype /application#2Fpdf /Length {} >>\nstream\n",
        annex.len()
    )
    .into_bytes();
    embedded.extend_from_slice(annex);
    embedded.extend_from_slice(b"\nendstream");
    let filespec = format!("<< /Type /Filespec /F ({name}) /UF ({name}) /EF << /F 7 0 R >> >>");
    let catalog = format!(
        "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [({name}) 6 0 R] >> >> >>"
    );
    pdf_with_trailer(
        &[
            catalog.as_bytes(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] \
               /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            b"<< /Length 35 >>\nstream\nBT /F1 12 Tf 20 40 Td (cover) Tj ET\nendstream",
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            filespec.as_bytes(),
            &embedded,
            b"<< /Author (Ana Pop) /CreationDate (D:20250301) >>",
        ],
        "/Info 8 0 R ",
    )
}

/// One page saying `word`.
fn text_pdf(word: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 20 40 Td ({word}) Tj ET");
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    pdf_with_trailer(
        &[
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] \
           /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            stream.as_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            b"<< /Author (Ana Pop) >>",
        ],
        "/Info 6 0 R ",
    )
}

/// `count` empty pages, for the thumbnails pane's laziness: a document long
/// enough that asking for every row would be obvious.
pub(in crate::shell) fn many_pages_pdf(count: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..count).map(|page| format!("{} 0 R", page + 3)).collect();
    let catalog = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    let tree = format!(
        "<< /Type /Pages /Kids [{}] /Count {count} >>",
        kids.join(" ")
    )
    .into_bytes();
    let page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_vec();
    let mut objects: Vec<Vec<u8>> = vec![catalog, tree];
    objects.extend(std::iter::repeat_n(page, count));
    let borrowed: Vec<&[u8]> = objects.iter().map(Vec::as_slice).collect();
    pdf(&borrowed)
}

/// One optional content group the document locks, so the pane has a control
/// it must refuse to offer.
pub(in crate::shell) fn locked_layer_pdf() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
           << /OCGs [4 0 R] /D << /Locked [4 0 R] >> >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
        b"<< /Type /OCG /Name (Locked layer) >>",
    ])
}

/// Two pages carrying "alpha" twice and once, so a find has hits to list and
/// the second page has one to click.
pub(in crate::shell) fn text_pages_pdf() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] \
           /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length 44 >>\nstream\nBT /F1 12 Tf 20 40 Td (alpha alpha) Tj ET\nendstream",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] \
           /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>",
        b"<< /Length 38 >>\nstream\nBT /F1 12 Tf 20 40 Td (alpha) Tj ET\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ])
}

/// One page, tagged: a `Document` holding a level 1 heading "Title", a
/// paragraph "Some words", a level 2 heading "Details" and a paragraph "More
/// words", and a figure with the alternate text "A cat", with an English catalog
/// language. Elements are objects 6 to 11.
pub(in crate::shell) fn tagged_pdf() -> Vec<u8> {
    let content = "/H1 << /MCID 0 >> BDC BT /F1 18 Tf 20 170 Td (Title) Tj ET EMC \
                   /P << /MCID 1 >> BDC BT /F1 12 Tf 20 140 Td (Some words) Tj ET EMC \
                   /H2 << /MCID 2 >> BDC BT /F1 14 Tf 20 100 Td (Details) Tj ET EMC \
                   /P << /MCID 3 >> BDC BT /F1 12 Tf 20 70 Td (More words) Tj ET EMC \
                   /Figure << /MCID 4 >> BDC 20 20 30 30 re f EMC";
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> /Lang (en) >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>",
        stream.as_bytes(),
        b"<< /Type /StructTreeRoot /K [6 0 R] >>",
        b"<< /S /Document /K [7 0 R 8 0 R 9 0 R 10 0 R 11 0 R] >>",
        b"<< /S /H1 /Pg 3 0 R /K 0 >>",
        b"<< /S /P /Pg 3 0 R /K 1 >>",
        b"<< /S /H2 /Pg 3 0 R /K 2 >>",
        b"<< /S /P /Pg 3 0 R /K 3 >>",
        b"<< /S /Figure /Pg 3 0 R /Alt (A cat) /K 4 >>",
    ])
}

/// `count` tagged pages, each holding one level 1 heading "Page n" under a
/// `Document`, so a page deletion changes what the structure says on every
/// page after it.
pub(in crate::shell) fn tagged_pages_pdf(count: usize) -> Vec<u8> {
    let kids: String = (0..count).map(|n| format!("{} 0 R ", 3 + n)).collect();
    let root = 3 + 2 * count;
    let document = root + 1;
    let first_heading = document + 1;
    let headings: String = (0..count)
        .map(|n| format!("{} 0 R ", first_heading + n))
        .collect();
    let mut objects: Vec<Vec<u8>> = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot {root} 0 R /MarkInfo << /Marked true >> /Lang (en) >>"
        )
        .into_bytes(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>").into_bytes(),
    ];
    for n in 0..count {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents {} 0 R /Resources \
                 << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>",
                3 + count + n
            )
            .into_bytes(),
        );
    }
    for n in 0..count {
        let content = format!(
            "/H1 << /MCID 0 >> BDC BT /F1 18 Tf 20 170 Td (Page {}) Tj ET EMC",
            n + 1
        );
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
            .into_bytes(),
        );
    }
    objects.push(format!("<< /Type /StructTreeRoot /K [{document} 0 R] >>").into_bytes());
    objects.push(format!("<< /S /Document /K [{headings}] >>").into_bytes());
    for n in 0..count {
        objects.push(format!("<< /S /H1 /Pg {} 0 R /K 0 >>", 3 + n).into_bytes());
    }
    let slices: Vec<&[u8]> = objects.iter().map(Vec::as_slice).collect();
    pdf(&slices)
}

/// [`tagged_pdf`] with a `/StructTreeRoot` the structure reader refuses (a
/// direct dictionary), so the page has text and no readable structure.
pub(in crate::shell) fn unreadable_structure_pdf() -> Vec<u8> {
    let mut bytes = tagged_pdf();
    let root = b"/StructTreeRoot 5 0 R";
    let at = bytes
        .windows(root.len())
        .position(|window| window == root)
        .expect("the catalog names a root");
    // Same length, so the cross-reference offsets stay right.
    bytes[at..at + root.len()].copy_from_slice(b"/StructTreeRoot <<>> ");
    bytes
}

/// A tagged document whose one page has text and whose structure marks none of
/// it, so the structure has no nodes for the page.
pub(in crate::shell) fn tagged_without_marked_content_pdf() -> Vec<u8> {
    let content = "BT /F1 12 Tf 20 100 Td (Unmarked words) Tj ET";
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources \
          << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>",
        stream.as_bytes(),
        b"<< /Type /StructTreeRoot /K [6 0 R] >>",
        b"<< /S /Document /Pg 3 0 R >>",
    ])
}
