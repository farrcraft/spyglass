//! Stronghold layout (`StrongholdStructure.generatePieces` and `StrongholdPieces`): every
//! piece's box, in the game's order, with what placing it needs. See
//! `docs/specs/structure-placement.md#strongholds`.
//!
//! From a spiral staircase at the start chunk, each piece's doors grow corridors, turns, rooms
//! and stairs, picked by weight with counts limiting the rarer ones, from a pending list in
//! random order. The first piece after the start is always a five-way crossing. A layout without
//! the portal room is thrown away and made again from the next seed.

use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};

use crate::assembly::Layout;
use crate::jigsaw::BoundingBox;
use crate::mineshaft::Heights;
use crate::scattered::{Exit, Facing, orient_box};

/// `StrongholdPieces.MAX_DEPTH`.
const MAX_DEPTH: i32 = 50;

/// `MAGIC_START_Y`: the start's bottom before the move.
const START_Y: i32 = 64;

/// How far a door may be from the start's corner on x or z and still grow a piece.
const MAX_DISTANCE: i32 = 112;

/// A stronghold piece's type (`StrongholdPieces`' piece classes), with what it decided as it
/// was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrongholdKind {
    /// `StartPiece`: the spiral staircase down.
    Start,
    /// `Straight`: a corridor, with doors to the sides or not.
    Straight {
        /// `leftChild`.
        left: bool,
        /// `rightChild`.
        right: bool,
    },
    /// `PrisonHall`.
    PrisonHall,
    /// `LeftTurn`.
    LeftTurn,
    /// `RightTurn`.
    RightTurn,
    /// `RoomCrossing`.
    RoomCrossing {
        /// `type`, 0 to 4: 2 has a chest.
        room: i32,
    },
    /// `StraightStairsDown`.
    StraightStairsDown,
    /// `StairsDown`, not the start's.
    StairsDown,
    /// `FiveCrossing`, with which of its four side doors lead on.
    FiveCrossing {
        /// `leftLow`.
        left_low: bool,
        /// `leftHigh`.
        left_high: bool,
        /// `rightLow`.
        right_low: bool,
        /// `rightHigh`.
        right_high: bool,
    },
    /// `ChestCorridor`.
    ChestCorridor,
    /// `Library`: two floors and a second chest if tall.
    Library {
        /// `isTall`.
        tall: bool,
    },
    /// `PortalRoom`.
    PortalRoom,
    /// `FillerCorridor`: a short dead end against another piece.
    FillerCorridor,
}

/// A piece type the weighted list may pick, in `STRONGHOLD_PIECE_WEIGHTS`' order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Straight,
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairsDown,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    PortalRoom,
}

/// `STRONGHOLD_PIECE_WEIGHTS`: each type, its weight and `maxPlaceCount` (0 for no limit).
const WEIGHTS: [(Type, i32, i32); 11] = [
    (Type::Straight, 40, 0),
    (Type::PrisonHall, 5, 5),
    (Type::LeftTurn, 20, 0),
    (Type::RightTurn, 20, 0),
    (Type::RoomCrossing, 10, 6),
    (Type::StraightStairsDown, 5, 5),
    (Type::StairsDown, 5, 5),
    (Type::FiveCrossing, 5, 4),
    (Type::ChestCorridor, 5, 4),
    (Type::Library, 10, 2),
    (Type::PortalRoom, 20, 1),
];

/// One of a stronghold's pieces, as placing it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrongholdPiece {
    /// Its type.
    pub kind: StrongholdKind,
    /// Its box.
    pub bounds: BoundingBox,
    /// Its orientation, which turns its own coordinates.
    pub facing: Facing,
    /// `genDepth`: 0 for the start, one more than its parent's.
    depth: i32,
}

/// One attempt's generation, with the game's statics (`currentPieces`, `imposedPiece`, the
/// counts) kept per start.
struct Builder {
    random: LegacyRandomSource,
    pieces: Vec<StrongholdPiece>,
    /// The start's box corner, which doors' distances are measured from.
    start: [i32; 3],
    /// `currentPieces`: the weights not yet used up, by index into [`WEIGHTS`].
    available: Vec<usize>,
    /// Each weight's `placeCount`.
    placed: [i32; WEIGHTS.len()],
    /// `previousPiece`: no type follows itself.
    previous: Option<usize>,
    /// `imposedPiece`: the start's first door always grows a five-way crossing.
    imposed: Option<Type>,
    /// `pendingChildren`: pieces whose doors aren't grown yet, by index.
    pending: Vec<usize>,
    /// Whether a portal room was made.
    portal: bool,
}

impl Builder {
    fn collides(&self, bbox: &BoundingBox) -> bool {
        self.pieces.iter().any(|p| p.bounds.intersects(bbox))
    }

    /// A box above y 10 that collides with nothing (`isOkBox`, `findCollisionPiece`).
    fn free(
        &self,
        at: [i32; 3],
        offset: [i32; 3],
        size: [i32; 3],
        facing: Facing,
    ) -> Option<BoundingBox> {
        let bounds = orient_box(at, offset, size, facing);
        (bounds.min[1] > 10 && !self.collides(&bounds)).then_some(bounds)
    }

    /// `randomSmallDoor`: the entry door, which places without drawing.
    fn door(&mut self) {
        self.random.next_int_bounded(5);
    }

    /// A type's `createPiece`: its box, and the piece, its constructor drawing what it draws.
    fn create(
        &mut self,
        ty: Type,
        at: [i32; 3],
        facing: Facing,
        depth: i32,
    ) -> Option<StrongholdPiece> {
        let shape = |offset: [i32; 3], size: [i32; 3]| (offset, size);
        let (offset, size) = match ty {
            Type::Straight | Type::ChestCorridor => shape([-1, -1, 0], [5, 5, 7]),
            Type::PrisonHall => shape([-1, -1, 0], [9, 5, 11]),
            Type::LeftTurn | Type::RightTurn => shape([-1, -1, 0], [5, 5, 5]),
            Type::RoomCrossing => shape([-4, -1, 0], [11, 7, 11]),
            Type::StraightStairsDown => shape([-1, -7, 0], [5, 11, 8]),
            Type::StairsDown => shape([-1, -7, 0], [5, 11, 5]),
            Type::FiveCrossing => shape([-4, -3, 0], [10, 9, 11]),
            Type::Library => shape([-4, -1, 0], [14, 11, 15]),
            Type::PortalRoom => shape([-4, -1, 0], [11, 8, 16]),
        };
        let mut bounds = self.free(at, offset, size, facing);
        // A library that doesn't fit tall may fit short.
        if ty == Type::Library && bounds.is_none() {
            bounds = self.free(at, offset, [14, 6, 15], facing);
        }
        let bounds = bounds?;
        let kind = match ty {
            Type::PortalRoom => StrongholdKind::PortalRoom,
            _ => {
                self.door();
                match ty {
                    Type::Straight => StrongholdKind::Straight {
                        left: self.random.next_int_bounded(2) == 0,
                        right: self.random.next_int_bounded(2) == 0,
                    },
                    Type::PrisonHall => StrongholdKind::PrisonHall,
                    Type::LeftTurn => StrongholdKind::LeftTurn,
                    Type::RightTurn => StrongholdKind::RightTurn,
                    Type::RoomCrossing => StrongholdKind::RoomCrossing {
                        room: self.random.next_int_bounded(5),
                    },
                    Type::StraightStairsDown => StrongholdKind::StraightStairsDown,
                    Type::StairsDown => StrongholdKind::StairsDown,
                    Type::FiveCrossing => StrongholdKind::FiveCrossing {
                        left_low: self.random.next_boolean(),
                        left_high: self.random.next_boolean(),
                        right_low: self.random.next_boolean(),
                        right_high: self.random.next_int_bounded(3) > 0,
                    },
                    Type::ChestCorridor => StrongholdKind::ChestCorridor,
                    Type::Library => StrongholdKind::Library {
                        tall: bounds.y_span() > 6,
                    },
                    Type::PortalRoom => StrongholdKind::PortalRoom,
                }
            }
        };
        Some(StrongholdPiece {
            kind,
            bounds,
            facing,
            depth,
        })
    }

    /// `FillerCorridor.findPieceBox`: where a door's piece would hit another at the same floor,
    /// a corridor up to it, one block longer than the gap. No draws.
    fn filler_box(&self, at: [i32; 3], facing: Facing) -> Option<BoundingBox> {
        let reach = orient_box(at, [-1, -1, 0], [5, 5, 4], facing);
        let hit = self.pieces.iter().find(|p| p.bounds.intersects(&reach))?;
        if hit.bounds.min[1] != reach.min[1] {
            return None;
        }
        (1..=2).rev().find_map(|len| {
            let gap = orient_box(at, [-1, -1, 0], [5, 5, len], facing);
            (!hit.bounds.intersects(&gap))
                .then(|| orient_box(at, [-1, -1, 0], [5, 5, len + 1], facing))
        })
    }

    /// `generatePieceFromSmallDoor`: the imposed piece if there is one and it fits; else up to
    /// five weighted rolls, where a type used up, not yet deep enough or the last one picked ends
    /// the roll, and one whose piece doesn't fit hands the roll to the next type; else a filler.
    fn door_piece(&mut self, at: [i32; 3], facing: Facing, depth: i32) -> Option<StrongholdPiece> {
        // updatePieceWeight: nothing grows once every limited type is used up.
        let total: i32 = self.available.iter().map(|&w| WEIGHTS[w].1).sum();
        if !self
            .available
            .iter()
            .any(|&w| WEIGHTS[w].2 > 0 && self.placed[w] < WEIGHTS[w].2)
        {
            return None;
        }
        if let Some(ty) = self.imposed.take()
            && let Some(piece) = self.create(ty, at, facing, depth)
        {
            return Some(piece);
        }
        for _ in 0..5 {
            let mut roll = self.random.next_int_bounded(total);
            for i in 0..self.available.len() {
                let w = self.available[i];
                let (ty, weight, max) = WEIGHTS[w];
                roll -= weight;
                if roll >= 0 {
                    continue;
                }
                // doPlace: the library deeper than 4, the portal room deeper than 5.
                let deep_enough = match ty {
                    Type::Library => depth > 4,
                    Type::PortalRoom => depth > 5,
                    _ => true,
                };
                let can_place = (max == 0 || self.placed[w] < max) && deep_enough;
                if !can_place || self.previous == Some(w) {
                    break;
                }
                if let Some(piece) = self.create(ty, at, facing, depth) {
                    self.placed[w] += 1;
                    self.previous = Some(w);
                    if max != 0 && self.placed[w] >= max {
                        self.available.remove(i);
                    }
                    return Some(piece);
                }
            }
        }
        let bounds = self.filler_box(at, facing)?;
        (bounds.min[1] > 1).then_some(StrongholdPiece {
            kind: StrongholdKind::FillerCorridor,
            bounds,
            facing,
            depth,
        })
    }

    /// `generateAndAddPiece` from a piece at `depth`: a piece, added with its doors pending.
    fn generate_and_add(&mut self, at: [i32; 3], facing: Facing, depth: i32) {
        if depth > MAX_DEPTH
            || at[0].wrapping_sub(self.start[0]).wrapping_abs() > MAX_DISTANCE
            || at[2].wrapping_sub(self.start[2]).wrapping_abs() > MAX_DISTANCE
        {
            return;
        }
        if let Some(piece) = self.door_piece(at, facing, depth + 1) {
            if piece.kind == StrongholdKind::PortalRoom {
                self.portal = true;
            }
            self.pieces.push(piece);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    /// A door of `from`, `across` along its side and `up` from its floor.
    fn child(&mut self, from: StrongholdPiece, exit: Exit, across: i32, up: i32) {
        let (at, facing) = exit.from(&from.bounds, from.facing, across, up);
        self.generate_and_add(at, facing, from.depth);
    }

    /// A piece's `addChildren`: its doors, in order.
    fn add_children(&mut self, piece: StrongholdPiece) {
        use StrongholdKind::*;
        let north_or_east = matches!(piece.facing, Facing::North | Facing::East);
        match piece.kind {
            Start => {
                self.imposed = Some(Type::FiveCrossing);
                self.child(piece, Exit::Forward, 1, 1);
            }
            StairsDown | StraightStairsDown | PrisonHall | ChestCorridor => {
                self.child(piece, Exit::Forward, 1, 1);
            }
            Straight { left, right } => {
                self.child(piece, Exit::Forward, 1, 1);
                if left {
                    self.child(piece, Exit::Left, 2, 1);
                }
                if right {
                    self.child(piece, Exit::Right, 2, 1);
                }
            }
            LeftTurn => {
                let exit = if north_or_east {
                    Exit::Left
                } else {
                    Exit::Right
                };
                self.child(piece, exit, 1, 1);
            }
            RightTurn => {
                let exit = if north_or_east {
                    Exit::Right
                } else {
                    Exit::Left
                };
                self.child(piece, exit, 1, 1);
            }
            RoomCrossing { .. } => {
                self.child(piece, Exit::Forward, 4, 1);
                self.child(piece, Exit::Left, 4, 1);
                self.child(piece, Exit::Right, 4, 1);
            }
            FiveCrossing {
                left_low,
                left_high,
                right_low,
                right_high,
            } => {
                // The low and high doors' heights swap facing west or north.
                let (low, high) = match piece.facing {
                    Facing::West | Facing::North => (5, 3),
                    _ => (3, 5),
                };
                self.child(piece, Exit::Forward, 5, 1);
                if left_low {
                    self.child(piece, Exit::Left, 1, low);
                }
                if left_high {
                    self.child(piece, Exit::Left, 7, high);
                }
                if right_low {
                    self.child(piece, Exit::Right, 1, low);
                }
                if right_high {
                    self.child(piece, Exit::Right, 7, high);
                }
            }
            Library { .. } | PortalRoom | FillerCorridor => {}
        }
    }
}

/// A stronghold's pieces for its start in `chunk`, for world `seed`, in the game's order, and
/// the layout. Nothing reads the world. `adapts_terrain` grows the bounds.
pub(crate) fn layout(
    seed: i64,
    chunk: [i32; 2],
    heights: &Heights,
    adapts_terrain: bool,
) -> (Vec<StrongholdPiece>, Layout) {
    let x = chunk[0].wrapping_shl(4).wrapping_add(2);
    let z = chunk[1].wrapping_shl(4).wrapping_add(2);
    let mut attempt: i64 = 0;
    loop {
        let mut random = worldgen::large_feature(seed.wrapping_add(attempt), chunk[0], chunk[1]);
        attempt += 1;
        let facing = Facing::HORIZONTAL[random.next_int_bounded(4) as usize];
        // makeBoundingBox's 5 × 11 × 5 from its corner, whichever way it faces.
        let start = StrongholdPiece {
            kind: StrongholdKind::Start,
            bounds: BoundingBox {
                min: [x, START_Y, z],
                max: [x + 4, START_Y + 10, z + 4],
            },
            facing,
            depth: 0,
        };
        let mut builder = Builder {
            random,
            pieces: vec![start],
            start: start.bounds.min,
            available: (0..WEIGHTS.len()).collect(),
            placed: [0; WEIGHTS.len()],
            previous: None,
            imposed: None,
            pending: Vec::new(),
            portal: false,
        };
        builder.add_children(start);
        while !builder.pending.is_empty() {
            let i = builder
                .random
                .next_int_bounded(builder.pending.len() as i32) as usize;
            let piece = builder.pieces[builder.pending.remove(i)];
            builder.add_children(piece);
        }
        let Builder {
            mut random,
            mut pieces,
            portal,
            ..
        } = builder;
        // StructurePiecesBuilder.moveBelowSeaLevel(seaLevel, minY, random, 10), every attempt.
        let around = pieces
            .iter()
            .map(|p| p.bounds)
            .reduce(BoundingBox::encapsulate)
            .unwrap_or(start.bounds);
        let top = heights.sea_level.wrapping_sub(10);
        let mut y = around
            .y_span()
            .wrapping_add(heights.generator_min_y)
            .wrapping_add(1);
        if y < top {
            y = y.wrapping_add(random.next_int_bounded(top.wrapping_sub(y)));
        }
        if !portal {
            continue;
        }
        let dy = y.wrapping_sub(around.max[1]);
        for p in &mut pieces {
            p.bounds = p.bounds.moved([0, dy, 0]);
        }
        let mut bounds = around.moved([0, dy, 0]);
        if adapts_terrain {
            bounds = bounds.inflated(12);
        }
        let layout = Layout {
            pieces: pieces.iter().map(|p| p.bounds).collect(),
            placed: Vec::new(),
            bounds,
        };
        return (pieces, layout);
    }
}
