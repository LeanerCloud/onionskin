//! Print to file: a PDF whose pages are the composed sheets. The backend CI
//! can test, and what Save as PDF on a print dialog produces.
//!
//! Each page is placed as a Form XObject made from the source page and its
//! printing annotations (`core::pages::import_page_for_print`), at its
//! placement's transform composed with the page's `/Rotate`. With Print as
//! Image on, each page is rendered instead and placed as one image, and
//! nothing of the source's own objects reaches the output.

use std::collections::BTreeMap;
use std::sync::Arc;

use onionskin_core::pages::import_page_for_print;
use onionskin_core::{Document, ObjRef, PageIndex};
use onionskin_cos::{Dict, Document as CosDocument, Name, Object, Stream};

use super::{PrintBackend, PrintError};
use crate::impose::{impose, PageSize};
use crate::job::PrintJob;
use crate::sheet::Sheet;

/// Print `doc` as it stands, edits included, to a new PDF's bytes: the
/// job's Comments and Forms filter applied, its pages imposed, its sheets
/// written. What Save as PDF on the print dialog calls.
pub fn print_to_file(doc: &mut Document, job: &PrintJob) -> Result<Vec<u8>, PrintError> {
    let mut backend = FileBackend::new(doc.preview_bytes(job.comments)?)?;
    let sheets = impose(job, &backend.page_sizes()?);
    backend.print(job, &sheets)?;
    Ok(backend
        .output
        .take()
        .expect("a print that succeeded wrote a file"))
}

/// Writes sheets into a new PDF, kept until asked for.
pub struct FileBackend {
    source: Document,
    output: Option<Vec<u8>>,
}

impl FileBackend {
    /// A backend printing `bytes`: the document as it should appear on
    /// paper, which is `core`'s preview bytes with the job's Comments and
    /// Forms filter applied.
    pub fn new(bytes: Arc<Vec<u8>>) -> Result<Self, PrintError> {
        Ok(FileBackend {
            source: Document::open_bytes(bytes.as_ref().clone())?,
            output: None,
        })
    }

    /// Every page's size as displayed, which is what imposition takes.
    pub fn page_sizes(&mut self) -> Result<Vec<PageSize>, PrintError> {
        (0..self.source.page_count())
            .map(|page| Ok(self.source.page_geometry(page)?.render_size))
            .collect()
    }

    /// The file the last print wrote.
    pub fn output(&self) -> Option<&[u8]> {
        self.output.as_deref()
    }
}

impl PrintBackend for FileBackend {
    fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<(), PrintError> {
        if sheets.iter().all(|sheet| sheet.placements.is_empty()) {
            return Err(PrintError::NothingToPrint);
        }
        if !job.print_as_image {
            if let Some(refusal) = self.source.read_out_refusal() {
                return Err(PrintError::Refused(refusal));
            }
        }
        let drawings = self.drawings(job, sheets)?;
        self.output = Some(write(sheets, &drawings)?);
        Ok(())
    }
}

/// How one source page is drawn on a sheet: an XObject in the output, and
/// the matrix that maps the page's displayed box onto the XObject's space.
#[derive(Debug, Clone)]
struct Drawing {
    object: Object,
    /// Applied after the placement: from the displayed box to the XObject.
    inner: [f64; 6],
}

/// One source page ready to place: the objects it needs, numbered as in
/// the document they came from, and how it is drawn.
type Imported = (Vec<(u32, Object)>, Drawing);

impl FileBackend {
    /// Each source page the sheets use, drawn once, as the objects it needs.
    fn drawings(
        &mut self,
        job: &PrintJob,
        sheets: &[Sheet],
    ) -> Result<BTreeMap<PageIndex, Imported>, PrintError> {
        let mut pages: Vec<PageIndex> = sheets
            .iter()
            .flat_map(|sheet| sheet.placements.iter().map(|p| p.source))
            .collect();
        pages.sort_unstable();
        pages.dedup();
        let mut out = BTreeMap::new();
        for page in pages {
            let drawn = if job.print_as_image {
                self.raster(page, job.image_dpi)?
            } else {
                self.vector(page)?
            };
            out.insert(page, drawn);
        }
        Ok(out)
    }

    /// The page as pixels, at `dpi`, on white.
    fn raster(&mut self, page: PageIndex, dpi: f32) -> Result<Imported, PrintError> {
        let (width, height) = self.source.page_geometry(page)?.render_size;
        let render = self.source.render_page_now(page, dpi / 72.0)?;
        let raster = &render.raster;
        let rgb: Vec<u8> = raster
            .rgba()
            .chunks_exact(4)
            .flat_map(|px| {
                let alpha = u16::from(px[3]);
                // Premultiplied or not, a transparent pixel prints as paper.
                [0, 1, 2].map(|i| (u16::from(px[i]) * alpha / 255 + (255 - alpha)) as u8)
            })
            .collect();
        let mut dict = Dict::new();
        dict.set(Name::new("Type"), Object::name("XObject"));
        dict.set(Name::new("Subtype"), Object::name("Image"));
        dict.set(
            Name::new("Width"),
            Object::Integer(i64::from(raster.width())),
        );
        dict.set(
            Name::new("Height"),
            Object::Integer(i64::from(raster.height())),
        );
        dict.set(Name::new("ColorSpace"), Object::name("DeviceRGB"));
        dict.set(Name::new("BitsPerComponent"), Object::Integer(8));
        dict.set(Name::new("Filter"), Object::name("FlateDecode"));
        let raw = onionskin_cos::flate_encode(&rgb);
        // Numbered later, when the output is laid out.
        Ok((
            vec![(0, Object::Stream(Stream { dict, raw }))],
            Drawing {
                object: Object::Null,
                inner: [width, 0.0, 0.0, height, 0.0, 0.0],
            },
        ))
    }

    /// The page as vectors: its content and printing annotations as one
    /// form, turned by its `/Rotate`.
    fn vector(&mut self, page: PageIndex) -> Result<Imported, PrintError> {
        let rotate = self.source.page_geometry(page)?.rotate;
        let source = self.source.structure()?;
        let mut scratch = Document::open_bytes(blank())?;
        let (form, bbox) = scratch.edit_document("Print", |tx| {
            let (form, bbox) = import_page_for_print(tx, source, page)?;
            // A transaction drops what nothing refers to, so the scratch
            // page draws the form: that is what keeps it.
            tx.put_object(3, 0, blank_page(Some(form)))?;
            Ok((form, bbox))
        })?;
        let objects = closure(scratch.structure()?, form)?;
        Ok((
            objects,
            Drawing {
                object: Object::Ref(form),
                inner: rotation(bbox, rotate),
            },
        ))
    }
}

/// Every object reachable from `root` in `doc`, with its number there.
fn closure(doc: &CosDocument, root: ObjRef) -> Result<Vec<(u32, Object)>, PrintError> {
    doc.reachable_from(vec![Object::Ref(root)])
        .into_iter()
        .map(|number| Ok((number, doc.get(number)?.object)))
        .collect()
}

/// The matrix from a page form's space (its `/BBox`) to the page's box as
/// displayed, for `/Rotate` of 0, 90, 180 or 270 degrees clockwise.
pub(crate) fn rotation(bbox: [f64; 4], rotate: i32) -> [f64; 6] {
    let [x0, y0, x1, y1] = bbox;
    match rotate.rem_euclid(360) {
        90 => [0.0, -1.0, 1.0, 0.0, -y0, x1],
        180 => [-1.0, 0.0, 0.0, -1.0, x1, y1],
        270 => [0.0, 1.0, -1.0, 0.0, y1, -x0],
        _ => [1.0, 0.0, 0.0, 1.0, -x0, -y0],
    }
}

/// A one-page, empty document: the scratch destination a page form is
/// imported into before its objects are carried into the output.
fn blank() -> Vec<u8> {
    let objects = [
        (
            ObjRef::new(1, 0),
            Object::Dict(dict(&[
                ("Type", Object::name("Catalog")),
                ("Pages", Object::Ref(ObjRef::new(2, 0))),
            ])),
        ),
        (
            ObjRef::new(2, 0),
            Object::Dict(dict(&[
                ("Type", Object::name("Pages")),
                ("Kids", Object::Array(vec![Object::Ref(ObjRef::new(3, 0))])),
                ("Count", Object::Integer(1)),
            ])),
        ),
        (ObjRef::new(3, 0), blank_page(None)),
    ];
    CosDocument::write_new(&objects, dict(&[("Root", Object::Ref(ObjRef::new(1, 0)))]))
        .expect("a fixed three-object document writes")
}

/// The scratch document's page, holding `form` in its resources once one
/// is imported.
fn blank_page(form: Option<ObjRef>) -> Object {
    let mut page = dict(&[
        ("Type", Object::name("Page")),
        ("Parent", Object::Ref(ObjRef::new(2, 0))),
        ("MediaBox", number_array(&[0.0, 0.0, 612.0, 792.0])),
    ]);
    if let Some(form) = form {
        let xobjects = dict(&[("Printed", Object::Ref(form))]);
        page.set(
            Name::new("Resources"),
            Object::Dict(dict(&[("XObject", Object::Dict(xobjects))])),
        );
    }
    Object::Dict(page)
}

fn dict(entries: &[(&str, Object)]) -> Dict {
    let mut dict = Dict::new();
    for (key, value) in entries {
        dict.set(Name::new(key), value.clone());
    }
    dict
}

fn number_array(values: &[f64]) -> Object {
    Object::Array(values.iter().copied().map(Object::Real).collect())
}

/// Lay out the output: catalog, page tree, one page per sheet, and every
/// drawing's objects renumbered so no two sources collide.
fn write(
    sheets: &[Sheet],
    drawings: &BTreeMap<PageIndex, Imported>,
) -> Result<Vec<u8>, PrintError> {
    let mut objects: Vec<(ObjRef, Object)> = Vec::new();
    let mut next = 3u32;
    let mut take = || {
        next += 1;
        next
    };
    // Each drawing's objects, moved to fresh numbers.
    let mut placed: BTreeMap<PageIndex, Drawing> = BTreeMap::new();
    for (page, (source_objects, drawing)) in drawings {
        let map: BTreeMap<u32, u32> = source_objects
            .iter()
            .map(|(number, _)| (*number, take()))
            .collect();
        for (number, object) in source_objects {
            objects.push((ObjRef::new(map[number], 0), renumber(object.clone(), &map)));
        }
        let object = match &drawing.object {
            Object::Ref(root) => Object::Ref(ObjRef::new(map[&root.number], 0)),
            // A raster is its only object.
            _ => Object::Ref(ObjRef::new(map[&0], 0)),
        };
        placed.insert(
            *page,
            Drawing {
                object,
                inner: drawing.inner,
            },
        );
    }
    let mut kids = Vec::new();
    for sheet in sheets {
        let mut content = String::new();
        let mut xobjects = Dict::new();
        for (slot, placement) in sheet.placements.iter().enumerate() {
            let drawing = &placed[&placement.source];
            let name = format!("P{slot}");
            let [a, b, c, d, e, f] = placement.transform;
            let [g, h, i, j, k, l] = drawing.inner;
            content.push_str(&format!(
                "q {a} {b} {c} {d} {e} {f} cm {g} {h} {i} {j} {k} {l} cm /{name} Do Q\n"
            ));
            xobjects.set(Name::new(&name), drawing.object.clone());
        }
        for [x0, y0, x1, y1] in &sheet.frames {
            content.push_str(&format!(
                "q 0 G 0.5 w {x0} {y0} {} {} re S Q\n",
                x1 - x0,
                y1 - y0
            ));
        }
        let contents = take();
        objects.push((
            ObjRef::new(contents, 0),
            Object::Stream(Stream {
                dict: Dict::new(),
                raw: content.into_bytes(),
            }),
        ));
        let page = take();
        objects.push((
            ObjRef::new(page, 0),
            Object::Dict(dict(&[
                ("Type", Object::name("Page")),
                ("Parent", Object::Ref(ObjRef::new(2, 0))),
                (
                    "MediaBox",
                    number_array(&[0.0, 0.0, sheet.width, sheet.height]),
                ),
                (
                    "Resources",
                    Object::Dict(dict(&[("XObject", Object::Dict(xobjects))])),
                ),
                ("Contents", Object::Ref(ObjRef::new(contents, 0))),
            ])),
        ));
        kids.push(Object::Ref(ObjRef::new(page, 0)));
    }
    objects.push((
        ObjRef::new(1, 0),
        Object::Dict(dict(&[
            ("Type", Object::name("Catalog")),
            ("Pages", Object::Ref(ObjRef::new(2, 0))),
        ])),
    ));
    objects.push((
        ObjRef::new(2, 0),
        Object::Dict(dict(&[
            ("Type", Object::name("Pages")),
            ("Count", Object::Integer(kids.len() as i64)),
            ("Kids", Object::Array(kids)),
        ])),
    ));
    Ok(CosDocument::write_new(
        &objects,
        dict(&[("Root", Object::Ref(ObjRef::new(1, 0)))]),
    )?)
}

/// `object` with every reference moved through `map`. A reference `map`
/// does not know stays as it was: it pointed nowhere in the drawing either.
fn renumber(object: Object, map: &BTreeMap<u32, u32>) -> Object {
    match object {
        Object::Ref(objref) => match map.get(&objref.number) {
            Some(number) => Object::Ref(ObjRef::new(*number, 0)),
            None => Object::Null,
        },
        Object::Array(items) => {
            Object::Array(items.into_iter().map(|item| renumber(item, map)).collect())
        }
        Object::Dict(dict) => Object::Dict(renumber_dict(dict, map)),
        Object::Stream(stream) => Object::Stream(Stream {
            dict: renumber_dict(stream.dict, map),
            raw: stream.raw,
        }),
        other => other,
    }
}

fn renumber_dict(dict: Dict, map: &BTreeMap<u32, u32>) -> Dict {
    let mut out = Dict::new();
    for (key, value) in dict.iter() {
        out.set(key.clone(), renumber(value.clone(), map));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply `m` to a point.
    fn apply(m: [f64; 6], (x, y): (f64, f64)) -> (f64, f64) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    #[test]
    fn each_rotation_puts_the_page_top_left_where_a_reader_shows_it() {
        // A 100 x 200 box at (10, 20). Its top-left corner is (10, 220).
        let bbox = [10.0, 20.0, 110.0, 220.0];
        assert_eq!(apply(rotation(bbox, 0), (10.0, 220.0)), (0.0, 200.0));
        // Turned a quarter clockwise: displayed 200 wide, 100 high, and the
        // old top-left is now the top-right.
        assert_eq!(apply(rotation(bbox, 90), (10.0, 220.0)), (200.0, 100.0));
        assert_eq!(apply(rotation(bbox, 180), (10.0, 220.0)), (100.0, 0.0));
        assert_eq!(apply(rotation(bbox, 270), (10.0, 220.0)), (0.0, 0.0));
        assert_eq!(apply(rotation(bbox, -90), (10.0, 220.0)), (0.0, 0.0));
    }

    #[test]
    fn renumbering_moves_every_reference_and_nulls_an_unknown_one() {
        let map = BTreeMap::from([(5, 40)]);
        let object = Object::Array(vec![
            Object::Ref(ObjRef::new(5, 0)),
            Object::Ref(ObjRef::new(6, 0)),
        ]);
        assert_eq!(
            renumber(object, &map),
            Object::Array(vec![Object::Ref(ObjRef::new(40, 0)), Object::Null])
        );
    }
}
