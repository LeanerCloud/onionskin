//! Edit Description over every place a file specification can live: its
//! own object in the name tree, inline in the name tree, and inline in a
//! file attachment comment. Read back by us and by pypdf.

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
    let dir = tempfile::Builder::new()
        .prefix("onionskin-describe-")
        .tempdir()
        .expect("temporary directory")
        .keep();
    let path = dir.join("attached.pdf");
    std::fs::write(&path, document()).unwrap();

    let mut file = DocumentFile::open(&path).expect("opens");
    let listed = file.attachments().expect("reads").to_vec();
    assert_eq!(listed.len(), 3);
    assert_eq!(
        listed
            .iter()
            .map(|attachment| (&attachment.name, &attachment.description))
            .collect::<Vec<_>>(),
        [
            (&"a.txt".to_owned(), &None),
            (&"b.txt".to_owned(), &None),
            (&"c.txt".to_owned(), &Some("old".to_owned())),
        ]
    );
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
    for expected in [
        [None, None, Some("old")],
        [Some("First file"), None, Some("old")],
        [Some("First file"), Some("Zweite Datei ä"), Some("old")],
    ]
    .into_iter()
    .rev()
    {
        assert!(file.undo().expect("undoes"));
        assert_eq!(
            file.attachments()
                .expect("reads")
                .iter()
                .map(|attachment| attachment.description.as_deref())
                .collect::<Vec<_>>(),
            expected
        );
    }
    for expected in [
        [Some("First file"), None, Some("old")],
        [Some("First file"), Some("Zweite Datei ä"), Some("old")],
        [Some("First file"), Some("Zweite Datei ä"), None],
    ] {
        assert!(file.redo().expect("redoes"));
        assert_eq!(
            file.attachments()
                .expect("reads")
                .iter()
                .map(|attachment| attachment.description.as_deref())
                .collect::<Vec<_>>(),
            expected
        );
    }
    file.save().expect("saves");
    drop(file);

    let mut reopened = DocumentFile::open(&path).expect("reopens");
    assert_eq!(
        reopened
            .attachments()
            .expect("reads")
            .iter()
            .map(|attachment| (&attachment.name, &attachment.description))
            .collect::<Vec<_>>(),
        [
            (&"a.txt".to_owned(), &Some("First file".to_owned())),
            (&"b.txt".to_owned(), &Some("Zweite Datei ä".to_owned())),
            (&"c.txt".to_owned(), &None),
        ]
    );

    let required = std::env::var_os("ONIONSKIN_REQUIRE_ATTACHMENT_ORACLE").is_some();
    let script = r#"
import importlib.util
import sys

if importlib.util.find_spec('pypdf') is None:
    print('MISSING_PYPDF')
    raise SystemExit(0)

from pypdf import PdfReader

pdf = PdfReader(sys.argv[1])
root = pdf.trailer['/Root'].get_object()
names = root['/Names'].get_object()['/EmbeddedFiles'].get_object()
while '/Kids' in names:
    names = names['/Kids'][0].get_object()
leaf = names['/Names']
print([str(leaf[i + 1].get_object().get('/Desc', '')) for i in range(0, len(leaf), 2)])
annotation = pdf.pages[0]['/Annots'].get_object()[0].get_object()
filespec = annotation['/FS'].get_object()
print('/Desc' in filespec)
"#;
    match Command::new("python3")
        .args(["-c", script])
        .arg(&path)
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8(output.stdout).expect("pypdf output is UTF-8");
            if stdout == "MISSING_PYPDF\n" {
                assert!(!required, "pypdf is required for this test");
                eprintln!("skipping pypdf readback: module is unavailable");
            } else {
                assert_eq!(stdout, "['First file', 'Zweite Datei ä']\nFalse\n");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            assert!(!required, "python3 is required for this test");
            eprintln!("skipping pypdf readback: python3 is unavailable");
        }
        Ok(output) => panic!("pypdf readback failed: {output:?}"),
        Err(error) => panic!("pypdf readback could not start: {error}"),
    }
    match Command::new("qpdf").arg("--check").arg(&path).output() {
        Ok(output) => assert!(output.status.success(), "qpdf --check: {output:?}"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping qpdf check: qpdf is unavailable");
        }
        Err(error) => panic!("qpdf check could not start: {error}"),
    }
}
