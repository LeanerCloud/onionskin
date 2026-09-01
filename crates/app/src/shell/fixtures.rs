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
fn pdf(objects: &[&[u8]]) -> Vec<u8> {
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
