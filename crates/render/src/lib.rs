//! The rendering trait seam. The CPU reference rasterizes base pages and
//! composites overlays into copy-on-write tiles with damage tracking, so
//! only dirty visible tiles recomposite; a GPU backend arrives later
//! behind the same trait and is parity-tested against the CPU. Rendering
//! is display-only - what a save writes is operators and bytes, never
//! pixels - so render parity is a display-consistency concern, not the
//! semantic contract.
