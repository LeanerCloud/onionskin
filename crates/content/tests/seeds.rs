//! Geometry and provenance against the seeds, whose layout this repository
//! controls.
//!
//! `corpus/make-seeds.py` writes `BT /F1 18 Tf 20 40 Td (<text>) Tj ET` on a
//! 200x100 media box in Helvetica with no font descriptor. Every expected
//! number below is derived from those parameters and the published Helvetica
//! metrics, never from what the extractor happened to produce.

mod common;

use std::path::PathBuf;

use onionskin_content::{extract_page, Mapping};

/// Seed parameters, straight out of `corpus/make-seeds.py`.
const SIZE: f64 = 18.0;
const ORIGIN_X: f64 = 20.0;
const ORIGIN_Y: f64 = 40.0;
/// `crates/content/src/font/mod.rs` falls back to these when a font has no
/// descriptor, which is the seeds' case.
const ASCENT: f64 = 0.75;
const DESCENT: f64 = -0.25;

/// Adobe Helvetica AFM widths, in glyph space, for the characters the seeds
/// use. Listed here rather than read from the crate's own table so the test
/// cannot agree with a wrong table.
fn helvetica(ch: char) -> f64 {
    match ch {
        'H' => 722.0,
        'e' => 556.0,
        'l' => 222.0,
        'o' => 556.0,
        ' ' => 278.0,
        'O' => 778.0,
        'n' => 556.0,
        'i' => 222.0,
        's' => 500.0,
        'k' => 500.0,
        'P' => 667.0,
        'a' => 556.0,
        'g' => 556.0,
        't' => 278.0,
        'w' => 722.0,
        other => panic!("no width listed for {other:?}"),
    }
}

fn seed(name: &str) -> Option<PathBuf> {
    let dir = common::corpus_dir("seeds")?;
    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Walks a seed's single run and checks every glyph lands where the generator
/// placed it.
fn assert_seed_layout(name: &str, expected: &str) {
    let Some(path) = seed(name) else {
        common::missing(&format!("{name} is absent"));
        return;
    };
    let doc = onionskin_cos::Document::open_path(&path).expect("seed opens clean");
    let page = extract_page(&doc, 0).expect("seed extracts");

    assert_eq!(page.runs.len(), 1, "{name} draws one showing operator");
    let run = &page.runs[0];
    assert_eq!(run.text, expected);
    assert_eq!(run.size, SIZE);
    assert_eq!(run.glyphs.len(), expected.chars().count());
    assert!(
        page.warnings.is_empty(),
        "{name} warned: {:?}",
        page.warnings
    );

    let mut pen = ORIGIN_X;
    for (glyph, ch) in run.glyphs.iter().zip(expected.chars()) {
        let width = helvetica(ch) / 1000.0 * SIZE;
        let [upper_left, upper_right, lower_left, lower_right] = glyph.quad.corners;

        assert!(
            close(lower_left.0, pen),
            "{name} {ch:?} starts at {} not {pen}",
            lower_left.0
        );
        assert!(
            close(lower_right.0, pen + width),
            "{name} {ch:?} ends at {} not {}",
            lower_right.0,
            pen + width
        );
        assert!(close(lower_left.1, ORIGIN_Y + DESCENT * SIZE));
        assert!(close(upper_left.1, ORIGIN_Y + ASCENT * SIZE));
        // Unrotated text keeps the two upper corners level with each other.
        assert!(close(upper_left.1, upper_right.1));
        assert!(close(upper_left.0, lower_left.0));

        pen += width;
    }
}

#[test]
fn hello_glyphs_land_where_the_generator_put_them() {
    assert_seed_layout("hello.pdf", "Hello Onionskin");
}

#[test]
fn two_page_seed_lays_out_both_pages() {
    assert_seed_layout("two-page.pdf", "Page one");

    let Some(path) = seed("two-page.pdf") else {
        return;
    };
    let doc = onionskin_cos::Document::open_path(&path).unwrap();
    let second = extract_page(&doc, 1).expect("second page extracts");
    assert_eq!(second.runs.len(), 1);
    assert_eq!(second.runs[0].text, "Page two");
    assert_eq!(second.runs[0].page, 1);
    assert!(close(second.runs[0].glyphs[0].quad.corners[2].0, ORIGIN_X));
}

#[test]
fn a_page_with_no_content_extracts_nothing_and_says_nothing() {
    let Some(path) = seed("minimal.pdf") else {
        return;
    };
    let doc = onionskin_cos::Document::open_path(&path).unwrap();
    let page = extract_page(&doc, 0).expect("minimal extracts");
    assert!(page.runs.is_empty());
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
}

#[test]
fn every_seed_glyph_traces_to_the_bytes_that_drew_it() {
    let Some(path) = seed("hello.pdf") else {
        return;
    };
    let raw = std::fs::read(&path).unwrap();
    let doc = onionskin_cos::Document::open_path(&path).unwrap();
    let page = extract_page(&doc, 0).unwrap();

    let run = &page.runs[0];
    assert!(!run.glyphs.is_empty());

    // The stream object the run names must be the page's /Contents, and the
    // file span cos recorded for it must contain the showing operator.
    let span = run
        .provenance
        .file_span()
        .expect("a run from a file has file bytes");
    let object = &raw[span.start as usize..span.end as usize];
    assert!(
        object.starts_with(b"4 0 obj"),
        "provenance points at {:?}",
        String::from_utf8_lossy(&object[..20.min(object.len())])
    );

    // The seed's content stream is unfiltered, so the decoded offsets are also
    // offsets into those file bytes.
    let decoded = onionskin_content::content(
        &doc,
        &onionskin_content::page(&doc, 0).unwrap(),
        &mut Vec::new(),
    )
    .unwrap();
    let operator =
        &decoded.bytes[run.provenance.decoded.start as usize..run.provenance.decoded.end as usize];
    assert_eq!(operator, b"(Hello Onionskin) Tj");
    assert!(
        object
            .windows(operator.len())
            .any(|window| window == operator),
        "the operator bytes are not inside the object's file span"
    );
}

#[test]
fn search_over_a_seed_returns_quads_and_provenance() {
    let Some(path) = seed("hello.pdf") else {
        return;
    };
    let doc = onionskin_cos::Document::open_path(&path).unwrap();
    let page = extract_page(&doc, 0).unwrap();

    let hits = onionskin_content::search(&page, "onionskin", Default::default());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "Onionskin");
    assert_eq!(hits[0].quads.len(), 9);
    assert_eq!(hits[0].provenance.len(), 1);

    // "Onionskin" starts after "Hello ".
    let expected_x = ORIGIN_X
        + "Hello "
            .chars()
            .map(|c| helvetica(c) / 1000.0 * SIZE)
            .sum::<f64>();
    assert!(close(hits[0].quads[0].corners[2].0, expected_x));

    // Whole-word finds it too; a strict prefix does not.
    let whole = onionskin_content::SearchOptions {
        whole_word: true,
        ..Default::default()
    };
    assert_eq!(onionskin_content::search(&page, "onion", whole).len(), 0);
    assert_eq!(
        onionskin_content::search(&page, "onionskin", whole).len(),
        1
    );
}

#[test]
fn no_glyph_in_a_seed_is_unmapped() {
    for name in ["hello.pdf", "two-page.pdf"] {
        let Some(path) = seed(name) else { continue };
        let doc = onionskin_cos::Document::open_path(&path).unwrap();
        let page = extract_page(&doc, 0).unwrap();
        for run in &page.runs {
            for glyph in &run.glyphs {
                assert!(
                    matches!(glyph.mapping, Mapping::Text(_)),
                    "{name} left code {} unmapped",
                    glyph.code
                );
            }
        }
    }
}
