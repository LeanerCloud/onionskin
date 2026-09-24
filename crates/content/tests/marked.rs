//! The marked-content ids a page's own content opens: written in the
//! operator, named in `/Properties`, and not those inside a form it draws.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::page_mcids;

#[test]
fn a_pages_own_marked_content_ids_are_found() {
    let content = "/P << /MCID 0 >> BDC BT ET EMC /Span /P1 BDC EMC /Artifact BMC EMC \
                   /Figure << /Alt (x) >> BDC EMC /Fm0 Do";
    let resources = "<< /Properties << /P1 << /MCID 3 >> >> /XObject << /Fm0 5 0 R >> >>";
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
        b"/P << /MCID 9 >> BDC EMC",
    );
    let doc = open_bytes(one_page(content, resources, &[form]));
    let found: Vec<i64> = page_mcids(&doc, 0).expect("reads").into_iter().collect();
    assert_eq!(
        found,
        [0, 3],
        "not the form's own, not a sequence without one"
    );
    assert!(page_mcids(&doc, 4).is_err(), "no such page");
}
