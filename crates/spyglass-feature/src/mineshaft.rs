//! Mineshafts' pieces placed in a chunk (`MineshaftPieces`' `postProcess`), for the chest
//! minecarts their corridors leave (`docs/specs/loot.md#containers`).
//!
//! Rooms, crossings and stairs only carve air and lay planks; corridors also draw from the
//! structure's random, block by block: cobwebs, supports and torches, a 1 in 100 chance of a
//! minecart on each side of every support, rails. How many draws come before a minecart's
//! seed depends on the blocks the corridor reads (a ceiling over a support, the floor under a
//! minecart, liquid beside the corridor), and those blocks on what every piece placed in the
//! chunk before it wrote. So every piece is placed, in order, on the region.

use spyglass_blocks::State;
use spyglass_blocks::behaviour::Face;
use spyglass_data::codec::LoadError;
use spyglass_rng::{RandomSource, WorldgenRandom};
use spyglass_structure::{BoundingBox, ScatteredPiece, ShaftPiece, Structure};

use crate::blocks::{Blocks, Heightmap};
use crate::containers::{ContainerDoubt, Reads, StructureContainer};
use crate::region::Region;

/// `BuiltInLootTables.ABANDONED_MINESHAFT`.
const LOOT_TABLE: &str = "minecraft:chests/abandoned_mineshaft";

/// The entity a corridor's chest is.
pub(crate) const MINECART: &str = "minecraft:chest_minecart";

/// The blocks a type of mineshaft builds with (`MineshaftStructure.Type`).
#[derive(Debug, Clone, Copy)]
struct Materials {
    wood: State,
    planks: State,
    fence: State,
}

/// The blocks mineshafts place and test for.
#[derive(Debug, Clone)]
pub(crate) struct ShaftBlocks {
    normal: Materials,
    mesa: Materials,
    cave_air: State,
    cobweb: State,
    rail: State,
    wall_torch: State,
    iron_chain: State,
    spawner: State,
}

impl ShaftBlocks {
    pub(crate) fn load(blocks: &mut Blocks) -> Result<Self, LoadError> {
        let materials = |blocks: &mut Blocks, kind: &str| -> Result<Materials, LoadError> {
            Ok(Materials {
                wood: blocks.state(&format!("minecraft:{kind}_log"))?,
                planks: blocks.state(&format!("minecraft:{kind}_planks"))?,
                fence: blocks.state(&format!("minecraft:{kind}_fence"))?,
            })
        };
        Ok(ShaftBlocks {
            normal: materials(blocks, "oak")?,
            mesa: materials(blocks, "dark_oak")?,
            cave_air: blocks.state("minecraft:cave_air")?,
            cobweb: blocks.state("minecraft:cobweb")?,
            rail: blocks.state("minecraft:rail")?,
            wall_torch: blocks.state("minecraft:wall_torch")?,
            iron_chain: blocks.state("minecraft:iron_chain")?,
            spawner: blocks.state("minecraft:spawner")?,
        })
    }
}

/// What a start's pieces need while they're placed in one chunk.
pub(crate) struct Shaft<'a, 'r, 'b> {
    pub region: &'a mut Region<'r>,
    pub blocks: &'a Blocks,
    pub shaft_blocks: &'a ShaftBlocks,
    /// The structure, for its type and `#mineshaft_blocking`.
    pub info: &'a Structure,
    /// Its index in its `StructureSets`.
    pub structure: usize,
    /// Whether a block is in a `#mineshaft_blocking` biome; `None` where a tie decides it.
    pub blocking: &'b dyn Fn([i32; 3]) -> Option<bool>,
    pub reads: &'b Reads<'b>,
    pub random: &'a mut WorldgenRandom,
    /// The first reason what's been read might not be the region's.
    pub doubt: Option<ContainerDoubt>,
    pub out: &'a mut Vec<StructureContainer>,
}

/// One piece being placed: its box and orientation, and the chunk's writable box.
struct Piece {
    /// Turns the piece's own coordinates into the world's, for a corridor or stairs.
    turned: Option<ScatteredPiece>,
    /// The chunk's writable box (`ChunkGenerator.getWritableArea`).
    chunk: BoundingBox,
    /// Whether something placed before could have written near the piece, so reads need
    /// checking one by one.
    gated: bool,
}

impl Piece {
    /// `getWorldPos`.
    fn world(&self, p: [i32; 3]) -> [i32; 3] {
        match &self.turned {
            Some(turned) => turned.world(p, turned.bounds.min[1]),
            None => p,
        }
    }
}

impl Shaft<'_, '_, '_> {
    /// The pieces of one start whose boxes meet the chunk, in their order.
    pub(crate) fn place(&mut self, pieces: &[ShaftPiece]) {
        let [cx, cz] = self.region.center();
        let chunk = BoundingBox {
            min: [
                cx.wrapping_shl(4),
                self.region.min_y() + 1,
                cz.wrapping_shl(4),
            ],
            max: [
                cx.wrapping_shl(4) + 15,
                self.region.max_y(),
                cz.wrapping_shl(4) + 15,
            ],
        };
        for shaft in pieces {
            let bounds = *shaft.bounds();
            if !bounds.intersects(&chunk) {
                continue;
            }
            let turned = match shaft {
                ShaftPiece::Corridor { facing, .. } | ShaftPiece::Stairs { facing, .. } => {
                    Some(ScatteredPiece {
                        bounds,
                        facing: *facing,
                    })
                }
                _ => None,
            };
            // A pillar or chain reads up to 20 blocks below the piece and 50 above.
            let near = BoundingBox {
                min: [bounds.min[0] - 1, bounds.min[1] - 22, bounds.min[2] - 1],
                max: [bounds.max[0] + 1, bounds.max[1] + 52, bounds.max[2] + 1],
            };
            let gated = (self.reads)(near.min, near.max, self.structure).is_some();
            let piece = Piece {
                turned,
                chunk,
                gated,
            };
            match shaft {
                ShaftPiece::Room { bounds, entrances } => self.room(&piece, bounds, entrances),
                ShaftPiece::Corridor {
                    bounds,
                    rails,
                    spider,
                    sections,
                    ..
                } => self.corridor(&piece, bounds, *rails, *spider, *sections),
                ShaftPiece::Crossing { bounds } => self.crossing(&piece, bounds),
                ShaftPiece::Stairs { bounds, .. } => self.stairs(&piece, bounds),
            }
        }
    }

    fn materials(&self) -> Materials {
        match self.info.mineshaft {
            Some(m) if m.mesa => self.shaft_blocks.mesa,
            _ => self.shaft_blocks.normal,
        }
    }

    /// `level.getBlockState`, noting why it might not be the block there.
    fn read(&mut self, piece: &Piece, pos: [i32; 3]) -> State {
        if piece.gated
            && self.doubt.is_none()
            && let Some(why) = (self.reads)(pos, pos, self.structure)
        {
            self.doubt = Some(ContainerDoubt::Read(why));
        }
        self.region.take_doubt();
        let state = self.region.get(pos);
        if let Some(why) = self.region.take_doubt() {
            self.doubt.get_or_insert(ContainerDoubt::Read(why));
        }
        state
    }

    /// `StructurePiece.getBlock`: air outside the chunk, without reading.
    fn get_block(&mut self, piece: &Piece, local: [i32; 3]) -> State {
        let pos = piece.world(local);
        if !piece.chunk.contains(pos) {
            return spyglass_blocks::state::AIR;
        }
        self.read(piece, pos)
    }

    fn is_air(&self, s: State) -> bool {
        self.blocks.is_air(s)
    }

    /// `level.setBlock`. What follows a doubt is in doubt anyway, so the blocks aren't marked.
    fn set(&mut self, pos: [i32; 3], state: State) {
        self.region.set(pos, state);
    }

    /// `MineShaftPiece.canBeReplaced`: not this type's planks, wood or fence, nor a chain.
    fn can_be_replaced(&mut self, piece: &Piece, local: [i32; 3]) -> bool {
        let here = self.get_block(piece, local);
        let m = self.materials();
        let id = |s: State| self.blocks.palette().block_id(s);
        ![m.planks, m.wood, m.fence, self.shaft_blocks.iron_chain]
            .iter()
            .any(|&b| id(b) == id(here))
    }

    /// `StructurePiece.placeBlock`: inside the chunk, where the block can be replaced.
    fn place_block(&mut self, piece: &Piece, state: State, local: [i32; 3]) {
        let pos = piece.world(local);
        if !piece.chunk.contains(pos) || !self.can_be_replaced(piece, local) {
            return;
        }
        self.set(pos, state);
    }

    /// `generateBox(…, edge, inner, skipAir)`, with one state for edge and inside (every use
    /// here), y then x then z.
    fn generate_box(&mut self, piece: &Piece, min: [i32; 3], max: [i32; 3], state: State) {
        for y in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                for z in min[2]..=max[2] {
                    self.place_block(piece, state, [x, y, z]);
                }
            }
        }
    }

    /// `isInterior(x, y, z)`: the block above is inside the chunk and below the ocean floor
    /// as terrain left it.
    fn is_interior(&mut self, piece: &Piece, [x, y, z]: [i32; 3]) -> bool {
        let p = piece.world([x, y + 1, z]);
        if !piece.chunk.contains(p) {
            return false;
        }
        self.region.take_doubt();
        let floor = self.region.height(Heightmap::OceanFloorWg, p[0], p[2]);
        if let Some(why) = self.region.take_doubt() {
            self.doubt.get_or_insert(ContainerDoubt::Read(why));
        }
        p[1] < floor
    }

    /// `generateMaybeBox`: a `nextFloat()` for every cell, and the block where it's at most
    /// `chance` and the cell is interior if `skyless`.
    fn generate_maybe_box(
        &mut self,
        piece: &Piece,
        chance: f32,
        min: [i32; 3],
        max: [i32; 3],
        state: State,
        skyless: bool,
    ) {
        for y in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                for z in min[2]..=max[2] {
                    if self.random.next_float() > chance {
                        continue;
                    }
                    if skyless && !self.is_interior(piece, [x, y, z]) {
                        continue;
                    }
                    self.place_block(piece, state, [x, y, z]);
                }
            }
        }
    }

    /// `maybeGenerateBlock`: always a `nextFloat()`.
    fn maybe_generate_block(&mut self, piece: &Piece, chance: f32, local: [i32; 3], state: State) {
        if self.random.next_float() < chance {
            self.place_block(piece, state, local);
        }
    }

    /// `MineShaftPiece.isInInvalidLocation`: the box grown by one and cut to the chunk has
    /// its centre in a `#mineshaft_blocking` biome, or a liquid on any face.
    fn is_in_invalid_location(&mut self, piece: &Piece, bounds: &BoundingBox) -> bool {
        let c = &piece.chunk;
        let min = [0, 1, 2].map(|i| (bounds.min[i] - 1).max(c.min[i]));
        let max = [0, 1, 2].map(|i| (bounds.max[i] + 1).min(c.max[i]));
        let centre = [0, 1, 2].map(|i| (min[i] + max[i]) / 2);
        match (self.blocking)(centre) {
            Some(true) => return true,
            Some(false) => {}
            None => {
                self.doubt
                    .get_or_insert(ContainerDoubt::Read(crate::Doubt::Tie));
            }
        }
        let liquid = |shaft: &mut Self, p: [i32; 3]| {
            let s = shaft.read(piece, p);
            shaft.blocks.behaviour(s).liquid
        };
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                if liquid(self, [x, min[1], z]) || liquid(self, [x, max[1], z]) {
                    return true;
                }
            }
        }
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                if liquid(self, [x, y, min[2]]) || liquid(self, [x, y, max[2]]) {
                    return true;
                }
            }
        }
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                if liquid(self, [min[0], y, z]) || liquid(self, [max[0], y, z]) {
                    return true;
                }
            }
        }
        false
    }

    /// `setPlanksBlock`: planks where the block isn't sturdy on top, if interior.
    fn set_planks_block(&mut self, piece: &Piece, local: [i32; 3]) {
        if !self.is_interior(piece, local) {
            return;
        }
        let pos = piece.world(local);
        let here = self.read(piece, pos);
        if !self.blocks.behaviour(here).sturdy(Face::Up) {
            let planks = self.materials().planks;
            self.set(pos, planks);
        }
    }

    /// `MineShaftRoom.postProcess`: its lower air, its exits' openings, and a dome above.
    fn room(&mut self, piece: &Piece, b: &BoundingBox, entrances: &[BoundingBox]) {
        if self.is_in_invalid_location(piece, b) {
            return;
        }
        let air = self.shaft_blocks.cave_air;
        self.generate_box(
            piece,
            [b.min[0], b.min[1] + 1, b.min[2]],
            [b.max[0], (b.min[1] + 3).min(b.max[1]), b.max[2]],
            air,
        );
        for e in entrances {
            self.generate_box(piece, [e.min[0], e.max[1] - 2, e.min[2]], e.max, air);
        }
        self.upper_half_sphere(piece, [b.min[0], b.min[1] + 4, b.min[2]], b.max, air);
    }

    /// `generateUpperHalfSphere`, without skipping air: the cells of the box within the
    /// ellipsoid around its bottom face's centre, in `f32` as the game computes it.
    fn upper_half_sphere(&mut self, piece: &Piece, min: [i32; 3], max: [i32; 3], state: State) {
        let width = (max[0] - min[0] + 1) as f32;
        let height = (max[1] - min[1] + 1) as f32;
        let depth = (max[2] - min[2] + 1) as f32;
        let centre_x = min[0] as f32 + width / 2.0;
        let centre_z = min[2] as f32 + depth / 2.0;
        for y in min[1]..=max[1] {
            let dy = (y - min[1]) as f32 / height;
            for x in min[0]..=max[0] {
                let dx = (x as f32 - centre_x) / (width * 0.5);
                for z in min[2]..=max[2] {
                    let dz = (z as f32 - centre_z) / (depth * 0.5);
                    if dx * dx + dy * dy + dz * dz <= 1.05 {
                        self.place_block(piece, state, [x, y, z]);
                    }
                }
            }
        }
    }

    /// `MineShaftCrossing.postProcess`: the cross of air on one or two floors, pillars at the
    /// corners under a ceiling, and a plank floor.
    fn crossing(&mut self, piece: &Piece, b: &BoundingBox) {
        if self.is_in_invalid_location(piece, b) {
            return;
        }
        let air = self.shaft_blocks.cave_air;
        let ([x0, y0, z0], [x1, y1, z1]) = (b.min, b.max);
        if y1 - y0 + 1 > 3 {
            self.generate_box(piece, [x0 + 1, y0, z0], [x1 - 1, y0 + 3 - 1, z1], air);
            self.generate_box(piece, [x0, y0, z0 + 1], [x1, y0 + 3 - 1, z1 - 1], air);
            self.generate_box(piece, [x0 + 1, y1 - 2, z0], [x1 - 1, y1, z1], air);
            self.generate_box(piece, [x0, y1 - 2, z0 + 1], [x1, y1, z1 - 1], air);
            self.generate_box(
                piece,
                [x0 + 1, y0 + 3, z0 + 1],
                [x1 - 1, y0 + 3, z1 - 1],
                air,
            );
        } else {
            self.generate_box(piece, [x0 + 1, y0, z0], [x1 - 1, y1, z1], air);
            self.generate_box(piece, [x0, y0, z0 + 1], [x1, y1, z1 - 1], air);
        }
        for (x, z) in [
            (x0 + 1, z0 + 1),
            (x0 + 1, z1 - 1),
            (x1 - 1, z0 + 1),
            (x1 - 1, z1 - 1),
        ] {
            // placeSupportPillar: planks up to the top where there's a ceiling above it.
            let above = self.get_block(piece, [x, y1 + 1, z]);
            if !self.is_air(above) {
                let planks = self.materials().planks;
                self.generate_box(piece, [x, y0, z], [x, y1, z], planks);
            }
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.set_planks_block(piece, [x, y0 - 1, z]);
            }
        }
    }

    /// `MineShaftStairs.postProcess`: air down the stairs.
    fn stairs(&mut self, piece: &Piece, b: &BoundingBox) {
        if self.is_in_invalid_location(piece, b) {
            return;
        }
        let air = self.shaft_blocks.cave_air;
        self.generate_box(piece, [0, 5, 0], [2, 7, 1], air);
        self.generate_box(piece, [0, 0, 7], [2, 2, 8], air);
        for i in 0..5 {
            let below = if i < 4 { 1 } else { 0 };
            self.generate_box(piece, [0, 5 - i - below, 2 + i], [2, 7 - i, 2 + i], air);
        }
    }

    /// `MineShaftCorridor.postProcess`, in its own coordinates: x across (0 to 2), y up (0 to
    /// 2), z along (0 to `len`).
    fn corridor(
        &mut self,
        piece: &Piece,
        b: &BoundingBox,
        rails: bool,
        spider: bool,
        sections: i32,
    ) {
        if self.is_in_invalid_location(piece, b) {
            return;
        }
        let air = self.shaft_blocks.cave_air;
        let len = sections * 5 - 1;
        self.generate_box(piece, [0, 0, 0], [2, 1, len], air);
        self.generate_maybe_box(piece, 0.8, [0, 2, 0], [2, 2, len], air, false);
        if spider {
            // Every cell of a box 2 high is on its edge, so these are all cobwebs.
            let cobweb = self.shaft_blocks.cobweb;
            self.generate_maybe_box(piece, 0.6, [0, 0, 0], [2, 1, len], cobweb, true);
        }
        let mut placed_spider = false;
        for i in 0..sections {
            let z = 2 + i * 5;
            self.support(piece, z);
            for (chance, x, dz) in [
                (0.1, 0, -1),
                (0.1, 2, -1),
                (0.1, 0, 1),
                (0.1, 2, 1),
                (0.05, 0, -2),
                (0.05, 2, -2),
                (0.05, 0, 2),
                (0.05, 2, 2),
            ] {
                self.maybe_cobweb(piece, chance, [x, 2, z + dz]);
            }
            if self.random.next_int_bounded(100) == 0 {
                self.minecart(piece, [2, 0, z - 1]);
            }
            if self.random.next_int_bounded(100) == 0 {
                self.minecart(piece, [0, 0, z + 1]);
            }
            if spider && !placed_spider {
                let spot = [1, 0, z - 1 + self.random.next_int_bounded(3)];
                let pos = piece.world(spot);
                if piece.chunk.contains(pos) && self.is_interior(piece, spot) {
                    placed_spider = true;
                    // Its entity's spawn potentials are empty: no draw.
                    let spawner = self.shaft_blocks.spawner;
                    self.set(pos, spawner);
                }
            }
        }
        for x in 0..=2 {
            for z in 0..=len {
                self.set_planks_block(piece, [x, -1, z]);
            }
        }
        self.double_support(piece, [0, -1, 2]);
        if sections > 1 {
            self.double_support(piece, [0, -1, len - 2]);
        }
        if rails {
            let rail = self.shaft_blocks.rail;
            for z in 0..=len {
                let floor = self.get_block(piece, [1, -1, z]);
                let floor = self.blocks.behaviour(floor);
                if !floor.air && floor.solid_render {
                    let chance = if self.is_interior(piece, [1, 0, z]) {
                        0.7
                    } else {
                        0.9
                    };
                    self.maybe_generate_block(piece, chance, [1, 0, z], rail);
                }
            }
        }
    }

    /// `placeSupport(0, 0, z, 2, 2)`: under a ceiling, two fence posts and planks across, or
    /// planks on the posts only, with a torch on either side by chance.
    fn support(&mut self, piece: &Piece, z: i32) {
        for x in 0..=2 {
            let above = self.get_block(piece, [x, 3, z]);
            if self.is_air(above) {
                return;
            }
        }
        let Materials { planks, fence, .. } = self.materials();
        self.generate_box(piece, [0, 0, z], [0, 1, z], fence);
        self.generate_box(piece, [2, 0, z], [2, 1, z], fence);
        if self.random.next_int_bounded(4) == 0 {
            self.generate_box(piece, [0, 2, z], [0, 2, z], planks);
            self.generate_box(piece, [2, 2, z], [2, 2, z], planks);
        } else {
            self.generate_box(piece, [0, 2, z], [2, 2, z], planks);
            let torch = self.shaft_blocks.wall_torch;
            self.maybe_generate_block(piece, 0.05, [1, 2, z - 1], torch);
            self.maybe_generate_block(piece, 0.05, [1, 2, z + 1], torch);
        }
    }

    /// `maybePlaceCobWeb`: where interior, a `nextFloat()`, and a cobweb if it's under
    /// `chance` and two neighbours inside the chunk are sturdy against it.
    fn maybe_cobweb(&mut self, piece: &Piece, chance: f32, local: [i32; 3]) {
        if !self.is_interior(piece, local) || self.random.next_float() >= chance {
            return;
        }
        let pos = piece.world(local);
        let mut sturdy = 0;
        for face in Face::ALL {
            let step = face.step();
            let n = [0, 1, 2].map(|i| pos[i] + step[i]);
            if piece.chunk.contains(n) {
                let s = self.read(piece, n);
                if self.blocks.behaviour(s).sturdy(face.opposite()) {
                    sturdy += 1;
                    if sturdy >= 2 {
                        let cobweb = self.shaft_blocks.cobweb;
                        self.place_block(piece, cobweb, local);
                        return;
                    }
                }
            }
        }
    }

    /// `MineShaftCorridor.createChest`: a minecart where the spot is air over a block that
    /// isn't, drawing the rail's shape and then its loot seed.
    fn minecart(&mut self, piece: &Piece, local: [i32; 3]) {
        let pos = piece.world(local);
        if !piece.chunk.contains(pos) {
            return;
        }
        let here = self.read(piece, pos);
        if !self.is_air(here) {
            return;
        }
        let below = self.read(piece, [pos[0], pos[1] - 1, pos[2]]);
        if self.is_air(below) {
            return;
        }
        self.random.next_boolean();
        let rail = self.shaft_blocks.rail;
        self.place_block(piece, rail, local);
        let seed = self.random.next_long();
        self.out.push(StructureContainer {
            structure: self.structure,
            pos,
            block: MINECART.into(),
            table: LOOT_TABLE.into(),
            seed: Some(seed),
            doubt: self.doubt.clone(),
        });
    }

    /// `placeDoubleLowerOrUpperSupport`: a pillar down, or a chain up, under each of the two
    /// floor blocks at `x` and `x + 2` that are planks.
    fn double_support(&mut self, piece: &Piece, [x, y, z]: [i32; 3]) {
        let Materials { wood, planks, .. } = self.materials();
        let id = |s: State| self.blocks.palette().block_id(s).to_string();
        for x in [x, x + 2] {
            let here = self.get_block(piece, [x, y, z]);
            if id(here) == id(planks) {
                self.pillar_or_chain(piece, wood, [x, y, z]);
            }
        }
    }

    /// `isReplaceableByStructures`.
    fn replaceable_by_structures(&self, s: State) -> bool {
        let b = self.blocks.behaviour(s);
        b.air
            || b.liquid
            || matches!(
                self.blocks.palette().block_id(s),
                "minecraft:glow_lichen" | "minecraft:seagrass" | "minecraft:tall_seagrass"
            )
    }

    /// `fillPillarDownOrChainUp`: looking down and up a block further each round, wood down to
    /// the first block it can stand on (within 20), or a fence and chain up to the first it
    /// can hang from (within 50), whichever comes first.
    fn pillar_or_chain(&mut self, piece: &Piece, wood: State, local: [i32; 3]) {
        let start = piece.world(local);
        if !piece.chunk.contains(start) {
            return;
        }
        let [x, y0, z] = start;
        let (mut down, mut up) = (true, true);
        let mut d = 1;
        while down || up {
            if down {
                let y = y0 - d;
                let s = self.read(piece, [x, y, z]);
                let lava = self.blocks.palette().block_id(s) == "minecraft:lava";
                let replaceable = self.replaceable_by_structures(s) && !lava;
                if !replaceable && self.blocks.behaviour(s).sturdy(Face::Up) {
                    for y in y0 - d + 1..y0 {
                        self.set([x, y, z], wood);
                    }
                    return;
                }
                down = d <= 20 && replaceable && y > self.region.min_y() + 1;
            }
            if up {
                let y = y0 + d;
                let s = self.read(piece, [x, y, z]);
                let replaceable = self.replaceable_by_structures(s);
                let b = self.blocks.behaviour(s);
                if !replaceable && b.center_down && !b.falling {
                    let fence = self.materials().fence;
                    self.set([x, y0 + 1, z], fence);
                    let chain = self.shaft_blocks.iron_chain;
                    for y in y0 + 2..y0 + d {
                        self.set([x, y, z], chain);
                    }
                    return;
                }
                up = d <= 50 && replaceable && y < self.region.max_y();
            }
            d += 1;
        }
    }
}
