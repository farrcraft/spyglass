//! Where the map looks: its centre, scale and height, and which tiles cover the window. Screen
//! offsets are in egui points from the map's centre, x right (east) and y down (south).

use serde::{Deserialize, Serialize};
use spyglass::{DEFAULT_MAP_Y, MAX_SCALE, MapArea};

/// A tile's side in pixels. One tile pixel is drawn as one point. Small enough that a window
/// up close is many jobs, so the last ones to finish are short.
pub const TILE: u32 = 128;
/// How far from 0 the centre may go, in blocks: past the world border, short of overflowing
/// block coordinates.
const MAX_CENTRE: f64 = 1_000_000_000.0;

/// A tile: `TILE` × `TILE` pixels at a scale, numbered from the world origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    /// Blocks per pixel.
    pub scale: u32,
    /// The tile's column, east of the origin: its west edge is at `x * TILE * scale`.
    pub x: i32,
    /// The tile's row, south of the origin: its north edge is at `z * TILE * scale`.
    pub z: i32,
}

/// A square of chunks, `size` on a side, numbered from the world origin: what features and loot
/// containers are kept by, whatever the view's scale, so zooming in or out draws from what's
/// known. Each feature or container belongs to the region of the chunk that placed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    /// Chunks on a side.
    pub size: i32,
    /// The region's column, east of the origin: its west chunk is `x * size`.
    pub x: i32,
    /// The region's row, south of the origin: its north chunk is `z * size`.
    pub z: i32,
}

/// Chunks on a side of a features or loot region: a 1:2 tile's, so a 1:1 tile is a quarter of
/// one. Each region's job carves the chunks around its own, so smaller regions cost more in
/// all: over a 1:2 view of 80 tiles on 26.3, dungeons took 357 s of work in regions of 16,
/// 672 s in regions of 8, and 401 s by tile.
pub const FEATURE_REGION: i32 = 16;

/// Chunks on a side of a structure starts region: a 1:4 tile's. Starts are cheap per chunk,
/// but confirming them with terrain isn't, and 1:4 to 1:1 is the common zoom while confirming.
pub const START_REGION: i32 = 32;

/// The coarsest scale whose starts are kept in [`START_REGION`]s, the scale structure bounds
/// are drawn at, where starts are confirmed and zoomed into. Coarser, each tile is one region
/// of its own chunks: a 1280 × 800 view at 1:16 is over 1,100 regions of 32 chunks, ring
/// included, but 70 tiles.
pub const MAX_START_REGION_SCALE: u32 = 4;

/// The blocks `tiles` cover together, the least and the greatest (x, z), or `None` for no
/// tiles. The tiles are all at the first one's scale.
pub fn blocks_under(tiles: &[Tile]) -> Option<([i64; 2], [i64; 2])> {
    let first = tiles.first()?;
    let side = i64::from(TILE) * i64::from(first.scale);
    let (mut lo, mut hi) = ([i64::MAX; 2], [i64::MIN; 2]);
    for tile in tiles {
        for (axis, i) in [tile.x, tile.z].into_iter().enumerate() {
            lo[axis] = lo[axis].min(i64::from(i) * side);
            hi[axis] = hi[axis].max((i64::from(i) + 1) * side - 1);
        }
    }
    Some((lo, hi))
}

impl Region {
    /// The region's first and last chunk, inclusive.
    pub fn chunks(self) -> ([i32; 2], [i32; 2]) {
        let first = |i: i32| i.saturating_mul(self.size);
        let min = [first(self.x), first(self.z)];
        (min, min.map(|m| m.saturating_add(self.size - 1)))
    }
}

impl Tile {
    /// The tile's map area at height `y`, or `None` if it falls outside block coordinates.
    pub fn area(self, y: i32) -> Option<MapArea> {
        let side = i64::from(TILE) * i64::from(self.scale);
        let min = [self.x, self.z].map(|t| i32::try_from(i64::from(t) * side).ok());
        let area = MapArea {
            min: [min[0]?, min[1]?],
            size: [TILE, TILE],
            scale: self.scale,
            y,
        };
        area.check().ok().map(|()| area)
    }

    /// The tile at twice the scale that contains this one, and which quarter of it this is
    /// (`[0 or 1, 0 or 1]`, x then z).
    pub fn parent(self) -> Option<(Tile, [i32; 2])> {
        (self.scale < MAX_SCALE).then(|| {
            (
                Tile {
                    scale: self.scale * 2,
                    x: self.x.div_euclid(2),
                    z: self.z.div_euclid(2),
                },
                [self.x.rem_euclid(2), self.z.rem_euclid(2)],
            )
        })
    }

    /// Whether this tile has terrain layers (relief, surface biomes) in a view at
    /// `view_scale`: always as one of the view's own tiles, and as a stand-in only up to
    /// `MAX_COARSE_TERRAIN_SCALE`.
    pub fn has_terrain(self, view_scale: u32) -> bool {
        self.scale == view_scale || self.scale <= MAX_COARSE_TERRAIN_SCALE
    }
}

/// How much coarser the stand-in tiles drawn first are: four times the scale, so one covers 16
/// of the view's tiles and, past `MAX_COARSE_TERRAIN_SCALE`, needs no heights.
pub const COARSE: u32 = 4;

/// The largest scale a stand-in tile has terrain layers at. Closer, neighbouring columns share
/// noise cells, so a coarser tile's heights cost less than its children's; from scale 4 out a
/// column costs as much or more at each coarser scale (`docs/specs/render.md#heights`), and a
/// stand-in with heights would take as long as the tiles it stands in for. Past it, stand-ins
/// show biomes at the view's height.
pub const MAX_COARSE_TERRAIN_SCALE: u32 = 4;

/// The coarser tiles containing `tiles`, each once, in the order first needed: what to draw
/// first so the view fills quickly. None if the view is too coarse to have them.
pub fn coarse(tiles: &[Tile]) -> Vec<Tile> {
    let mut out: Vec<Tile> = Vec::new();
    for &tile in tiles {
        let Some((parent, _)) = tile.parent() else {
            continue;
        };
        let Some((grandparent, _)) = parent.parent() else {
            continue;
        };
        debug_assert_eq!(grandparent.scale, tile.scale * COARSE);
        if !out.contains(&grandparent) {
            out.push(grandparent);
        }
    }
    out
}

/// The map's view.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct View {
    /// The block at the centre of the map.
    pub centre: [f64; 2],
    /// Blocks per pixel: a power of two from 1 to [`MAX_SCALE`].
    pub scale: u32,
    /// The height biomes are drawn at.
    pub y: i32,
}

impl Default for View {
    fn default() -> Self {
        View {
            centre: [0.0, 0.0],
            scale: 4,
            y: DEFAULT_MAP_Y,
        }
    }
}

impl View {
    /// Fixes a view read from settings: a valid scale, a centre in range.
    pub fn sanitized(mut self) -> View {
        if !self.scale.is_power_of_two() || self.scale > MAX_SCALE {
            self.scale = View::default().scale;
        }
        self.centre = self.centre.map(|c| {
            if c.is_finite() {
                c.clamp(-MAX_CENTRE, MAX_CENTRE)
            } else {
                0.0
            }
        });
        self
    }

    /// The pixel offset of the world's block grid: where block (0, 0)'s pixel corner is, from the
    /// map's centre, rounded so tiles land on whole pixels.
    pub fn origin(&self) -> [f64; 2] {
        let blocks_per_point = f64::from(self.scale);
        self.centre.map(|c| (-c / blocks_per_point).round())
    }

    /// The block under a screen offset from the map's centre.
    pub fn block_at(&self, offset: [f32; 2]) -> [f64; 2] {
        let blocks_per_point = f64::from(self.scale);
        let origin = self.origin();
        [0, 1].map(|axis| (f64::from(offset[axis]) - origin[axis]) * blocks_per_point)
    }

    /// Moves the map by `delta` points, as when dragged.
    pub fn pan(&mut self, delta: [f32; 2]) {
        let blocks_per_point = f64::from(self.scale);
        let centre = self.centre;
        self.centre = [0, 1].map(|axis| {
            (centre[axis] - f64::from(delta[axis]) * blocks_per_point)
                .clamp(-MAX_CENTRE, MAX_CENTRE)
        });
    }

    /// Zooms one step in (halving the scale) or out, keeping the block under `offset` still.
    pub fn zoom(&mut self, zoom_in: bool, offset: [f32; 2]) {
        let scale = if zoom_in {
            (self.scale / 2).max(1)
        } else {
            (self.scale * 2).min(MAX_SCALE)
        };
        let fixed = self.block_at(offset);
        self.scale = scale;
        let blocks_per_point = f64::from(scale);
        self.centre = [0, 1].map(|axis| {
            (fixed[axis] - f64::from(offset[axis]) * blocks_per_point)
                .clamp(-MAX_CENTRE, MAX_CENTRE)
        });
    }

    /// The regions of `size` chunks under `tiles`, and one more all round for what's placed
    /// beside its chunk or reaches in from outside, nearest the view's centre first.
    pub fn regions(&self, tiles: &[Tile], size: i32) -> Vec<Region> {
        // The tiles' blocks, then their chunks, then their regions.
        let Some((lo, hi)) = blocks_under(tiles) else {
            return Vec::new();
        };
        let region = |block: i64| (block >> 4).div_euclid(i64::from(size));
        let range = |axis: usize| {
            let clamp = |r: i64| r.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            clamp(region(lo[axis]) - 1)..=clamp(region(hi[axis]) + 1)
        };
        let mut regions: Vec<Region> = range(1)
            .flat_map(|z| range(0).map(move |x| Region { size, x, z }))
            .collect();
        let blocks = f64::from(size) * 16.0;
        let distance = |r: &Region| {
            let off = |i: i32, centre: f64| (f64::from(i) + 0.5) * blocks - centre;
            let (dx, dz) = (off(r.x, self.centre[0]), off(r.z, self.centre[1]));
            dx * dx + dz * dz
        };
        regions.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        regions
    }

    /// The regions structure starts are kept by for the view's `tiles`, nearest the centre
    /// first: those of [`START_REGION`] chunks under them and a ring up to
    /// [`MAX_START_REGION_SCALE`], and past it one region per tile, the tile's own chunks.
    pub fn start_regions(&self, tiles: &[Tile]) -> Vec<Region> {
        if self.scale <= MAX_START_REGION_SCALE {
            return self.regions(tiles, START_REGION);
        }
        tiles
            .iter()
            .map(|t| Region {
                // A tile's chunks on a side; tiles start at multiples of that.
                size: (TILE / 16 * t.scale) as i32,
                x: t.x,
                z: t.z,
            })
            .collect()
    }

    /// The tiles covering a map of `size` points, nearest the centre first.
    pub fn tiles(&self, size: [f32; 2]) -> Vec<Tile> {
        let side = f64::from(TILE);
        let origin = self.origin();
        // The tile numbers on one axis that overlap the map, from the edge's offset.
        let range = |axis: usize| {
            let half = f64::from(size[axis]) / 2.0;
            let lo = ((-half - origin[axis]) / side).floor() as i32;
            let hi = ((half - origin[axis]) / side).ceil() as i32 - 1;
            lo..=hi
        };
        let mut tiles: Vec<Tile> = range(1)
            .flat_map(|z| range(0).map(move |x| (x, z)))
            .map(|(x, z)| Tile {
                scale: self.scale,
                x,
                z,
            })
            .collect();
        let distance = |t: &Tile| {
            let centre = |i: i32, axis: usize| (f64::from(i) + 0.5) * side + origin[axis];
            let (dx, dz) = (centre(t.x, 0), centre(t.z, 1));
            dx * dx + dz * dz
        };
        tiles.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        tiles
    }

    /// A tile's top-left corner as a screen offset from the map's centre.
    pub fn tile_offset(&self, tile: Tile) -> [f32; 2] {
        let origin = self.origin();
        let side = f64::from(TILE) * f64::from(tile.scale) / f64::from(self.scale);
        [
            (f64::from(tile.x) * side + origin[0]) as f32,
            (f64::from(tile.z) * side + origin[1]) as f32,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_in_from_1_2_needs_no_new_regions() {
        for centre in [[0.0, 0.0], [1000.5, -77.0], [-3333.0, 4096.0]] {
            let view = |scale| View {
                centre,
                scale,
                y: 64,
            };
            let size = [1100.0, 810.0];
            let near = view(1);
            let far = view(2);
            let regions = |v: &View| v.regions(&v.tiles(size), FEATURE_REGION);
            let known = regions(&far);
            for region in regions(&near) {
                assert!(known.contains(&region), "{region:?} at {centre:?}");
            }
        }
    }

    #[test]
    fn zooming_in_needs_no_new_start_regions_up_to_1_4() {
        for centre in [[0.0, 0.0], [1000.5, -77.0], [-3333.0, 4096.0]] {
            let view = |scale| View {
                centre,
                scale,
                y: 64,
            };
            let size = [1100.0, 810.0];
            let regions = |v: View| v.start_regions(&v.tiles(size));
            for scale in [4, 2] {
                let known = regions(view(scale));
                for region in regions(view(scale / 2)) {
                    assert!(
                        known.contains(&region),
                        "1:{scale}: {region:?} at {centre:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn past_1_4_each_tile_is_a_start_region() {
        let view = View {
            centre: [-5000.0, 77.0],
            scale: 32,
            y: 64,
        };
        let tiles = view.tiles([500.0, 400.0]);
        let regions = view.start_regions(&tiles);
        assert_eq!(regions.len(), tiles.len());
        for (tile, region) in tiles.iter().zip(&regions) {
            let area = tile.area(64).unwrap();
            let (min, max) = region.chunks();
            assert_eq!(min, area.min.map(|m| m >> 4));
            let last = |d: usize| (area.min[d] + (TILE * tile.scale) as i32 - 1) >> 4;
            assert_eq!(max, [last(0), last(1)]);
        }
        // At 1:4, regions of 32 chunks: one a tile, and a ring.
        let near = View { scale: 4, ..view };
        let tiles = near.tiles([128.0, 128.0]);
        assert!(
            near.start_regions(&tiles)
                .iter()
                .all(|r| r.size == START_REGION)
        );
    }

    #[test]
    fn regions_cover_the_tiles_chunks_and_a_ring() {
        let view = View {
            centre: [0.0, 0.0],
            scale: 2,
            y: 64,
        };
        let tiles = view.tiles([256.0, 256.0]);
        let regions = view.regions(&tiles, FEATURE_REGION);
        // 512 blocks are 32 chunks, 2 regions a side, and a ring: 4 a side, each once.
        assert_eq!(regions.len(), 16);
        let mut unique = regions.clone();
        unique.sort_by_key(|r| (r.z, r.x));
        unique.dedup();
        assert_eq!(unique.len(), 16);
        // Nearest the centre first.
        assert!([-1, 0].contains(&regions[0].x) && [-1, 0].contains(&regions[0].z));
        assert_eq!(
            Region {
                size: 8,
                x: -1,
                z: 2
            }
            .chunks(),
            ([-8, 16], [-1, 23])
        );
    }

    #[test]
    fn blocks_and_offsets_round_trip() {
        let view = View {
            centre: [100.0, -40.0],
            scale: 4,
            y: 64,
        };
        // Block 100, -40 is pixel 25, -10 from the origin, so the origin is at -25, 10.
        assert_eq!(view.origin(), [-25.0, 10.0]);
        assert_eq!(view.block_at([0.0, 0.0]), [100.0, -40.0]);
        assert_eq!(view.block_at([2.5, -1.0]), [110.0, -44.0]);
    }

    #[test]
    fn zoom_keeps_the_block_under_the_cursor() {
        let mut view = View::default();
        let cursor = [64.0, -32.0];
        let before = view.block_at(cursor);
        view.zoom(false, cursor);
        assert_eq!(view.scale, 8);
        assert_eq!(view.block_at(cursor), before);
        view.zoom(true, cursor);
        view.zoom(true, cursor);
        assert_eq!(view.scale, 2);
        assert_eq!(view.block_at(cursor), before);
        // Clamped at both ends.
        for _ in 0..20 {
            view.zoom(true, cursor);
        }
        assert_eq!(view.scale, 1);
        for _ in 0..20 {
            view.zoom(false, cursor);
        }
        assert_eq!(view.scale, MAX_SCALE);
    }

    #[test]
    fn panning_moves_against_the_drag() {
        let mut view = View::default();
        view.pan([10.0, -5.0]);
        assert_eq!(view.centre, [-40.0, 20.0]);
        view.pan([f32::MAX, 0.0]);
        assert_eq!(view.centre[0], -MAX_CENTRE);
    }

    #[test]
    fn tiles_cover_the_map_nearest_first() {
        let view = View {
            centre: [0.0, 0.0],
            scale: 2,
            y: 64,
        };
        let tiles = view.tiles([600.0, 300.0]);
        // x from -300 to 300 points: tiles -3..=2; z from -150 to 150: tiles -2..=1.
        assert_eq!(TILE, 128);
        assert_eq!(tiles.len(), 24);
        assert!(tiles[..4].iter().all(|t| (-1..=0).contains(&t.x)));
        let side = TILE as f32;
        for t in &tiles {
            let [x, z] = view.tile_offset(*t);
            assert!(
                x < 300.0 && x + side > -300.0 && z < 150.0 && z + side > -150.0,
                "{t:?}"
            );
        }
    }

    #[test]
    fn stand_ins_have_terrain_only_where_heights_are_cheap() {
        let tile = |scale| Tile { scale, x: 0, z: 0 };
        // The view's own tiles always have it, however far out.
        for scale in [1, 4, 8, 64, MAX_SCALE] {
            assert!(tile(scale).has_terrain(scale), "1:{scale}");
        }
        // Up close the stand-ins have it too: at 1:1 they're at 1:4.
        assert!(tile(COARSE).has_terrain(1));
        assert!(tile(MAX_COARSE_TERRAIN_SCALE).has_terrain(1));
        // Past that they show biomes at the view's height.
        assert!(!tile(MAX_COARSE_TERRAIN_SCALE * 2).has_terrain(2));
        assert!(!tile(8 * COARSE).has_terrain(8));
    }

    #[test]
    fn tiles_have_areas_and_parents() {
        let tile = Tile {
            scale: 4,
            x: -3,
            z: 2,
        };
        let area = tile.area(70).unwrap();
        let side = TILE as i32 * 4;
        assert_eq!(area.min, [-3 * side, 2 * side]);
        assert_eq!(area.y, 70);
        assert_eq!(
            tile.parent(),
            Some((
                Tile {
                    scale: 8,
                    x: -2,
                    z: 1
                },
                [1, 0]
            ))
        );
        assert_eq!(
            Tile {
                scale: MAX_SCALE,
                x: 0,
                z: 0
            }
            .parent(),
            None
        );
        assert_eq!(
            Tile {
                scale: 256,
                x: 80_000,
                z: 0
            }
            .area(64),
            None
        );
    }

    #[test]
    fn coarse_tiles_contain_the_view_s_tiles() {
        let view = View {
            centre: [100.0, -3000.0],
            scale: 4,
            y: 64,
        };
        let tiles = view.tiles([1200.0, 800.0]);
        let coarse = coarse(&tiles);
        // Each coarse tile covers 4 × 4 of the view's, so far fewer are needed.
        assert!(!coarse.is_empty() && coarse.len() * 4 <= tiles.len() + 8);
        for tile in &tiles {
            let side = |t: &Tile| i64::from(TILE) * i64::from(t.scale);
            let inside = coarse.iter().filter(|c| {
                [(tile.x, c.x), (tile.z, c.z)].iter().all(|&(t, c)| {
                    let start = i64::from(t) * side(tile);
                    let outer = i64::from(c) * side(&coarse[0]);
                    (outer..outer + side(&coarse[0])).contains(&start)
                })
            });
            assert_eq!(inside.count(), 1, "{tile:?}");
        }
        assert!(coarse.iter().all(|c| c.scale == 4 * COARSE));
        // Their heights would cost as much as the view's, so they have no terrain layers.
        assert!(tiles.iter().all(|t| t.has_terrain(view.scale)));
        assert!(coarse.iter().all(|c| !c.has_terrain(view.scale)));
        // Nearest first, as the view's tiles are.
        assert_eq!(coarse[0], super::coarse(&tiles[..1])[0]);
        // No coarser tiles than the largest scale.
        let far = Tile {
            scale: MAX_SCALE,
            x: 0,
            z: 0,
        };
        assert!(super::coarse(&[far]).is_empty());
    }

    #[test]
    fn bad_settings_are_fixed() {
        let view = View {
            centre: [f64::NAN, 1e300],
            scale: 3,
            y: 64,
        }
        .sanitized();
        assert_eq!(view.scale, 4);
        assert_eq!(view.centre, [0.0, MAX_CENTRE]);
    }
}
