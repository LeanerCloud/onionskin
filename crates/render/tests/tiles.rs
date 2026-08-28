//! Damage tracking is the whole point of the tile cache, so it is tested
//! against a synthetic base raster rather than a PDF: the grid, the
//! invalidation arithmetic and the compositing are backend independent, and
//! the corpus is not in the repository.

use onionskin_render::tiny_skia::Pixmap;
use onionskin_render::{BaseRaster, DeviceRect, Overlay, OverlayError, Rgba, TileCache, TILE_SIZE};

const WIDTH: u32 = 1000;
const HEIGHT: u32 = 800;

const YELLOW: Rgba = Rgba {
    r: 255,
    g: 235,
    b: 0,
    a: 255,
};
const BLUE: Rgba = Rgba {
    r: 0,
    g: 0,
    b: 200,
    a: 255,
};

/// 4x4 tiles of opaque white, at zoom 1 so page points and device pixels
/// coincide and the expected tile spans are readable.
fn cache() -> TileCache {
    let base = BaseRaster::new(WIDTH, HEIGHT, 1.0, vec![255; (WIDTH * HEIGHT * 4) as usize]);
    TileCache::new(base)
}

fn warm(cache: &TileCache) {
    for row in 0..cache.rows() {
        for col in 0..cache.cols() {
            let _ = cache.tile(col, row);
        }
    }
}

/// A highlight quad with its upper-left corner at `(x, y)`.
fn highlight(x: f32, y: f32, w: f32, h: f32) -> Overlay {
    Overlay::Highlight {
        corners: [(x, y), (x + w, y), (x, y + h), (x + w, y + h)],
        color: YELLOW,
    }
}

fn pixel(page: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let i = (y * page.width() + x) as usize * 4;
    page.data()[i..i + 4].try_into().unwrap()
}

#[test]
fn grid_covers_the_page() {
    let cache = cache();
    assert_eq!((cache.cols(), cache.rows()), (4, 4));
    assert_eq!(TILE_SIZE, 256);
}

#[test]
fn each_tile_composites_once_and_reads_are_free() {
    // `&self`, so a paint pass can walk several pages' tiles at once.
    let cache = cache();
    warm(&cache);
    assert_eq!(cache.composites(), 16);

    warm(&cache);
    warm(&cache);
    assert_eq!(cache.composites(), 16, "cached tiles must not recomposite");
}

#[test]
fn damage_drops_only_the_intersecting_tile() {
    let mut cache = cache();
    warm(&cache);

    let dropped = cache.damage(DeviceRect {
        x: 300.0,
        y: 300.0,
        width: 10.0,
        height: 10.0,
    });
    assert_eq!(dropped, 1);

    warm(&cache);
    assert_eq!(cache.composites(), 17, "only tile (1, 1) may recomposite");
}

#[test]
fn damage_across_a_tile_boundary_drops_both() {
    let mut cache = cache();
    warm(&cache);

    let dropped = cache.damage(DeviceRect {
        x: 250.0,
        y: 50.0,
        width: 20.0,
        height: 20.0,
    });
    assert_eq!(dropped, 2);
}

#[test]
fn damage_ending_on_a_boundary_stops_there() {
    let mut cache = cache();
    warm(&cache);

    let dropped = cache.damage(DeviceRect {
        x: 0.0,
        y: 0.0,
        width: 256.0,
        height: 256.0,
    });
    assert_eq!(dropped, 1, "the far edge is exclusive");
}

#[test]
fn damage_off_the_page_drops_nothing() {
    let mut cache = cache();
    warm(&cache);

    for rect in [
        DeviceRect {
            x: 2000.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
        },
        DeviceRect {
            x: -100.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
        },
        DeviceRect {
            x: 10.0,
            y: 10.0,
            width: 0.0,
            height: 50.0,
        },
    ] {
        assert_eq!(cache.damage(rect), 0, "{rect:?}");
    }
    assert_eq!(cache.composites(), 16);
}

#[test]
fn an_ink_stroke_damages_only_the_tiles_it_crosses() {
    let mut cache = cache();
    warm(&cache);

    let before = cache.composites();
    let damaged = cache.add_overlay(Overlay::Ink {
        points: vec![(300.0, 300.0), (400.0, 400.0)],
        color: BLUE,
        width: 4.0,
    });
    assert_eq!(damaged, Ok(1));

    // The M1 finding: an ink stroke inside one tile costs one composite and
    // never re-enters the interpreter.
    warm(&cache);
    assert_eq!(cache.composites() - before, 1);
}

#[test]
fn a_degenerate_overlay_is_refused_and_never_reaches_a_composite() {
    let mut cache = cache();
    warm(&cache);

    let refused = [
        (
            Overlay::Ink {
                points: vec![(300.0, 300.0)],
                color: BLUE,
                width: 4.0,
            },
            OverlayError::TooFewPoints { points: 1 },
        ),
        (
            Overlay::Ink {
                points: vec![(300.0, 300.0), (f32::NAN, 400.0)],
                color: BLUE,
                width: 4.0,
            },
            OverlayError::NotFinite {
                x: f32::NAN,
                y: 400.0,
            },
        ),
        (
            Overlay::Ink {
                points: vec![(300.0, 300.0), (400.0, 400.0)],
                color: BLUE,
                width: f32::INFINITY,
            },
            OverlayError::InvalidWidth {
                width: f32::INFINITY,
            },
        ),
        (
            Overlay::Ink {
                points: vec![(300.0, 300.0), (400.0, 400.0)],
                color: BLUE,
                width: 0.0,
            },
            OverlayError::InvalidWidth { width: 0.0 },
        ),
        (
            highlight(f32::INFINITY, 300.0, 100.0, 40.0),
            OverlayError::NotFinite {
                x: f32::INFINITY,
                y: 300.0,
            },
        ),
        (
            // What a selection of no characters produces.
            highlight(300.0, 300.0, 100.0, 0.0),
            OverlayError::EmptyQuad,
        ),
        (
            // Collinear corners: a quad with a real bounding box and no area,
            // which is what a broken coordinate mapping produces.
            Overlay::Highlight {
                corners: [
                    (300.0, 300.0),
                    (400.0, 340.0),
                    (300.0, 300.0),
                    (400.0, 340.0),
                ],
                color: YELLOW,
            },
            OverlayError::EmptyQuad,
        ),
    ];

    for (overlay, expected) in refused {
        match cache.add_overlay(overlay) {
            // NaN never compares equal, so the variant is what can be asserted.
            Err(e) => assert_eq!(
                std::mem::discriminant(&e),
                std::mem::discriminant(&expected),
                "expected {expected}, got {e}"
            ),
            Ok(damaged) => panic!("accepted a degenerate overlay, damaging {damaged} tiles"),
        }
    }

    // Refused means gone: nothing was damaged, and no tile recomposites with a
    // rejected overlay in the draw list.
    assert_eq!(cache.composites(), 16);
    warm(&cache);
    assert_eq!(cache.composites(), 16);
    for row in 0..cache.rows() {
        for col in 0..cache.cols() {
            assert_eq!(cache.overlays_in_tile(col, row), 0, "tile ({col}, {row})");
        }
    }
}

#[test]
fn a_highlight_multiplies_its_quad_and_leaves_the_rest_alone() {
    let mut cache = cache();
    cache
        .add_overlay(highlight(300.0, 300.0, 100.0, 40.0))
        .expect("a finite quad");

    // The whole page, not one pixel inside and one outside: a composite that
    // painted the quad at the wrong offset, or smeared it across a tile, still
    // passes a two-pixel check.
    let page = cache.page_image();
    let quad = (300..400, 300..340);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let inside = quad.0.contains(&x) && quad.1.contains(&y);
            let expected = if inside {
                [255, 235, 0, 255]
            } else {
                [255, 255, 255, 255]
            };
            assert_eq!(pixel(&page, x, y), expected, "at ({x}, {y})");
        }
    }
}

#[test]
fn overlay_coordinates_scale_with_zoom() {
    // Same page, rasterized at 2x: overlay coordinates stay in page points,
    // so the quad has to land at twice the device offset.
    let base = BaseRaster::new(
        WIDTH * 2,
        HEIGHT * 2,
        2.0,
        vec![255; (WIDTH * 2 * HEIGHT * 2 * 4) as usize],
    );
    let mut cache = TileCache::new(base);
    cache
        .add_overlay(highlight(300.0, 300.0, 100.0, 40.0))
        .expect("a finite quad");

    // The quad covers page points 300..400 by 300..340, so at 2x its device
    // edges are exactly 600 and 800, 600 and 680. Checked on both sides of an
    // edge, because a placement a few pixels out still lands inside.
    let page = cache.page_image();
    assert_eq!(pixel(&page, 700, 640), [255, 235, 0, 255], "inside");
    assert_eq!(pixel(&page, 599, 640), [255, 255, 255, 255], "left of it");
    assert_eq!(pixel(&page, 601, 640), [255, 235, 0, 255], "just inside");
    assert_eq!(pixel(&page, 700, 599), [255, 255, 255, 255], "above it");
    assert_eq!(pixel(&page, 700, 679), [255, 235, 0, 255], "just inside");
    assert_eq!(pixel(&page, 700, 681), [255, 255, 255, 255], "below it");
    assert_eq!(pixel(&page, 350, 320), [255, 255, 255, 255], "1x position");
}

#[test]
fn a_tile_paints_only_the_overlays_indexed_to_it() {
    let mut cache = cache();
    for i in 0..50 {
        cache
            .add_overlay(highlight(10.0 + i as f32, 10.0, 20.0, 20.0))
            .expect("a finite quad");
    }
    cache
        .add_overlay(highlight(800.0, 780.0, 20.0, 10.0))
        .expect("a finite quad");

    assert_eq!(cache.overlays_in_tile(0, 0), 50);
    assert_eq!(cache.overlays_in_tile(3, 3), 1, "the far quad, and only it");
    assert_eq!(cache.overlays_in_tile(2, 2), 0, "no overlay reaches here");
}

#[test]
fn tiles_past_the_page_edge_stay_transparent() {
    let cache = cache();
    let corner = cache.tile(3, 3);

    // Tile (3, 3) starts at (768, 768); the page ends at (1000, 800).
    let at = |x: u32, y: u32| {
        let i = (y * TILE_SIZE + x) as usize * 4;
        <[u8; 4]>::try_from(&corner.rgba()[i..i + 4]).unwrap()
    };
    assert_eq!(at(0, 0), [255, 255, 255, 255]);
    assert_eq!(at(255, 0), [0, 0, 0, 0], "x 1023 is past the page");
    assert_eq!(at(0, 255), [0, 0, 0, 0], "y 1023 is past the page");
}
