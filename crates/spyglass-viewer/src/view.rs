//! Where the map looks: its centre, scale and height, and which tiles cover the window. Screen
//! offsets are in egui points from the map's centre, x right (east) and y down (south).

use serde::{Deserialize, Serialize};
use spyglass::{DEFAULT_MAP_Y, MAX_SCALE, MapArea};

/// A tile's side in pixels. One tile pixel is drawn as one point.
pub const TILE: u32 = 256;
/// How far from 0 the centre may go, in blocks: past the world border, short of overflowing
/// block coordinates.
const MAX_CENTRE: f64 = 1_000_000_000.0;

/// A tile: `TILE` × `TILE` pixels at a scale, numbered from the world origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    pub scale: u32,
    pub x: i32,
    pub z: i32,
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
        let s = f64::from(self.scale);
        self.centre.map(|c| (-c / s).round())
    }

    /// The block under a screen offset from the map's centre.
    pub fn block_at(&self, offset: [f32; 2]) -> [f64; 2] {
        let s = f64::from(self.scale);
        let origin = self.origin();
        [0, 1].map(|d| (f64::from(offset[d]) - origin[d]) * s)
    }

    /// Moves the map by `delta` points, as when dragged.
    pub fn pan(&mut self, delta: [f32; 2]) {
        let s = f64::from(self.scale);
        let centre = self.centre;
        self.centre =
            [0, 1].map(|d| (centre[d] - f64::from(delta[d]) * s).clamp(-MAX_CENTRE, MAX_CENTRE));
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
        let s = f64::from(scale);
        self.centre =
            [0, 1].map(|d| (fixed[d] - f64::from(offset[d]) * s).clamp(-MAX_CENTRE, MAX_CENTRE));
    }

    /// The tiles covering a map of `size` points, nearest the centre first.
    pub fn tiles(&self, size: [f32; 2]) -> Vec<Tile> {
        let side = f64::from(TILE);
        let origin = self.origin();
        let range = |d: usize| {
            let half = f64::from(size[d]) / 2.0;
            let lo = ((-half - origin[d]) / side).floor() as i32;
            let hi = ((half - origin[d]) / side).ceil() as i32 - 1;
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
            let centre = |i: i32, d: usize| (f64::from(i) + 0.5) * side + origin[d];
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
        // x from -300 to 300 points: tiles -2..=1; z from -150 to 150: tiles -1..=0.
        assert_eq!(tiles.len(), 8);
        assert!(tiles[..4].iter().all(|t| (-1..=0).contains(&t.x)));
        for t in &tiles {
            let [x, z] = view.tile_offset(*t);
            assert!(
                x < 300.0 && x + 256.0 > -300.0 && z < 150.0 && z + 256.0 > -150.0,
                "{t:?}"
            );
        }
    }

    #[test]
    fn tiles_have_areas_and_parents() {
        let tile = Tile {
            scale: 4,
            x: -3,
            z: 2,
        };
        let area = tile.area(70).unwrap();
        assert_eq!(area.min, [-3072, 2048]);
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
                x: 40_000,
                z: 0
            }
            .area(64),
            None
        );
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
