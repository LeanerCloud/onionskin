//! What every comment tool needs to put an annotation on a page.
//!
//! Three lines each, in one place, because three copies of "which object is
//! this page" is three chances for one of them to resolve against the base
//! document while the others resolve against the edited one.

use onionskin_core::{Document, ObjRef, PageIndex};

/// The page's own object, which an annotation is written onto.
pub(crate) fn page_object(document: &mut Document, page: PageIndex) -> Option<ObjRef> {
    document.structure().ok()?.page(page).ok().map(|p| p.objref)
}

/// Seconds since the Unix epoch, or zero when the clock is before it.
///
/// `core` takes the timestamp rather than reading one, so this is the only
/// place in these tools that touches a clock - which is what lets every test
/// here pass a fixed instant instead of tolerating one.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}
