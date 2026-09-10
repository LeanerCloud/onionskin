//! The overlay-taking section builder: the caller holds the edits, the
//! document holds none, and one call serves both a save and the preview that
//! has to agree with it byte for byte.

mod common;

use common::{classic_pdf, skeleton};
use onionskin_cos::{BytesSource, Document, Object};

fn fixture() -> Vec<u8> {
    let mut bodies: Vec<&[u8]> = skeleton();
    bodies.push(b"<</Type/Spare/Which 4>>");
    classic_pdf(&bodies, &[])
}

fn open(bytes: &[u8]) -> Document {
    Document::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the fixture opens clean")
}

/// A caller that keeps its own overlay allocates its own numbers, so it needs
/// to know where the file's own numbering stops. Calling `add_object` to find
/// out would leave an edit the document cannot withdraw.
#[test]
fn the_next_object_number_is_one_above_everything_the_file_names() {
    let mut document = open(&fixture());
    // The fixture writes objects 1 through 4 and a /Size of 5.
    assert_eq!(document.next_object_number(), 5);

    let first = document.add_object(Object::Integer(1)).expect("a number");
    assert_eq!(
        first.number, 5,
        "the accessor names the number that is handed out"
    );
    assert_eq!(
        document.next_object_number(),
        6,
        "and it moves on once that number is taken"
    );

    document
        .set_object(2, 0, Object::Integer(2))
        .expect("object 2 is writable");
    assert_eq!(
        document.next_object_number(),
        6,
        "rewriting an existing object takes no new number"
    );
}
