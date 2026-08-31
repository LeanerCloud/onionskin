//! The rendering trait seam. The CPU reference rasterizes base pages and
//! composites overlays into copy-on-write tiles with damage tracking, so
//! only dirty visible tiles recomposite; a GPU backend arrives later
//! behind the same trait and is parity-tested against the CPU. Rendering
//! is display-only - what a save writes is operators and bytes, never
//! pixels - so render parity is a display-consistency concern, not the
//! semantic contract.
//!
//! # M1 spike state
//!
//! There is no trait yet, on purpose. [`Document::render_page`] is the CPU
//! reference and [`TileCache`] is its consumer; the seam will cut between
//! them, at "produce the base raster for page P at zoom Z". Everything above
//! that line (the tile grid, damage tracking, overlay compositing) is backend
//! independent and stays where it is when vello arrives underneath.
//!
//! Base rendering is whole-page, not per-tile, because hayro 0.7 has no way
//! to rasterize a sub-rectangle: `RenderSettings` carries a scale and an
//! optional viewport size, but no origin, and the `Device` implementation
//! that would accept an arbitrary transform is private to the crate. Tiles
//! therefore cache the *composite*, which is what the interactive path
//! actually needs - drawing an ink stroke recomposites the two tiles under
//! the pointer and never re-runs the interpreter.

mod base;
mod overlay;
mod store;
mod svg;
mod tile;

/// Re-exported because [`TileCache::page_image`] hands back a
/// `tiny_skia::Pixmap`. That leak is deliberate for the spike: PNG encoding
/// is evidence plumbing, not part of the seam being proven.
pub use tiny_skia;

/// hayro's own warning enum, surfaced rather than restated: it names exactly
/// the gaps the CPU reference has today, and a copy would drift.
pub use hayro::hayro_interpret::InterpreterWarning;

/// How an optional content group is named in
/// [`RenderOptions::layer_visibility`]: by the object its dictionary lives in.
/// hayro's own type, for the same reason [`InterpreterWarning`] is - restating
/// it would only add a conversion.
pub use hayro::hayro_syntax::object::ObjectIdentifier;

pub use base::{
    raster_size, BaseRaster, Document, PageRender, PageRenderGeometry, PageTransform, RenderError,
    RenderOptions, RenderSession, TransformError,
};
pub use overlay::{Overlay, OverlayError, Rgba};
pub use store::TileStore;
pub use svg::PageSvg;
pub use tile::{DeviceRect, Tile, TileCache, TILE_SIZE};
