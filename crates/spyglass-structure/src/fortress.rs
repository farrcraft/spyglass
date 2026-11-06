//! Nether fortress layout (`NetherFortressStructure.generatePieces` and `NetherFortressPieces`):
//! every piece's box, in the game's order, and which small corridor turns hold a chest. See
//! `docs/specs/structure-placement.md#nether-fortresses`.
//!
//! From a bridge crossing at the start chunk, each piece names the exits it grows: a bridge
//! piece or a castle piece, picked by weight from that kind's list, with counts limiting the
//! rarer ones. Exits are grown in random order from a pending list, not depth first, so the
//! layout is a breadth of bridges and castle corridors around the start. A piece whose box would
//! collide, go below y 11 or past depth 30 isn't made; an end filler caps the exit instead.

use spyglass_rng::{LegacyRandomSource, RandomSource, worldgen};

use crate::assembly::Layout;
use crate::jigsaw::BoundingBox;
use crate::scattered::{Exit, Facing, orient_box};

/// `NetherFortressPieces.MAX_DEPTH`.
const MAX_DEPTH: i32 = 30;

/// `MAGIC_START_Y`: the start's floor before the move.
const START_Y: i32 = 64;

/// How far an exit may be from the start's corner on x or z before it's only capped.
const MAX_DISTANCE: i32 = 112;

/// A fortress piece's type (`NetherFortressPieces`' piece classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FortressKind {
    /// `StartPiece`: a bridge crossing.
    Start,
    /// `BridgeStraight`.
    BridgeStraight,
    /// `BridgeCrossing`.
    BridgeCrossing,
    /// `RoomCrossing`.
    RoomCrossing,
    /// `StairsRoom`.
    StairsRoom,
    /// `MonsterThrone`: the blaze spawner.
    MonsterThrone,
    /// `CastleEntrance`.
    CastleEntrance,
    /// `BridgeEndFiller`: an exit's ragged end.
    BridgeEndFiller,
    /// `CastleSmallCorridorPiece`.
    CastleSmallCorridor,
    /// `CastleSmallCorridorCrossingPiece`.
    CastleSmallCorridorCrossing,
    /// `CastleSmallCorridorRightTurnPiece`, which may hold a chest.
    CastleSmallCorridorRightTurn,
    /// `CastleSmallCorridorLeftTurnPiece`, which may hold a chest.
    CastleSmallCorridorLeftTurn,
    /// `CastleCorridorStairsPiece`.
    CastleCorridorStairs,
    /// `CastleCorridorTBalconyPiece`.
    CastleCorridorTBalcony,
    /// `CastleStalkRoom`: nether wart.
    CastleStalkRoom,
}

impl FortressKind {
    /// Its `orientBox` offset and size, across, up and along.
    fn shape(self) -> ([i32; 3], [i32; 3]) {
        use FortressKind::*;
        match self {
            Start | BridgeCrossing => ([-8, -3, 0], [19, 10, 19]),
            BridgeStraight => ([-1, -3, 0], [5, 10, 19]),
            RoomCrossing => ([-2, 0, 0], [7, 9, 7]),
            StairsRoom => ([-2, 0, 0], [7, 11, 7]),
            MonsterThrone => ([-2, 0, 0], [7, 8, 9]),
            CastleEntrance | CastleStalkRoom => ([-5, -3, 0], [13, 14, 13]),
            BridgeEndFiller => ([-1, -3, 0], [5, 10, 8]),
            CastleSmallCorridor
            | CastleSmallCorridorCrossing
            | CastleSmallCorridorRightTurn
            | CastleSmallCorridorLeftTurn => ([-1, 0, 0], [5, 7, 5]),
            CastleCorridorStairs => ([-1, -7, 0], [5, 14, 10]),
            CastleCorridorTBalcony => ([-3, 0, 0], [9, 7, 9]),
        }
    }
}

/// One of a fortress's pieces, as placing it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FortressPiece {
    /// Its type.
    pub kind: FortressKind,
    /// Its box.
    pub bounds: BoundingBox,
    /// Its orientation, which turns its own coordinates.
    pub facing: Facing,
    /// For a small corridor turn, `isNeedingChest`, decided as it's made.
    pub chest: bool,
    /// `genDepth`: 0 for the start, one more than its parent's.
    depth: i32,
}

/// `PieceWeight`: a piece type a list may pick.
struct Weight {
    kind: FortressKind,
    weight: i32,
    /// `maxPlaceCount`; 0 for no limit.
    max: i32,
    /// `allowInRow`: whether it may follow itself.
    allow_in_row: bool,
}

/// `BRIDGE_PIECE_WEIGHTS`.
const BRIDGE_WEIGHTS: [Weight; 6] = [
    weight(FortressKind::BridgeStraight, 30, 0, true),
    weight(FortressKind::BridgeCrossing, 10, 4, false),
    weight(FortressKind::RoomCrossing, 10, 4, false),
    weight(FortressKind::StairsRoom, 10, 3, false),
    weight(FortressKind::MonsterThrone, 5, 2, false),
    weight(FortressKind::CastleEntrance, 5, 1, false),
];

/// `CASTLE_PIECE_WEIGHTS`.
const CASTLE_WEIGHTS: [Weight; 7] = [
    weight(FortressKind::CastleSmallCorridor, 25, 0, true),
    weight(FortressKind::CastleSmallCorridorCrossing, 15, 5, false),
    weight(FortressKind::CastleSmallCorridorRightTurn, 5, 10, false),
    weight(FortressKind::CastleSmallCorridorLeftTurn, 5, 10, false),
    weight(FortressKind::CastleCorridorStairs, 10, 3, true),
    weight(FortressKind::CastleCorridorTBalcony, 7, 2, false),
    weight(FortressKind::CastleStalkRoom, 5, 2, false),
];

const fn weight(kind: FortressKind, weight: i32, max: i32, allow_in_row: bool) -> Weight {
    Weight {
        kind,
        weight,
        max,
        allow_in_row,
    }
}

/// One start's generation. A weight is `(castle, index)` into [`BRIDGE_WEIGHTS`] or
/// [`CASTLE_WEIGHTS`]; the counts are the game's static `placeCount`s, reset for each start.
struct Builder {
    random: LegacyRandomSource,
    pieces: Vec<FortressPiece>,
    /// The start's box corner, which exits' distances are measured from.
    start: [i32; 3],
    /// `availableBridgePieces` and `availableCastlePieces`: the weights not yet used up.
    available: [Vec<usize>; 2],
    /// Each weight's `placeCount`, bridge then castle.
    placed: [Vec<i32>; 2],
    /// `previousPiece`, shared by both lists.
    previous: Option<(bool, usize)>,
    /// `pendingChildren`: pieces whose exits aren't grown yet, by index.
    pending: Vec<usize>,
}

impl Builder {
    fn weights(castle: bool) -> &'static [Weight] {
        if castle {
            &CASTLE_WEIGHTS
        } else {
            &BRIDGE_WEIGHTS
        }
    }

    fn collides(&self, bbox: &BoundingBox) -> bool {
        self.pieces.iter().any(|p| p.bounds.intersects(bbox))
    }

    /// A piece's `createPiece`: its box if it's above y 10 and free, and the piece, its
    /// constructor drawing what it draws.
    fn create(
        &mut self,
        kind: FortressKind,
        at: [i32; 3],
        facing: Facing,
        depth: i32,
    ) -> Option<FortressPiece> {
        let (offset, size) = kind.shape();
        let bounds = orient_box(at, offset, size, facing);
        // isOkBox.
        if bounds.min[1] <= 10 || self.collides(&bounds) {
            return None;
        }
        let chest = match kind {
            FortressKind::CastleSmallCorridorRightTurn
            | FortressKind::CastleSmallCorridorLeftTurn => self.random.next_int_bounded(3) == 0,
            // `selfSeed`, which seeds its own random for its ragged edges.
            FortressKind::BridgeEndFiller => {
                self.random.next_int();
                false
            }
            _ => false,
        };
        Some(FortressPiece {
            kind,
            bounds,
            facing,
            chest,
            depth,
        })
    }

    /// `updatePieceWeight`: the list's total weight, or −1 once only unlimited pieces are left.
    fn total_weight(&self, castle: bool) -> i32 {
        let weights = Self::weights(castle);
        let c = usize::from(castle);
        let mut limited_left = false;
        let mut total = 0;
        for &w in &self.available[c] {
            if weights[w].max > 0 && self.placed[c][w] < weights[w].max {
                limited_left = true;
            }
            total += weights[w].weight;
        }
        if limited_left { total } else { -1 }
    }

    /// `generatePiece`: up to five weighted rolls of the list; a type used up, or following
    /// itself where it may not, ends the roll, and one whose piece doesn't fit hands the roll to
    /// the next type. Without a piece, an end filler.
    fn generate_piece(
        &mut self,
        castle: bool,
        at: [i32; 3],
        facing: Facing,
        depth: i32,
    ) -> Option<FortressPiece> {
        let weights = Self::weights(castle);
        let c = usize::from(castle);
        let total = self.total_weight(castle);
        let ok = total > 0 && depth <= MAX_DEPTH;
        let mut tries = 0;
        while tries < 5 && ok {
            tries += 1;
            let mut roll = self.random.next_int_bounded(total);
            for i in 0..self.available[c].len() {
                let w = self.available[c][i];
                roll -= weights[w].weight;
                if roll >= 0 {
                    continue;
                }
                let can_place = weights[w].max == 0 || self.placed[c][w] < weights[w].max;
                if !can_place || (self.previous == Some((castle, w)) && !weights[w].allow_in_row) {
                    break;
                }
                if let Some(piece) = self.create(weights[w].kind, at, facing, depth) {
                    self.placed[c][w] += 1;
                    self.previous = Some((castle, w));
                    if weights[w].max != 0 && self.placed[c][w] >= weights[w].max {
                        self.available[c].remove(i);
                    }
                    return Some(piece);
                }
            }
        }
        self.create(FortressKind::BridgeEndFiller, at, facing, depth)
    }

    /// `generateAndAddPiece` from a piece at `depth`: a piece of the bridge or castle list,
    /// added with its exits pending. Too far from the start, an end filler is made, drawing, and
    /// dropped.
    fn generate_and_add(&mut self, at: [i32; 3], facing: Facing, depth: i32, castle: bool) {
        if at[0].wrapping_sub(self.start[0]).wrapping_abs() > MAX_DISTANCE
            || at[2].wrapping_sub(self.start[2]).wrapping_abs() > MAX_DISTANCE
        {
            self.create(FortressKind::BridgeEndFiller, at, facing, depth);
            return;
        }
        if let Some(piece) = self.generate_piece(castle, at, facing, depth + 1) {
            self.pieces.push(piece);
            self.pending.push(self.pieces.len() - 1);
        }
    }

    /// An exit of `from`, `across` along its side and `up` from its floor.
    fn child(&mut self, from: FortressPiece, exit: Exit, across: i32, up: i32, castle: bool) {
        let (at, facing) = exit.from(&from.bounds, from.facing, across, up);
        self.generate_and_add(at, facing, from.depth, castle);
    }

    /// A piece's `addChildren`: its exits, in order.
    fn add_children(&mut self, piece: FortressPiece) {
        use FortressKind::*;
        match piece.kind {
            Start | BridgeCrossing => {
                self.child(piece, Exit::Forward, 8, 3, false);
                self.child(piece, Exit::Left, 8, 3, false);
                self.child(piece, Exit::Right, 8, 3, false);
            }
            BridgeStraight => self.child(piece, Exit::Forward, 1, 3, false),
            RoomCrossing => {
                self.child(piece, Exit::Forward, 2, 0, false);
                self.child(piece, Exit::Left, 2, 0, false);
                self.child(piece, Exit::Right, 2, 0, false);
            }
            StairsRoom => self.child(piece, Exit::Right, 2, 6, false),
            MonsterThrone | BridgeEndFiller => {}
            CastleEntrance => self.child(piece, Exit::Forward, 5, 3, true),
            CastleSmallCorridor | CastleCorridorStairs => {
                self.child(piece, Exit::Forward, 1, 0, true);
            }
            CastleSmallCorridorCrossing => {
                self.child(piece, Exit::Forward, 1, 0, true);
                self.child(piece, Exit::Left, 1, 0, true);
                self.child(piece, Exit::Right, 1, 0, true);
            }
            CastleSmallCorridorRightTurn => self.child(piece, Exit::Right, 1, 0, true),
            CastleSmallCorridorLeftTurn => self.child(piece, Exit::Left, 1, 0, true),
            CastleCorridorTBalcony => {
                let across = match piece.facing {
                    Facing::West | Facing::North => 5,
                    _ => 1,
                };
                // Which list each side picks from is drawn before that side grows.
                let castle = self.random.next_int_bounded(8) > 0;
                self.child(piece, Exit::Left, across, 0, castle);
                let castle = self.random.next_int_bounded(8) > 0;
                self.child(piece, Exit::Right, across, 0, castle);
            }
            CastleStalkRoom => {
                self.child(piece, Exit::Forward, 5, 3, true);
                self.child(piece, Exit::Forward, 5, 11, true);
            }
        }
    }
}

/// A fortress's pieces for its start in `chunk`, for world `seed`, in the game's order, and the
/// layout. Nothing reads the world. `adapts_terrain` grows the bounds.
pub(crate) fn layout(
    seed: i64,
    chunk: [i32; 2],
    adapts_terrain: bool,
) -> (Vec<FortressPiece>, Layout) {
    let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
    let facing = Facing::HORIZONTAL[random.next_int_bounded(4) as usize];
    let x = chunk[0].wrapping_shl(4).wrapping_add(2);
    let z = chunk[1].wrapping_shl(4).wrapping_add(2);
    // StartPiece: makeBoundingBox's 19 × 10 × 19 from its corner, whichever way it faces.
    let start = FortressPiece {
        kind: FortressKind::Start,
        bounds: BoundingBox {
            min: [x, START_Y, z],
            max: [x + 18, START_Y + 9, z + 18],
        },
        facing,
        chest: false,
        depth: 0,
    };
    let mut builder = Builder {
        random,
        pieces: vec![start],
        start: start.bounds.min,
        available: [
            (0..BRIDGE_WEIGHTS.len()).collect(),
            (0..CASTLE_WEIGHTS.len()).collect(),
        ],
        placed: [vec![0; BRIDGE_WEIGHTS.len()], vec![0; CASTLE_WEIGHTS.len()]],
        previous: None,
        pending: Vec::new(),
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
        ..
    } = builder;
    // StructurePiecesBuilder.moveInsideHeights(random, 48, 70).
    let around = pieces
        .iter()
        .map(|p| p.bounds)
        .reduce(BoundingBox::encapsulate)
        .unwrap_or(start.bounds);
    let room = 70 - 48 + 1 - around.y_span();
    let y = if room > 1 {
        48 + random.next_int_bounded(room)
    } else {
        48
    };
    let dy = y - around.min[1];
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
    (pieces, layout)
}
