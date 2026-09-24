//! Page marks: watermarks, backgrounds, headers and footers, and Bates
//! numbers. Each is drawn on the page itself, not in an annotation, the way
//! Acrobat's Edit PDF toolset draws them, and each can be updated or removed
//! later without touching anything else on the page.
//!
//! # What one mark is in the file
//!
//! - A **Form XObject** holding the drawing, in the page's *shown* space:
//!   its origin is the bottom-left corner of the crop box as the page is
//!   displayed, x to the right and y up, whatever `/Rotate` says. The form's
//!   `/Matrix` takes that space onto the page, so a header written at the
//!   top of the shown page is at the top of a turned page too.
//! - The form carries Acrobat's `/PieceInfo /ADBE_CompoundType` with the
//!   `/Private` name Acrobat uses for the kind, so Acrobat's own Update and
//!   Remove recognise it, and `/OnionskinMark` naming the kind exactly.
//! - A **content stream of its own** in the page's `/Contents`, drawing the
//!   form inside an `/Artifact` marked-content sequence. An artifact is not
//!   part of the structure tree, so a tagged document stays valid.
//! - A mark drawn over the page is appended after the page's own content,
//!   which is first wrapped in a `q` ... `Q` pair of guard streams, so
//!   graphics state the page leaves behind cannot move or recolour the mark.
//!   A background, or a watermark asked to go behind, is prepended.
//!
//! # Removing
//!
//! A content stream is a mark of a kind if it carries `/OnionskinMark`, or
//! if it does nothing but draw forms whose `/PieceInfo` names that kind: the
//! shape Acrobat's own marks have. Removing drops those streams and the
//! form names only they used; the guards go when no drawn-over mark is left.

mod contents;
mod recognise;

use onionskin_cos::{Dict, Name, Object, Stream};

use super::boxes::{media_box, rotation};
use super::ops::{check_indices, leaves};
use super::rewrite::{dict_at, resolve};
use crate::edit::Transaction;
use crate::{PageIndex, Result};

pub use recognise::{mark_settings, marked_pages, page_marks};

/// What a page mark is. Headers and footers are one mark, as in Acrobat;
/// Bates numbers are written the same way but kept apart, so removing one
/// leaves the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkKind {
    Watermark,
    Background,
    HeaderFooter,
    Bates,
}

impl MarkKind {
    pub const ALL: [MarkKind; 4] = [
        MarkKind::Watermark,
        MarkKind::Background,
        MarkKind::HeaderFooter,
        MarkKind::Bates,
    ];

    /// The name under `/OnionskinMark`.
    fn marker(self) -> &'static str {
        match self {
            MarkKind::Watermark => "Watermark",
            MarkKind::Background => "Background",
            MarkKind::HeaderFooter => "HeaderFooter",
            MarkKind::Bates => "Bates",
        }
    }

    /// Acrobat's `/PieceInfo /ADBE_CompoundType /Private` name for the kind.
    /// Acrobat writes Bates numbers as a header or footer.
    fn private(self) -> &'static str {
        match self {
            MarkKind::Watermark => "Watermark",
            MarkKind::Background => "Background",
            MarkKind::HeaderFooter | MarkKind::Bates => "HeaderFooter",
        }
    }

    /// The artifact's properties, PDF 2.0 table 363.
    fn artifact(self) -> &'static str {
        match self {
            MarkKind::Watermark => "<< /Type /Pagination /Subtype /Watermark >>",
            MarkKind::Background => "<< /Type /Background >>",
            MarkKind::HeaderFooter | MarkKind::Bates => "<< /Type /Pagination >>",
        }
    }

    /// Whether the mark is drawn behind the page's content.
    fn behind(self) -> bool {
        self == MarkKind::Background
    }
}

/// One page's drawing, in its shown space (see the module documentation).
#[derive(Debug, Clone, PartialEq)]
pub struct PageMark {
    /// Content-stream operators.
    pub content: Vec<u8>,
    /// What the operators name: fonts, graphics states, forms.
    pub resources: Dict,
    /// Drawn behind the page's content rather than over it. A background
    /// always is; a watermark may be.
    pub behind: bool,
    /// What made the mark, in whatever form the caller reads back, so Update
    /// can open on it. Kept on the form as `/OnionskinSettings`; empty keeps
    /// nothing.
    pub settings: Vec<u8>,
}

/// Draw `marks` of `kind`, one per page. With `replace`, the pages' marks of
/// that kind go first, which is Acrobat's Update and its "Replace Existing".
pub fn add_page_marks(
    tx: &mut Transaction<'_>,
    kind: MarkKind,
    marks: &[(PageIndex, PageMark)],
    replace: bool,
) -> Result<()> {
    let leaves = leaves(tx)?;
    let pages = marks.iter().map(|(page, _)| *page).collect();
    check_indices(&pages, leaves.len())?;
    for (page, mark) in marks {
        let leaf = &leaves[*page];
        let mut dict = dict_at(tx, leaf.objref)?;
        let mut resources = page_resources(tx, leaf.inherited.resources.as_ref())?;
        let mut parts = contents::parts(tx, dict.get(b"Contents"))?;
        if replace {
            recognise::drop_marks(tx, kind, &mut parts, &mut resources)?;
        }
        let shown = shown_space(tx, leaf)?;
        let form = tx.reserve();
        tx.put_object(form, 0, Object::Stream(form_xobject(kind, mark, shown)))?;
        let name = unused_name(&resources);
        add_form(
            tx,
            &mut resources,
            &name,
            onionskin_cos::ObjRef::new(form, 0),
        )?;
        let draw = contents::stream(tx, kind.marker(), invocation(kind, &name))?;
        contents::insert(tx, &mut parts, draw, kind.behind() || mark.behind)?;
        dict.set(Name::new("Resources"), Object::Dict(resources));
        dict.set(Name::new("Contents"), Object::Array(parts));
        tx.put_object(
            leaf.objref.number,
            leaf.objref.generation,
            Object::Dict(dict),
        )?;
    }
    Ok(())
}

/// Remove every mark of `kind` from `pages`, Acrobat's own included. How
/// many pages had one.
pub fn remove_page_marks(
    tx: &mut Transaction<'_>,
    kind: MarkKind,
    pages: &[PageIndex],
) -> Result<usize> {
    let leaves = leaves(tx)?;
    let set = pages.iter().copied().collect();
    check_indices(&set, leaves.len())?;
    let mut changed = 0;
    for index in set {
        let leaf = &leaves[index];
        let mut dict = dict_at(tx, leaf.objref)?;
        let mut resources = page_resources(tx, leaf.inherited.resources.as_ref())?;
        let mut parts = contents::parts(tx, dict.get(b"Contents"))?;
        if !recognise::drop_marks(tx, kind, &mut parts, &mut resources)? {
            continue;
        }
        changed += 1;
        dict.set(Name::new("Resources"), Object::Dict(resources));
        dict.set(Name::new("Contents"), Object::Array(parts));
        tx.put_object(
            leaf.objref.number,
            leaf.objref.generation,
            Object::Dict(dict),
        )?;
    }
    Ok(changed)
}

/// The page's resources as a dictionary it can own: what it inherits,
/// resolved, and copied so a dictionary other pages share is not changed.
fn page_resources(tx: &Transaction<'_>, inherited: Option<&Object>) -> Result<Dict> {
    Ok(match resolve(tx, inherited)? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    })
}

/// Name `form` `name` in the `/XObject` sub-dictionary, which is resolved
/// into `resources` so a shared one is not changed.
fn add_form(
    tx: &Transaction<'_>,
    resources: &mut Dict,
    name: &str,
    form: onionskin_cos::ObjRef,
) -> Result<()> {
    let mut forms = match resolve(tx, resources.get(b"XObject"))? {
        Some(Object::Dict(dict)) => dict,
        _ => Dict::new(),
    };
    forms.set(Name::new(name), Object::Ref(form));
    resources.set(Name::new("XObject"), Object::Dict(forms));
    Ok(())
}

/// `OSMk0`, `OSMk1`, ...: the first the page's forms do not already use.
fn unused_name(resources: &Dict) -> String {
    let taken = |name: &str| match resources.get(b"XObject") {
        Some(Object::Dict(forms)) => forms.get(name.as_bytes()).is_some(),
        _ => false,
    };
    (0..)
        .map(|index| format!("OSMk{index}"))
        .find(|name| !taken(name))
        .expect("an unused name")
}

/// Draw form `name` as an artifact of `kind`.
fn invocation(kind: MarkKind, name: &str) -> Vec<u8> {
    format!("q\n/Artifact {} BDC\n/{name} Do\nEMC\nQ\n", kind.artifact()).into_bytes()
}

/// The page's shown space: its size as displayed, and the matrix from that
/// space onto the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShownSpace {
    pub width: f64,
    pub height: f64,
    pub matrix: [f64; 6],
}

fn shown_space(tx: &Transaction<'_>, leaf: &super::inherit::Leaf) -> Result<ShownSpace> {
    let media = media_box(resolve(tx, leaf.inherited.media_box.as_ref())?);
    let crop = match resolve(tx, leaf.inherited.crop_box.as_ref())? {
        Some(object @ Object::Array(_)) => media_box(Some(object)),
        _ => media,
    };
    let rotate = rotation(resolve(tx, leaf.inherited.rotate.as_ref())?);
    Ok(shown(crop, rotate))
}

/// The shown space of a page whose crop box is `crop`, turned `rotate`
/// degrees clockwise.
pub fn shown(crop: [f64; 4], rotate: i32) -> ShownSpace {
    let [x0, y0, x1, y1] = crop;
    let (width, height) = (x1 - x0, y1 - y0);
    let (width, height, matrix) = match rotate {
        90 => (height, width, [0.0, 1.0, -1.0, 0.0, x1, y0]),
        180 => (width, height, [-1.0, 0.0, 0.0, -1.0, x1, y1]),
        270 => (height, width, [0.0, -1.0, 1.0, 0.0, x0, y1]),
        _ => (width, height, [1.0, 0.0, 0.0, 1.0, x0, y0]),
    };
    ShownSpace {
        width,
        height,
        matrix,
    }
}

fn numbers(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|value| Object::Real(*value)).collect())
}

fn form_xobject(kind: MarkKind, mark: &PageMark, space: ShownSpace) -> Stream {
    let mut dict = Dict::new();
    dict.set(Name::new("Type"), Object::name("XObject"));
    dict.set(Name::new("Subtype"), Object::name("Form"));
    dict.set(
        Name::new("BBox"),
        numbers(&[0.0, 0.0, space.width, space.height]),
    );
    dict.set(Name::new("Matrix"), numbers(&space.matrix));
    dict.set(Name::new("Resources"), Object::Dict(mark.resources.clone()));
    let mut compound = Dict::new();
    compound.set(Name::new("Private"), Object::name(kind.private()));
    let mut piece = Dict::new();
    piece.set(Name::new("ADBE_CompoundType"), Object::Dict(compound));
    dict.set(Name::new("PieceInfo"), Object::Dict(piece));
    dict.set(Name::new("OnionskinMark"), Object::name(kind.marker()));
    if !mark.settings.is_empty() {
        dict.set(
            Name::new("OnionskinSettings"),
            Object::String(mark.settings.clone()),
        );
    }
    dict.set(
        Name::new("Length"),
        Object::Integer(mark.content.len() as i64),
    );
    Stream {
        dict,
        raw: mark.content.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CROP: [f64; 4] = [10.0, 20.0, 110.0, 220.0];

    /// Where a shown point lands on the page.
    fn apply(space: ShownSpace, (u, v): (f64, f64)) -> (f64, f64) {
        let [a, b, c, d, e, f] = space.matrix;
        (a * u + c * v + e, b * u + d * v + f)
    }

    /// The shown bottom-left and top-right corners land on the corners of
    /// the crop box that are bottom-left and top-right on screen.
    #[test]
    fn the_shown_space_covers_the_crop_box_at_every_rotation() {
        let cases = [
            (0, (100.0, 200.0), (10.0, 20.0), (110.0, 220.0)),
            (90, (200.0, 100.0), (110.0, 20.0), (10.0, 220.0)),
            (180, (100.0, 200.0), (110.0, 220.0), (10.0, 20.0)),
            (270, (200.0, 100.0), (10.0, 220.0), (110.0, 20.0)),
        ];
        for (rotate, size, bottom_left, top_right) in cases {
            let space = shown(CROP, rotate);
            assert_eq!((space.width, space.height), size, "{rotate}");
            assert_eq!(apply(space, (0.0, 0.0)), bottom_left, "{rotate}");
            assert_eq!(apply(space, size), top_right, "{rotate}");
        }
    }

    #[test]
    fn each_kind_has_its_names() {
        let names: Vec<_> = MarkKind::ALL
            .iter()
            .map(|kind| (kind.marker(), kind.private(), kind.behind()))
            .collect();
        assert_eq!(
            names,
            [
                ("Watermark", "Watermark", false),
                ("Background", "Background", true),
                ("HeaderFooter", "HeaderFooter", false),
                ("Bates", "HeaderFooter", false),
            ]
        );
        assert!(MarkKind::Background.artifact().contains("/Background"));
        assert!(invocation(MarkKind::Watermark, "OSMk3")
            .windows(9)
            .any(|window| window == b"/OSMk3 Do"));
    }

    #[test]
    fn a_new_form_name_skips_the_ones_taken() {
        let mut forms = Dict::new();
        forms.set(Name::new("OSMk0"), Object::Null);
        forms.set(Name::new("OSMk1"), Object::Null);
        let mut resources = Dict::new();
        assert_eq!(unused_name(&resources), "OSMk0");
        resources.set(Name::new("XObject"), Object::Dict(forms));
        assert_eq!(unused_name(&resources), "OSMk2");
    }
}
