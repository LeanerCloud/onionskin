//! The CUPS backend, for Linux and the BSDs: the file backend's sheets piped
//! to `lp`, with the job's settings as `lp` options.
//!
//! **Why `lp` and not libcups.** `lp` is on every system that has CUPS, it
//! needs no C library at build time, and it takes a PDF on standard input,
//! which is exactly what the file backend writes. The printer receives the
//! same bytes `tests/file_backend.rs` checks, as vectors.
//!
//! **What reaches the printer.** The sheets already carry sizing, N-up,
//! booklet and poster order, so `lp` is told the paper, copies, collation,
//! the sides, the printer and the title, and `print-scaling=none` so CUPS
//! puts each sheet on the paper as it is. CUPS turns a landscape sheet to
//! fit portrait paper by itself, so no orientation is sent.
//!
//! The arguments are a pure function, tested on every platform; the tests
//! in `tests/cups.rs` run the backend against a stand-in `lp`, and once
//! against a real scheduler (docs/evidence/m4-cups.md).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;

use super::native::NativeSettings;
use super::{FileBackend, PrintBackend, PrintError};
use crate::impose::PageSize;
use crate::job::{Duplex, PaperSize, PrintJob};
use crate::sheet::Sheet;

/// The CUPS commands a backend runs. The defaults are found on `PATH`.
#[derive(Debug, Clone, PartialEq)]
pub struct Programs {
    /// Submits a job.
    pub lp: PathBuf,
    /// Lists the printers.
    pub lpstat: PathBuf,
}

impl Default for Programs {
    fn default() -> Self {
        Programs {
            lp: "lp".into(),
            lpstat: "lpstat".into(),
        }
    }
}

/// Prints through CUPS.
pub struct CupsBackend {
    file: FileBackend,
    title: String,
    programs: Programs,
}

impl CupsBackend {
    /// A backend printing `bytes`, as [`FileBackend::new`] takes them, under
    /// `title` in the print queue, through `programs`.
    pub fn new(
        bytes: Arc<Vec<u8>>,
        title: impl Into<String>,
        programs: Programs,
    ) -> Result<Self, PrintError> {
        Ok(CupsBackend {
            file: FileBackend::new(bytes)?,
            title: title.into(),
            programs,
        })
    }

    pub fn page_sizes(&mut self) -> Result<Vec<PageSize>, PrintError> {
        self.file.page_sizes()
    }
}

impl PrintBackend for CupsBackend {
    fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<(), PrintError> {
        self.file.print(job, sheets)?;
        let bytes = self
            .file
            .output()
            .expect("a print that succeeded wrote sheets");
        let arguments = lp_arguments(&NativeSettings::new(job, sheets), &self.title);
        submit(&self.programs.lp, &arguments, bytes)
    }
}

/// The printers CUPS knows, by name, for the dialog's printer list: none
/// when `lpstat` is missing or fails, as on a system without CUPS.
pub fn printers(programs: &Programs) -> Vec<String> {
    Command::new(&programs.lpstat)
        .arg("-e")
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(Vec::new, |output| {
            parse_destinations(&String::from_utf8_lossy(&output.stdout))
        })
}

/// `lpstat -e` prints one destination a line.
fn parse_destinations(listing: &str) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The `lp` arguments that print `settings` under `title`, the PDF coming
/// on standard input.
pub fn lp_arguments(settings: &NativeSettings, title: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    if let Some(printer) = &settings.printer {
        arguments.extend(["-d".to_owned(), printer.clone()]);
    }
    arguments.extend([
        "-t".to_owned(),
        title.to_owned(),
        "-n".to_owned(),
        settings.copies.max(1).to_string(),
    ]);
    for option in [
        format!("media={}", media(settings.paper)),
        format!("sides={}", sides(settings.duplex)),
        format!("collate={}", settings.collate),
        "print-scaling=none".to_owned(),
        "document-format=application/pdf".to_owned(),
    ] {
        arguments.extend(["-o".to_owned(), option]);
    }
    // Nothing after the options: `lp` reads the file from standard input.
    arguments
}

/// The PWG self-describing media name CUPS knows the paper by; a paper it
/// has no name for is a custom size in points.
fn media(paper: PaperSize) -> String {
    match paper.name {
        "Letter" => "na_letter_8.5x11in".to_owned(),
        "Legal" => "na_legal_8.5x14in".to_owned(),
        "A4" => "iso_a4_210x297mm".to_owned(),
        _ => format!("Custom.{}x{}", paper.width.round(), paper.height.round()),
    }
}

/// IPP's `sides` keyword.
fn sides(duplex: Duplex) -> &'static str {
    match duplex {
        Duplex::Off => "one-sided",
        Duplex::LongEdge => "two-sided-long-edge",
        Duplex::ShortEdge => "two-sided-short-edge",
    }
}

/// Run `lp` with `arguments`, `pdf` on its standard input.
fn submit(lp: &Path, arguments: &[String], pdf: &[u8]) -> Result<(), PrintError> {
    let mut child = Command::new(lp)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                PrintError::Platform("CUPS is not installed: there is no lp command".into())
            }
            _ => PrintError::Platform(format!("lp did not start: {error}")),
        })?;
    // A write that fails because `lp` gave up early is reported by its exit
    // status, which says why; one that fails while `lp` succeeds is not.
    let written = child.stdin.take().expect("stdin is piped").write_all(pdf);
    let output = child
        .wait_with_output()
        .map_err(|error| PrintError::Platform(format!("lp did not finish: {error}")))?;
    refusal(&output).map_or_else(
        || {
            written
                .map_err(|error| PrintError::Platform(format!("lp did not take the job: {error}")))
        },
        Err,
    )
}

/// `lp`'s refusal, in its words, if it refused.
fn refusal(output: &Output) -> Option<PrintError> {
    if output.status.success() {
        return None;
    }
    let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Some(PrintError::Platform(if said.is_empty() {
        format!("lp failed ({})", output.status)
    } else {
        said
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> NativeSettings {
        NativeSettings {
            paper: PaperSize::A4,
            landscape: false,
            copies: 2,
            collate: true,
            duplex: Duplex::LongEdge,
            printer: Some("Office".into()),
        }
    }

    /// The value after each `-o`.
    fn options(arguments: &[String]) -> Vec<&str> {
        arguments
            .windows(2)
            .filter(|pair| pair[0] == "-o")
            .map(|pair| pair[1].as_str())
            .collect()
    }

    #[test]
    fn a_job_becomes_lp_arguments() {
        let arguments = lp_arguments(&settings(), "report.pdf");
        assert_eq!(
            arguments[..6],
            ["-d", "Office", "-t", "report.pdf", "-n", "2"]
        );
        assert_eq!(
            options(&arguments),
            [
                "media=iso_a4_210x297mm",
                "sides=two-sided-long-edge",
                "collate=true",
                "print-scaling=none",
                "document-format=application/pdf",
            ]
        );
        assert_eq!(
            arguments.last().map(String::as_str),
            Some("document-format=application/pdf"),
            "nothing after the options: the PDF comes on standard input"
        );
    }

    #[test]
    fn no_printer_means_the_default_destination_and_copies_are_at_least_one() {
        let arguments = lp_arguments(
            &NativeSettings {
                printer: None,
                copies: 0,
                collate: false,
                duplex: Duplex::Off,
                ..settings()
            },
            "t",
        );
        assert!(!arguments.contains(&"-d".to_owned()));
        assert_eq!(arguments[..4], ["-t", "t", "-n", "1"]);
        let options = options(&arguments);
        assert!(options.contains(&"sides=one-sided"));
        assert!(options.contains(&"collate=false"));
    }

    #[test]
    fn each_paper_and_duplex_has_its_cups_name() {
        assert_eq!(media(PaperSize::LETTER), "na_letter_8.5x11in");
        assert_eq!(media(PaperSize::LEGAL), "na_legal_8.5x14in");
        let tabloid = PaperSize {
            name: "Tabloid",
            width: 792.0,
            height: 1224.0,
        };
        assert_eq!(media(tabloid), "Custom.792x1224");
        assert_eq!(sides(Duplex::ShortEdge), "two-sided-short-edge");
    }

    #[test]
    fn lpstat_lists_one_destination_a_line() {
        assert_eq!(
            parse_destinations("Office\n  Home_Laser \n\n"),
            ["Office", "Home_Laser"]
        );
        assert!(parse_destinations("").is_empty());
    }

    #[test]
    fn a_missing_lpstat_lists_no_printer_and_a_missing_lp_says_cups_is_absent() {
        let missing = Programs {
            lp: "/nonexistent/lp".into(),
            lpstat: "/nonexistent/lpstat".into(),
        };
        assert!(printers(&missing).is_empty());
        let error = submit(&missing.lp, &[], b"%PDF").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not print: CUPS is not installed: there is no lp command"
        );
        assert_eq!(Programs::default().lp, PathBuf::from("lp"));
    }
}
