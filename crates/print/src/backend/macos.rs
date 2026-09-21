//! The macOS backend: the same sheets the file backend writes, sent to a
//! printer through PDFKit and `NSPrintOperation`.
//!
//! **Why PDFKit and not an `NSView` that renders each sheet.** The plan
//! sketched a view whose `drawRect:` rasterizes a sheet through
//! `crates/render`. Printing the file backend's output instead means the
//! printer receives exactly the bytes `tests/file_backend.rs` checks, as
//! vectors, and no sheet is ever held as a printer-resolution raster (a
//! 1200 dpi A3 sheet is about 200 MB of RGBA). Print as Image still sends
//! rasters from our renderer when that is what the job asks for.
//!
//! **What it cannot prove in CI.** No hosted runner has a printer. The
//! settings are [`NativeSettings`], a pure function tested everywhere; this
//! file applies them key by key and is exercised by the manual acceptance
//! run recorded in the P16 evidence.

use std::ffi::c_void;
use std::sync::Arc;

use objc2::runtime::ProtocolObject;
use objc2::{AnyThread as _, MainThreadMarker};
use objc2_app_kit::{
    NSPaperOrientation, NSPrintCopies, NSPrintInfo, NSPrintInfoAttributeKey, NSPrintMustCollate,
    NSPrinter,
};
use objc2_foundation::{NSData, NSNumber, NSSize, NSString};
use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};

use super::native::{NativeSettings, Value, DUPLEX_KEY};
use super::{FileBackend, PrintBackend, PrintError};
use crate::impose::PageSize;
use crate::job::PrintJob;
use crate::sheet::Sheet;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// PrintCore: set a print settings object's duplex mode.
    fn PMSetDuplex(settings: *mut c_void, mode: u32) -> i32;
}

/// Prints to a macOS printer.
pub struct MacBackend {
    file: FileBackend,
    title: String,
}

impl MacBackend {
    /// A backend printing `bytes`, as [`FileBackend::new`] takes them, under
    /// `title` in the print queue.
    pub fn new(bytes: Arc<Vec<u8>>, title: impl Into<String>) -> Result<Self, PrintError> {
        Ok(MacBackend {
            file: FileBackend::new(bytes)?,
            title: title.into(),
        })
    }

    pub fn page_sizes(&mut self) -> Result<Vec<PageSize>, PrintError> {
        self.file.page_sizes()
    }
}

impl PrintBackend for MacBackend {
    fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<(), PrintError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            PrintError::Platform("printing has to start on the main thread".into())
        })?;
        self.file.print(job, sheets)?;
        let bytes = self
            .file
            .output()
            .expect("a print that succeeded wrote sheets");
        send(bytes, &NativeSettings::new(job, sheets), &self.title, mtm)
    }
}

/// The printers macOS knows, by name, for the dialog's printer list.
pub fn printers() -> Vec<String> {
    NSPrinter::printerNames()
        .iter()
        .map(|name| name.to_string())
        .collect()
}

/// Spool `bytes` to the printer with `settings`, showing macOS's progress
/// panel and no second print dialog: ours has already asked.
fn send(
    bytes: &[u8],
    settings: &NativeSettings,
    title: &str,
    mtm: MainThreadMarker,
) -> Result<(), PrintError> {
    let data = NSData::with_bytes(bytes);
    // SAFETY: `data` is a valid NSData for the call's duration.
    let document = unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }
        .ok_or_else(|| PrintError::Platform("macOS could not read the sheets".into()))?;
    let info = NSPrintInfo::init(NSPrintInfo::alloc());
    for (key, value) in settings.entries() {
        apply(&info, key, &value)?;
    }
    // SAFETY: `info` is a valid print info; no scaling or rotation is asked
    // for because every sheet is already the paper's size and way round.
    let operation = unsafe {
        document.printOperationForPrintInfo_scalingMode_autoRotate(
            Some(&info),
            PDFPrintScalingMode::PageScaleNone,
            false,
            mtm,
        )
    }
    .ok_or_else(|| PrintError::Platform("macOS could not start the print job".into()))?;
    operation.setShowsPrintPanel(false);
    operation.setShowsProgressPanel(true);
    operation.setJobTitle(Some(&NSString::from_str(title)));
    if operation.runOperation() {
        Ok(())
    } else {
        Err(PrintError::Platform(
            "the print job was cancelled or the printer refused it".into(),
        ))
    }
}

/// Set one entry of [`NativeSettings::entries`] on `info`.
fn apply(info: &NSPrintInfo, key: &str, value: &Value) -> Result<(), PrintError> {
    match (key, value) {
        ("NSPrintPaperName", Value::Text(name)) => {
            info.setPaperName(Some(&NSString::from_str(name)));
        }
        ("NSPrintPaperSize", Value::Size(width, height)) => info.setPaperSize(NSSize {
            width: *width,
            height: *height,
        }),
        ("NSPrintOrientation", Value::Number(orientation)) => {
            info.setOrientation(if *orientation == 0.0 {
                NSPaperOrientation::Portrait
            } else {
                NSPaperOrientation::Landscape
            })
        }
        ("NSPrintLeftMargin", Value::Number(margin)) => info.setLeftMargin(*margin),
        ("NSPrintRightMargin", Value::Number(margin)) => info.setRightMargin(*margin),
        ("NSPrintTopMargin", Value::Number(margin)) => info.setTopMargin(*margin),
        ("NSPrintBottomMargin", Value::Number(margin)) => info.setBottomMargin(*margin),
        ("NSPrintHorizontallyCentered", Value::Flag(centred)) => {
            info.setHorizontallyCentered(*centred)
        }
        ("NSPrintVerticallyCentered", Value::Flag(centred)) => info.setVerticallyCentered(*centred),
        ("NSPrintScalingFactor", Value::Number(factor)) => info.setScalingFactor(*factor),
        // The attribute names are Rust-side names; the dictionary is keyed
        // by AppKit's own constants, whose strings differ ("NSCopies").
        ("NSPrintCopies", Value::Number(copies)) => {
            // SAFETY: an AppKit constant, valid for the process's lifetime.
            set_in_dictionary(info, unsafe { NSPrintCopies }, &NSNumber::new_f64(*copies))
        }
        ("NSPrintMustCollate", Value::Flag(collate)) => set_in_dictionary(
            info,
            // SAFETY: as above.
            unsafe { NSPrintMustCollate },
            &NSNumber::new_bool(*collate),
        ),
        (DUPLEX_KEY, Value::Number(mode)) => {
            // SAFETY: `PMPrintSettings` is the print info's own settings
            // object, valid while `info` is; the mode is one of PrintCore's.
            let status = unsafe { PMSetDuplex(info.PMPrintSettings().as_ptr(), *mode as u32) };
            if status != 0 {
                return Err(PrintError::Platform(format!(
                    "the printer settings refused two-sided printing ({status})"
                )));
            }
            info.updateFromPMPrintSettings();
        }
        ("NSPrintPrinterName", Value::Text(name)) => {
            let printer = NSPrinter::printerWithName(&NSString::from_str(name))
                .ok_or_else(|| PrintError::Platform(format!("there is no printer named {name}")))?;
            info.setPrinter(&printer);
        }
        _ => {
            return Err(PrintError::Platform(format!(
                "no macOS print setting for {key}"
            )))
        }
    }
    Ok(())
}

/// Set a print info attribute that has no setter of its own.
fn set_in_dictionary(info: &NSPrintInfo, key: &NSPrintInfoAttributeKey, value: &NSNumber) {
    // SAFETY: the dictionary is the print info's own, and every value set
    // here is an NSNumber, which is what these attributes hold.
    unsafe {
        let dictionary = info.dictionary();
        dictionary.setObject_forKey(value, ProtocolObject::from_ref(key));
    }
}
