//! Redaction: content-stream rewriting, image region scrub and metadata
//! scrub. The one destructive path in the product, saved as a flattening
//! rewrite rather than an incremental section, because redaction under
//! incremental update would be a lie. Ships with a verifier that
//! re-extracts text and images from the output and proves the target is
//! gone; the verifier is part of the feature, not the test suite.
//!
//! - [`mark`]: marking text, regions, pages and search results, as
//!   ordinary undoable edits;
//! - [`find`]: Find Text & Redact's words, phrases and patterns;
//! - [`codes`]: the exemption codes overlay text cites;
//! - [`apply_redactions`]: the rewrite, verified; [`sanitize`], the same
//!   removing hidden information;
//! - [`RedactTool`]: marking text and regions on the page.

mod apply;
pub mod codes;
pub mod find;
pub mod mark;
mod tool;
mod verify;

use onionskin_core::protection::Refusal;
use onionskin_plugin_api::{PluginManifest, PluginRegistry};

pub use apply::{apply_redactions, apply_with, sanitize, Applied, ApplyOptions, Report, Sanitized};
pub use tool::RedactTool;
pub use verify::Verification;

/// Why a redaction was not applied. Nothing is written when it is not.
#[derive(Debug)]
pub enum RedactError {
    /// The document may not be read out: an encrypted source.
    Refused(Refusal),
    /// There are no marks to apply.
    NothingMarked,
    /// A page has more glyphs than the interpreter looks at, so some would
    /// go unchecked.
    TooMuchText {
        page: usize,
    },
    /// An object that should be a stream is not.
    NotAStream(u32),
    /// The verifier found what was removed in the new file.
    NotVerified(Vec<String>),
    Core(onionskin_core::Error),
    Content(onionskin_content::Error),
    Cos(onionskin_cos::Error),
}

impl std::fmt::Display for RedactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "{refusal}"),
            Self::NothingMarked => write!(f, "nothing is marked for redaction"),
            Self::TooMuchText { page } => write!(
                f,
                "page {} has more text than can be checked, so it was not redacted",
                page + 1
            ),
            Self::NotAStream(number) => write!(f, "object {number} is not a stream"),
            Self::NotVerified(problems) => write!(
                f,
                "the redacted file failed verification, so it was not written: {}",
                problems.join("; ")
            ),
            Self::Core(error) => write!(f, "{error}"),
            Self::Content(error) => write!(f, "{error}"),
            Self::Cos(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RedactError {}

impl From<onionskin_core::Error> for RedactError {
    fn from(error: onionskin_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<onionskin_content::Error> for RedactError {
    fn from(error: onionskin_content::Error) -> Self {
        Self::Content(error)
    }
}

impl From<onionskin_cos::Error> for RedactError {
    fn from(error: onionskin_cos::Error) -> Self {
        Self::Cos(error)
    }
}

pub struct RedactPlugin;

impl PluginManifest for RedactPlugin {
    fn id(&self) -> &'static str {
        "onionskin.redact"
    }

    fn name(&self) -> &'static str {
        "Redact"
    }

    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_tool(Box::new(RedactTool::new()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_says_what_happened() {
        assert_eq!(
            RedactError::TooMuchText { page: 2 }.to_string(),
            "page 3 has more text than can be checked, so it was not redacted"
        );
        assert_eq!(RedactError::NotAStream(7).to_string(), "object 7 is not a stream");
        let failed = RedactError::NotVerified(vec!["one".into(), "two".into()]).to_string();
        assert!(failed.ends_with("one; two"), "{failed}");
        let cos = RedactError::from(onionskin_cos::Error::Unrecoverable {
            detail: "broken".into(),
        });
        assert!(cos.to_string().contains("broken"));
    }
}
