//! Where the sheets go: a backend takes imposed sheets and puts them on
//! paper, or in a file.

pub mod file;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod native;

use onionskin_core::protection::Refusal;

pub use file::FileBackend;

use crate::job::PrintJob;
use crate::sheet::Sheet;

/// Why a print did not happen.
#[derive(Debug)]
pub enum PrintError {
    /// The encrypted-source rule: an encrypted document prints only as
    /// pixels, because printing its pages as vectors copies its content into
    /// another file.
    Refused(Refusal),
    Core(onionskin_core::Error),
    Cos(onionskin_cos::Error),
    /// The job selected no page.
    NothingToPrint,
    /// The platform's print system failed, in its words.
    Platform(String),
}

impl std::fmt::Display for PrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => write!(
                f,
                "{refusal} Turn on Print as Image to print it as pictures of its pages."
            ),
            Self::Core(error) => write!(f, "{error}"),
            Self::Cos(error) => write!(f, "{error}"),
            Self::NothingToPrint => write!(f, "The page selection selects no page"),
            Self::Platform(reason) => write!(f, "Could not print: {reason}"),
        }
    }
}

impl std::error::Error for PrintError {}

impl From<onionskin_core::Error> for PrintError {
    fn from(error: onionskin_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<onionskin_cos::Error> for PrintError {
    fn from(error: onionskin_cos::Error) -> Self {
        Self::Cos(error)
    }
}

/// A place sheets can be printed to.
pub trait PrintBackend {
    fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<(), PrintError>;
}

/// The platform backend this build has, by name: `None` where there is
/// none yet (Linux and Windows print at M4), so a build without one cannot
/// pretend to print.
pub fn native_backend() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("macOS")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_says_what_went_wrong() {
        assert_eq!(
            PrintError::Platform("the printer refused it".into()).to_string(),
            "Could not print: the printer refused it"
        );
        assert_eq!(
            PrintError::NothingToPrint.to_string(),
            "The page selection selects no page"
        );
        assert!(PrintError::Refused(Refusal::EncryptedSource)
            .to_string()
            .contains("Print as Image"));
    }

    #[test]
    fn only_macos_has_a_platform_backend_at_m3() {
        assert_eq!(native_backend().is_some(), cfg!(target_os = "macos"));
    }
}
