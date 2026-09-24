//! The sheets as pictures for GDI.
//!
//! Windows has no built-in way to print a PDF, so each sheet the file
//! backend wrote is rendered by our own renderer and handed to the printer
//! as a bitmap: the same sheets, drawn by the same renderer the screen uses.
//! One sheet is held at a time, and the resolution is capped, because a
//! 1200 dpi A3 sheet would be about 200 MB of pixels.

use onionskin_core::Document;

use crate::backend::PrintError;

/// The resolution sheets are rendered at when the printer's is higher: fine
/// print for text and line art, at 34 MB a Letter sheet.
pub const MAX_DPI: i32 = 300;

/// The resolution to render at for a printer reporting `printer_dpi`: its
/// own, up to [`MAX_DPI`], and [`MAX_DPI`] when it reports none.
pub fn render_dpi(printer_dpi: i32) -> i32 {
    if printer_dpi <= 0 {
        MAX_DPI
    } else {
        printer_dpi.min(MAX_DPI)
    }
}

/// One sheet as a top-down 32-bit BGRA bitmap, opaque, as `StretchDIBits`
/// takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetImage {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// The sheet's size in points, which is what its placement on the
    /// paper is worked out from.
    pub points: (f64, f64),
}

impl SheetImage {
    /// A bitmap of premultiplied RGBA pixels, laid on white paper.
    pub fn from_premultiplied(width: u32, height: u32, rgba: &[u8], points: (f64, f64)) -> Self {
        let bgra = rgba
            .chunks_exact(4)
            .flat_map(|px| {
                let paper = 255 - px[3];
                // Premultiplied: over white, each channel gains what the
                // pixel does not cover.
                let over = |channel: u8| channel.saturating_add(paper);
                [over(px[2]), over(px[1]), over(px[0]), 255]
            })
            .collect();
        SheetImage {
            width,
            height,
            bgra,
            points,
        }
    }
}

/// Where a sheet goes on the device, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The whole sheet, `points` in size, on paper whose device reports `dpi`
/// and a printable area starting `offset` pixels in from the paper's corner.
/// GDI's origin is the printable area's corner, so the paper's corner is up
/// and to the left of it; placing the sheet there keeps every mark where the
/// sheet put it, and the device clips what falls outside the printable area.
pub fn device_rect(points: (f64, f64), dpi: (i32, i32), offset: (i32, i32)) -> DeviceRect {
    let pixels = |points: f64, dpi: i32| (points / 72.0 * f64::from(dpi)).round() as i32;
    DeviceRect {
        x: -offset.0,
        y: -offset.1,
        width: pixels(points.0, dpi.0),
        height: pixels(points.1, dpi.1),
    }
}

/// The sheets of `pdf`, the file backend's output, rendered at `dpi` one at
/// a time as they are asked for.
pub struct Sheets {
    document: Document,
    next: usize,
    dpi: i32,
}

impl Sheets {
    pub fn new(pdf: Vec<u8>, dpi: i32) -> Result<Self, PrintError> {
        Ok(Sheets {
            document: Document::open_bytes(pdf)?,
            next: 0,
            dpi,
        })
    }

    pub fn sheet_count(&self) -> usize {
        self.document.page_count()
    }

    fn render(&mut self, sheet: usize) -> Result<SheetImage, PrintError> {
        let points = self.document.page_geometry(sheet)?.render_size;
        let render = self
            .document
            .render_page_now(sheet, self.dpi as f32 / 72.0)?;
        let raster = &render.raster;
        Ok(SheetImage::from_premultiplied(
            raster.width(),
            raster.height(),
            raster.rgba(),
            points,
        ))
    }
}

impl Iterator for Sheets {
    type Item = Result<SheetImage, PrintError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.sheet_count() {
            return None;
        }
        let sheet = self.next;
        self.next += 1;
        Some(self.render(sheet))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printer_resolution_is_capped_and_a_missing_one_defaults() {
        assert_eq!(render_dpi(600), MAX_DPI);
        assert_eq!(render_dpi(150), 150);
        assert_eq!(render_dpi(0), MAX_DPI);
        assert_eq!(render_dpi(-1), MAX_DPI);
    }

    #[test]
    fn pixels_become_opaque_bgra_on_white() {
        // Opaque red, transparent, and half-covered black (premultiplied 0).
        let rgba = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 128];
        let image = SheetImage::from_premultiplied(3, 1, &rgba, (3.0, 1.0));
        assert_eq!(
            image.bgra,
            [0, 0, 255, 255, 255, 255, 255, 255, 127, 127, 127, 255]
        );
        assert_eq!(image.points, (3.0, 1.0));
    }

    /// A Letter sheet on a 600 dpi printer whose printable area starts a
    /// quarter inch in: the sheet covers the whole paper, starting at the
    /// paper's corner, up and left of the device origin.
    #[test]
    fn a_sheet_covers_the_paper_from_its_corner() {
        assert_eq!(
            device_rect((612.0, 792.0), (600, 600), (150, 150)),
            DeviceRect {
                x: -150,
                y: -150,
                width: 5100,
                height: 6600,
            }
        );
        assert_eq!(
            device_rect((612.0, 792.0), (300, 600), (0, 0)).width,
            2550,
            "each axis at its own resolution"
        );
    }
}
