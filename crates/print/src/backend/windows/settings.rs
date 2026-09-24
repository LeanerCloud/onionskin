//! What a Windows print job's `DEVMODEW` carries, as plain data.
//!
//! The values are Win32's, written out here so the mapping compiles and is
//! tested on every platform. `ffi.rs` checks each constant against
//! `windows-sys` at compile time, so a wrong number here cannot reach a
//! printer.

use crate::backend::native::NativeSettings;
use crate::job::{Duplex, PaperSize};

/// `dmFields` bits: which members of the `DEVMODEW` the driver is to read.
pub const DM_ORIENTATION: u32 = 0x1;
pub const DM_PAPERSIZE: u32 = 0x2;
pub const DM_PAPERLENGTH: u32 = 0x4;
pub const DM_PAPERWIDTH: u32 = 0x8;
pub const DM_COPIES: u32 = 0x100;
pub const DM_DUPLEX: u32 = 0x1000;
pub const DM_COLLATE: u32 = 0x8000;

pub const DMORIENT_PORTRAIT: i16 = 1;
pub const DMORIENT_LANDSCAPE: i16 = 2;

pub const DMPAPER_LETTER: i16 = 1;
pub const DMPAPER_LEGAL: i16 = 5;
pub const DMPAPER_A4: i16 = 9;
/// A size given in `dmPaperWidth` and `dmPaperLength` instead of by number.
pub const DMPAPER_USER: i16 = 256;

pub const DMDUP_SIMPLEX: i16 = 1;
/// Flip on the long edge of a portrait sheet.
pub const DMDUP_VERTICAL: i16 = 2;
/// Flip on the short edge of a portrait sheet.
pub const DMDUP_HORIZONTAL: i16 = 3;

pub const DMCOLLATE_FALSE: i16 = 0;
pub const DMCOLLATE_TRUE: i16 = 1;

/// The `DEVMODEW` members a job sets, and the `dmFields` naming them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevMode {
    pub fields: u32,
    pub orientation: i16,
    pub paper_size: i16,
    /// Tenths of a millimetre, for [`DMPAPER_USER`] only.
    pub paper_width: i16,
    pub paper_length: i16,
    pub copies: i16,
    pub duplex: i16,
    pub collate: i16,
}

impl DevMode {
    /// The members that print `settings`. The sheets already carry sizing,
    /// N-up, booklet and poster order, so the driver is told only the paper,
    /// which way it is turned, copies, collation and sides.
    pub fn new(settings: &NativeSettings) -> DevMode {
        let paper_size = paper_number(settings.paper);
        let custom = paper_size == DMPAPER_USER;
        DevMode {
            fields: DM_ORIENTATION
                | DM_PAPERSIZE
                | DM_COPIES
                | DM_DUPLEX
                | DM_COLLATE
                | if custom {
                    DM_PAPERWIDTH | DM_PAPERLENGTH
                } else {
                    0
                },
            orientation: if settings.landscape {
                DMORIENT_LANDSCAPE
            } else {
                DMORIENT_PORTRAIT
            },
            paper_size,
            paper_width: if custom {
                tenths_of_a_millimetre(settings.paper.width)
            } else {
                0
            },
            paper_length: if custom {
                tenths_of_a_millimetre(settings.paper.height)
            } else {
                0
            },
            copies: i16::try_from(settings.copies.max(1)).unwrap_or(i16::MAX),
            duplex: duplex(settings.duplex),
            collate: if settings.collate {
                DMCOLLATE_TRUE
            } else {
                DMCOLLATE_FALSE
            },
        }
    }
}

/// Win32's number for the paper, or [`DMPAPER_USER`] for one it has none for.
fn paper_number(paper: PaperSize) -> i16 {
    match paper.name {
        "Letter" => DMPAPER_LETTER,
        "Legal" => DMPAPER_LEGAL,
        "A4" => DMPAPER_A4,
        _ => DMPAPER_USER,
    }
}

/// `points` in the tenths of a millimetre `DEVMODEW` measures paper in.
fn tenths_of_a_millimetre(points: f64) -> i16 {
    let tenths = (points / 72.0 * 254.0).round();
    tenths.clamp(1.0, f64::from(i16::MAX)) as i16
}

/// Windows names the flip by the axis of a portrait sheet it turns about:
/// the long edge is the vertical one.
fn duplex(duplex: Duplex) -> i16 {
    match duplex {
        Duplex::Off => DMDUP_SIMPLEX,
        Duplex::LongEdge => DMDUP_VERTICAL,
        Duplex::ShortEdge => DMDUP_HORIZONTAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> NativeSettings {
        NativeSettings {
            paper: PaperSize::A4,
            landscape: true,
            copies: 3,
            collate: true,
            duplex: Duplex::LongEdge,
            printer: Some("Office".into()),
        }
    }

    #[test]
    fn a_job_becomes_devmode_members() {
        assert_eq!(
            DevMode::new(&settings()),
            DevMode {
                fields: DM_ORIENTATION | DM_PAPERSIZE | DM_COPIES | DM_DUPLEX | DM_COLLATE,
                orientation: DMORIENT_LANDSCAPE,
                paper_size: DMPAPER_A4,
                paper_width: 0,
                paper_length: 0,
                copies: 3,
                duplex: DMDUP_VERTICAL,
                collate: DMCOLLATE_TRUE,
            }
        );
    }

    #[test]
    fn a_paper_windows_has_no_number_for_is_given_in_tenths_of_a_millimetre() {
        let tabloid = PaperSize {
            name: "Tabloid",
            width: 792.0,
            height: 1224.0,
        };
        let mode = DevMode::new(&NativeSettings {
            paper: tabloid,
            ..settings()
        });
        assert_eq!(mode.paper_size, DMPAPER_USER);
        assert_eq!((mode.paper_width, mode.paper_length), (2794, 4318));
        assert_ne!(mode.fields & DM_PAPERWIDTH, 0);
        assert_ne!(mode.fields & DM_PAPERLENGTH, 0);
    }

    #[test]
    fn every_paper_duplex_and_count_has_its_value() {
        assert_eq!(paper_number(PaperSize::LETTER), DMPAPER_LETTER);
        assert_eq!(paper_number(PaperSize::LEGAL), DMPAPER_LEGAL);
        assert_eq!(duplex(Duplex::Off), DMDUP_SIMPLEX);
        assert_eq!(duplex(Duplex::ShortEdge), DMDUP_HORIZONTAL);
        let quiet = DevMode::new(&NativeSettings {
            copies: 0,
            collate: false,
            landscape: false,
            ..settings()
        });
        assert_eq!(quiet.copies, 1, "at least one copy");
        assert_eq!(quiet.collate, DMCOLLATE_FALSE);
        assert_eq!(quiet.orientation, DMORIENT_PORTRAIT);
        assert_eq!(tenths_of_a_millimetre(0.0), 1, "never a zero size");
    }
}
