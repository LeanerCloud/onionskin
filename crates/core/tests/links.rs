//! `core::links`: links read from what a file carries, and written by the
//! Link tool, Create Links from URLs and Remove Web Links.
//!
//! Every assertion reads a fresh parse of the original bytes with the edit
//! appended, so a link is what the next reader of the file sees.

use onionskin_core::links::{
    add_link, link_at, read_links, remove_link, remove_web_links, set_link, Highlight, LineStyle,
    Link, LinkLook, LinkTarget,
};
use onionskin_core::{check, read_structure, Error};
use onionskin_cos::{ObjRef, Object};

mod common;
use common::{flat, open, pdf, stream, tagged, try_apply as apply};

fn links(bytes: &[u8]) -> Vec<Link> {
    let document = open(bytes);
    let count = document.page_count().expect("pages") as usize;
    read_links(&document, count).expect("links read")
}

const RECT: [f64; 4] = [72.0, 690.0, 200.0, 730.0];

fn red_dashed() -> LinkLook {
    LinkLook {
        visible: true,
        width: 2.0,
        color: [1.0, 0.0, 0.0],
        style: LineStyle::Dashed,
        highlight: Highlight::Outline,
    }
}

#[test]
fn a_link_to_a_page_is_written_and_read_back() {
    let (saved, link) = apply(&flat(3), |tx, structure| {
        add_link(tx, structure, 0, RECT, &LinkTarget::Page(2), red_dashed())
    })
    .expect("adds");
    let found = links(&saved);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].objref, link);
    assert_eq!(found[0].page, 0);
    assert_eq!(found[0].rect, RECT);
    assert_eq!(found[0].target, LinkTarget::Page(2));
    assert_eq!(found[0].look, red_dashed());
    assert!(link_at(&found, 0, (100.0, 700.0)).is_some());
    assert!(link_at(&found, 0, (10.0, 10.0)).is_none());
    assert!(link_at(&found, 1, (100.0, 700.0)).is_none());
    assert_eq!(open(&saved).audit_references().expect("audits"), Vec::new());
}

#[test]
fn a_links_target_and_look_change_and_it_can_be_removed() {
    let (saved, link) = apply(&flat(2), |tx, structure| {
        add_link(
            tx,
            structure,
            1,
            RECT,
            &LinkTarget::Page(0),
            LinkLook::default(),
        )
    })
    .expect("adds");
    let default = links(&saved)[0].look;
    assert_eq!(default, LinkLook::default(), "invisible and inverted");

    let web = LinkTarget::Web("https://example.com/a?b=c".to_owned());
    let (saved, ()) = apply(&saved, |tx, _| set_link(tx, link, &web, red_dashed())).expect("sets");
    assert_eq!(links(&saved)[0].target, web);
    assert_eq!(links(&saved)[0].look, red_dashed());

    let file = LinkTarget::File("appendix.pdf".to_owned());
    let (saved, ()) = apply(&saved, |tx, _| set_link(tx, link, &file, red_dashed())).expect("sets");
    assert_eq!(links(&saved)[0].target, file);

    // A target the dialog does not write keeps the action the link has.
    let (saved, ()) = apply(&saved, |tx, _| {
        set_link(
            tx,
            link,
            &LinkTarget::Other("Named".into()),
            LinkLook::default(),
        )
    })
    .expect("sets");
    assert_eq!(links(&saved)[0].target, file);

    let (saved, removed) = apply(&saved, |tx, _| remove_link(tx, 1, link)).expect("removes");
    assert!(removed);
    assert!(links(&saved).is_empty());
    let (_, again) = apply(&saved, |tx, _| remove_link(tx, 1, link)).expect("runs");
    assert!(!again);
}

#[test]
fn remove_web_links_leaves_the_links_to_pages() {
    let (saved, ()) = apply(&flat(2), |tx, structure| {
        add_link(
            tx,
            structure,
            0,
            RECT,
            &LinkTarget::Web("https://a.example".into()),
            LinkLook::default(),
        )?;
        add_link(
            tx,
            structure,
            0,
            RECT,
            &LinkTarget::Page(1),
            LinkLook::default(),
        )?;
        add_link(
            tx,
            structure,
            1,
            RECT,
            &LinkTarget::Web("https://b.example".into()),
            LinkLook::default(),
        )?;
        Ok(())
    })
    .expect("adds");
    let (saved, removed) = apply(&saved, |tx, _| remove_web_links(tx, &[0, 1])).expect("removes");
    assert_eq!(removed, 2);
    let left = links(&saved);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].target, LinkTarget::Page(1));
    let (_, none) = apply(&saved, |tx, _| remove_web_links(tx, &[0, 1])).expect("runs");
    assert_eq!(none, 0);
}

#[test]
fn a_link_on_a_tagged_page_gets_a_link_element() {
    let original = tagged();
    let (saved, link) = apply(&original, |tx, structure| {
        add_link(
            tx,
            structure,
            0,
            RECT,
            &LinkTarget::Page(1),
            LinkLook::default(),
        )
    })
    .expect("adds");
    let after = open(&saved);
    let structure = read_structure(&after).expect("reads");
    let report = check(&after, &structure, 3).expect("checks");
    assert!(report.is_clean(), "{:?}", report.violations);
    let dict = after.get(link.number).expect("present").object;
    let key = dict
        .as_dict()
        .and_then(|dict| dict.get(b"StructParent"))
        .and_then(Object::as_integer);
    assert_eq!(key, Some(3), "the next parent tree key");
    let tree = structure.tree().expect("tagged");
    let element = tree.elements.values().find(|element| {
        element
            .struct_type
            .as_ref()
            .is_some_and(|name| name.as_bytes() == b"Link")
    });
    assert!(element.is_some(), "a /Link element");
}

/// Links as other producers write them: a `/Dest` of their own, a named
/// destination, a file spec dictionary, and actions this crate only reads.
#[test]
fn links_as_other_producers_write_them_are_read() {
    let original = pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Names << /Dests << /Names [(chapter) [4 0 R /Fit]] >> >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 200] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 10 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [10 10 0 0] /Dest [4 0 R /XYZ 0 0 0] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /GoTo /D (chapter) >> /Border [0 0 0] >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /GoToR /F << /Type /Filespec /F (other.pdf) >> /D [0 /Fit] >> /C [0.5] /H /P >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /JavaScript /JS (app.alert(1)) >> /BS << /W 3 /S /U >> >>".to_vec(),
        b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] >>".to_vec(),
        stream("not a dictionary"),
    ]);
    let found = links(&original);
    let targets: Vec<_> = found.iter().map(|link| link.target.clone()).collect();
    assert_eq!(
        targets,
        [
            LinkTarget::Page(1),
            LinkTarget::Page(1),
            LinkTarget::File("other.pdf".into()),
            LinkTarget::Other("JavaScript".into()),
        ],
        "the note and the stream are not links"
    );
    assert_eq!(
        found[0].rect,
        [0.0, 0.0, 10.0, 10.0],
        "corners put in order"
    );
    assert!(found[0].look.visible, "no /Border means a one-point border");
    assert!(!found[1].look.visible);
    assert_eq!(found[2].look.color, [0.5; 3]);
    assert_eq!(found[2].look.highlight, Highlight::Push);
    assert_eq!(found[3].look.style, LineStyle::Underline);
    assert_eq!(found[3].look.width, 3.0);
}

#[test]
fn a_page_or_link_that_is_not_there_is_refused() {
    let refused = apply(&flat(1), |tx, structure| {
        add_link(
            tx,
            structure,
            4,
            RECT,
            &LinkTarget::Page(0),
            LinkLook::default(),
        )
    });
    assert!(matches!(
        refused,
        Err(Error::NoSuchPage { page: 4, count: 1 })
    ));
    let refused = apply(&flat(1), |tx, structure| {
        add_link(
            tx,
            structure,
            0,
            RECT,
            &LinkTarget::Page(7),
            LinkLook::default(),
        )
    });
    assert!(matches!(refused, Err(Error::NoSuchPage { page: 7, .. })));
    // Object 3 is the page, not a link.
    let refused = apply(&flat(1), |tx, _| {
        set_link(
            tx,
            ObjRef::new(3, 0),
            &LinkTarget::Page(0),
            LinkLook::default(),
        )
    });
    assert!(matches!(refused, Err(Error::NotADictionary { number: 3 })));
    let refused = apply(&flat(1), |tx, _| {
        set_link(
            tx,
            ObjRef::new(999, 0),
            &LinkTarget::Page(0),
            LinkLook::default(),
        )
    });
    assert!(matches!(
        refused,
        Err(Error::NotADictionary { number: 999 })
    ));
}
