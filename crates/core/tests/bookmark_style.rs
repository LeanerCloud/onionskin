//! Bookmark style: `/F` for bold and italic, `/C` for the colour, read back
//! through our own reader and by pikepdf, which is the reader the plan names
//! for this.
//!
//! The bit field is the thing to get right. ISO 32000-1 12.3.3 makes `/F` a bit
//! POSITION, where italic is bit 1 and bold bit 2, so "bold but not italic" is
//! 2. Reading the two as independent flags would write 1 and turn on italics
//! instead, which is why each combination is pinned below.

mod common;

use std::process::Command;

use onionskin_core::{set_bookmark_style, Document};
use onionskin_cos::Object;

/// A document with one bookmark, at the path `[0]`.
fn document() -> Vec<u8> {
    common::pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> >>".to_vec(),
        b"<< /Type /Outlines /First 5 0 R /Last 5 0 R /Count 1 >>".to_vec(),
        b"<< /Title (Chapter) /Parent 4 0 R >>".to_vec(),
    ])
}

fn flags(document: &mut Document) -> Option<i64> {
    let current = document.structure().expect("structure");
    current
        .get(5)
        .expect("the bookmark")
        .object
        .as_dict()
        .and_then(|dict| dict.get(b"F"))
        .and_then(Object::as_integer)
}

fn colour(document: &mut Document) -> Option<Vec<f64>> {
    let current = document.structure().expect("structure");
    current
        .get(5)
        .expect("the bookmark")
        .object
        .as_dict()
        .and_then(|dict| dict.get(b"C"))
        .and_then(Object::as_array)
        .map(|array| {
            array
                .iter()
                .map(|value| match value {
                    Object::Real(channel) => *channel,
                    Object::Integer(channel) => *channel as f64,
                    other => panic!("a colour channel is a number, not {other:?}"),
                })
                .collect()
        })
}

/// Every combination of the two bits, which is what a flags reading would get
/// wrong for half of them.
#[test]
fn the_style_bits_are_a_position_and_not_two_flags() {
    for (bold, italic, expected) in [
        (false, false, None),
        (false, true, Some(1)),
        (true, false, Some(2)),
        (true, true, Some(3)),
    ] {
        let mut document = Document::open_bytes(document()).expect("opens");
        document
            .edit_document("Bookmark Properties", |tx| {
                set_bookmark_style(tx, &[0], bold, italic, None)
            })
            .unwrap_or_else(|error| panic!("bold={bold} italic={italic}: {error:?}"));
        assert_eq!(
            flags(&mut document),
            expected,
            "bold={bold} italic={italic}"
        );
    }
}

#[test]
fn a_colour_is_three_floats_and_absent_means_removed() {
    let mut document = Document::open_bytes(document()).expect("opens");
    document
        .edit_document("Bookmark Properties", |tx| {
            set_bookmark_style(tx, &[0], true, false, Some([1.0, 0.0, 0.5]))
        })
        .expect("colours");
    assert_eq!(colour(&mut document), Some(vec![1.0, 0.0, 0.5]));
    // Writing no colour removes the key rather than writing a black one, which
    // is how a bookmark goes back to the renderer's own choice.
    document
        .edit_document("Bookmark Properties", |tx| {
            set_bookmark_style(tx, &[0], true, false, None)
        })
        .expect("removes the colour");
    assert_eq!(colour(&mut document), None);
    assert_eq!(flags(&mut document), Some(2), "the style bits survive");
}

/// The whole entry: an undo takes the colour and the bits back, and a save is
/// one incremental section like any other edit.
#[test]
fn the_style_is_one_undoable_step() {
    let mut document = Document::open_bytes(document()).expect("opens");
    let original = document.bytes().as_ref().clone();
    document
        .edit_document("Bookmark Properties", |tx| {
            set_bookmark_style(tx, &[0], true, true, Some([0.0, 0.0, 1.0]))
        })
        .expect("styles");
    assert_eq!(flags(&mut document), Some(3));
    assert_eq!(colour(&mut document), Some(vec![0.0, 0.0, 1.0]));
    assert!(document.bytes().as_ref() == &original, "not yet written");
    assert!(document.undo().expect("undoes"));
    assert_eq!(flags(&mut document), None, "the undo takes the bits back");
    assert_eq!(colour(&mut document), None, "and the colour");
    assert!(document.redo().expect("redoes"));
    assert_eq!(flags(&mut document), Some(3));
}

/// pikepdf reads the keys we wrote, which is the independent check the plan
/// asks for: our own reader and ours could agree on the same misreading.
#[test]
fn pikepdf_reads_back_the_style_keys() {
    let dir = tempfile::Builder::new()
        .prefix("onionskin-bookmark-style-")
        .tempdir()
        .expect("temporary directory")
        .keep();
    let path = dir.join("styled.pdf");
    std::fs::write(&path, document()).expect("writes the fixture");

    let mut file = onionskin_core::DocumentFile::open(&path).expect("opens");
    file.edit_document("Bookmark Properties", |tx| {
        set_bookmark_style(tx, &[0], true, true, Some([0.25, 0.5, 0.75]))
    })
    .expect("styles");
    file.save().expect("saves");
    drop(file);

    let required = std::env::var_os("ONIONSKIN_REQUIRE_OUTLINE_ORACLE").is_some();
    let script = r#"
import importlib.util
import sys

if importlib.util.find_spec('pikepdf') is None:
    print('MISSING_PIKEPDF')
    raise SystemExit(0)

import pikepdf

pdf = pikepdf.open(sys.argv[1])
item = pdf.Root.Outlines.First
print(int(item.F))
print([float(channel) for channel in item.C])
"#;
    match Command::new("python3")
        .args(["-c", script])
        .arg(&path)
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8(output.stdout).expect("pikepdf output is UTF-8");
            if stdout == "MISSING_PIKEPDF\n" {
                assert!(!required, "pikepdf is required for this test");
                eprintln!("skipping pikepdf readback: module is unavailable");
            } else {
                assert_eq!(stdout, "3\n[0.25, 0.5, 0.75]\n");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            assert!(!required, "python3 is required for this test");
            eprintln!("skipping pikepdf readback: python3 is unavailable");
        }
        Ok(output) => panic!("pikepdf readback failed: {output:?}"),
        Err(error) => panic!("pikepdf readback could not start: {error}"),
    }
    match Command::new("qpdf").arg("--check").arg(&path).output() {
        Ok(output) => assert!(output.status.success(), "qpdf --check: {output:?}"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping qpdf check: qpdf is unavailable");
        }
        Err(error) => panic!("qpdf check could not start: {error}"),
    }
}
