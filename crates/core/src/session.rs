use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use onionskin_content as content;
use onionskin_cos::{BytesSource, Provenance};

use crate::render::WorkerHandle;
use crate::{
    PageGeometry, PageIndex, PageRect, RenderRequest, RenderResponse, SearchMatch, SearchOptions,
    SearchState, Selection,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    EncryptedUnsupported,
    NoSuchPage {
        page: PageIndex,
        count: usize,
    },
    GeometryPageMismatch {
        request: PageIndex,
        geometry: PageIndex,
    },
    Cos(onionskin_cos::Error),
    Content(content::Error),
    Worker(crate::WorkerError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::EncryptedUnsupported => {
                write!(f, "encrypted PDFs are not viewable in M2")
            }
            Error::NoSuchPage { page, count } => {
                write!(f, "page {page} is outside a {count}-page document")
            }
            Error::GeometryPageMismatch { request, geometry } => write!(
                f,
                "render request is for page {request}, but its geometry is for page {geometry}"
            ),
            Error::Cos(e) => write!(f, "{e}"),
            Error::Content(e) => write!(f, "{e}"),
            Error::Worker(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Cos(e) => Some(e),
            Error::Content(e) => Some(e),
            Error::Worker(e) => Some(e),
            Error::EncryptedUnsupported
            | Error::NoSuchPage { .. }
            | Error::GeometryPageMismatch { .. } => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<onionskin_cos::Error> for Error {
    fn from(e: onionskin_cos::Error) -> Self {
        match e {
            onionskin_cos::Error::Encrypted => Error::EncryptedUnsupported,
            other => Error::Cos(other),
        }
    }
}

impl From<content::Error> for Error {
    fn from(e: content::Error) -> Self {
        match e {
            content::Error::Cos(onionskin_cos::Error::Encrypted) => Error::EncryptedUnsupported,
            other => Error::Content(other),
        }
    }
}

impl From<crate::WorkerError> for Error {
    fn from(e: crate::WorkerError) -> Self {
        Error::Worker(e)
    }
}

#[derive(Debug)]
pub enum PageGeometryResponse {
    Ready(PageGeometry),
    Failed { page: PageIndex, error: Error },
}

impl PageGeometryResponse {
    pub fn page(&self) -> PageIndex {
        match self {
            Self::Ready(geometry) => geometry.index,
            Self::Failed { page, .. } => *page,
        }
    }
}

/// A page region the snapshot tool asked the shell to copy as an image.
///
/// The tool raises this instead of holding a render handle, so the plugin
/// surface stays document and viewport only and the clipboard stays in
/// `app`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapshotRequest {
    pub region: PageRect,
}

const PAGE_CACHE_LIMIT: usize = 128;
const TEXT_CACHE_LIMIT: usize = 16;

pub struct Document {
    bytes: Arc<Vec<u8>>,
    cos: onionskin_cos::Document,
    provenance: Provenance,
    page_count: usize,
    render: WorkerHandle,
    pending_geometry: BTreeSet<PageIndex>,
    geometry: PageCache<PageGeometry>,
    text: PageCache<content::PageText>,
    selection: Selection,
    search: SearchState,
    snapshot: Option<SnapshotRequest>,
}

impl Document {
    pub fn open_path(path: &Path) -> Result<Self> {
        Self::open_bytes(std::fs::read(path)?)
    }

    pub fn open_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::open_shared(Arc::new(bytes))
    }

    pub fn open_shared(bytes: Arc<Vec<u8>>) -> Result<Self> {
        let (cos, provenance) = onionskin_cos::Document::open_repairing(Box::new(
            BytesSource::from_shared(Arc::clone(&bytes)),
        ))?;
        let page_count = content::page_count(&cos)?;
        let render = WorkerHandle::spawn(Arc::clone(&bytes))?;
        Ok(Document {
            bytes,
            cos,
            provenance,
            page_count,
            render,
            pending_geometry: BTreeSet::new(),
            geometry: PageCache::new(PAGE_CACHE_LIMIT),
            text: PageCache::new(TEXT_CACHE_LIMIT),
            selection: Selection::default(),
            search: SearchState::default(),
            snapshot: None,
        })
    }

    pub fn bytes(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.bytes)
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn page_count(&self) -> usize {
        self.page_count
    }

    pub fn page_geometry(&mut self, index: PageIndex) -> Result<&PageGeometry> {
        let cos = &self.cos;
        let render = &self.render;
        self.geometry.get_or_try_insert_with(index, || {
            let page = content::page(cos, index)?;
            let rendered = render.page_geometry(index)?;
            Ok(PageGeometry::new(&page, rendered))
        })
    }

    pub fn request_page_geometry(&mut self, index: PageIndex) -> Result<bool> {
        if index >= self.page_count {
            return Err(Error::NoSuchPage {
                page: index,
                count: self.page_count,
            });
        }
        if !self.pending_geometry.insert(index) {
            return Ok(false);
        }
        if let Err(error) = self.render.request_page_geometry(index) {
            self.pending_geometry.remove(&index);
            return Err(error.into());
        }
        Ok(true)
    }

    pub fn try_page_geometry_response(&mut self) -> Result<Option<PageGeometryResponse>> {
        let Some((index, result)) = self.render.try_geometry_response()? else {
            return Ok(None);
        };
        self.pending_geometry.remove(&index);
        let rendered = match result {
            Ok(rendered) => rendered,
            Err(error) => {
                return Ok(Some(PageGeometryResponse::Failed {
                    page: index,
                    error: Error::Worker(error),
                }));
            }
        };
        let page = match content::page(&self.cos, index) {
            Ok(page) => page,
            Err(error) => {
                return Ok(Some(PageGeometryResponse::Failed {
                    page: index,
                    error: error.into(),
                }));
            }
        };
        let geometry = PageGeometry::new(&page, rendered);
        self.geometry.insert(index, geometry.clone());
        Ok(Some(PageGeometryResponse::Ready(geometry)))
    }

    pub fn page_text(&mut self, index: PageIndex) -> Result<&content::PageText> {
        let cos = &self.cos;
        self.text
            .get_or_try_insert_with(index, || Ok(content::extract_page(cos, index)?))
    }

    pub fn request_render(
        &mut self,
        request: RenderRequest,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<()> {
        self.render.validate_request(request)?;
        let geometry = self.page_geometry(request.page)?.clone();
        self.render.request_render(request, &geometry, source)?;
        Ok(())
    }

    pub fn request_render_with_geometry(
        &mut self,
        request: RenderRequest,
        geometry: &PageGeometry,
        source: Option<&onionskin_render::BaseRaster>,
    ) -> Result<()> {
        self.render.validate_request(request)?;
        if request.page >= self.page_count {
            return Err(Error::NoSuchPage {
                page: request.page,
                count: self.page_count,
            });
        }
        if geometry.index != request.page {
            return Err(Error::GeometryPageMismatch {
                request: request.page,
                geometry: geometry.index,
            });
        }
        self.render.request_render(request, geometry, source)?;
        Ok(())
    }

    pub fn try_render_response(&mut self) -> Result<Option<RenderResponse>> {
        Ok(self.render.try_response()?)
    }

    pub fn search_page(
        &mut self,
        index: PageIndex,
        needle: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchMatch>> {
        let text = self.page_text(index)?;
        Ok(content::search(text, needle, options)
            .into_iter()
            .map(SearchMatch::from)
            .collect())
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// Select `region` and ask the shell to copy it as an image. Raising a
    /// request the shell has not taken yet replaces it: the user asked for
    /// the region they drew last.
    pub fn request_snapshot(&mut self, region: PageRect) {
        self.selection.set_region(region);
        self.snapshot = Some(SnapshotRequest { region });
    }

    pub fn take_snapshot_request(&mut self) -> Option<SnapshotRequest> {
        self.snapshot.take()
    }

    pub fn search(&self) -> &SearchState {
        &self.search
    }

    pub fn search_mut(&mut self) -> &mut SearchState {
        &mut self.search
    }
}

struct PageCache<T> {
    limit: usize,
    items: BTreeMap<PageIndex, T>,
    order: VecDeque<PageIndex>,
}

impl<T> PageCache<T> {
    fn new(limit: usize) -> Self {
        PageCache {
            limit: limit.max(1),
            items: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get_or_try_insert_with<E>(
        &mut self,
        index: PageIndex,
        load: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<&T, E> {
        if self.items.contains_key(&index) {
            self.touch(index);
            return Ok(self.items.get(&index).expect("cache hit remains present"));
        }

        let value = load()?;
        Ok(self.insert(index, value))
    }

    fn insert(&mut self, index: PageIndex, value: T) -> &T {
        if let Some(existing) = self.items.get_mut(&index) {
            *existing = value;
            self.touch(index);
            return self
                .items
                .get(&index)
                .expect("replaced cache item is present");
        }
        while self.items.len() >= self.limit {
            if let Some(oldest) = self.order.pop_front() {
                self.items.remove(&oldest);
            }
        }
        self.items.insert(index, value);
        self.order.push_back(index);
        self.items
            .get(&index)
            .expect("inserted cache item is present")
    }

    fn touch(&mut self, index: PageIndex) {
        if let Some(position) = self.order.iter().position(|seen| *seen == index) {
            self.order.remove(position);
        }
        self.order.push_back(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_cache_is_lazy_and_bounded() {
        let mut cache = PageCache::new(2);
        let mut loads = 0usize;

        assert_eq!(
            *cache
                .get_or_try_insert_with(0, || load(&mut loads, 10))
                .unwrap(),
            10
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(1, || load(&mut loads, 11))
                .unwrap(),
            11
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(0, || load(&mut loads, 99))
                .unwrap(),
            10
        );
        assert_eq!(
            *cache
                .get_or_try_insert_with(2, || load(&mut loads, 12))
                .unwrap(),
            12
        );

        assert_eq!(loads, 3);
        assert!(cache.items.contains_key(&0));
        assert!(!cache.items.contains_key(&1));
        assert!(cache.items.contains_key(&2));
    }

    fn load(loads: &mut usize, value: i32) -> std::result::Result<i32, ()> {
        *loads += 1;
        Ok(value)
    }
}
