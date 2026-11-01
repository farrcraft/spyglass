//! Mineshaft layout (`MineshaftStructure.generatePiecesAndAdjust` and `MineshaftPieces`): every
//! piece's box and the start's bounds, before any piece is placed, and the vertical move that
//! gives the biome check its y. See `docs/specs/structure-placement.md#mineshafts`.

use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};
use spyglass_terrain::Heightmap;

use crate::assembly::{Assembly, Layout};
use crate::jigsaw::BoundingBox;

/// A mineshaft's settings (`MineshaftStructure`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mineshaft {
    /// `mineshaft_type` is `mesa`: the layout moves to a height between the sea level and the
    /// surface, rather than below the sea level.
    pub mesa: bool,
    /// Whether the structure's `terrain_adaptation` isn't `none`, which grows its bounds.
    pub adapts_terrain: bool,
}

/// `MineshaftPieces.MAX_DEPTH`.
const MAX_DEPTH: i32 = 8;

/// `MineshaftPieces.MAGIC_START_Y`: the room's floor, and the check's y before the move.
pub(crate) const START_Y: i32 = 50;

/// What the layout needs from the dimension.
pub(crate) struct Heights {
    /// The noise settings' `sea_level`.
    pub sea_level: i32,
    /// `ChunkGenerator.getMinY()`: the noise settings' `noise.min_y`.
    pub generator_min_y: i32,
}

/// The game's `Direction`, horizontal only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    North,
    South,
    West,
    East,
}

/// What a piece is, as far as its children need.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// `MineShaftRoom`, the start.
    Room,
    /// `MineShaftCorridor`, with its orientation.
    Corridor(Direction),
    /// `MineShaftCrossing`, with its direction.
    Crossing(Direction),
    /// `MineShaftStairs`, with its orientation.
    Stairs(Direction),
}

/// A piece just made, before its children.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// What it is.
    kind: Kind,
    /// The blocks it takes.
    bbox: BoundingBox,
    /// `genDepth`.
    depth: i32,
}

/// How many blocks `bbox` spans on `axis` (0 for x, 1 for y, 2 for z).
fn span(bbox: &BoundingBox, axis: usize) -> i32 {
    bbox.max[axis].wrapping_sub(bbox.min[axis]).wrapping_add(1)
}

/// `StructurePiecesBuilder` and the context random, while the pieces are laid out.
///
/// The layout grows from the room, depth first: each piece, once added, adds its children at
/// its exits, each a random corridor, crossing or stairs that's kept only if its box hits no
/// piece so far. Every choice draws from the one random, so the order of the calls is the
/// order of the draws.
struct Builder {
    /// The context random.
    random: LegacyRandomSource,
    /// Every piece's box so far, in the order added.
    pieces: Vec<BoundingBox>,
    /// The start piece's box (the room's), for the distance limit.
    start: BoundingBox,
}

impl Builder {
    fn next_int(&mut self, bound: i32) -> i32 {
        self.random.next_int_bounded(bound)
    }

    /// `findCollisionPiece(box) != null`.
    fn collides(&self, bbox: &BoundingBox) -> bool {
        self.pieces.iter().any(|p| p.intersects(bbox))
    }

    /// `MineshaftPieces.generateAndAddPiece`: no piece beyond the depth limit or more than 80
    /// blocks from the room's minimum corner in x or z; otherwise a random piece, added, and
    /// then its children, depth first.
    fn generate_and_add(&mut self, x: i32, y: i32, z: i32, direction: Direction, depth: i32) {
        if depth > MAX_DEPTH {
            return;
        }
        if x.wrapping_sub(self.start.min[0]).wrapping_abs() > 80
            || z.wrapping_sub(self.start.min[2]).wrapping_abs() > 80
        {
            return;
        }
        if let Some(piece) = self.random_shaft_piece(x, y, z, direction, depth.wrapping_add(1)) {
            self.pieces.push(piece.bbox);
            self.add_children(&piece);
        }
    }

    /// `MineshaftPieces.createRandomShaftPiece`: a crossing on 80..100, stairs on 70..80, else a
    /// corridor, if its box finds room.
    fn random_shaft_piece(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        direction: Direction,
        depth: i32,
    ) -> Option<Piece> {
        let roll = self.next_int(100);
        let (kind, bbox) = if roll >= 80 {
            (
                Kind::Crossing(direction),
                self.find_crossing(x, y, z, direction)?,
            )
        } else if roll >= 70 {
            (
                Kind::Stairs(direction),
                self.find_stairs(x, y, z, direction)?,
            )
        } else {
            let bbox = self.find_corridor_size(x, y, z, direction)?;
            // The constructor: hasRails, then spiderCorridor only without rails.
            let rails = self.next_int(3) == 0;
            if !rails {
                self.next_int(23);
            }
            (Kind::Corridor(direction), bbox)
        };
        Some(Piece { kind, bbox, depth })
    }

    /// A box at the origin, moved to `(x, y, z)`, if nothing collides with it.
    fn free_at(&self, min: [i32; 3], max: [i32; 3], x: i32, y: i32, z: i32) -> Option<BoundingBox> {
        let bbox = BoundingBox { min, max }.moved([x, y, z]);
        (!self.collides(&bbox)).then_some(bbox)
    }

    /// `MineShaftCrossing.findCrossing`: two floors (6 high) on `nextInt(4) == 0`.
    fn find_crossing(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        direction: Direction,
    ) -> Option<BoundingBox> {
        let top = if self.next_int(4) == 0 { 6 } else { 2 };
        let (min, max) = match direction {
            Direction::North => ([-1, 0, -4], [3, top, 0]),
            Direction::South => ([-1, 0, 0], [3, top, 4]),
            Direction::West => ([-4, 0, -1], [0, top, 3]),
            Direction::East => ([0, 0, -1], [4, top, 3]),
        };
        self.free_at(min, max, x, y, z)
    }

    /// `MineShaftStairs.findStairs`: no draws.
    fn find_stairs(&mut self, x: i32, y: i32, z: i32, direction: Direction) -> Option<BoundingBox> {
        let (min, max) = match direction {
            Direction::North => ([0, -5, -8], [2, 2, 0]),
            Direction::South => ([0, -5, 0], [2, 2, 8]),
            Direction::West => ([-8, -5, 0], [0, 2, 2]),
            Direction::East => ([0, -5, 0], [8, 2, 2]),
        };
        self.free_at(min, max, x, y, z)
    }

    /// `MineShaftCorridor.findCorridorSize`: `nextInt(3) + 2` sections of 5, one fewer each
    /// time the box collides, down to none.
    fn find_corridor_size(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        direction: Direction,
    ) -> Option<BoundingBox> {
        let mut sections = self.next_int(3).wrapping_add(2);
        while sections > 0 {
            let len = sections.wrapping_mul(5).wrapping_sub(1);
            let (min, max) = match direction {
                Direction::North => ([0, 0, len.wrapping_neg()], [2, 2, 0]),
                Direction::South => ([0, 0, 0], [2, 2, len]),
                Direction::West => ([len.wrapping_neg(), 0, 0], [0, 2, 2]),
                Direction::East => ([0, 0, 0], [len, 2, 2]),
            };
            if let Some(bbox) = self.free_at(min, max, x, y, z) {
                return Some(bbox);
            }
            sections -= 1;
        }
        None
    }

    /// Each piece type's `addChildren`.
    fn add_children(&mut self, piece: &Piece) {
        let bbox = piece.bbox;
        let depth = piece.depth;
        let [min_x, min_y, min_z] = bbox.min;
        let [max_x, _, max_z] = bbox.max;
        match piece.kind {
            Kind::Room => self.room_children(&bbox, depth),
            Kind::Corridor(orientation) => {
                // One exit at the end, or to either side near it, by nextInt(4); its y draws
                // after the choice.
                let end = self.next_int(4);
                let (x, z, exit) = match (orientation, end) {
                    (Direction::North, 0 | 1) => (min_x, min_z.wrapping_sub(1), orientation),
                    (Direction::North, 2) => (min_x.wrapping_sub(1), min_z, Direction::West),
                    (Direction::North, _) => (max_x.wrapping_add(1), min_z, Direction::East),
                    (Direction::South, 0 | 1) => (min_x, max_z.wrapping_add(1), orientation),
                    (Direction::South, 2) => (
                        min_x.wrapping_sub(1),
                        max_z.wrapping_sub(3),
                        Direction::West,
                    ),
                    (Direction::South, _) => (
                        max_x.wrapping_add(1),
                        max_z.wrapping_sub(3),
                        Direction::East,
                    ),
                    (Direction::West, 0 | 1) => (min_x.wrapping_sub(1), min_z, orientation),
                    (Direction::West, 2) => (min_x, min_z.wrapping_sub(1), Direction::North),
                    (Direction::West, _) => (min_x, max_z.wrapping_add(1), Direction::South),
                    (Direction::East, 0 | 1) => (max_x.wrapping_add(1), min_z, orientation),
                    (Direction::East, 2) => (
                        max_x.wrapping_sub(3),
                        min_z.wrapping_sub(1),
                        Direction::North,
                    ),
                    (Direction::East, _) => (
                        max_x.wrapping_sub(3),
                        max_z.wrapping_add(1),
                        Direction::South,
                    ),
                };
                let y = min_y.wrapping_sub(1).wrapping_add(self.next_int(3));
                self.generate_and_add(x, y, z, exit, depth);
                // Side branches every 5 blocks, one level deeper.
                if depth < MAX_DEPTH {
                    let deeper = depth.wrapping_add(1);
                    if matches!(orientation, Direction::North | Direction::South) {
                        let mut z = min_z.wrapping_add(3);
                        while z.wrapping_add(3) <= max_z {
                            match self.next_int(5) {
                                0 => self.generate_and_add(
                                    min_x.wrapping_sub(1),
                                    min_y,
                                    z,
                                    Direction::West,
                                    deeper,
                                ),
                                1 => self.generate_and_add(
                                    max_x.wrapping_add(1),
                                    min_y,
                                    z,
                                    Direction::East,
                                    deeper,
                                ),
                                _ => {}
                            }
                            z = z.wrapping_add(5);
                        }
                    } else {
                        let mut x = min_x.wrapping_add(3);
                        while x.wrapping_add(3) <= max_x {
                            match self.next_int(5) {
                                0 => self.generate_and_add(
                                    x,
                                    min_y,
                                    min_z.wrapping_sub(1),
                                    Direction::North,
                                    deeper,
                                ),
                                1 => self.generate_and_add(
                                    x,
                                    min_y,
                                    max_z.wrapping_add(1),
                                    Direction::South,
                                    deeper,
                                ),
                                _ => {}
                            }
                            x = x.wrapping_add(5);
                        }
                    }
                }
            }
            Kind::Crossing(direction) => {
                // An exit on each side but the one it was entered from.
                let north = |builder: &mut Self, y| {
                    builder.generate_and_add(
                        min_x.wrapping_add(1),
                        y,
                        min_z.wrapping_sub(1),
                        Direction::North,
                        depth,
                    )
                };
                let south = |builder: &mut Self, y| {
                    builder.generate_and_add(
                        min_x.wrapping_add(1),
                        y,
                        max_z.wrapping_add(1),
                        Direction::South,
                        depth,
                    )
                };
                let west = |builder: &mut Self, y| {
                    builder.generate_and_add(
                        min_x.wrapping_sub(1),
                        y,
                        min_z.wrapping_add(1),
                        Direction::West,
                        depth,
                    )
                };
                let east = |builder: &mut Self, y| {
                    builder.generate_and_add(
                        max_x.wrapping_add(1),
                        y,
                        min_z.wrapping_add(1),
                        Direction::East,
                        depth,
                    )
                };
                match direction {
                    Direction::North => {
                        north(self, min_y);
                        west(self, min_y);
                        east(self, min_y);
                    }
                    Direction::South => {
                        south(self, min_y);
                        west(self, min_y);
                        east(self, min_y);
                    }
                    Direction::West => {
                        north(self, min_y);
                        south(self, min_y);
                        west(self, min_y);
                    }
                    Direction::East => {
                        north(self, min_y);
                        south(self, min_y);
                        east(self, min_y);
                    }
                }
                // isTwoFloored: the upper floor's exits, each on a coin.
                if span(&bbox, 1) > 3 {
                    let upper = min_y.wrapping_add(3).wrapping_add(1);
                    if self.random.next_boolean() {
                        north(self, upper);
                    }
                    if self.random.next_boolean() {
                        west(self, upper);
                    }
                    if self.random.next_boolean() {
                        east(self, upper);
                    }
                    if self.random.next_boolean() {
                        south(self, upper);
                    }
                }
            }
            Kind::Stairs(orientation) => match orientation {
                Direction::North => self.generate_and_add(
                    min_x,
                    min_y,
                    min_z.wrapping_sub(1),
                    Direction::North,
                    depth,
                ),
                Direction::South => self.generate_and_add(
                    min_x,
                    min_y,
                    max_z.wrapping_add(1),
                    Direction::South,
                    depth,
                ),
                Direction::West => self.generate_and_add(
                    min_x.wrapping_sub(1),
                    min_y,
                    min_z,
                    Direction::West,
                    depth,
                ),
                Direction::East => self.generate_and_add(
                    max_x.wrapping_add(1),
                    min_y,
                    min_z,
                    Direction::East,
                    depth,
                ),
            },
        }
    }

    /// `MineShaftRoom.addChildren`: exits along each side (north, south, west, east), at a
    /// random offset and height, at least 4 blocks apart.
    fn room_children(&mut self, bbox: &BoundingBox, depth: i32) {
        let mut height = span(bbox, 1).wrapping_sub(3).wrapping_sub(1);
        if height <= 0 {
            height = 1;
        }
        let [min_x, min_y, min_z] = bbox.min;
        let [max_x, _, max_z] = bbox.max;
        let sides = [
            (Direction::North, 0),
            (Direction::South, 0),
            (Direction::West, 2),
            (Direction::East, 2),
        ];
        for (direction, axis) in sides {
            let len = span(bbox, axis);
            let mut pos: i32 = 0;
            while pos < len {
                pos = pos.wrapping_add(self.next_int(len));
                if pos.wrapping_add(3) > len {
                    break;
                }
                let y = min_y.wrapping_add(self.next_int(height)).wrapping_add(1);
                let (x, z) = match direction {
                    Direction::North => (min_x.wrapping_add(pos), min_z.wrapping_sub(1)),
                    Direction::South => (min_x.wrapping_add(pos), max_z.wrapping_add(1)),
                    Direction::West => (min_x.wrapping_sub(1), min_z.wrapping_add(pos)),
                    Direction::East => (max_x.wrapping_add(1), min_z.wrapping_add(pos)),
                };
                self.generate_and_add(x, y, z, direction, depth);
                pos = pos.wrapping_add(4);
            }
        }
    }
}

/// The box around every box (`StructurePiece.createBoundingBox`); there's always the room.
fn around(pieces: &[BoundingBox]) -> BoundingBox {
    pieces
        .iter()
        .copied()
        .reduce(BoundingBox::encapsulate)
        .expect("a mineshaft has its room")
}

impl Mineshaft {
    /// The start's pieces in `chunk` and the vertical move (`dy`) of the check from
    /// `(minX + 8, 50, minZ)`: `findGenerationPoint`, with the context random
    /// `setLargeFeatureSeed(seed, x, z)`. A mesa needs the first free `WORLD_SURFACE_WG` height
    /// at the layout's centre: without it, `None`.
    pub(crate) fn layout(
        &self,
        seed: i64,
        chunk: [i32; 2],
        heights: &Heights,
        first_free: &mut dyn FnMut(i32, i32, Heightmap) -> Option<i32>,
    ) -> Option<(Layout, i32)> {
        let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
        random.next_double();
        // MineShaftRoom at getBlockX(2), getBlockZ(2): its maxima draw x, y, z in order.
        let x = chunk[0].wrapping_shl(4).wrapping_add(2);
        let z = chunk[1].wrapping_shl(4).wrapping_add(2);
        let max_x = x.wrapping_add(7).wrapping_add(random.next_int_bounded(6));
        let max_y = 54_i32.wrapping_add(random.next_int_bounded(6));
        let max_z = z.wrapping_add(7).wrapping_add(random.next_int_bounded(6));
        let room = Piece {
            kind: Kind::Room,
            bbox: BoundingBox {
                min: [x, START_Y, z],
                max: [max_x, max_y, max_z],
            },
            depth: 0,
        };
        let mut builder = Builder {
            random,
            pieces: vec![room.bbox],
            start: room.bbox,
        };
        builder.add_children(&room);
        let Builder {
            mut random,
            mut pieces,
            ..
        } = builder;
        let bounds = around(&pieces);
        let sea_level = heights.sea_level;
        let dy = if self.mesa {
            // getCenter, then the target between the sea level and the surface there.
            let center = |axis: usize| bounds.min[axis].wrapping_add(span(&bounds, axis) / 2);
            let surface = first_free(center(0), center(2), Heightmap::WorldSurfaceWg)?;
            let target = if surface <= sea_level {
                sea_level
            } else {
                // Mth.randomBetweenInclusive(random, seaLevel, surface).
                random
                    .next_int_bounded(surface.wrapping_sub(sea_level).wrapping_add(1))
                    .wrapping_add(sea_level)
            };
            target.wrapping_sub(center(1))
        } else {
            // StructurePiecesBuilder.moveBelowSeaLevel(seaLevel, minY, random, 10).
            let top = sea_level.wrapping_sub(10);
            let mut y = bounds
                .y_span()
                .wrapping_add(heights.generator_min_y)
                .wrapping_add(1);
            if y < top {
                y = y.wrapping_add(random.next_int_bounded(top.wrapping_sub(y)));
            }
            y.wrapping_sub(bounds.max[1])
        };
        for piece in &mut pieces {
            *piece = piece.moved([0, dy, 0]);
        }
        let mut bounds = bounds.moved([0, dy, 0]);
        if self.adapts_terrain {
            bounds = bounds.inflated(12);
        }
        Some((Layout { pieces, bounds }, dy))
    }

    /// [`layout`](Self::layout) as an [`Assembly`].
    pub(crate) fn assemble(
        &self,
        seed: i64,
        chunk: [i32; 2],
        heights: &Heights,
        first_free: &mut dyn FnMut(i32, i32, Heightmap) -> Option<i32>,
    ) -> Assembly {
        match self.layout(seed, chunk, heights, first_free) {
            Some((layout, _)) => Assembly::Built(layout),
            None => Assembly::NeedsTerrain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEIGHTS: Heights = Heights {
        sea_level: 63,
        generator_min_y: -64,
    };

    fn no_terrain(_: i32, _: i32, _: Heightmap) -> Option<i32> {
        None
    }

    #[test]
    fn the_room_comes_first_at_the_chunk_corner_and_pieces_are_disjoint() {
        let normal = Mineshaft {
            mesa: false,
            adapts_terrain: false,
        };
        for chunk in [[0, 0], [-3, 7], [100, -100]] {
            let (layout, dy) = normal
                .layout(12345, chunk, &HEIGHTS, &mut no_terrain)
                .unwrap();
            let room = layout.pieces[0];
            assert_eq!(room.min[0], chunk[0] * 16 + 2);
            assert_eq!(room.min[2], chunk[1] * 16 + 2);
            assert_eq!(room.min[1], START_Y + dy);
            for (i, a) in layout.pieces.iter().enumerate() {
                assert!(layout.bounds.contains_box(a));
                for b in &layout.pieces[i + 1..] {
                    assert!(!a.intersects(b), "{a:?} and {b:?} overlap");
                }
            }
            // Below the sea level, less 10.
            assert!(layout.bounds.max[1] < HEIGHTS.sea_level - 10);
        }
    }

    #[test]
    fn a_mesa_needs_terrain() {
        let mesa = Mineshaft {
            mesa: true,
            adapts_terrain: false,
        };
        assert_eq!(
            mesa.assemble(1, [0, 0], &HEIGHTS, &mut no_terrain),
            Assembly::NeedsTerrain
        );
        // At or below the sea level the target is the sea level, with no draw.
        let (layout, dy) = mesa
            .layout(1, [0, 0], &HEIGHTS, &mut |_, _, _| Some(40))
            .unwrap();
        let b = layout.bounds;
        assert_eq!(b.min[1] + (b.max[1] - b.min[1] + 1) / 2, HEIGHTS.sea_level);
        assert_eq!(layout.pieces[0].min[1], START_Y + dy);
    }
}
