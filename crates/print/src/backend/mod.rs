//! Where the sheets go: a backend takes imposed sheets and puts them on
//! paper, or in a file.

pub mod file;

use onionskin_core::protection::Refusal;

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
