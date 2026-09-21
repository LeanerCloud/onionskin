//! What a platform print system is told about a job, as plain data.
//!
//! The imposed sheets already carry the page sizing, N-up and page order,
//! so the printer is asked for none of that: it gets the paper, which way
//! the sheet is turned, copies, collation, the printer, and whether to
//! print both sides, which is the one choice only the hardware can make.
//!
//! The mapping is a pure function so it can be tested on any platform. The
//! macOS backend applies [`NativeSettings::entries`] key by key, so the
//! keys tested here are the keys that reach `NSPrintInfo`.

use crate::job::{Duplex, PaperSize, PrintJob};
use crate::sheet::Sheet;

/// A value for one print setting.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Flag(bool),
    /// Width and height, in points.
    Size(f64, f64),
}

/// The settings a platform print job carries.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeSettings {
    pub paper: PaperSize,
    /// Whether the sheets are landscape, as imposition turned them.
    pub landscape: bool,
    pub copies: u16,
    pub collate: bool,
    pub duplex: Duplex,
    pub printer: Option<String>,
}

/// `NSPrintInfo`'s orientation values.
const PORTRAIT: f64 = 0.0;
const LANDSCAPE: f64 = 1.0;

/// The key the duplex mode is set under. Not an `NSPrintInfo` attribute:
/// duplex lives in PrintCore's `PMPrintSettings`, which the backend reaches
/// through `NSPrintInfo`.
pub const DUPLEX_KEY: &str = "PMDuplexMode";

impl NativeSettings {
    /// The settings for printing `sheets` as `job` asks.
    pub fn new(job: &PrintJob, sheets: &[Sheet]) -> NativeSettings {
        NativeSettings {
            paper: job.paper,
            landscape: sheets.first().is_some_and(Sheet::is_landscape),
            copies: job.copies.max(1),
            collate: job.collate,
            duplex: job.duplex,
            printer: job.printer.clone(),
        }
    }

    /// Every setting, keyed by the `NSPrintInfo` attribute it sets.
    /// Margins are zero and nothing is centred or scaled, because each sheet
    /// is already the paper's size.
    pub fn entries(&self) -> Vec<(&'static str, Value)> {
        let mut entries = vec![
            (
                "NSPrintPaperName",
                Value::Text(paper_name(self.paper).into()),
            ),
            (
                "NSPrintPaperSize",
                Value::Size(self.paper.width, self.paper.height),
            ),
            (
                "NSPrintOrientation",
                Value::Number(if self.landscape { LANDSCAPE } else { PORTRAIT }),
            ),
            ("NSPrintLeftMargin", Value::Number(0.0)),
            ("NSPrintRightMargin", Value::Number(0.0)),
            ("NSPrintTopMargin", Value::Number(0.0)),
            ("NSPrintBottomMargin", Value::Number(0.0)),
            ("NSPrintHorizontallyCentered", Value::Flag(false)),
            ("NSPrintVerticallyCentered", Value::Flag(false)),
            ("NSPrintScalingFactor", Value::Number(1.0)),
            ("NSPrintCopies", Value::Number(f64::from(self.copies))),
            ("NSPrintMustCollate", Value::Flag(self.collate)),
            (
                DUPLEX_KEY,
                Value::Number(f64::from(duplex_mode(self.duplex))),
            ),
        ];
        if let Some(printer) = &self.printer {
            entries.push(("NSPrintPrinterName", Value::Text(printer.clone())));
        }
        entries
    }
}

/// The PWG media name macOS knows the paper by.
fn paper_name(paper: PaperSize) -> &'static str {
    match paper.name {
        "Legal" => "na-legal",
        "A4" => "iso-a4",
        _ => "na-letter",
    }
}

/// PrintCore's `PMDuplexMode`: none, no tumble (long edge), tumble (short
/// edge).
pub fn duplex_mode(duplex: Duplex) -> u32 {
    match duplex {
        Duplex::Off => 1,
        Duplex::LongEdge => 2,
        Duplex::ShortEdge => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impose::impose;
    use crate::job::{NUp, Orientation};

    fn job() -> PrintJob {
        PrintJob {
            paper: PaperSize::A4,
            copies: 3,
            collate: false,
            duplex: Duplex::ShortEdge,
            printer: Some("Office".into()),
            ..PrintJob::default()
        }
    }

    #[test]
    fn a_job_maps_to_exactly_these_settings_in_this_order() {
        let sheets = impose(&job(), &[(595.276, 841.89)]);
        let entries = NativeSettings::new(&job(), &sheets).entries();
        assert_eq!(
            entries,
            vec![
                ("NSPrintPaperName", Value::Text("iso-a4".into())),
                ("NSPrintPaperSize", Value::Size(595.276, 841.89)),
                ("NSPrintOrientation", Value::Number(0.0)),
                ("NSPrintLeftMargin", Value::Number(0.0)),
                ("NSPrintRightMargin", Value::Number(0.0)),
                ("NSPrintTopMargin", Value::Number(0.0)),
                ("NSPrintBottomMargin", Value::Number(0.0)),
                ("NSPrintHorizontallyCentered", Value::Flag(false)),
                ("NSPrintVerticallyCentered", Value::Flag(false)),
                ("NSPrintScalingFactor", Value::Number(1.0)),
                ("NSPrintCopies", Value::Number(3.0)),
                ("NSPrintMustCollate", Value::Flag(false)),
                ("PMDuplexMode", Value::Number(3.0)),
                ("NSPrintPrinterName", Value::Text("Office".into())),
            ]
        );
    }

    #[test]
    fn the_orientation_is_the_sheets_not_the_request() {
        // Auto on two-up turns the sheet, and the printer is told so.
        let two_up = PrintJob {
            orientation: Orientation::Auto,
            n_up: NUp {
                per_sheet: 2,
                ..NUp::default()
            },
            ..PrintJob::default()
        };
        let sheets = impose(&two_up, &[(612.0, 792.0); 2]);
        assert!(NativeSettings::new(&two_up, &sheets).landscape);
        assert!(!NativeSettings::new(&two_up, &[]).landscape, "no sheet");
    }

    #[test]
    fn defaults_print_one_collated_single_sided_copy_on_the_default_printer() {
        let settings = NativeSettings::new(&PrintJob::default(), &[]);
        let entries = settings.entries();
        let get = |key: &str| {
            entries
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(get("NSPrintCopies"), Some(Value::Number(1.0)));
        assert_eq!(get("NSPrintMustCollate"), Some(Value::Flag(true)));
        assert_eq!(get("PMDuplexMode"), Some(Value::Number(1.0)));
        assert_eq!(get("NSPrintPrinterName"), None);
        assert_eq!(
            get("NSPrintPaperName"),
            Some(Value::Text("na-letter".into()))
        );
    }

    #[test]
    fn zero_copies_is_one_and_every_paper_and_duplex_mode_has_a_name() {
        let none = PrintJob {
            copies: 0,
            ..PrintJob::default()
        };
        assert_eq!(NativeSettings::new(&none, &[]).copies, 1);
        assert_eq!(paper_name(PaperSize::LEGAL), "na-legal");
        assert_eq!(
            [Duplex::Off, Duplex::LongEdge, Duplex::ShortEdge].map(duplex_mode),
            [1, 2, 3]
        );
    }
}
