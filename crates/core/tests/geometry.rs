use onionskin_core::Document;
use onionskin_render::{Document as RenderDocument, RenderOptions};

#[test]
fn crop_and_rotation_map_extracted_glyphs_onto_rendered_pixels() {
    for rotation in [0, 90, 180, 270] {
        let bytes = geometry_pdf(rotation);
        let mut session = Document::open_bytes(bytes.clone()).expect("session opens");
        let text = session.page_text(0).expect("text extracts");
        let glyph_quad = text
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .find(|glyph| !glyph.is_unmapped())
            .expect("fixture contains a mapped glyph")
            .quad;
        let geometry = session.page_geometry(0).expect("geometry loads").clone();
        let expected_size = if rotation == 0 || rotation == 180 {
            (200.0, 150.0)
        } else {
            (150.0, 200.0)
        };
        assert_eq!(geometry.render_size, expected_size);
        let renderer = RenderDocument::open(bytes).expect("renderer opens");

        for zoom in [1.0, 3.0] {
            let device = geometry
                .user_to_device(glyph_quad, zoom)
                .expect("glyph belongs to the page");
            let page = renderer
                .render_page(0, zoom, &RenderOptions::default())
                .expect("page renders");
            assert!(
                quad_contains_mark(
                    &device.corners,
                    page.raster.width(),
                    page.raster.height(),
                    page.raster.rgba()
                ),
                "rotation {rotation} at zoom {zoom} mapped the glyph away from its pixels"
            );
        }
    }
}

#[test]
fn crop_and_intrinsic_rotation_round_trip_between_user_and_device_space() {
    for rotation in [0, 90, 180, 270] {
        let mut session = Document::open_bytes(geometry_pdf(rotation)).expect("session opens");
        let geometry = session.page_geometry(0).expect("geometry loads");
        let quad = onionskin_core::PageQuad {
            page: 0,
            corners: [(70.0, 120.0), (180.0, 120.0), (70.0, 60.0), (180.0, 60.0)],
        };

        for zoom in [1.0, 3.0] {
            let device = geometry
                .user_to_device(quad, zoom)
                .expect("quad belongs to the page");
            for (expected, (x, y)) in quad.corners.into_iter().zip(device.corners) {
                let actual = geometry
                    .device_to_user(x, y, zoom)
                    .expect("device point maps back to the page");
                assert_eq!(actual.page, 0);
                assert_close(actual.x, expected.0, rotation, zoom);
                assert_close(actual.y, expected.1, rotation, zoom);
            }
        }
    }
}

#[test]
fn mapping_rejects_the_wrong_page_and_invalid_zoom() {
    let mut session = Document::open_bytes(geometry_pdf(90)).expect("session opens");
    let geometry = session.page_geometry(0).expect("geometry loads");
    let quad = onionskin_core::PageQuad {
        page: 1,
        corners: [(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
    };
    assert!(geometry.user_to_device(quad, 1.0).is_err());

    let quad = onionskin_core::PageQuad { page: 0, ..quad };
    for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(geometry.user_to_device(quad, zoom).is_err());
        assert!(geometry.device_to_user(1.0, 1.0, zoom).is_err());
    }
}

fn assert_close(actual: f64, expected: f64, rotation: i32, zoom: f32) {
    assert!(
        (actual - expected).abs() < 1e-8,
        "rotation {rotation} at zoom {zoom}: {actual} != {expected}"
    );
}

fn quad_contains_mark(corners: &[(f64, f64); 4], width: u32, height: u32, rgba: &[u8]) -> bool {
    let min_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as u32;
    let max_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(f64::from(width)) as u32;
    let min_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as u32;
    let max_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(f64::from(height)) as u32;

    (min_y..max_y).any(|y| {
        (min_x..max_x).any(|x| {
            let offset = (y as usize * width as usize + x as usize) * 4;
            rgba[offset..offset + 3] != [255, 255, 255]
        })
    })
}

fn geometry_pdf(rotation: i32) -> Vec<u8> {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [10 20 310 220] /CropBox [60 45 260 195] /Rotate {rotation} /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        b"<< /Length 32 >>\nstream\nBT /F1 20 Tf 80 80 Td (X) Tj ET\nendstream".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}
