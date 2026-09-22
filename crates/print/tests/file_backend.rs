//! The print-to-file backend, read back. Every assertion parses the PDF it
//! wrote: its sheets, the XObjects each sheet draws and the matrix each is
//! drawn at, and, once per mode, the rendered pixels.

use onionskin_content::{Operation, Tokenizer};
use onionskin_core::protection::Refusal;
use onionskin_core::{
    add_annotation, Annotation, AnnotationFilter, Color, Document, Rect, Subtype,
};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};
use onionskin_print::{
    print_to_file, Duplex, NUp, Orientation, PageSelection, PaperSize, PrintError, PrintJob,
    Sizing, Subset,
};

const NOW: i64 = 1_758_000_000;

/// A black square the pixel checks look for: page coordinates.
const MARK: [f64; 4] = [20.0, 20.0, 60.0, 60.0];

// ----- sources --------------------------------------------------------

/// A document of `pages`, each `(width, height, rotate)`, each with a black
/// square at `MARK`.
fn marked(pages: &[(f64, f64, i64)]) -> Document {
    let [x0, y0, x1, y1] = MARK;
    let content = format!("0 g {x0} {y0} {} {} re f", x1 - x0, y1 - y0);
    let mut objects = vec![
        (
            ObjRef::new(1, 0),
            dict(&[("Type", Object::name("Catalog")), ("Pages", reference(2))]),
        ),
        (ObjRef::new(3, 0), stream(Dict::new(), content.as_bytes())),
    ];
    let mut kids = Vec::new();
    for (index, (width, height, rotate)) in pages.iter().enumerate() {
        let number = 4 + index as u32;
        objects.push((
            ObjRef::new(number, 0),
            dict(&[
                ("Type", Object::name("Page")),
                ("Parent", reference(2)),
                ("MediaBox", numbers(&[0.0, 0.0, *width, *height])),
                ("Rotate", Object::Integer(*rotate)),
                ("Contents", reference(3)),
            ]),
        ));
        kids.push(reference(number));
    }
    objects.push((
        ObjRef::new(2, 0),
        dict(&[
            ("Type", Object::name("Pages")),
            ("Count", Object::Integer(kids.len() as i64)),
            ("Kids", Object::Array(kids)),
        ]),
    ));
    let trailer = match dict(&[("Root", reference(1))]) {
        Object::Dict(trailer) => trailer,
        _ => unreachable!(),
    };
    let bytes = CosDocument::write_new(&objects, trailer).expect("writes");
    Document::open_bytes(bytes).expect("opens")
}

/// `hello.pdf` with a blue rectangle and a stamp on it.
fn with_markups() -> Document {
    let mut document = Document::open_path(&seed("hello.pdf")).expect("opens");
    let page = document
        .structure()
        .expect("doc")
        .page(0)
        .expect("page")
        .objref;
    document
        .edit_annotations("Markups", |tx, structure| {
            let mut square = Annotation::new(Subtype::Square, Rect::new(10.0, 10.0, 90.0, 90.0));
            square.color = Some(Color::new(0.0, 0.0, 1.0));
            square.border_width = 4.0;
            add_annotation(tx, structure, page, &square, NOW)?;
            let stamp = Annotation::new(Subtype::Stamp, Rect::new(110.0, 10.0, 190.0, 40.0));
            add_annotation(tx, structure, page, &stamp, NOW)
        })
        .expect("markups placed");
    document
}

fn dict(entries: &[(&str, Object)]) -> Object {
    let mut dict = Dict::new();
    for (key, value) in entries {
        dict.set(Name::new(key), value.clone());
    }
    Object::Dict(dict)
}

fn stream(dict: Dict, raw: &[u8]) -> Object {
    Object::Stream(Stream {
        dict,
        raw: raw.to_vec(),
    })
}

fn reference(number: u32) -> Object {
    Object::Ref(ObjRef::new(number, 0))
}

fn numbers(values: &[f64]) -> Object {
    Object::Array(values.iter().copied().map(Object::Real).collect())
}

fn letter() -> PrintJob {
    PrintJob {
        paper: PaperSize::LETTER,
        orientation: Orientation::Portrait,
        ..PrintJob::default()
    }
}

// ----- reading the output back ---------------------------------------

/// One `Do` on a sheet: the XObject's subtype and the full matrix it is
/// drawn at, the `cm`s around it multiplied out.
#[derive(Debug)]
struct Drawn {
    subtype: String,
    matrix: [f64; 6],
    object: Object,
}

struct ReadSheet {
    size: (f64, f64),
    drawn: Vec<Drawn>,
    /// Stroked rectangles: the N-up borders.
    frames: usize,
}

fn cos(bytes: &[u8]) -> CosDocument {
    CosDocument::open(Box::new(BytesSource::new(bytes.to_vec()))).expect("the output parses")
}

fn read_sheets(bytes: &[u8]) -> Vec<ReadSheet> {
    let doc = cos(bytes);
    let count = doc.page_count().expect("a page tree") as usize;
    (0..count).map(|index| read_sheet(&doc, index)).collect()
}

fn read_sheet(doc: &CosDocument, index: usize) -> ReadSheet {
    let page = doc.page(index).expect("a sheet");
    let [x0, y0, x1, y1] = page.media_box.expect("a media box");
    let contents = doc
        .resolve(page.dict.get(b"Contents").expect("contents"))
        .expect("resolves");
    let data = doc
        .decode_stream(contents.as_stream().expect("a stream"))
        .expect("decodes");
    let xobjects = page
        .resources
        .as_ref()
        .and_then(|resources| resources.get(b"XObject"))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let mut stack = vec![IDENTITY];
    let mut drawn = Vec::new();
    let mut frames = 0;
    let mut tokens = Tokenizer::new(&data);
    while let Some(operation) = tokens.next_operation() {
        match operation.operator.as_bytes() {
            b"q" => stack.push(*stack.last().expect("a state")),
            b"Q" => {
                stack.pop();
            }
            b"cm" => {
                let top = stack.last_mut().expect("a state");
                *top = multiply(matrix(&operation), *top);
            }
            b"Do" => drawn.push(resolve_do(
                doc,
                &xobjects,
                &operation,
                stack[stack.len() - 1],
            )),
            b"S" => frames += 1,
            _ => {}
        }
    }
    ReadSheet {
        size: (x1 - x0, y1 - y0),
        drawn,
        frames,
    }
}

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn matrix(operation: &Operation) -> [f64; 6] {
    operation.numbers::<6>().expect("six numbers")
}

/// `m` then `n`: a point goes through `m` first.
fn multiply(m: [f64; 6], n: [f64; 6]) -> [f64; 6] {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn apply(m: [f64; 6], (x, y): (f64, f64)) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn resolve_do(doc: &CosDocument, xobjects: &Dict, operation: &Operation, at: [f64; 6]) -> Drawn {
    let name = operation
        .operands
        .last()
        .and_then(Object::as_name)
        .expect("a name")
        .as_bytes()
        .to_vec();
    let object = doc
        .resolve(xobjects.get(&name).expect("the name is a resource"))
        .expect("resolves");
    let subtype = object
        .as_stream()
        .and_then(|stream| stream.dict.get(b"Subtype"))
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned())
        .expect("an XObject subtype");
    Drawn {
        subtype,
        matrix: at,
        object,
    }
}

/// Where the corners of the drawn page's form space land: for a form that
/// is its `/BBox`, for an image its unit square.
fn footprint(drawn: &Drawn) -> [f64; 4] {
    let [x0, y0, x1, y1] = match drawn.subtype.as_str() {
        "Image" => [0.0, 0.0, 1.0, 1.0],
        _ => {
            let bbox = drawn
                .object
                .as_stream()
                .and_then(|stream| stream.dict.get(b"BBox"))
                .and_then(Object::as_array)
                .expect("a form has a BBox");
            let n: Vec<f64> = bbox.iter().filter_map(number).collect();
            [n[0], n[1], n[2], n[3]]
        }
    };
    let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|p| apply(drawn.matrix, p));
    let xs = corners.map(|(x, _)| x);
    let ys = corners.map(|(_, y)| y);
    let min = |values: [f64; 4]| values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |values: [f64; 4]| values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    [min(xs), min(ys), max(xs), max(ys)].map(|value| (value * 1000.0).round() / 1000.0)
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

/// How many annotation appearances a printed page form draws over the page.
fn appearances(doc: &CosDocument, form: &Object) -> usize {
    form.as_stream()
        .and_then(|stream| stream.dict.get(b"Resources"))
        .and_then(|resources| doc.resolve(resources).ok())
        .and_then(|resources| resources.as_dict()?.get(b"XObject").cloned())
        .and_then(|xobjects| doc.resolve(&xobjects).ok())
        .and_then(|xobjects| {
            Some(
                xobjects
                    .as_dict()?
                    .iter()
                    .filter(|(key, _)| key.as_bytes().starts_with(b"A"))
                    .count(),
            )
        })
        .unwrap_or(0)
}

// ----- pixels ----------------------------------------------------------

const ZOOM: f32 = 2.0;

struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Pixels {
    fn of(bytes: Vec<u8>, sheet: usize) -> Pixels {
        let mut doc = Document::open_bytes(bytes).expect("the output opens");
        let render = doc.render_page_now(sheet, ZOOM).expect("renders");
        Pixels {
            width: render.raster.width(),
            height: render.raster.height(),
            rgba: render.raster.rgba().to_vec(),
        }
    }

    /// The pixel at sheet coordinates `(x, y)`, y up.
    fn at(&self, (x, y): (f64, f64)) -> [u8; 4] {
        let column = ((x * f64::from(ZOOM)) as u32).min(self.width - 1);
        let row = (((self.height as f64) - y * f64::from(ZOOM)) as u32).min(self.height - 1);
        let offset = ((row * self.width + column) * 4) as usize;
        self.rgba[offset..offset + 4]
            .try_into()
            .expect("four bytes")
    }

    fn is_ink(&self, at: (f64, f64)) -> bool {
        let [r, g, b, a] = self.at(at);
        a > 200 && r < 80 && g < 80 && b < 80
    }

    fn any(&self, [x0, y0, x1, y1]: [f64; 4], test: impl Fn([u8; 4]) -> bool) -> bool {
        let step = 0.5;
        let mut y = y0;
        while y <= y1 {
            let mut x = x0;
            while x <= x1 {
                if test(self.at((x, y))) {
                    return true;
                }
                x += step;
            }
            y += step;
        }
        false
    }
}

/// Where page point `(x, y)` of a drawn page lands on the sheet, for an
/// unrotated page drawn as a form whose BBox is its media box.
fn on_sheet(drawn: &Drawn, point: (f64, f64)) -> (f64, f64) {
    apply(drawn.matrix, point)
}

fn mark_centre() -> (f64, f64) {
    ((MARK[0] + MARK[2]) / 2.0, (MARK[1] + MARK[3]) / 2.0)
}

// ----- structure ------------------------------------------------------

#[test]
fn one_page_prints_as_one_sheet_drawing_one_form_at_the_placement() {
    let mut doc = marked(&[(612.0, 792.0, 0)]);
    let sheets = read_sheets(&print_to_file(&mut doc, &letter()).expect("prints"));
    assert_eq!(sheets.len(), 1);
    assert_eq!(sheets[0].size, (612.0, 792.0));
    assert_eq!(sheets[0].drawn.len(), 1);
    assert_eq!(sheets[0].drawn[0].subtype, "Form");
    assert_eq!(footprint(&sheets[0].drawn[0]), [0.0, 0.0, 612.0, 792.0]);
}

#[test]
fn four_up_writes_one_landscape_sheet_with_four_forms_in_reading_order() {
    let mut doc = marked(&[(612.0, 792.0, 0); 5]);
    let job = PrintJob {
        orientation: Orientation::Landscape,
        n_up: NUp {
            per_sheet: 4,
            borders: true,
            ..NUp::default()
        },
        ..letter()
    };
    let sheets = read_sheets(&print_to_file(&mut doc, &job).expect("prints"));
    assert_eq!(sheets.len(), 2, "five pages, four a sheet");
    assert_eq!(sheets[0].size, (792.0, 612.0));
    assert_eq!(sheets[0].drawn.len(), 4);
    assert_eq!(sheets[1].drawn.len(), 1);
    assert_eq!(sheets[0].frames, 4, "one border per placed page");
    let boxes: Vec<[f64; 4]> = sheets[0].drawn.iter().map(footprint).collect();
    assert!(
        boxes[0][0] < boxes[1][0] && boxes[0][1] == boxes[1][1],
        "left to right"
    );
    assert!(boxes[2][1] < boxes[0][1], "then down");
    for [x0, y0, x1, y1] in &boxes {
        assert!(
            *x1 - *x0 <= 396.0 + 1e-6 && *y1 - *y0 <= 306.0 + 1e-6,
            "fits its cell"
        );
    }
}

#[test]
fn custom_scale_draws_the_page_at_half_size_centred() {
    let mut doc = marked(&[(612.0, 792.0, 0)]);
    let job = PrintJob {
        sizing: Sizing::Custom(50),
        ..letter()
    };
    let sheets = read_sheets(&print_to_file(&mut doc, &job).expect("prints"));
    assert_eq!(footprint(&sheets[0].drawn[0]), [153.0, 198.0, 459.0, 594.0]);
}

#[test]
fn a_rotated_page_prints_turned_as_a_reader_shows_it() {
    // 200 wide and 100 high in its media box, turned a quarter: shown 100 x 200.
    let mut doc = marked(&[(200.0, 100.0, 90)]);
    let job = PrintJob {
        sizing: Sizing::ActualSize,
        ..letter()
    };
    let bytes = print_to_file(&mut doc, &job).expect("prints");
    let sheets = read_sheets(&bytes);
    let [x0, y0, x1, y1] = footprint(&sheets[0].drawn[0]);
    assert_eq!((x1 - x0, y1 - y0), (100.0, 200.0));
    // Turned clockwise, page point (x, y) shows at (y, 200 - x): the square
    // centred on (40, 40) is near the top, not the bottom.
    let pixels = Pixels::of(bytes, 0);
    assert!(pixels.is_ink((x0 + 40.0, y0 + 160.0)), "turned");
    assert!(!pixels.is_ink((x0 + 40.0, y0 + 40.0)), "not left as it was");
}

#[test]
fn duplex_with_an_odd_page_count_writes_a_blank_last_sheet() {
    let mut doc = marked(&[(612.0, 792.0, 0); 3]);
    let job = PrintJob {
        duplex: Duplex::LongEdge,
        ..letter()
    };
    let sheets = read_sheets(&print_to_file(&mut doc, &job).expect("prints"));
    assert_eq!(sheets.len(), 4);
    assert!(sheets[3].drawn.is_empty());
}

#[test]
fn a_page_used_twice_is_written_once() {
    let mut doc = marked(&[(612.0, 792.0, 0); 2]);
    let job = PrintJob {
        n_up: NUp {
            per_sheet: 2,
            ..NUp::default()
        },
        ..letter()
    };
    let bytes = print_to_file(&mut doc, &job).expect("prints");
    let output = cos(&bytes);
    let forms = output
        .reachable_from_trailer()
        .into_iter()
        .filter(|number| {
            output
                .get(*number)
                .ok()
                .and_then(|parsed| {
                    let subtype = parsed.object.as_stream()?.dict.get(b"Subtype")?.clone();
                    Some(subtype == Object::name("Form"))
                })
                .unwrap_or(false)
        })
        .count();
    // Two pages, each a wrapper form and a page form: four, never more.
    assert_eq!(forms, 4);
}

#[test]
fn a_selection_that_selects_nothing_is_not_printed() {
    let mut doc = marked(&[(612.0, 792.0, 0)]);
    let job = PrintJob {
        selection: onionskin_print::PageSelection {
            ranges: vec![(3, 4)],
            ..Default::default()
        },
        ..letter()
    };
    assert!(matches!(
        print_to_file(&mut doc, &job),
        Err(PrintError::NothingToPrint)
    ));
}

// ----- pixels, once per mode --------------------------------------------

#[test]
fn vector_printing_puts_the_page_content_where_the_transform_says() {
    let mut doc = marked(&[(200.0, 100.0, 0); 2]);
    let job = PrintJob {
        n_up: NUp {
            per_sheet: 2,
            ..NUp::default()
        },
        ..letter()
    };
    let bytes = print_to_file(&mut doc, &job).expect("prints");
    let sheets = read_sheets(&bytes);
    let pixels = Pixels::of(bytes, 0);
    for drawn in &sheets[0].drawn {
        assert!(
            pixels.is_ink(on_sheet(drawn, mark_centre())),
            "the black square is where the placement puts it"
        );
        assert!(
            !pixels.is_ink(on_sheet(drawn, (150.0, 80.0))),
            "and the rest of the page is paper"
        );
    }
}

#[test]
fn print_as_image_draws_each_page_as_one_image_that_matches_it() {
    let mut doc = marked(&[(200.0, 100.0, 0)]);
    let job = PrintJob {
        print_as_image: true,
        image_dpi: 144.0,
        sizing: Sizing::ActualSize,
        ..letter()
    };
    let bytes = print_to_file(&mut doc, &job).expect("prints");
    let sheets = read_sheets(&bytes);
    assert_eq!(sheets[0].drawn.len(), 1);
    assert_eq!(sheets[0].drawn[0].subtype, "Image");
    let [x0, y0, ..] = footprint(&sheets[0].drawn[0]);
    let pixels = Pixels::of(bytes, 0);
    let centre = mark_centre();
    assert!(pixels.is_ink((x0 + centre.0, y0 + centre.1)));
    assert!(!pixels.is_ink((x0 + 150.0, y0 + 80.0)));
}

#[test]
fn print_as_image_flattens_a_transparency_group_into_one_image() {
    // A half-transparent square in a transparency group: what would need a
    // blending printer is pixels by the time it leaves.
    let objects = vec![
        (
            ObjRef::new(1, 0),
            dict(&[("Type", Object::name("Catalog")), ("Pages", reference(2))]),
        ),
        (
            ObjRef::new(2, 0),
            dict(&[
                ("Type", Object::name("Pages")),
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![reference(3)])),
            ]),
        ),
        (
            ObjRef::new(3, 0),
            dict(&[
                ("Type", Object::name("Page")),
                ("Parent", reference(2)),
                ("MediaBox", numbers(&[0.0, 0.0, 200.0, 100.0])),
                (
                    "Group",
                    dict(&[
                        ("S", Object::name("Transparency")),
                        ("CS", Object::name("DeviceRGB")),
                    ]),
                ),
                (
                    "Resources",
                    dict(&[(
                        "ExtGState",
                        dict(&[("Half", dict(&[("ca", Object::Real(0.5))]))]),
                    )]),
                ),
                ("Contents", reference(4)),
            ]),
        ),
        (
            ObjRef::new(4, 0),
            stream(Dict::new(), b"/Half gs 0 g 20 20 40 40 re f"),
        ),
    ];
    let trailer = match dict(&[("Root", reference(1))]) {
        Object::Dict(trailer) => trailer,
        _ => unreachable!(),
    };
    let mut doc = Document::open_bytes(CosDocument::write_new(&objects, trailer).expect("writes"))
        .expect("opens");
    let job = PrintJob {
        print_as_image: true,
        image_dpi: 144.0,
        sizing: Sizing::ActualSize,
        ..letter()
    };
    let bytes = print_to_file(&mut doc, &job).expect("prints");
    let sheets = read_sheets(&bytes);
    assert_eq!(sheets[0].drawn.len(), 1);
    assert_eq!(sheets[0].drawn[0].subtype, "Image");
    let [x0, y0, ..] = footprint(&sheets[0].drawn[0]);
    let direct = doc.render_page_now(0, ZOOM).expect("renders");
    let pixels = Pixels::of(bytes, 0);
    let grey = pixels.at((x0 + 40.0, y0 + 40.0));
    let expected = {
        let (width, height) = (direct.raster.width(), direct.raster.height());
        let row = height - (40.0 * f64::from(ZOOM)) as u32;
        let column = (40.0 * f64::from(ZOOM)) as u32;
        let offset = ((row * width + column) * 4) as usize;
        let px = &direct.raster.rgba()[offset..offset + 4];
        // The direct render may be transparent where the printed one is on
        // paper: composite on white to compare.
        let alpha = u16::from(px[3]);
        (u16::from(px[0]) * alpha / 255 + (255 - alpha)) as u8
    };
    assert!(
        (i16::from(grey[0]) - i16::from(expected)).abs() <= 8,
        "printed {grey:?}, direct {expected}"
    );
    assert!(
        grey[0] > 90 && grey[0] < 170,
        "half ink is grey, not black or paper"
    );
}

// ----- Comments and Forms -------------------------------------------------

fn printed_with(filter: AnnotationFilter) -> (Vec<u8>, usize) {
    let mut doc = with_markups();
    let job = PrintJob {
        comments: filter,
        sizing: Sizing::ActualSize,
        ..letter()
    };
    let bytes = print_to_file(&mut doc, &job).expect("prints");
    let output = cos(&bytes);
    let sheet = read_sheet(&output, 0);
    let count = appearances(&output, &sheet.drawn[0].object);
    (bytes, count)
}

fn is_blue([r, g, b, a]: [u8; 4]) -> bool {
    a > 200 && b > 180 && r < 100 && g < 100
}

#[test]
fn comments_and_forms_prints_the_annotations_each_mode_names() {
    // hello.pdf is 200 x 100; at actual size on letter it is centred.
    let offset = ((612.0 - 200.0) / 2.0, (792.0 - 100.0) / 2.0);
    let square_border = [
        offset.0 + 8.0,
        offset.1 + 8.0,
        offset.0 + 14.0,
        offset.1 + 92.0,
    ];
    for (filter, printed, blue) in [
        (AnnotationFilter::DocumentAndMarkups, 2, true),
        (AnnotationFilter::DocumentAndStamps, 1, false),
        (AnnotationFilter::DocumentOnly, 0, false),
        (AnnotationFilter::FormFieldsOnly, 0, false),
    ] {
        let (bytes, count) = printed_with(filter);
        assert_eq!(count, printed, "{filter:?}: annotation appearances drawn");
        let pixels = Pixels::of(bytes, 0);
        assert_eq!(
            pixels.any(square_border, is_blue),
            blue,
            "{filter:?}: the blue rectangle"
        );
    }
}

// ----- the encrypted-source rule --------------------------------------

fn encrypted() -> Document {
    Document::open_path(&encrypted_fixture("r4-aes-128.pdf")).expect("the fixture opens")
}

#[test]
fn an_encrypted_document_prints_only_as_image() {
    let refused = print_to_file(&mut encrypted(), &letter());
    match refused {
        Err(PrintError::Refused(refusal)) => {
            assert_eq!(refusal, Refusal::EncryptedSource);
            assert_eq!(refusal.milestone(), "M6");
        }
        other => panic!(
            "vector printing an encrypted document: {:?}",
            other.map(|b| b.len())
        ),
    }

    let job = PrintJob {
        print_as_image: true,
        image_dpi: 72.0,
        ..letter()
    };
    let bytes = print_to_file(&mut encrypted(), &job).expect("prints as pictures");
    let output = cos(&bytes);
    assert!(!output.is_encrypted());
    // Nothing of the source: only the output's own page tree, its content
    // streams, and images.
    for number in output.reachable_from_trailer() {
        let object = output.get(number).expect("reads").object;
        match &object {
            Object::Dict(dict) => {
                let kind = dict.get(b"Type").cloned();
                assert!(
                    [
                        Object::name("Catalog"),
                        Object::name("Pages"),
                        Object::name("Page")
                    ]
                    .iter()
                    .any(|allowed| kind.as_ref() == Some(allowed)),
                    "object {number} is {object:?}"
                );
            }
            Object::Stream(stream) => {
                let subtype = stream.dict.get(b"Subtype").cloned();
                assert!(
                    subtype.is_none() || subtype == Some(Object::name("Image")),
                    "object {number} is a {subtype:?} stream"
                );
                if subtype.is_none() {
                    // A sheet's content: it draws images and nothing else.
                    let data = output.decode_stream(stream).expect("decodes");
                    let mut tokens = Tokenizer::new(&data);
                    while let Some(operation) = tokens.next_operation() {
                        assert!(
                            [&b"q"[..], b"Q", b"cm", b"Do"]
                                .contains(&operation.operator.as_bytes()),
                            "a sheet draws {:?}",
                            operation.operator
                        );
                    }
                }
            }
            other => panic!("object {number} is {other:?}"),
        }
    }
    let sheets = read_sheets(&bytes);
    assert!(sheets
        .iter()
        .flat_map(|s| &s.drawn)
        .all(|d| d.subtype == "Image"));
}

#[test]
fn a_filtered_print_refuses_as_the_unfiltered_one_does() {
    let job = PrintJob {
        comments: AnnotationFilter::DocumentOnly,
        ..letter()
    };
    assert!(matches!(
        print_to_file(&mut encrypted(), &job),
        Err(PrintError::Refused(Refusal::EncryptedSource))
    ));
}

#[test]
fn the_refusal_tells_the_reader_how_to_print_anyway() {
    let error = print_to_file(&mut encrypted(), &letter()).expect_err("refused");
    let message = error.to_string();
    assert!(
        message.contains("M6") && message.contains("Print as Image"),
        "{message}"
    );
}

// ----- Summarize Comments: an appendix printed after the document -----

/// A three-page document printing only page 2 as odd pages in reverse, and
/// a two-page summary: the summary's pages all print, in order, after the
/// document's one sheet, on the job's paper.
#[test]
fn an_appendix_prints_every_page_after_the_documents_sheets() {
    let mut document = marked(&[(612.0, 792.0, 0); 3]);
    let summary = marked(&[(300.0, 400.0, 0), (400.0, 300.0, 0)]);
    let job = PrintJob {
        selection: PageSelection {
            ranges: vec![(0, 2)],
            subset: Subset::Odd,
            reverse: true,
        },
        ..letter()
    };
    let printed =
        onionskin_print::print_with_appendix(&mut document, &job, summary.bytes().as_ref())
            .expect("prints");
    let sheets = read_sheets(&printed);
    // Pages 3 and 1 of the document, then both summary pages.
    assert_eq!(sheets.len(), 4);
    assert!(sheets.iter().all(|sheet| sheet.size == (612.0, 792.0)));
    let appendix_job = onionskin_print::appendix_job(&job);
    assert_eq!(appendix_job.selection, PageSelection::all());
    assert_eq!(appendix_job.paper, job.paper);
}

/// Pages per sheet apply to the appendix too: three document pages two up
/// are two sheets, and a two-page summary two up is one more.
#[test]
fn an_appendix_is_imposed_with_the_jobs_layout() {
    let mut document = marked(&[(612.0, 792.0, 0); 3]);
    let summary = marked(&[(612.0, 792.0, 0); 2]);
    let job = PrintJob {
        n_up: NUp {
            per_sheet: 2,
            ..NUp::default()
        },
        ..letter()
    };
    let printed =
        onionskin_print::print_with_appendix(&mut document, &job, summary.bytes().as_ref())
            .expect("prints");
    assert_eq!(read_sheets(&printed).len(), 3);
}

#[test]
fn concatenating_keeps_both_documents_pages_in_order() {
    let first = marked(&[(100.0, 100.0, 0)]);
    let second = marked(&[(200.0, 200.0, 0), (300.0, 300.0, 0)]);
    let joined = onionskin_print::concatenate(
        first.bytes().as_ref().clone(),
        second.bytes().as_ref().clone(),
    )
    .expect("joins");
    let sizes: Vec<_> = read_sheets(&joined)
        .iter()
        .map(|sheet| sheet.size)
        .collect();
    assert_eq!(sizes, [(100.0, 100.0), (200.0, 200.0), (300.0, 300.0)]);
    assert!(onionskin_print::concatenate(b"not a pdf".to_vec(), Vec::new()).is_err());
}

// ----- Booklet and Poster (M4) ---------------------------------------

/// Poster at 200%: four Letter tiles, each drawing the page clipped to its
/// tile, so no tile shows past its own edge.
#[test]
fn a_poster_prints_one_clipped_tile_per_sheet() {
    let mut document = marked(&[(612.0, 792.0, 0)]);
    let job = PrintJob {
        handling: onionskin_print::Handling::Poster(onionskin_print::Poster {
            scale: 200,
            overlap: 0.0,
            cut_marks: false,
        }),
        ..letter()
    };
    let printed = print_to_file(&mut document, &job).expect("prints");
    let sheets = read_sheets(&printed);
    assert_eq!(sheets.len(), 4);
    let doc = cos(&printed);
    for index in 0..4 {
        let page = doc.page(index).expect("a sheet");
        let contents = doc
            .resolve(page.dict.get(b"Contents").expect("contents"))
            .expect("resolves");
        let data = doc
            .decode_stream(contents.as_stream().expect("a stream"))
            .expect("decodes");
        let text = String::from_utf8_lossy(&data).into_owned();
        assert!(text.contains("0 0 612 792 re W n"), "sheet {index}: {text}");
    }
    assert!(sheets.iter().all(|sheet| sheet.drawn.len() == 1));
}

/// Booklet: eight pages print as four landscape sides of two pages each.
#[test]
fn a_booklet_prints_two_pages_to_each_landscape_side() {
    let mut document = marked(&[(612.0, 792.0, 0); 8]);
    let job = PrintJob {
        handling: onionskin_print::Handling::Booklet(onionskin_print::Booklet::default()),
        ..letter()
    };
    let sheets = read_sheets(&print_to_file(&mut document, &job).expect("prints"));
    assert_eq!(sheets.len(), 4);
    assert!(sheets
        .iter()
        .all(|sheet| sheet.size == (792.0, 612.0) && sheet.drawn.len() == 2));
}
