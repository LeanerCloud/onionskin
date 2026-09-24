//! What an applied mark leaves on the page: its areas filled, and its
//! overlay text over them.

use onionskin_content::{encode_win_ansi, standard_text_width};
use onionskin_core::redactions::{redaction_areas, Align, Overlay, RedactionMark};

/// The resource name the overlay's Helvetica is given.
pub(crate) const FONT_NAME: &str = "OsRdHelv";
const FONT: &str = "Helvetica";
/// Room left between overlay text and the edge of its area, in points.
const INSET: f64 = 1.0;

/// The content that draws `marks`' fills and overlay text, in page space.
pub(crate) fn drawing(marks: &[&RedactionMark]) -> Vec<u8> {
    let mut out = String::new();
    for mark in marks {
        if let Some([r, g, b]) = mark.look.fill {
            out.push_str(&format!("q {r} {g} {b} rg\n"));
            for area in redaction_areas(mark) {
                let [a, b, c, d] = area.corners();
                out.push_str(&format!(
                    "{} {} m {} {} l {} {} l {} {} l h f\n",
                    a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1
                ));
            }
            out.push_str("Q\n");
        }
        if let Some(overlay) = &mark.look.overlay {
            out.push_str(&text(mark.rect, overlay));
        }
    }
    out.into_bytes()
}

/// Whether any of `marks` writes text, and so needs the font.
pub(crate) fn needs_font(marks: &[&RedactionMark]) -> bool {
    marks.iter().any(|mark| mark.look.overlay.is_some())
}

fn text([x0, y0, x1, y1]: [f64; 4], overlay: &Overlay) -> String {
    let bytes = encode_win_ansi(&overlay.text);
    let (width, height) = (x1 - x0, y1 - y0);
    let unit = standard_text_width(FONT, &bytes).unwrap_or(0.0) / 1000.0;
    if unit <= 0.0 || width <= 2.0 * INSET || height <= 0.0 {
        return String::new();
    }
    let size = if overlay.size > 0.0 {
        overlay.size
    } else {
        ((width - 2.0 * INSET) / unit).min(height * 0.8)
    };
    let line = unit * size;
    let (copies, rows) = if overlay.repeat {
        let gap = size * 0.5;
        let copies = (((width - 2.0 * INSET) + gap) / (line + gap))
            .floor()
            .max(1.0) as usize;
        let rows = (height / (size * 1.2)).floor().max(1.0) as usize;
        (copies, rows)
    } else {
        (1, 1)
    };
    let row: Vec<u8> = vec![bytes.as_slice(); copies].join(&b' ');
    let row_width = standard_text_width(FONT, &row).unwrap_or(0.0) / 1000.0 * size;
    let x = match overlay.align {
        Align::Left => x0 + INSET,
        Align::Centre => x0 + (width - row_width) / 2.0,
        Align::Right => x1 - INSET - row_width,
    };
    let leading = size * 1.2;
    let block = leading * (rows as f64 - 1.0) + size * 0.7;
    let top = y0 + (height + block) / 2.0 - size * 0.7;
    let [r, g, b] = overlay.color;
    let mut out = format!(
        "q {x0} {y0} {width} {height} re W n BT /{FONT_NAME} {size} Tf {r} {g} {b} rg {leading} TL {x} {top} Td\n"
    );
    for index in 0..rows {
        if index > 0 {
            out.push_str("T* ");
        }
        out.push('<');
        for byte in &row {
            out.push_str(&format!("{byte:02X}"));
        }
        out.push_str("> Tj\n");
    }
    out.push_str("ET Q\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_core::redactions::RedactionLook;
    use onionskin_core::ObjRef;

    fn mark(overlay: Option<Overlay>, fill: Option<[f64; 3]>) -> RedactionMark {
        RedactionMark {
            objref: ObjRef::new(9, 0),
            page: 0,
            rect: [100.0, 100.0, 300.0, 120.0],
            quads: Vec::new(),
            look: RedactionLook {
                fill,
                overlay,
                ..RedactionLook::default()
            },
        }
    }

    fn drawn(mark: &RedactionMark) -> String {
        String::from_utf8(drawing(&[mark])).expect("ascii")
    }

    #[test]
    fn a_fill_covers_each_area() {
        let black = mark(None, Some([0.0; 3]));
        assert_eq!(
            drawn(&black),
            "q 0 0 0 rg\n100 100 m 300 100 l 300 120 l 100 120 l h f\nQ\n"
        );
        assert_eq!(drawn(&mark(None, None)), "");
        assert!(!needs_font(&[&black]));
    }

    #[test]
    fn overlay_text_fits_aligns_and_repeats() {
        let overlay = Overlay {
            text: "(b)(6)".to_owned(),
            ..Overlay::default()
        };
        let fitted = mark(Some(overlay.clone()), None);
        let text = drawn(&fitted);
        assert!(
            text.contains("/OsRdHelv 16 Tf"),
            "fitted to the height: {text}"
        );
        assert_eq!(text.matches("> Tj").count(), 1);
        assert!(needs_font(&[&fitted]));

        let repeated = Overlay {
            size: 6.0,
            repeat: true,
            align: Align::Left,
            ..overlay.clone()
        };
        let text = drawn(&mark(Some(repeated), None));
        assert!(
            text.contains("/OsRdHelv 6 Tf") && text.contains(" 101 "),
            "{text}"
        );
        assert!(text.contains("T* "), "two rows fit: {text}");
        let right = Overlay {
            size: 10.0,
            align: Align::Right,
            ..overlay.clone()
        };
        assert!(drawn(&mark(Some(right), None)).contains("/OsRdHelv 10 Tf"));
        let nothing = Overlay {
            text: String::new(),
            ..overlay
        };
        assert_eq!(drawn(&mark(Some(nothing), None)), "");
    }
}
