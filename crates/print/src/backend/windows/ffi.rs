//! The Win32 half of the Windows backend: the spooler's printers, a device
//! context set up from [`DevMode`], and each sheet drawn with
//! `StretchDIBits` between `StartPage` and `EndPage`.
//!
//! Compiled on Windows, and on any host with `--cfg onionskin_check_windows`
//! for `cargo check`: `windows-sys` declares its functions on every
//! platform, so this file type-checks where it can neither link nor run.
//! Everything that can be tested without a printer lives in `settings.rs`
//! and `raster.rs`; what is left here is calls.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Graphics::Gdi::{
    self, CreateDCW, DeleteDC, GetDeviceCaps, SetBrushOrgEx, SetStretchBltMode, StretchDIBits,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DEVMODEW, DIB_RGB_COLORS, HALFTONE, HDC, LOGPIXELSX,
    LOGPIXELSY, PHYSICALOFFSETX, PHYSICALOFFSETY, SRCCOPY,
};
use windows_sys::Win32::Graphics::Printing::{
    ClosePrinter, DocumentPropertiesW, EnumPrintersW, GetDefaultPrinterW, OpenPrinterW,
    PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_4W,
};
use windows_sys::Win32::Storage::Xps::{AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};

use super::raster::{device_rect, render_dpi, SheetImage, Sheets};
use super::settings::{self, DevMode};
use crate::backend::native::NativeSettings;
use crate::backend::{FileBackend, PrintBackend, PrintError};
use crate::impose::PageSize;
use crate::job::PrintJob;
use crate::sheet::Sheet;

// `settings.rs` writes Win32's numbers out so it compiles everywhere; these
// hold it to them.
const _: () = {
    assert!(settings::DM_ORIENTATION == Gdi::DM_ORIENTATION);
    assert!(settings::DM_PAPERSIZE == Gdi::DM_PAPERSIZE);
    assert!(settings::DM_PAPERLENGTH == Gdi::DM_PAPERLENGTH);
    assert!(settings::DM_PAPERWIDTH == Gdi::DM_PAPERWIDTH);
    assert!(settings::DM_COPIES == Gdi::DM_COPIES);
    assert!(settings::DM_DUPLEX == Gdi::DM_DUPLEX);
    assert!(settings::DM_COLLATE == Gdi::DM_COLLATE);
    assert!(settings::DMORIENT_PORTRAIT as u32 == Gdi::DMORIENT_PORTRAIT);
    assert!(settings::DMORIENT_LANDSCAPE as u32 == Gdi::DMORIENT_LANDSCAPE);
    assert!(settings::DMPAPER_LETTER as u32 == Gdi::DMPAPER_LETTER);
    assert!(settings::DMPAPER_LEGAL as u32 == Gdi::DMPAPER_LEGAL);
    assert!(settings::DMPAPER_A4 as u32 == Gdi::DMPAPER_A4);
    assert!(settings::DMPAPER_USER as u32 == Gdi::DMPAPER_USER);
    assert!(settings::DMDUP_SIMPLEX == Gdi::DMDUP_SIMPLEX);
    assert!(settings::DMDUP_VERTICAL == Gdi::DMDUP_VERTICAL);
    assert!(settings::DMDUP_HORIZONTAL == Gdi::DMDUP_HORIZONTAL);
    assert!(settings::DMCOLLATE_FALSE == Gdi::DMCOLLATE_FALSE);
    assert!(settings::DMCOLLATE_TRUE == Gdi::DMCOLLATE_TRUE);
};

/// Prints to a Windows printer.
pub struct WindowsBackend {
    file: FileBackend,
    title: String,
}

impl WindowsBackend {
    /// A backend printing `bytes`, as [`FileBackend::new`] takes them, under
    /// `title` in the print queue.
    pub fn new(
        bytes: std::sync::Arc<Vec<u8>>,
        title: impl Into<String>,
    ) -> Result<Self, PrintError> {
        Ok(WindowsBackend {
            file: FileBackend::new(bytes)?,
            title: title.into(),
        })
    }

    pub fn page_sizes(&mut self) -> Result<Vec<PageSize>, PrintError> {
        self.file.page_sizes()
    }
}

impl PrintBackend for WindowsBackend {
    fn print(&mut self, job: &PrintJob, sheets: &[Sheet]) -> Result<(), PrintError> {
        self.file.print(job, sheets)?;
        let pdf = self
            .file
            .output()
            .expect("a print that succeeded wrote sheets")
            .to_vec();
        let settings = NativeSettings::new(job, sheets);
        let printer = match &settings.printer {
            Some(name) => name.clone(),
            None => default_printer()?,
        };
        PrinterDc::open(&printer, &DevMode::new(&settings))?.print(&self.title, pdf)
    }
}

/// The printers the spooler knows, local and connected, by name.
pub fn printers() -> Vec<String> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: a size query; the null buffer with size 0 is what it asks for.
    unsafe { EnumPrintersW(flags, null(), 4, null_mut(), 0, &mut needed, &mut count) };
    if needed == 0 {
        return Vec::new();
    }
    // u64 words, so the buffer is aligned for the structs it is filled with.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` holds `needed` bytes, as the size query asked for.
    let listed = unsafe {
        EnumPrintersW(
            flags,
            null(),
            4,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
            &mut count,
        )
    };
    if listed == 0 {
        return Vec::new();
    }
    // SAFETY: on success the buffer starts with `count` PRINTER_INFO_4W.
    let infos = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<PRINTER_INFO_4W>(), count as usize)
    };
    infos
        .iter()
        // SAFETY: each name points into `buffer`, which is still alive.
        .filter_map(|info| unsafe { from_wide(info.pPrinterName) })
        .collect()
}

/// The printer a job with none chosen goes to.
fn default_printer() -> Result<String, PrintError> {
    let mut length = 0u32;
    // SAFETY: a size query with a null buffer.
    unsafe { GetDefaultPrinterW(null_mut(), &mut length) };
    if length == 0 {
        return Err(PrintError::Platform(
            "Windows has no default printer; choose one".into(),
        ));
    }
    let mut name = vec![0u16; length as usize];
    // SAFETY: `name` holds `length` characters, as asked for.
    if unsafe { GetDefaultPrinterW(name.as_mut_ptr(), &mut length) } == 0 {
        return Err(last_error("find the default printer"));
    }
    // SAFETY: the buffer is nul-terminated on success.
    unsafe { from_wide(name.as_ptr()) }.ok_or_else(|| last_error("read the default printer"))
}

/// A printer device context, deleted when dropped.
struct PrinterDc(HDC);

impl PrinterDc {
    /// A device context for `printer`, set up as `mode` says.
    fn open(printer: &str, mode: &DevMode) -> Result<Self, PrintError> {
        let name = wide(printer);
        let devmode = driver_devmode(&name, mode)?;
        // SAFETY: `name` is nul-terminated and `devmode` is a whole
        // DEVMODEW the driver itself filled in; both outlive the call.
        let dc = unsafe { CreateDCW(null(), name.as_ptr(), null(), devmode.as_ptr()) };
        if dc.is_null() {
            return Err(last_error(&format!("open the printer {printer}")));
        }
        Ok(PrinterDc(dc))
    }

    fn caps(&self, index: u32) -> i32 {
        // SAFETY: `self.0` is a live device context.
        unsafe { GetDeviceCaps(self.0, index as i32) }
    }

    /// Every sheet of `pdf`, as one job titled `title`.
    fn print(&self, title: &str, pdf: Vec<u8>) -> Result<(), PrintError> {
        let dpi = (self.caps(LOGPIXELSX), self.caps(LOGPIXELSY));
        let offset = (self.caps(PHYSICALOFFSETX), self.caps(PHYSICALOFFSETY));
        let sheets = Sheets::new(pdf, render_dpi(dpi.0.min(dpi.1)))?;
        let title = wide(title);
        let info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: title.as_ptr(),
            ..Default::default()
        };
        // SAFETY: `info` and the title it points at outlive the call.
        if unsafe { StartDocW(self.0, &info) } <= 0 {
            return Err(last_error("start the print job"));
        }
        let printed = sheets
            .into_iter()
            .try_for_each(|sheet| self.page(&sheet?, dpi, offset));
        if let Err(error) = printed {
            // SAFETY: a document was started on this context.
            unsafe { AbortDoc(self.0) };
            return Err(error);
        }
        // SAFETY: as above.
        if unsafe { EndDoc(self.0) } <= 0 {
            return Err(last_error("finish the print job"));
        }
        Ok(())
    }

    /// One sheet, covering the paper from its corner.
    fn page(
        &self,
        sheet: &SheetImage,
        dpi: (i32, i32),
        offset: (i32, i32),
    ) -> Result<(), PrintError> {
        // SAFETY: a document is started on this context.
        if unsafe { StartPage(self.0) } <= 0 {
            return Err(last_error("start a sheet"));
        }
        let rect = device_rect(sheet.points, dpi, offset);
        let info = bitmap_info(sheet);
        // SAFETY: `sheet.bgra` holds width × height 32-bit pixels, which is
        // what `info` describes, and both outlive the call.
        let drawn = unsafe {
            SetStretchBltMode(self.0, HALFTONE);
            SetBrushOrgEx(self.0, 0, 0, null_mut());
            StretchDIBits(
                self.0,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                0,
                0,
                sheet.width as i32,
                sheet.height as i32,
                sheet.bgra.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        if drawn <= 0 {
            return Err(last_error("draw a sheet"));
        }
        // SAFETY: a page is started on this context.
        if unsafe { EndPage(self.0) } <= 0 {
            return Err(last_error("finish a sheet"));
        }
        Ok(())
    }
}

impl Drop for PrinterDc {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from CreateDCW and is deleted once, here.
        unsafe { DeleteDC(self.0) };
    }
}

/// A top-down 32-bit bitmap the size of `sheet`.
fn bitmap_info(sheet: &SheetImage) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sheet.width as i32,
            // Negative: the rows run top to bottom, as the renderer writes them.
            biHeight: -(sheet.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The printer's own `DEVMODEW` with `mode` merged in, validated by its
/// driver. Held in u64 words so it is aligned for the struct, and sized as
/// the driver asks, since drivers append private data to it.
fn driver_devmode(name: &[u16], mode: &DevMode) -> Result<DevModeBuffer, PrintError> {
    let printer = Printer::open(name)?;
    // SAFETY: a size query with null buffers.
    let size =
        unsafe { DocumentPropertiesW(null_mut(), printer.0, name.as_ptr(), null_mut(), null(), 0) };
    if size <= 0 {
        return Err(last_error("read the printer's settings"));
    }
    let mut buffer = DevModeBuffer(vec![0u64; (size as usize).div_ceil(8)]);
    // SAFETY: `buffer` holds `size` bytes, as the query asked for.
    let read = unsafe {
        DocumentPropertiesW(
            null_mut(),
            printer.0,
            name.as_ptr(),
            buffer.as_mut_ptr(),
            null(),
            Gdi::DM_OUT_BUFFER,
        )
    };
    if read < 0 {
        return Err(last_error("read the printer's settings"));
    }
    buffer.apply(mode);
    // SAFETY: input and output are the same whole DEVMODEW, which the
    // function documents as allowed.
    let merged = unsafe {
        DocumentPropertiesW(
            null_mut(),
            printer.0,
            name.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.as_ptr(),
            Gdi::DM_IN_BUFFER | Gdi::DM_OUT_BUFFER,
        )
    };
    if merged < 0 {
        return Err(last_error("apply the print settings"));
    }
    Ok(buffer)
}

/// A driver-sized `DEVMODEW`.
struct DevModeBuffer(Vec<u64>);

impl DevModeBuffer {
    fn as_ptr(&self) -> *const DEVMODEW {
        self.0.as_ptr().cast()
    }

    fn as_mut_ptr(&mut self) -> *mut DEVMODEW {
        self.0.as_mut_ptr().cast()
    }

    fn apply(&mut self, mode: &DevMode) {
        // SAFETY: the buffer holds a whole DEVMODEW the driver wrote, and is
        // aligned for it.
        let devmode = unsafe { &mut *self.as_mut_ptr() };
        devmode.dmFields |= mode.fields;
        devmode.dmDuplex = mode.duplex;
        devmode.dmCollate = mode.collate;
        // SAFETY: printers use the first variant of this union; writing
        // plain integers to it is always valid.
        unsafe {
            let paper = &mut devmode.Anonymous1.Anonymous1;
            paper.dmOrientation = mode.orientation;
            paper.dmPaperSize = mode.paper_size;
            paper.dmCopies = mode.copies;
            if mode.fields & settings::DM_PAPERWIDTH != 0 {
                paper.dmPaperWidth = mode.paper_width;
                paper.dmPaperLength = mode.paper_length;
            }
        }
    }
}

/// An open printer handle, closed when dropped.
struct Printer(PRINTER_HANDLE);

impl Printer {
    fn open(name: &[u16]) -> Result<Self, PrintError> {
        let mut handle = PRINTER_HANDLE::default();
        // SAFETY: `name` is nul-terminated; no defaults are passed.
        if unsafe { OpenPrinterW(name.as_ptr(), &mut handle, null()) } == 0 {
            return Err(last_error("find the printer"));
        }
        Ok(Printer(handle))
    }
}

impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenPrinterW and is closed once, here.
        unsafe { ClosePrinter(self.0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The nul-terminated UTF-16 string at `text`, or none for a null pointer.
///
/// # Safety
///
/// `text` is null or points at a nul-terminated UTF-16 string.
unsafe fn from_wide(text: *const u16) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let mut length = 0;
    // SAFETY: the string is nul-terminated, so the walk stops inside it.
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` characters precede the nul.
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(text, length)
    }))
}

fn last_error(what: &str) -> PrintError {
    // SAFETY: no preconditions.
    let code = unsafe { GetLastError() };
    PrintError::Platform(format!("Windows could not {what} (error {code})"))
}
