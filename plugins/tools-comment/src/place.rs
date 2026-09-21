//! What every comment tool needs to put an annotation on a page.
//!
//! Three lines each, in one place, because three copies of "which object is
//! this page" is three chances for one of them to resolve against the base
//! document while the others resolve against the edited one.

use std::collections::BTreeMap;

use onionskin_core::{Annotation, Color, Document, ObjRef, PageIndex};
use onionskin_plugin_api::{CommentDefault, ToolEnvironment};

/// The page's own object, which an annotation is written onto.
pub(crate) fn page_object(document: &mut Document, page: PageIndex) -> Option<ObjRef> {
    document.structure().ok()?.page(page).ok().map(|p| p.objref)
}

/// What the user chose for every comment a tool places: the name it is
/// signed with, and the look they made the default for its kind. Both come
/// from the shell's environment. With no name a comment is unsigned rather
/// than signed with the operating system's account; with no default for its
/// kind a comment keeps the tool's own look.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Signer {
    author: Option<String>,
    defaults: BTreeMap<String, CommentDefault>,
}

impl Signer {
    pub(crate) fn configure(&mut self, environment: &ToolEnvironment) {
        self.author.clone_from(&environment.author);
        self.defaults.clone_from(&environment.comment_defaults);
    }

    /// Put the author on `annotation` as its `/T`, and the default colour and
    /// opacity for its kind, when the user made one.
    pub(crate) fn sign(&self, annotation: &mut Annotation) {
        annotation.author.clone_from(&self.author);
        let Some(default) = self.defaults.get(annotation.subtype.as_str()) else {
            return;
        };
        if let Some([red, green, blue]) = default.color {
            let channel = |value: u8| f64::from(value) / 255.0;
            annotation.color = Some(Color::new(channel(red), channel(green), channel(blue)));
        }
        annotation.opacity =
            (default.opacity_percent < 100).then(|| f64::from(default.opacity_percent) / 100.0);
    }
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
