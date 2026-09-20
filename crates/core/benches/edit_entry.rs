//! What one undo entry costs, so the history bound is derived from a number
//! rather than chosen.
//!
//! The worst case in M3 is not an ink session. It is **one page reorder on the
//! thousand-page bench file**, where a flat rewrite produces about a thousand
//! rewritten page dictionaries as `after` and a thousand captured base values
//! as `before`: roughly two thousand objects in a single entry. A hundred
//! highlights is a hundred entries and three orders of magnitude smaller, which
//! is exactly why [`MAX_HISTORY_BYTES`] is a bound over resident bytes and not
//! over entry count.
//!
//! Like the other benches here, this one asserts rather than prints: a measured
//! entry that does not fit the bound ends the process.

mod harness;

use onionskin_core::{EditSession, MAX_HISTORY_BYTES};
use onionskin_cos::{Document as CosDocument, Name, Object, PageNode};

/// `corpus/make-bench.py` branches the page tree by ten, so a thousand pages
/// is three levels of internal nodes above the leaves.
const PAGES: usize = 1000;

/// Enough highlights to stand in for a working session on one document.
const ANNOTATIONS: usize = 100;

fn main() {
    let Some(path) = harness::bench_document() else {
        return;
    };

    let reorder = one_reorder_entry(&path);
    let annotations = a_hundred_annotation_entries(&path);

    harness::heading("history bound");
    println!("  bound {MAX_HISTORY_BYTES} bytes");
    println!("  one {PAGES}-page reorder: {reorder} bytes in one entry");
    println!("  {ANNOTATIONS} annotations: {annotations} bytes across {ANNOTATIONS} entries");

    assert!(
        reorder < MAX_HISTORY_BYTES,
        "the worst single entry in M3 ({reorder} bytes) must fit the bound, \
         or the first page reorder evicts the whole stack"
    );
    assert!(
        annotations < MAX_HISTORY_BYTES,
        "a {ANNOTATIONS}-annotation session ({annotations} bytes) must fit the bound"
    );
    assert!(
        reorder > annotations,
        "the reorder is the case the bound exists for; if the annotation session \
         is the larger of the two, the bound is being derived from the wrong shape"
    );
}

/// One transaction that rewrites every page dictionary, which is the shape a
/// flat page-tree rewrite produces.
fn one_reorder_entry(path: &std::path::Path) -> usize {
    harness::heading("one page reorder");
    let base = CosDocument::open_path(path).expect("the bench file opens");
    let pages = page_nodes(&base);
    let mut edit = EditSession::for_base(&base);

    edit.transact(&base, "Reorder Pages", |tx| {
        for page in &pages {
            let mut dict = page.dict.clone();
            // A reorder rewrites each leaf; which key moves does not change the
            // resident cost, and /Rotate keeps the document valid.
            dict.set(Name::new("Rotate"), Object::Integer(90));
            tx.set_object(
                page.objref.number,
                page.objref.generation,
                Object::Dict(dict),
            )?;
        }
        Ok(())
    })
    .expect("the reorder commits");

    assert_eq!(edit.history().reach(), 1, "one reorder is one undo step");
    edit.history().resident_bytes()
}

/// A hundred separate transactions, each adding one annotation object and
/// naming it from its page.
fn a_hundred_annotation_entries(path: &std::path::Path) -> usize {
    harness::heading("a hundred annotations");
    let base = CosDocument::open_path(path).expect("the bench file opens");
    let pages = page_nodes(&base);
    let mut edit = EditSession::for_base(&base);

    for index in 0..ANNOTATIONS {
        let page = &pages[index % pages.len()];
        edit.transact(&base, "Highlight", |tx| {
            let number = tx.reserve();
            tx.set_object(number, 0, highlight())?;
            let mut dict = page.dict.clone();
            dict.set(
                Name::new("Annots"),
                Object::Array(vec![Object::Ref(onionskin_cos::ObjRef::new(number, 0))]),
            );
            tx.set_object(
                page.objref.number,
                page.objref.generation,
                Object::Dict(dict),
            )
        })
        .expect("the highlight commits");
    }

    assert_eq!(
        edit.history().reach(),
        ANNOTATIONS,
        "each highlight is its own undo step"
    );
    assert_eq!(
        edit.history().forgotten(),
        0,
        "a hundred highlights do not reach the bound"
    );
    edit.history().resident_bytes()
}

fn page_nodes(base: &CosDocument) -> Vec<PageNode> {
    let pages: Vec<PageNode> = (0..PAGES)
        .map(|index| base.page(index).expect("the bench page resolves"))
        .collect();
    assert_eq!(pages.len(), PAGES);
    pages
}

fn highlight() -> Object {
    let mut dict = onionskin_cos::Dict::new();
    dict.set(Name::new("Type"), Object::name("Annot"));
    dict.set(Name::new("Subtype"), Object::name("Highlight"));
    dict.set(
        Name::new("Rect"),
        Object::Array(vec![
            Object::Integer(72),
            Object::Integer(72),
            Object::Integer(272),
            Object::Integer(96),
        ]),
    );
    Object::Dict(dict)
}
