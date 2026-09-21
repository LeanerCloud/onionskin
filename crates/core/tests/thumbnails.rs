//! Embed and Remove All Page Thumbnails: each is one edit, and what they
//! write is what a reader of `/Thumb` expects.

use onionskin_core::pages::{embed_thumbnails, remove_thumbnails, THUMBNAIL_SIDE};
use onionskin_core::Document;
use onionskin_corpus_testing::seed;
use onionskin_cos::Object;

fn thumb(doc: &mut Document, page: usize) -> Option<Object> {
    let structure = doc.structure().expect("structure");
    let node = structure.page(page).expect("page");
    let raw = structure.get(node.objref.number).expect("reads").object;
    let reference = raw.as_dict()?.get(b"Thumb")?.clone();
    Some(structure.resolve(&reference).expect("resolves"))
}

#[test]
fn every_page_gets_a_small_picture_of_itself_in_one_undoable_edit() {
    let mut doc = Document::open_path(&seed("two-page.pdf")).expect("opens");
    assert_eq!(embed_thumbnails(&mut doc).expect("embeds"), 2);
    for page in 0..2 {
        let Some(Object::Stream(image)) = thumb(&mut doc, page) else {
            panic!("page {page} has a thumbnail");
        };
        let side = |key: &[u8]| image.dict.get(key).and_then(Object::as_integer).unwrap();
        let (width, height) = (side(b"Width"), side(b"Height"));
        assert!(width.max(height) as f32 <= THUMBNAIL_SIDE + 1.0);
        assert!(width > 0 && height > 0);
        let structure = doc.structure().expect("structure");
        let pixels = structure.decode_stream(&image).expect("decodes");
        assert_eq!(pixels.len() as i64, width * height * 3);
        assert!(
            pixels.iter().any(|value| *value < 200),
            "page {page}'s text is in the picture"
        );
    }
    assert!(doc.undo().expect("undoes"), "one step");
    assert!(thumb(&mut doc, 0).is_none() && thumb(&mut doc, 1).is_none());
}

#[test]
fn removing_takes_every_thumbnail_away_and_counts_them() {
    let mut doc = Document::open_path(&seed("two-page.pdf")).expect("opens");
    assert_eq!(remove_thumbnails(&mut doc).expect("nothing to remove"), 0);
    embed_thumbnails(&mut doc).expect("embeds");
    assert_eq!(remove_thumbnails(&mut doc).expect("removes"), 2);
    assert!(thumb(&mut doc, 0).is_none());
}
