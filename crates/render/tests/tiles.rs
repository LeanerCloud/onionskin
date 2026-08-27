//! Damage tracking is the whole point of the tile cache, so it is tested
//! against a synthetic base raster rather than a PDF: the grid, the
//! invalidation arithmetic and the compositing are backend independent, and
//! the corpus is not in the repository.

use onionskin_render::tiny_skia::Pixmap;
use onionskin_render::{BaseRaster, DeviceRect, Overlay, Rgba, TileCache, TILE_SIZE};

const WIDTH: u32 = 1000;
const HEIGHT: u32 = 800;

/// 4x4 tiles of opaque white, at zoom 1 so page points and device pixels
/// coincide and the expected tile spans are readable.
fn cache() -> TileCache {
    let base = BaseRaster::new(WIDTH, HEIGHT, 1.0, vec![255; (WIDTH * HEIGHT * 4) as usize]);
    TileCache::new(base)
}

fn warm(cache: &mut TileCache) {
    for row in 0..cache.rows() {
        for col in 0..cache.cols() {
            let _ = cache.tile(col, row);
        }
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
    let mut cache = cache();
    warm(&mut cache);
    assert_eq!(cache.composites(), 16);

    warm(&mut cache);
    warm(&mut cache);
    assert_eq!(cache.composites(), 16, "cached tiles must not recomposite");
}

#[test]
fn damage_drops_only_the_intersecting_tile() {
    let mut cache = cache();
    warm(&mut cache);

    let dropped = cache.damage(DeviceRect {
        x: 300.0,
        y: 300.0,
        width: 10.0,
        height: 10.0,
    });
    assert_eq!(dropped, 1);

    warm(&mut cache);
    assert_eq!(cache.composites(), 17, "only tile (1, 1) may recomposite");
}

#[test]
fn damage_across_a_tile_boundary_drops_both() {
    let mut cache = cache();
    warm(&mut cache);

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
    warm(&mut cache);

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
    warm(&mut cache);

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
    warm(&mut cache);

    let damaged = cache.add_overlay(Overlay::Ink {
        points: vec![(300.0, 300.0), (400.0, 400.0)],
        color: Rgba {
            r: 0,
            g: 0,
            b: 200,
            a: 255,
        },
        width: 4.0,
    });
    assert_eq!(damaged, 1);

    warm(&mut cache);
    assert_eq!(cache.composites(), 17);
}

#[test]
fn a_degenerate_ink_stroke_damages_nothing() {
    let mut cache = cache();
    warm(&mut cache);

    let damaged = cache.add_overlay(Overlay::Ink {
        points: vec![(300.0, 300.0)],
        color: Rgba {
            r: 0,
            g: 0,
            b: 200,
            a: 255,
        },
        width: 4.0,
    });
    assert_eq!(damaged, 0);
    assert_eq!(cache.composites(), 16);
}

#[test]
fn a_highlight_multiplies_its_quad_and_leaves_the_rest_alone() {
    let mut cache = cache();
    cache.add_overlay(Overlay::Highlight {
        corners: [
            (300.0, 300.0),
            (400.0, 300.0),
            (300.0, 340.0),
            (400.0, 340.0),
        ],
        color: Rgba {
            r: 255,
            g: 235,
            b: 0,
            a: 255,
        },
    });

    let page = cache.page_image();
    assert_eq!(pixel(&page, 350, 320), [255, 235, 0, 255]);
    assert_eq!(pixel(&page, 600, 600), [255, 255, 255, 255]);
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
    cache.add_overlay(Overlay::Highlight {
        corners: [
            (300.0, 300.0),
            (400.0, 300.0),
            (300.0, 340.0),
            (400.0, 340.0),
        ],
        color: Rgba {
            r: 255,
            g: 235,
            b: 0,
            a: 255,
        },
    });

    let page = cache.page_image();
    assert_eq!(pixel(&page, 700, 640), [255, 235, 0, 255]);
    assert_eq!(pixel(&page, 350, 320), [255, 255, 255, 255], "1x position");
}

#[test]
fn tiles_past_the_page_edge_stay_transparent() {
    let mut cache = cache();
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
