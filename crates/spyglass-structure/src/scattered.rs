//! The one piece of a desert pyramid or a jungle temple (`ScatteredFeaturePiece`): a box of a
//! fixed size at its start chunk's corner, turned one of four ways, whose height the first
//! chunk decorating it settles. See `docs/specs/loot.md#containers`.

use spyglass_rng::{RandomSource, worldgen};

use crate::jigsaw::BoundingBox;

/// A piece's orientation (`StructurePiece.getOrientation`), in `Direction.Plane.HORIZONTAL`'s
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    /// Towards −z.
    North,
    /// Towards +x.
    East,
    /// Towards +z.
    South,
    /// Towards −x.
    West,
}

impl Facing {
    /// `Direction.Plane.HORIZONTAL`, the order `getRandomHorizontalDirection` picks from.
    pub const HORIZONTAL: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];
}

/// A scattered feature's piece, before its height is settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScatteredPiece {
    /// Its box, at y 64 as made.
    pub bounds: BoundingBox,
    /// Which way it's turned.
    pub facing: Facing,
}

impl ScatteredPiece {
    /// The piece a start in `chunk` makes, `width` × `height` × `depth` before turning
    /// (`ScatteredFeaturePiece`'s constructor): the orientation is the first draw of the
    /// structure's generation random, `setLargeFeatureSeed(seed, chunk)`.
    pub fn new(seed: i64, chunk: [i32; 2], [width, height, depth]: [i32; 3]) -> Self {
        let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
        let facing = Facing::HORIZONTAL[random.next_int_bounded(4) as usize];
        let [x, y, z] = [chunk[0].wrapping_shl(4), 64, chunk[1].wrapping_shl(4)];
        // StructurePiece.makeBoundingBox: width runs along x when facing north or south.
        let (dx, dz) = match facing {
            Facing::North | Facing::South => (width, depth),
            Facing::East | Facing::West => (depth, width),
        };
        ScatteredPiece {
            bounds: BoundingBox {
                min: [x, y, z],
                max: [
                    x.wrapping_add(dx - 1),
                    y.wrapping_add(height - 1),
                    z.wrapping_add(dz - 1),
                ],
            },
            facing,
        }
    }

    /// `getWorldPos(x, y, z)`: a position in the piece's own coordinates, in the world, with
    /// its box's least y at `min_y`.
    pub fn world(&self, [x, y, z]: [i32; 3], min_y: i32) -> [i32; 3] {
        let BoundingBox { min, max } = self.bounds;
        let (wx, wz) = match self.facing {
            Facing::North => (min[0].wrapping_add(x), max[2].wrapping_sub(z)),
            Facing::South => (min[0].wrapping_add(x), min[2].wrapping_add(z)),
            Facing::West => (max[0].wrapping_sub(z), min[2].wrapping_add(x)),
            Facing::East => (min[0].wrapping_add(z), min[2].wrapping_add(x)),
        };
        [wx, min_y.wrapping_add(y), wz]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_swaps_width_and_depth() {
        let piece = ScatteredPiece {
            bounds: BoundingBox {
                min: [0, 64, 0],
                max: [14, 73, 11],
            },
            facing: Facing::East,
        };
        assert_eq!(piece.world([0, 0, 0], 70), [0, 70, 0]);
        assert_eq!(piece.world([11, 1, 14], 70), [14, 71, 11]);
        let north = ScatteredPiece {
            bounds: BoundingBox {
                min: [0, 64, 0],
                max: [11, 73, 14],
            },
            facing: Facing::North,
        };
        assert_eq!(north.world([0, 0, 0], 64), [0, 64, 14]);
    }
}
