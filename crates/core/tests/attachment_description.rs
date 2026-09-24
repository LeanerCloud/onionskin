//! Edit Description over every place a file specification can live: its
//! own object in the name tree, inline in the name tree, and inline in a
//! file attachment comment. Read back by us and by pikepdf.

mod common;

use std::process::Command;

use onionskin_core::DocumentFile;

fn document() -> Vec<u8> {
    common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [(a.txt) 5 0 R (b.txt) << /Type /Filespec /F (b.txt) /EF << /F 7 0 R >> >>] >> >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Annots [4 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /FileAttachment /Rect [10 10 30 30] /FS << /Type /Filespec /F (c.txt) /Desc (old) /EF << /F 8 0 R >> >> >>".to_vec(),
        b"<< /Type /Filespec /F (a.txt) /EF << /F 6 0 R >> >>".to_vec(),
        common::stream("alpha"),
        common::stream("beta"),
        common::stream("gamma"),
    ])
}

#[test]
fn a_description_is_written_wherever_the_specification_lives() {
    let dir = std::env::temp_dir().join(format!("onionskin-describe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("attached.pdf");
    std::fs::write(&path, document()).unwrap();

    let mut file = DocumentFile::open(&path).expect("opens");
    let listed = file.attachments().expect("reads").to_vec();
    assert_eq!(listed.len(), 3);
    for (attachment, text) in listed.iter().zip(["First file", "Zweite Datei ä", ""]) {
        let stream = attachment.stream;
        let changed = file
            .edit_document("Edit Description", |tx| {
                onionskin_core::set_attachment_description(tx, stream, text)
            })
            .expect("the description is written");
        assert_eq!(changed, 1, "{}", attachment.name);
    }
    let now: Vec<_> = file
        .attachments()
        .expect("reads")
        .iter()
        .map(|attachment| attachment.description.clone())
        .collect();
    assert_eq!(
        now,
        [
            Some("First file".to_owned()),
            Some("Zweite Datei ä".to_owned()),
            None
        ],
        "a blank description removes the old one"
    );
    file.save().expect("saves");

    let script = "import pikepdf,sys\n\
        pdf = pikepdf.open(sys.argv[1])\n\
        names = pdf.Root.Names.EmbeddedFiles\n\
        leaf = names.Names if '/Names' in names else names.Kids[0].Names\n\
        print([str(leaf[i + 1].get('/Desc', '')) for i in range(0, len(leaf), 2)])\n\
        print('/Desc' in pdf.pages[0].Annots[0].FS)";
    if let Ok(output) = Command::new("python3")
        .args(["-c", script])
        .arg(&path)
        .output()
    {
        if output.status.success() {
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "['First file', 'Zweite Datei ä']\nFalse\n"
            );
        }
    }
    if let Ok(output) = Command::new("qpdf").arg("--check").arg(&path).output() {
        assert!(output.status.success(), "qpdf --check: {output:?}");
    }
}
