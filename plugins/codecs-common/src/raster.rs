//! A rendered page as the opaque RGB the formats without alpha store.

use onionskin_plugin_api::BaseRaster;

/// The raster's premultiplied RGBA composited over white, as RGB.
///
/// Today every base raster is already opaque (the renderer paints the page
/// white first), which makes this dropping the alpha byte. It composites
/// rather than drops so a transparent render could never come out with black
/// where the page was clear.
pub(crate) fn rgb_over_white(raster: &BaseRaster) -> Vec<u8> {
    let mut out = Vec::with_capacity(raster.rgba().len() / 4 * 3);
    for pixel in raster.rgba().chunks_exact(4) {
        let clear = u8::MAX - pixel[3];
        out.extend(
            pixel[..3]
                .iter()
                .map(|channel| channel.saturating_add(clear)),
        );
    }
    out
}

/// A render resolution as whole pixels per inch, for the formats that store
/// it as an integer.
pub(crate) fn whole_dpi(dpi: f32) -> u16 {
    dpi.round().clamp(1.0, f32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_pixels_keep_their_colour_and_clear_ones_are_white() {
        let raster = BaseRaster::new(3, 1, 1.0, vec![10, 20, 30, 255, 0, 0, 0, 0, 64, 0, 0, 128]);
        assert_eq!(
            rgb_over_white(&raster),
            vec![10, 20, 30, 255, 255, 255, 191, 127, 127]
        );
    }

    #[test]
    fn a_resolution_rounds_and_stays_in_range() {
        assert_eq!(whole_dpi(299.6), 300);
        assert_eq!(whole_dpi(0.1), 1);
        assert_eq!(whole_dpi(1.0e9), u16::MAX);
    }
}
