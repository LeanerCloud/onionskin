use std::path::{Path, PathBuf};

use onionskin_core::{Document, Error, PageRect, Provenance, SearchOptions};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("core crate lives under workspace/crates")
        .join("corpus")
}

fn seed(name: &str) -> PathBuf {
    corpus_root().join("seeds").join(name)
}

#[test]
fn clean_seeds_open_with_expected_page_counts() {
    for (name, pages) in [("minimal.pdf", 1), ("hello.pdf", 1), ("two-page.pdf", 2)] {
        let mut doc = Document::open_path(&seed(name)).expect("seed opens");
        assert_eq!(doc.provenance(), &Provenance::Clean);
        assert_eq!(doc.page_count(), pages, "{name}");

        let geometry = doc.page_geometry(0).expect("page geometry loads");
        assert_eq!(geometry.index, 0);
        assert!(geometry.media_box[2] > geometry.media_box[0]);
        assert!(geometry.media_box[3] > geometry.media_box[1]);
    }
}

#[test]
fn repaired_input_keeps_provenance_and_serves_geometry() {
    let mut bytes = b"junk before the header\n".to_vec();
    bytes.extend_from_slice(&std::fs::read(seed("hello.pdf")).expect("seed is readable"));

    let mut doc = Document::open_bytes(bytes).expect("repairing open succeeds");
    assert!(matches!(doc.provenance(), Provenance::Repaired(_)));
    assert_eq!(doc.page_count(), 1);

    let geometry = doc.page_geometry(0).expect("repaired page geometry loads");
    assert_eq!(geometry.media_box, [0.0, 0.0, 200.0, 100.0]);
}

#[test]
fn every_generated_malformed_pdf_opens_as_repaired() {
    let malformed = corpus_root().join("malformed");
    if !malformed.is_dir() {
        return;
    }

    let mut paths = std::fs::read_dir(&malformed)
        .expect("malformed corpus is readable")
        .map(|entry| entry.expect("malformed corpus entry is readable").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "malformed corpus contains PDF fixtures");

    for path in paths {
        let mut doc = Document::open_path(&path)
            .unwrap_or_else(|error| panic!("{} did not repair: {error}", path.display()));
        assert!(
            matches!(doc.provenance(), Provenance::Repaired(_)),
            "{} opened without reporting its repair",
            path.display()
        );
        if doc.page_count() > 0 {
            doc.page_geometry(0)
                .unwrap_or_else(|error| panic!("{} has no page geometry: {error}", path.display()));
        }
    }
}

#[test]
fn page_text_search_and_state_use_core_types() {
    let mut doc = Document::open_path(&seed("hello.pdf")).expect("seed opens");
    let options = SearchOptions::default();

    let text = doc.page_text(0).expect("page text extracts");
    assert!(text.runs.iter().any(|run| run.text.contains("Hello")));

    let matches = doc
        .search_page(0, "Onionskin", options)
        .expect("page search succeeds");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].page, 0);
    assert!(!matches[0].quads.is_empty());

    doc.search_mut().set_query("Onionskin", options);
    doc.search_mut().replace_matches(matches);
    assert_eq!(doc.search().needle(), "Onionskin");
    assert!(doc.search().current().is_some());

    doc.selection_mut().set_region(PageRect {
        page: 0,
        x0: 10.0,
        y0: 10.0,
        x1: 20.0,
        y1: 20.0,
    });
    assert!(doc.selection().region().is_some());
}

#[test]
fn content_and_core_share_one_quad_type() {
    let content_quad = onionskin_content::PageQuad {
        page: 0,
        corners: [(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
    };
    let core_quad: onionskin_core::PageQuad = content_quad;
    assert_eq!(core_quad.page, 0);
}

#[test]
fn encrypted_error_display_names_encryption_and_m2() {
    let err = match Document::open_bytes(encrypted_pdf()) {
        Ok(_) => panic!("encrypted file opened"),
        Err(err) => err,
    };
    assert!(matches!(err, Error::EncryptedUnsupported));

    let message = err.to_string();
    assert!(message.contains("encrypted"));
    assert!(message.contains("M2"));
    assert!(!message.contains("EncryptedUnsupported"));
}

fn encrypted_pdf() -> Vec<u8> {
    let objects: [&[u8]; 3] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources <<>> >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    let xref = out.len();
    out.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R /Encrypt <<>> >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    out
}
