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
    pub fn world(&self, local: [i32; 3], min_y: i32) -> [i32; 3] {
        let at = BoundingBox {
            min: [self.bounds.min[0], min_y, self.bounds.min[2]],
            ..self.bounds
        };
        world_pos(&at, self.facing, local)
    }
}

/// `StructurePiece.getWorldPos(x, y, z)`: a position in a legacy piece's own coordinates, in
/// the world, for a piece with box `bounds` facing `facing`.
pub fn world_pos(bounds: &BoundingBox, facing: Facing, [x, y, z]: [i32; 3]) -> [i32; 3] {
    let BoundingBox { min, max } = *bounds;
    let (wx, wz) = match facing {
        Facing::North => (min[0].wrapping_add(x), max[2].wrapping_sub(z)),
        Facing::South => (min[0].wrapping_add(x), min[2].wrapping_add(z)),
        Facing::West => (max[0].wrapping_sub(z), min[2].wrapping_add(x)),
        Facing::East => (min[0].wrapping_add(z), min[2].wrapping_add(x)),
    };
    [wx, min[1].wrapping_add(y), wz]
}

/// Which way a legacy piece's exit leaves it (`generateChildForward`, `Left`, `Right`, and
/// strongholds' `generateSmallDoorChild...`). Left is always towards −x or −z, whichever way the
/// piece faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    Forward,
    Left,
    Right,
}

impl Exit {
    /// Where the exit's child grows from, and which way it faces, for a piece with box `bounds`
    /// facing `facing`: `across` along the side it leaves from and `up` from the floor.
    pub(crate) fn from(
        self,
        bounds: &BoundingBox,
        facing: Facing,
        across: i32,
        up: i32,
    ) -> ([i32; 3], Facing) {
        let BoundingBox { min, max } = *bounds;
        let y = min[1] + up;
        use Facing::*;
        let (x, z, facing) = match (self, facing) {
            (Exit::Forward, North) => (min[0] + across, min[2] - 1, North),
            (Exit::Forward, South) => (min[0] + across, max[2] + 1, South),
            (Exit::Forward, West) => (min[0] - 1, min[2] + across, West),
            (Exit::Forward, East) => (max[0] + 1, min[2] + across, East),
            (Exit::Left, North | South) => (min[0] - 1, min[2] + across, West),
            (Exit::Left, West | East) => (min[0] + across, min[2] - 1, North),
            (Exit::Right, North | South) => (max[0] + 1, min[2] + across, East),
            (Exit::Right, West | East) => (min[0] + across, max[2] + 1, South),
        };
        ([x, y, z], facing)
    }
}

/// `BoundingBox.orientBox(x, y, z, ox, oy, oz, width, height, depth, facing)`: a legacy piece's
/// box from the point it grows from, `offset` across, up and along, and `size` across, up and
/// along, turned to `facing`.
pub(crate) fn orient_box(
    [x, y, z]: [i32; 3],
    [ox, oy, oz]: [i32; 3],
    [w, h, d]: [i32; 3],
    facing: Facing,
) -> BoundingBox {
    let (min, max) = match facing {
        Facing::North => (
            [x + ox, y + oy, z - d + 1 + oz],
            [x + w - 1 + ox, y + h - 1 + oy, z + oz],
        ),
        Facing::West => (
            [x - d + 1 + oz, y + oy, z + ox],
            [x + oz, y + h - 1 + oy, z + w - 1 + ox],
        ),
        Facing::East => (
            [x + oz, y + oy, z + ox],
            [x + d - 1 + oz, y + h - 1 + oy, z + w - 1 + ox],
        ),
        Facing::South => (
            [x + ox, y + oy, z + oz],
            [x + w - 1 + ox, y + h - 1 + oy, z + d - 1 + oz],
        ),
    };
    BoundingBox { min, max }
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
