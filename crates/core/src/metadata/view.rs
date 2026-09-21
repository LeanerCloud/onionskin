//! Initial View: how the document opens.
//!
//! The catalog's `/PageLayout` and `/PageMode`, and an `/OpenAction` that is
//! an explicit destination: the page it opens at and how that page is fitted.
//! A viewer honours what this module reads; Onionskin's shell does, on open.

use onionskin_cos::{Dict, Document as CosDocument, Name, ObjRef, Object};

use crate::edit::Transaction;
use crate::pages::{dict_at, resolve};
use crate::{Error, Result};

/// `/PageLayout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLayout {
    SinglePage,
    OneColumn,
    TwoColumnLeft,
    TwoColumnRight,
    TwoPageLeft,
    TwoPageRight,
}

impl PageLayout {
    pub const ALL: [Self; 6] = [
        Self::SinglePage,
        Self::OneColumn,
        Self::TwoColumnLeft,
        Self::TwoColumnRight,
        Self::TwoPageLeft,
        Self::TwoPageRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SinglePage => "SinglePage",
            Self::OneColumn => "OneColumn",
            Self::TwoColumnLeft => "TwoColumnLeft",
            Self::TwoColumnRight => "TwoColumnRight",
            Self::TwoPageLeft => "TwoPageLeft",
            Self::TwoPageRight => "TwoPageRight",
        }
    }
}

/// `/PageMode`: which pane the document opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageMode {
    UseNone,
    UseOutlines,
    UseThumbs,
    UseAttachments,
    UseOC,
    FullScreen,
}

impl PageMode {
    pub const ALL: [Self; 6] = [
        Self::UseNone,
        Self::UseOutlines,
        Self::UseThumbs,
        Self::UseAttachments,
        Self::UseOC,
        Self::FullScreen,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::UseNone => "UseNone",
            Self::UseOutlines => "UseOutlines",
            Self::UseThumbs => "UseThumbs",
            Self::UseAttachments => "UseAttachments",
            Self::UseOC => "UseOC",
            Self::FullScreen => "FullScreen",
        }
    }
}

/// How the opening page is fitted: an explicit destination's kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenFit {
    /// `/XYZ` with no zoom: whatever the viewer would do.
    Default,
    /// `/Fit`.
    Page,
    /// `/FitH`.
    Width,
    /// `/FitV`.
    Height,
    /// `/FitB`.
    Visible,
    /// `/XYZ` at this zoom, where 1.0 is 100%.
    Zoom(f64),
}

/// What the Initial View tab sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InitialView {
    pub layout: Option<PageLayout>,
    pub mode: Option<PageMode>,
    /// The page the document opens at, from zero. `None`: no `/OpenAction`.
    pub page: Option<usize>,
    pub fit: OpenFit,
}

impl Default for InitialView {
    fn default() -> Self {
        Self {
            layout: None,
            mode: None,
            page: None,
            fit: OpenFit::Default,
        }
    }
}

/// Read the catalog's initial view. Anything this cannot interpret - a
/// JavaScript open action, an unknown layout name - reads as unset rather
/// than as an error: it is how the document opens, not whether it does.
pub fn read_initial_view(doc: &CosDocument) -> Result<InitialView> {
    let catalog = doc.catalog()?;
    let name = |key: &[u8]| {
        catalog
            .get(key)
            .and_then(|value| doc.resolve(value).ok())
            .and_then(|value| value.as_name().map(|name| name.as_bytes().to_vec()))
    };
    let layout = name(b"PageLayout").and_then(|value| {
        PageLayout::ALL
            .into_iter()
            .find(|layout| layout.name().as_bytes() == value)
    });
    let mode = name(b"PageMode").and_then(|value| {
        PageMode::ALL
            .into_iter()
            .find(|mode| mode.name().as_bytes() == value)
    });
    let (page, fit) = match open_destination(doc, &catalog) {
        Some(destination) => read_destination(doc, &destination),
        None => (None, OpenFit::Default),
    };
    Ok(InitialView {
        layout,
        mode,
        page,
        fit,
    })
}

/// `/OpenAction` as a destination array: itself, or a GoTo action's `/D`.
fn open_destination(doc: &CosDocument, catalog: &Dict) -> Option<Vec<Object>> {
    match doc.resolve(catalog.get(b"OpenAction")?).ok()? {
        Object::Array(items) => Some(items),
        Object::Dict(action) => {
            let is_goto = action
                .get(b"S")
                .and_then(Object::as_name)
                .is_some_and(|name| name.as_bytes() == b"GoTo");
            match doc.resolve(action.get(b"D").filter(|_| is_goto)?).ok()? {
                Object::Array(items) => Some(items),
                _ => None,
            }
        }
        _ => None,
    }
}

fn read_destination(doc: &CosDocument, destination: &[Object]) -> (Option<usize>, OpenFit) {
    let page = destination.first().and_then(|target| match target {
        Object::Ref(objref) => page_index(doc, *objref),
        Object::Integer(index) => usize::try_from(*index).ok(),
        _ => None,
    });
    let kind = destination
        .get(1)
        .and_then(Object::as_name)
        .map(|name| name.as_bytes().to_vec());
    let number = |at: usize| match destination.get(at) {
        Some(Object::Integer(value)) => Some(*value as f64),
        Some(Object::Real(value)) => Some(*value),
        _ => None,
    };
    let fit = match kind.as_deref() {
        Some(b"Fit") => OpenFit::Page,
        Some(b"FitH" | b"FitBH") => OpenFit::Width,
        Some(b"FitV" | b"FitBV") => OpenFit::Height,
        Some(b"FitB") => OpenFit::Visible,
        Some(b"XYZ") => number(4)
            .filter(|zoom| *zoom > 0.0)
            .map_or(OpenFit::Default, OpenFit::Zoom),
        _ => OpenFit::Default,
    };
    (page, fit)
}

fn page_index(doc: &CosDocument, target: ObjRef) -> Option<usize> {
    let count = usize::try_from(doc.page_count().ok()?).ok()?;
    (0..count).find(|index| {
        doc.page(*index)
            .is_ok_and(|page| page.objref.number == target.number)
    })
}

/// Write the initial view into the catalog. An unset field removes its key.
pub fn write_initial_view(tx: &mut Transaction<'_>, view: &InitialView) -> Result<()> {
    let catalog_ref = tx
        .trailer_value(b"Root")
        .and_then(|root| root.as_reference())
        .ok_or(Error::NoCatalog)?;
    let mut catalog = dict_at(tx, catalog_ref)?;
    set_name(
        &mut catalog,
        "PageLayout",
        view.layout.map(PageLayout::name),
    );
    set_name(&mut catalog, "PageMode", view.mode.map(PageMode::name));
    match view.page {
        Some(page) => {
            let target = page_ref(tx, page)?;
            catalog.set(
                Name::new("OpenAction"),
                Object::Array(destination(target, view.fit)),
            );
        }
        None => {
            catalog.remove(b"OpenAction");
        }
    }
    tx.put_object(
        catalog_ref.number,
        catalog_ref.generation,
        Object::Dict(catalog),
    )
}

fn set_name(dict: &mut Dict, key: &str, value: Option<&str>) {
    match value {
        Some(value) => dict.set(Name::new(key), Object::name(value)),
        None => {
            dict.remove(key.as_bytes());
        }
    }
}

/// The page object at `index`, in the document as the transaction sees it.
fn page_ref(tx: &Transaction<'_>, index: usize) -> Result<ObjRef> {
    let catalog_ref = tx
        .trailer_value(b"Root")
        .and_then(|root| root.as_reference())
        .ok_or(Error::NoCatalog)?;
    let catalog = dict_at(tx, catalog_ref)?;
    let pages = catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .ok_or(Error::NoPageTree)?;
    let mut leaves = Vec::new();
    collect_leaves(tx, pages, &mut leaves, 0)?;
    leaves.get(index).copied().ok_or(Error::NoSuchPage {
        page: index,
        count: leaves.len(),
    })
}

fn collect_leaves(
    tx: &Transaction<'_>,
    node: ObjRef,
    into: &mut Vec<ObjRef>,
    depth: usize,
) -> Result<()> {
    if depth > 64 {
        return Ok(());
    }
    let dict = dict_at(tx, node)?;
    match resolve(tx, dict.get(b"Kids"))? {
        Some(Object::Array(kids)) => {
            for kid in kids.iter().filter_map(Object::as_reference) {
                collect_leaves(tx, kid, into, depth + 1)?;
            }
        }
        _ => into.push(node),
    }
    Ok(())
}

fn destination(page: ObjRef, fit: OpenFit) -> Vec<Object> {
    let mut items = vec![Object::Ref(page)];
    match fit {
        OpenFit::Default => {
            items.extend([
                Object::name("XYZ"),
                Object::Null,
                Object::Null,
                Object::Null,
            ]);
        }
        OpenFit::Page => items.push(Object::name("Fit")),
        OpenFit::Width => items.extend([Object::name("FitH"), Object::Null]),
        OpenFit::Height => items.extend([Object::name("FitV"), Object::Null]),
        OpenFit::Visible => items.push(Object::name("FitB")),
        OpenFit::Zoom(zoom) => items.extend([
            Object::name("XYZ"),
            Object::Null,
            Object::Null,
            Object::Real(zoom),
        ]),
    }
    items
}
