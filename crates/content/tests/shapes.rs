//! Line art read off a page: rules and boxes drawn directly and inside form
//! XObjects, placed on the page by the transformation in force.

mod common;

use common::{one_page, open_bytes, stream};
use onionskin_content::page_shapes;

#[test]
fn a_page_s_rules_and_boxes_are_read_where_they_are_drawn() {
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 200]",
        b"0 0 12 12 re S",
    );
    let content = "10 20 m 110 20 l S q 1 0 0 1 50 60 cm /Fm0 Do Q 0 0 5 5 re W n 1 1 m 2 2 l";
    let bytes = one_page(content, "<< /XObject << /Fm0 5 0 R >> >>", &[form]);
    let shapes = page_shapes(&open_bytes(bytes), 0).expect("reads");
    assert_eq!(
        shapes.len(),
        2,
        "a clip and an unpainted path are not line art"
    );
    assert_eq!(shapes[0].bounds(), [10.0, 20.0, 110.0, 20.0]);
    assert_eq!(
        shapes[1].rects,
        [[50.0, 60.0, 62.0, 72.0]],
        "inside the form, moved by cm"
    );
    assert!(page_shapes(&open_bytes(one_page("", "<< >>", &[])), 0)
        .expect("reads")
        .is_empty());
}
