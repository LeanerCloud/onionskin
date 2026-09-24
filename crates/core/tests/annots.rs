//! Annotations, from the outside.
//!
//! Two assertions carry this file and neither is sufficient alone. The
//! **structural** test says the file contains what was asked for; the
//! **render** test says a reader draws it. Removing the appearance stream
//! leaves the first green and the second red, which is exactly why both exist.
//!
//! The render tests lean on `BaseRaster::content_bounds`, which is the
//! bounding box of every pixel that is not the opaque white the rasterizer
//! starts from. On an otherwise blank page that box *is* the annotation, so a
//! render assertion can be made on geometry rather than on a pixel count.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use onionskin_core::{
    add_annotation, check, pdf_date, read_annotations, read_structure, remove_annotation,
    Annotation, AnnotationFilter, Color, Document, EditSession, Quad, Rect, Subtype,
};
use onionskin_cos::{BytesSource, Document as CosDocument, ObjRef, Object};

/// Every subtype `core::annots` authors. A new one added to the enum without a
/// generator fails to compile here, which is the point of listing them.
const SUBTYPES: &[Subtype] = &[
    Subtype::Highlight,
    Subtype::Underline,
    Subtype::StrikeOut,
    Subtype::Squiggly,
    Subtype::Text,
    Subtype::FreeText,
    Subtype::Ink,
    Subtype::Square,
    Subtype::Circle,
    Subtype::Line,
    Subtype::Polygon,
    Subtype::PolyLine,
    Subtype::Stamp,
    Subtype::FileAttachment,
];

/// 2026-09-21T00:00:00Z, so the date assertions are on a fixed instant.
const WHEN: i64 = 1_789_948_800;

// ---------------------------------------------------------------------------
// Structural
// ---------------------------------------------------------------------------

/// One annotation of each subtype: saved, reopened through `cos`, and asserted
/// on the object graph. No substring scanning of the file.
#[test]
fn every_subtype_authors_a_structurally_correct_annotation() {
    for subtype in SUBTYPES {
        let original = blank_page();
        let base = open(&original);
        let before = annots_len(&base);

        let annotation = sample(*subtype);
        let (bytes, objref) = author(&original, &base, &annotation);
        let after = open(&bytes);

        let annots = annots(&after);
        assert_eq!(
            annots.len(),
            before + 1,
            "{subtype:?}: /Annots gained exactly one reference"
        );
        assert!(
            annots
                .iter()
                .any(|item| matches!(item, Object::Ref(r) if r.number == objref.number)),
            "{subtype:?}: the page names the new annotation"
        );

        let dict = after
            .get(objref.number)
            .expect("the annotation parses")
            .object
            .as_dict()
            .cloned()
            .unwrap_or_else(|| panic!("{subtype:?}: the annotation is a dictionary"));

        assert_eq!(
            dict.get(b"Subtype")
                .and_then(Object::as_name)
                .map(|n| n.as_bytes().to_vec()),
            Some(subtype.as_str().as_bytes().to_vec()),
            "{subtype:?}: /Subtype"
        );
        assert_eq!(
            numbers(dict.get(b"Rect")),
            vec![
                annotation.rect.x0,
                annotation.rect.y0,
                annotation.rect.x1,
                annotation.rect.y1
            ],
            "{subtype:?}: /Rect"
        );
        assert_eq!(
            dict.get(b"F").and_then(Object::as_integer),
            Some(annotation.flags.0),
            "{subtype:?}: /F"
        );

        // /AP /N is a stream, and its /BBox contains the /Rect expressed in the
        // annotation's own space, which is the rect translated to the origin.
        let appearance = dict
            .get(b"AP")
            .and_then(Object::as_dict)
            .and_then(|ap| ap.get(b"N"))
            .cloned()
            .unwrap_or_else(|| panic!("{subtype:?}: /AP /N is present"));
        let Object::Ref(appearance_ref) = appearance else {
            panic!("{subtype:?}: /AP /N is an indirect reference");
        };
        let form = after
            .get(appearance_ref.number)
            .expect("the appearance parses")
            .object;
        let form = form
            .as_stream()
            .unwrap_or_else(|| panic!("{subtype:?}: /AP /N is a stream"));
        let bbox = numbers(form.dict.get(b"BBox"));
        let [bx0, by0, bx1, by1] = bbox[..] else {
            panic!("{subtype:?}: /BBox has four numbers");
        };
        let box_rect = Rect::new(bx0, by0, bx1, by1);
        let rect_in_own_space =
            Rect::new(0.0, 0.0, annotation.rect.width(), annotation.rect.height());
        assert!(
            box_rect.contains(&rect_in_own_space),
            "{subtype:?}: /BBox {box_rect:?} must contain the rect in its own space {rect_in_own_space:?}"
        );
    }
}

#[test]
fn text_markup_writes_quadpoints_in_the_order_readers_expect() {
    let original = blank_page();
    let base = open(&original);
    let quad = Quad::from_rect(Rect::new(20.0, 40.0, 120.0, 60.0));
    let annotation = Annotation::markup(Subtype::Highlight, vec![quad]).expect("one quad");

    let (bytes, objref) = author(&original, &base, &annotation);
    let after = open(&bytes);
    let dict = after
        .get(objref.number)
        .unwrap()
        .object
        .as_dict()
        .cloned()
        .unwrap();

    assert_eq!(
        numbers(dict.get(b"QuadPoints")),
        vec![20.0, 60.0, 120.0, 60.0, 20.0, 40.0, 120.0, 40.0],
        "upper-left, upper-right, lower-left, lower-right: the order readers \
         expect, not the counterclockwise order 12.5.6.10's prose describes"
    );
}

#[test]
fn dates_are_written_in_pdf_format_with_a_timezone() {
    assert_eq!(pdf_date(WHEN), "D:20260921000000Z00'00'");

    let original = blank_page();
    let base = open(&original);
    let (bytes, objref) = author(&original, &base, &sample(Subtype::Square));
    let after = open(&bytes);
    let dict = after
        .get(objref.number)
        .unwrap()
        .object
        .as_dict()
        .cloned()
        .unwrap();

    for key in [b"CreationDate".as_slice(), b"M".as_slice()] {
        let Some(Object::String(value)) = dict.get(key) else {
            panic!("{} is a string", String::from_utf8_lossy(key));
        };
        assert_eq!(value, pdf_date(WHEN).as_bytes());
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// The pair the mutation test turns on: a drawn annotation marks pixels, and
/// the same annotation with `/F` Hidden does not.
#[test]
fn an_annotation_renders_and_its_hidden_twin_does_not() {
    let original = blank_page();
    let base = open(&original);

    assert!(
        content_bounds(&original, 1.0).is_none(),
        "the fixture page is blank, so anything drawn is the annotation"
    );

    let (visible, _) = author(&original, &base, &sample(Subtype::Square));
    let drawn = content_bounds(&visible, 1.0).expect("a visible annotation marks pixels");
    assert!(
        drawn.2 > 50 && drawn.3 > 50,
        "the annotation covers its rect, got {drawn:?}"
    );

    let mut hidden_annotation = sample(Subtype::Square);
    hidden_annotation.flags = hidden_annotation.flags.with_hidden(true);
    let (hidden, _) = author(&original, &base, &hidden_annotation);
    assert!(
        content_bounds(&hidden, 1.0).is_none(),
        "an annotation with /F Hidden draws nothing"
    );
}

/// A highlighter lays ink over words; it does not erase them.
///
/// An opaque fill puts the mark in exactly the right place and takes the text
/// with it, and every structural assertion in this file stays green while it
/// does: the `/QuadPoints` are right, the `/AP` is present, the rect is right,
/// and the page now reads as a blank yellow band. What makes the difference is
/// `/BM /Multiply` in the appearance's own graphics state, so the claim is made
/// here, in pixels, where an absent blend mode is visible.
#[test]
fn a_highlight_lets_the_text_beneath_it_show_through() {
    let original = text_page();
    let ink = dark_pixels(&original);
    assert!(
        ink > 100,
        "the fixture has to carry real text for this to measure anything, got {ink} dark pixels"
    );

    let mut highlight = Annotation::new(Subtype::Highlight, TEXT_BAND);
    highlight.quads = vec![Quad::from_rect(TEXT_BAND)];
    highlight.color = Some(Color::new(1.0, 0.92, 0.23));
    let base = open(&original);
    let (highlighted, _) = author(&original, &base, &highlight);

    assert!(
        yellow_pixels(&highlighted) > 100,
        "the highlight has to have drawn, or the survival of the text below proves nothing"
    );
    let surviving = dark_pixels(&highlighted);
    assert!(
        surviving * 5 >= ink * 4,
        "the text under the highlight was painted out: {ink} dark pixels became {surviving}"
    );
}

/// The review risk this package names: a `/BBox` and `/Matrix` that do not map
/// onto the `/Rect` produce an annotation that is right at one zoom and drifts
/// at another, which a single-zoom test cannot catch.
#[test]
fn the_appearance_lands_on_the_rect_at_every_zoom() {
    let original = blank_page();
    let base = open(&original);
    let (bytes, _) = author(&original, &base, &sample(Subtype::Square));

    let at_one = content_bounds(&bytes, 1.0).expect("drawn at zoom 1");
    let at_two = content_bounds(&bytes, 2.0).expect("drawn at zoom 2");

    // Doubling the zoom doubles every coordinate. A /BBox in the wrong space
    // scales the appearance by the box-to-rect ratio instead, which shows up
    // here as a width that is not twice the width.
    for (one, two, what) in [
        (at_one.0, at_two.0, "x"),
        (at_one.1, at_two.1, "y"),
        (at_one.2, at_two.2, "width"),
        (at_one.3, at_two.3, "height"),
    ] {
        let expected = one as f64 * 2.0;
        assert!(
            (two as f64 - expected).abs() <= 3.0,
            "{what}: {two} at zoom 2 should be about {expected}, twice the {one} at zoom 1"
        );
    }
}

/// Each mode asserted by rendering, not by inspecting the filter's own output.
#[test]
fn the_filter_hides_by_mode_and_never_touches_the_saved_document() {
    let original = blank_page();
    let base = open(&original);
    let (with_highlight, _) = author(&original, &base, &sample(Subtype::Highlight));

    let document = open(&with_highlight);
    assert!(
        content_bounds(&with_highlight, 1.0).is_some(),
        "the unfiltered document draws its markup"
    );

    let hidden = preview(&with_highlight, &document, AnnotationFilter::DocumentOnly);
    assert!(
        content_bounds(&hidden, 1.0).is_none(),
        "DocumentOnly hides every markup"
    );

    let stamps_only = preview(
        &with_highlight,
        &document,
        AnnotationFilter::DocumentAndStamps,
    );
    assert!(
        content_bounds(&stamps_only, 1.0).is_none(),
        "DocumentAndStamps hides a highlight"
    );

    let (with_stamp, _) = author(&original, &base, &sample(Subtype::Stamp));
    let stamped = open(&with_stamp);
    let kept = preview(&with_stamp, &stamped, AnnotationFilter::DocumentAndStamps);
    assert!(
        content_bounds(&kept, 1.0).is_some(),
        "DocumentAndStamps keeps a stamp"
    );

    // The filter is a preview: the document it was asked about is unchanged.
    assert_eq!(
        with_highlight,
        author(&original, &base, &sample(Subtype::Highlight)).0,
        "asking the filter anything leaves the saved bytes identical"
    );
}

// ---------------------------------------------------------------------------
// Undo, structure, and reading other people's files
// ---------------------------------------------------------------------------

/// By value, which is what the collapse rule makes checkable: an overlay that
/// equals the base is empty, so the following save writes nothing at all.
#[test]
fn undoing_an_annotation_restores_the_pages_annots_exactly() {
    let original = blank_page();
    let base = open(&original);
    let structure = read_structure(&base).expect("structure reads");
    let mut edit = EditSession::for_base(&base);

    let annotation = sample(Subtype::Square);
    let objref = edit
        .transact(&base, "Add Comment", |tx| {
            add_annotation(tx, &structure, ObjRef::new(3, 0), &annotation, WHEN)
        })
        .expect("the annotation commits");
    assert!(!edit.overlay().is_empty());

    assert!(edit.undo(&base).expect("undo runs"));

    assert!(
        edit.overlay().is_empty(),
        "undo restores the page's /Annots to its original object by value"
    );
    assert!(
        base.section_for(&edit.pending_edits(), &edit.trailer_edits())
            .expect("section")
            .is_none(),
        "so the following save writes nothing"
    );

    // And removing it again by reference produces the same empty result.
    let mut edit = EditSession::for_base(&base);
    edit.transact(&base, "Delete Comment", |tx| {
        remove_annotation(tx, ObjRef::new(3, 0), objref)
    })
    .expect("the removal commits");
}

/// P4's invariant passes after authoring, and the annotation is reachable from
/// the page's structure element.
#[test]
fn authoring_on_a_tagged_document_keeps_the_structure_tree_valid() {
    let original = tagged_page();
    let base = open(&original);
    let structure = read_structure(&base).expect("structure reads");
    assert!(structure.is_tagged());

    let mut edit = EditSession::for_base(&base);
    let objref = edit
        .transact(&base, "Add Comment", |tx| {
            add_annotation(
                tx,
                &structure,
                ObjRef::new(3, 0),
                &sample(Subtype::Square),
                WHEN,
            )
        })
        .expect("the annotation commits");

    let bytes = append(&original, section(&base, &edit));
    let after = open(&bytes);
    let structure = read_structure(&after).expect("the tree still reads");
    let report = check(&after, &structure, 1).expect("the invariant runs");
    assert!(
        report.is_clean(),
        "authoring leaves the tree consistent: {:?}",
        report.violations
    );

    let tree = structure.tree().expect("tagged");
    assert!(
        tree.elements
            .values()
            .any(|element| element.kids.iter().any(|kid| matches!(
                kid,
                onionskin_core::Kid::Object { object, .. } if object.number == objref.number
            ))),
        "the annotation is reachable from a structure element"
    );

    let dict = after
        .get(objref.number)
        .unwrap()
        .object
        .as_dict()
        .cloned()
        .unwrap();
    assert!(
        dict.get(b"StructParent")
            .and_then(Object::as_integer)
            .is_some(),
        "a tagged document's annotation carries a /StructParent"
    );
}

/// The reader against an independent one, on files this project did not write.
///
/// **Provenance.** pikepdf 10.5.1, a binding over qpdf, read the expectations
/// below out of `corpus/external/verapdf/PDF_UA-1/7.18 Annotations`. They
/// include `/Widget`, `/Link` and `/Popup`, which `core::annots` does not
/// author: those appear with `subtype: None` and a populated `raw_subtype`,
/// which is what keeps a comment pane from silently dropping annotations it
/// cannot author.
#[test]
fn the_reader_agrees_with_an_independent_reader_on_real_files() {
    let Some(root) = annotations_corpus() else {
        eprintln!("SKIPPED: corpus/external/verapdf is absent; fetch it with corpus/fetch.sh");
        return;
    };

    let expected: &[(&str, &[(usize, &str)])] = &[
        (
            "7-18.3-t01-pass-a.pdf",
            &[(0, "Widget"), (0, "Widget"), (1, "Link")],
        ),
        ("7.18.1-t01-fail-a.pdf", &[(0, "Highlight"), (0, "Popup")]),
        (
            "7.18.1-t03-pass-f.pdf",
            &[(0, "Widget"), (0, "Widget"), (0, "Widget")],
        ),
    ];

    for (name, wanted) in expected {
        let path = find(&root, name).unwrap_or_else(|| panic!("{name} is in the fetched set"));
        let doc = CosDocument::open_path(&path).unwrap_or_else(|e| panic!("{name} opens: {e}"));
        let page_count = doc.page_count().expect("page count") as usize;
        let read = read_annotations(&doc, page_count, &BTreeMap::new()).expect("annotations read");

        let mine: BTreeSet<(usize, String)> = read
            .iter()
            .map(|a| (a.page, a.raw_subtype.clone()))
            .collect();
        let theirs: BTreeSet<(usize, String)> = wanted
            .iter()
            .map(|(page, subtype)| (*page, (*subtype).to_string()))
            .collect();
        assert_eq!(
            mine, theirs,
            "{name}: annotation set disagrees with pikepdf"
        );
        assert_eq!(read.len(), wanted.len(), "{name}: annotation count");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample(subtype: Subtype) -> Annotation {
    let rect = Rect::new(50.0, 50.0, 150.0, 150.0);
    let mut annotation = Annotation::new(subtype, rect);
    annotation.contents = Some("a comment".into());
    annotation.author = Some("A Reviewer".into());
    annotation.color = Some(Color::new(0.1, 0.2, 0.9));
    annotation.border_width = 2.0;
    match subtype {
        Subtype::Highlight | Subtype::Underline | Subtype::StrikeOut | Subtype::Squiggly => {
            annotation.quads = vec![Quad::from_rect(rect)];
        }
        Subtype::Ink => {
            annotation.ink = vec![vec![(60.0, 60.0), (100.0, 120.0), (140.0, 60.0)]];
        }
        Subtype::Line => {
            annotation.line = Some(((55.0, 55.0), (145.0, 145.0)));
        }
        Subtype::Square | Subtype::Circle => {
            annotation.interior_color = Some(Color::new(0.9, 0.9, 0.2));
        }
        Subtype::Polygon | Subtype::PolyLine => {
            annotation.vertices = vec![(60.0, 60.0), (140.0, 60.0), (100.0, 140.0)];
        }
        _ => {}
    }
    annotation
}

/// Author one annotation onto page object 3 and return the saved bytes.
fn author(original: &[u8], base: &CosDocument, annotation: &Annotation) -> (Vec<u8>, ObjRef) {
    let structure = read_structure(base).expect("structure reads");
    let mut edit = EditSession::for_base(base);
    let objref = edit
        .transact(base, "Add Comment", |tx| {
            add_annotation(tx, &structure, ObjRef::new(3, 0), annotation, WHEN)
        })
        .expect("the annotation commits");
    (append(original, section(base, &edit)), objref)
}

/// A preview buffer: the filter's overrides appended as a section that is
/// rendered and thrown away.
fn preview(bytes: &[u8], doc: &CosDocument, filter: AnnotationFilter) -> Vec<u8> {
    let overrides = filter
        .preview_overrides(
            doc,
            doc.page_count().expect("pages") as usize,
            &BTreeMap::new(),
        )
        .expect("overrides");
    let section = doc
        .section_for(&overrides, &BTreeMap::new())
        .expect("preview section");
    append(bytes, section)
}

/// `(x, y, width, height)` of everything drawn, or `None` for a blank page.
fn content_bounds(bytes: &[u8], zoom: f32) -> Option<(u32, u32, u32, u32)> {
    let mut document = Document::open_bytes(bytes.to_vec()).expect("the document opens");
    let render = document.render_page_now(0, zoom).expect("the page renders");
    render
        .raster
        .content_bounds()
        .map(|b| (b.x, b.y, b.width, b.height))
}

/// Every pixel of a page at 72 dpi, as premultiplied RGBA.
fn raster(bytes: &[u8]) -> Vec<[u8; 4]> {
    let mut document = Document::open_bytes(bytes.to_vec()).expect("the document opens");
    let render = document.render_page_now(0, 1.0).expect("the page renders");
    render
        .raster
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect()
}

/// Pixels dark enough to be glyph ink rather than page or highlight.
fn dark_pixels(bytes: &[u8]) -> usize {
    raster(bytes)
        .into_iter()
        .filter(|[r, g, b, _]| *r < 128 && *g < 128 && *b < 128)
        .count()
}

/// Pixels carrying the highlight's own colour: bright, warm, and short of blue.
fn yellow_pixels(bytes: &[u8]) -> usize {
    raster(bytes)
        .into_iter()
        .filter(|[r, g, b, _]| *r > 180 && *g > 150 && *b < 140)
        .count()
}

fn annots(doc: &CosDocument) -> Vec<Object> {
    let page = doc.page(0).expect("page 0");
    match page.dict.get(b"Annots") {
        Some(object) => match doc.resolve(object).expect("annots resolve") {
            Object::Array(items) => items,
            _ => Vec::new(),
        },
        None => Vec::new(),
    }
}

fn annots_len(doc: &CosDocument) -> usize {
    annots(doc).len()
}

fn numbers(object: Option<&Object>) -> Vec<f64> {
    let Some(Object::Array(items)) = object else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Object::Integer(v) => Some(*v as f64),
            Object::Real(v) => Some(*v),
            _ => None,
        })
        .collect()
}

fn section(base: &CosDocument, edit: &EditSession) -> Option<Vec<u8>> {
    base.section_for(&edit.pending_edits(), &edit.trailer_edits())
        .expect("the section builds")
}

fn append(bytes: &[u8], section: Option<Vec<u8>>) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if let Some(section) = section {
        out.extend_from_slice(&section);
    }
    out
}

fn open(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the document opens")
}

fn annotations_corpus() -> Option<PathBuf> {
    let root = onionskin_corpus_testing::corpus_dir("external/verapdf/PDF_UA-1/7.18 Annotations")?;
    root.is_dir().then_some(root)
}

fn find(root: &PathBuf, name: &str) -> Option<PathBuf> {
    fn walk(dir: &PathBuf, name: &str, out: &mut Option<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, name, out);
            } else if path.file_name().is_some_and(|f| f == name) {
                *out = Some(path);
            }
        }
    }
    let mut out = None;
    walk(root, name, &mut out);
    out
}

/// A blank 200x200 page. Object 3 is the page, which is what the tests address.
fn blank_page() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>".to_vec(),
    ])
}

/// The band `text_page`'s line of text occupies, which is also the highlight's
/// rect: stated once so the fixture and the annotation cannot drift apart.
const TEXT_BAND: Rect = Rect {
    x0: 18.0,
    y0: 96.0,
    x1: 182.0,
    y1: 114.0,
};

/// A page carrying one dense line of black text, so a highlight drawn over it
/// has something to obscure.
fn text_page() -> Vec<u8> {
    let content = "BT /F1 16 Tf 0 0 0 rg 20 100 Td (HHHHHHHHHHHHHHHH) Tj ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        {
            let mut out = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            out.extend_from_slice(content.as_bytes());
            out.extend_from_slice(b"\nendstream");
            out
        },
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

/// The same page, tagged, with one structure element and a `/ParentTree`.
fn tagged_page() -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 4 0 R /MarkInfo << /Marked true >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /StructParents 0 >>"
            .to_vec(),
        b"<< /Type /StructTreeRoot /K [5 0 R] /ParentTree << /Nums [0 [5 0 R]] >> /ParentTreeNextKey 1 >>".to_vec(),
        b"<< /Type /StructElem /S /P /P 4 0 R /Pg 3 0 R /K 0 >>".to_vec(),
    ])
}

fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

// ---------------------------------------------------------------------------
// Measurements
// ---------------------------------------------------------------------------

/// A distance measured at a scale is kept as Acrobat keeps one: a `/Line`
/// with `/IT /LineDimension`, the scale in `/Measure`, the value in
/// `/Contents`, and the value drawn as the line's caption, above it.
#[test]
fn a_measurement_keeps_its_scale_and_draws_its_value() {
    use onionskin_core::measure::{Kind, Measure, Scale, Unit};
    use onionskin_core::properties::{set_properties, CommentProperties};

    let original = blank_page();
    let base = open(&original);
    let scale = Scale::new(1.0, Unit::Inch, 10.0, Unit::Foot);
    let measure = Measure::new(Kind::Distance, scale);
    let mut annotation = measure
        .annotation(&[(20.0, 60.0), (164.0, 60.0)])
        .expect("two points");
    annotation.color = Some(Color::BLACK);
    let (bytes, objref) = author(&original, &base, &annotation);

    let reopened = open(&bytes);
    let Object::Dict(dict) = reopened.resolve(&Object::Ref(objref)).expect("resolves") else {
        panic!("a dictionary");
    };
    assert_eq!(
        dict.get(b"IT")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes().to_vec()),
        Some(b"LineDimension".to_vec())
    );
    assert_eq!(dict.get(b"Cap"), Some(&Object::Bool(true)));
    assert_eq!(
        dict.get(b"Contents"),
        Some(&Object::String(b"20.00 ft".to_vec()))
    );
    let measure_dict = dict
        .get(b"Measure")
        .and_then(Object::as_dict)
        .expect("/Measure");
    assert_eq!(
        measure_dict.get(b"R"),
        Some(&Object::String(b"1 in = 10 ft".to_vec()))
    );
    assert_eq!(
        measure_dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|name| name.as_bytes().to_vec()),
        Some(b"RL".to_vec())
    );

    // The line is at y = 60, so row 140 of a 200-point page; the caption is
    // drawn above it, in the rows between.
    let ink_above = |bytes: &[u8]| {
        raster(bytes)
            .chunks(200)
            .enumerate()
            .filter(|(row, _)| (126..138).contains(row))
            .flat_map(|(_, pixels)| pixels.iter())
            .filter(|pixel| pixel[3] > 0 && pixel[0] < 128)
            .count()
    };
    assert!(ink_above(&bytes) > 40, "the value is drawn above the line");

    // A new colour draws the appearance again, caption and all.
    let mut edit = EditSession::for_base(&reopened);
    edit.transact(&reopened, "Properties", |tx| {
        set_properties(
            tx,
            objref,
            &CommentProperties {
                color: Some(Color::new(0.0, 0.0, 1.0)),
                ..CommentProperties::default()
            },
            WHEN,
        )
    })
    .expect("recoloured");
    let recoloured = append(&bytes, section(&reopened, &edit));
    assert!(ink_above(&recoloured) > 40, "the caption is still drawn");
}
