//! The loot containers of the single-piece structures spyglass places
//! (`docs/specs/loot.md#containers`). What each draws from the structure's random before its
//! containers doesn't depend on the world: the treasure's chest is the first draw, a pyramid's
//! four chests the second to fifth, a temple's dispensers, chests and stone selector a fixed
//! count, a template piece one draw per container it places and then one per marker that fills
//! one. Mostly only where they are does: the heights below them, the blocks a treasure's scan
//! reads; and for a ruined portal, whether its chest is placed at all. End cities' template
//! pieces draw the same way, and nothing about them depends on the world; an ocean ruin's chests
//! draw only their seeds, and only their heights do.

use std::collections::HashMap;

use spyglass_blocks::behaviour::FluidKind;
use spyglass_rng::{LegacyRandomSource, RandomSource, WorldgenRandom, block_seed};
use spyglass_structure::{
    BoundingBox, CityPiece, Facing, FortressKind, FortressPiece, MansionPiece, ScatteredPiece,
    Starts, StrongholdKind, StrongholdPiece, Structure, StructureType, Template, TemplatePiece,
    template_pieces, world_pos,
};

use crate::blocks::{Blocks, Heightmap, Tag};
use crate::containers::{ContainerDoubt, Reads, StructureContainer};
use crate::place::template::Template as PlaceTemplate;
use crate::region::Region;

/// What a container's position read, and the first reason it might not be what spyglass read.
struct Placing<'a, 'r, 'b> {
    region: &'a mut Region<'r>,
    reads: &'b Reads<'b>,
    structure: usize,
    doubt: Option<ContainerDoubt>,
}

impl Placing<'_, '_, '_> {
    /// `getBlockState`.
    fn get(&mut self, pos: [i32; 3]) -> spyglass_blocks::State {
        if self.doubt.is_none()
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

    /// `getHeight(heightmap, x, z)`, as earlier steps left it: in doubt where something
    /// placed before could have written at or above the top, or the top block is tied.
    fn height(&mut self, heightmap: Heightmap, x: i32, z: i32) -> i32 {
        self.region.take_doubt();
        let y = self.region.height(heightmap, x, z);
        if let Some(why) = self.region.take_doubt() {
            self.doubt.get_or_insert(ContainerDoubt::Read(why));
        }
        if self.doubt.is_none() {
            let top = [x, self.region.max_y(), z];
            if let Some(why) = (self.reads)([x, y - 1, z], top, self.structure) {
                self.doubt = Some(ContainerDoubt::Read(why));
            }
        }
        // The top block and the one above: a tied one could change which counts.
        self.get([x, y - 1, z]);
        self.get([x, y, z]);
        y
    }

    /// `getHeight(heightmap, x, z)` for a `_WG` heightmap, which nothing changes after terrain.
    fn terrain_height(&mut self, heightmap: Heightmap, x: i32, z: i32) -> i32 {
        self.region.take_doubt();
        let y = self.region.height(heightmap, x, z);
        if let Some(why) = self.region.take_doubt() {
            self.doubt.get_or_insert(ContainerDoubt::Read(why));
        }
        y
    }

    fn container(&self, pos: [i32; 3], block: &str, table: &str, seed: i64) -> StructureContainer {
        StructureContainer {
            structure: self.structure,
            pos,
            block: block.into(),
            table: table.into(),
            seed: Some(seed),
            doubt: self.doubt.clone(),
        }
    }
}

/// Whether spyglass places `kind`'s containers here.
pub(crate) fn places(kind: StructureType) -> bool {
    matches!(
        kind,
        StructureType::BuriedTreasure
            | StructureType::DesertPyramid
            | StructureType::JungleTemple
            | StructureType::Igloo
            | StructureType::Shipwreck
            | StructureType::RuinedPortal
            | StructureType::OceanRuin
    )
}

/// How many chunks from its start a start of `kind` can place a container: the scattered
/// pieces and buried treasure only in their start chunk, template pieces turned about their
/// pivot up to two away, an ocean ruin's cluster three.
pub(crate) fn reach(kind: StructureType) -> i32 {
    match kind {
        StructureType::Igloo | StructureType::Shipwreck | StructureType::RuinedPortal => 2,
        StructureType::OceanRuin => 3,
        _ => 0,
    }
}

/// A start, and what placing its containers needs.
pub(crate) struct Start<'a> {
    /// The structure.
    pub info: &'a Structure,
    /// Its index in its `StructureSets`.
    pub structure: usize,
    /// Its start chunk.
    pub chunk: [i32; 2],
    /// The seed's starts, for a ruined portal's layout.
    pub starts: &'a Starts<'a>,
    /// For which blocks are containers.
    pub blocks: &'a Blocks,
    /// `#features_cannot_replace`, which a ruined portal's chest doesn't replace.
    pub cannot_replace: Tag,
    /// `#ice`, which an ocean ruin's height scan passes.
    pub ice: Tag,
    /// Each ocean ruin template's blocks, by id.
    pub ruins: &'a HashMap<String, PlaceTemplate>,
}

/// The containers `start` places in the region's chunk, drawing from `random`, the structure's
/// random for the chunk, appended to `out`. `seed` is the world's.
pub(crate) fn containers(
    start: &Start<'_>,
    region: &mut Region<'_>,
    seed: i64,
    random: &mut WorldgenRandom,
    reads: &Reads<'_>,
    out: &mut Vec<StructureContainer>,
) {
    let mut placing = Placing {
        region,
        reads,
        structure: start.structure,
        doubt: None,
    };
    let in_start_chunk = placing.region.center() == start.chunk;
    match start.info.kind {
        StructureType::BuriedTreasure if in_start_chunk => {
            buried_treasure(&mut placing, random, out);
        }
        StructureType::DesertPyramid if in_start_chunk => {
            desert_pyramid(&mut placing, seed, random, out);
        }
        StructureType::JungleTemple if in_start_chunk => {
            jungle_temple(&mut placing, seed, random, out);
        }
        StructureType::Shipwreck => shipwreck(start, &mut placing, seed, random, out),
        StructureType::Igloo => igloo(start, &mut placing, seed, random, out),
        StructureType::RuinedPortal => ruined_portal(start, &mut placing, random, out),
        StructureType::OceanRuin => ocean_ruin(start, &mut placing, random, out),
        _ => {}
    }
}

/// `BuriedTreasurePiece.postProcess`: from the ocean floor at the chunk's (9, 9) down to the
/// first block over sandstone or stone-like rock, where the chest goes; none if there's no
/// such floor above the bottom.
fn buried_treasure(
    placing: &mut Placing<'_, '_, '_>,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let [cx, cz] = placing.region.center();
    let (x, z) = (cx.wrapping_shl(4) + 9, cz.wrapping_shl(4) + 9);
    // OCEAN_FLOOR_WG never changes after terrain.
    let mut y = placing.region.height(Heightmap::OceanFloorWg, x, z);
    while y > placing.region.min_y() {
        let below = placing.get([x, y - 1, z]);
        let below = placing.region.blocks().palette().block_id(below);
        if matches!(
            below,
            "minecraft:sandstone"
                | "minecraft:stone"
                | "minecraft:andesite"
                | "minecraft:granite"
                | "minecraft:diorite"
        ) {
            let seed = random.next_long();
            out.push(placing.container(
                [x, y, z],
                "minecraft:chest",
                "minecraft:chests/buried_treasure",
                seed,
            ));
            return;
        }
        y -= 1;
    }
}

/// `DesertPyramidPiece.postProcess` in its start chunk: `nextInt(3)` lowers the pyramid below
/// the lowest ground under it, then its four chests, north, east, south and west of the
/// treasure room's centre, draw their seeds.
fn desert_pyramid(
    placing: &mut Placing<'_, '_, '_>,
    seed: i64,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let piece = ScatteredPiece::new(seed, placing.region.center(), [21, 15, 21]);
    let sink = random.next_int_bounded(3);
    // updateHeightPositionToLowestGroundHeight, over the whole footprint: the chunk decorated
    // first settles it, and on its own that's this one.
    let mut lowest = placing.region.max_y() + 1;
    for z in piece.bounds.min[2]..=piece.bounds.max[2] {
        for x in piece.bounds.min[0]..=piece.bounds.max[0] {
            lowest = lowest.min(placing.height(Heightmap::MotionBlockingNoLeaves, x, z));
        }
    }
    let min_y = lowest - sink;
    for [x, z] in [[10, 8], [12, 10], [10, 12], [8, 10]] {
        let pos = piece.world([x, -11, z], min_y);
        let seed = random.next_long();
        out.push(placing.container(
            pos,
            "minecraft:chest",
            "minecraft:chests/desert_pyramid",
            seed,
        ));
    }
}

/// `JungleTemplePiece.postProcess`: the stone selector's 1,511 draws, the two dispensers and
/// the main chest, 11 more, then the hidden chest. The piece is inside its start chunk, at the
/// average ground height under it.
fn jungle_temple(
    placing: &mut Placing<'_, '_, '_>,
    seed: i64,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    /// `MossStoneSelector`'s draws before the dispensers: one `nextFloat()` per cell of each
    /// box it fills, whatever the world holds there (`generateBox` with `skipAir` false).
    const BEFORE_DISPENSERS: usize = 1511;
    /// Its draws between the main and hidden chests: the boxes (9, −1, 1)–(9, −1, 5),
    /// (8, −3, 8)–(8, −3, 10) and (10, −3, 8)–(10, −3, 10).
    const BEFORE_HIDDEN_CHEST: usize = 11;
    let piece = ScatteredPiece::new(seed, placing.region.center(), [12, 10, 15]);
    // updateAverageGroundHeight with no offset.
    let (mut sum, mut columns) = (0i32, 0i32);
    for z in piece.bounds.min[2]..=piece.bounds.max[2] {
        for x in piece.bounds.min[0]..=piece.bounds.max[0] {
            sum += placing.height(Heightmap::MotionBlockingNoLeaves, x, z);
            columns += 1;
        }
    }
    let min_y = sum / columns;
    for _ in 0..BEFORE_DISPENSERS {
        random.next_float();
    }
    let dispenser = |placing: &Placing<'_, '_, '_>, at, seed| {
        placing.container(
            piece.world(at, min_y),
            "minecraft:dispenser",
            "minecraft:chests/jungle_temple_dispenser",
            seed,
        )
    };
    let chest = |placing: &Placing<'_, '_, '_>, at, seed| {
        placing.container(
            piece.world(at, min_y),
            "minecraft:chest",
            "minecraft:chests/jungle_temple",
            seed,
        )
    };
    let seed = random.next_long();
    out.push(dispenser(placing, [3, -2, 1], seed));
    let seed = random.next_long();
    out.push(dispenser(placing, [9, -2, 3], seed));
    let seed = random.next_long();
    out.push(chest(placing, [8, -3, 3], seed));
    for _ in 0..BEFORE_HIDDEN_CHEST {
        random.next_float();
    }
    let seed = random.next_long();
    out.push(chest(placing, [9, -3, 10], seed));
}

/// `ChunkGenerator.getWritableArea`: the chunk's columns, from a block above the bottom.
fn writable(region: &Region<'_>) -> BoundingBox {
    let [cx, cz] = region.center();
    BoundingBox {
        min: [cx.wrapping_shl(4), region.min_y() + 1, cz.wrapping_shl(4)],
        max: [
            cx.wrapping_shl(4) + 15,
            region.max_y(),
            cz.wrapping_shl(4) + 15,
        ],
    }
}

/// `TemplateStructurePiece.postProcess` for a piece with no processor that changes a
/// container, its corner at `pos`: one draw for each block with NBT inside the chunk that's a
/// loot container (`placeInWorld`), then for each data marker inside it that `marker` gives a
/// table for and that sits on one of them, a draw for its seed, the container's.
fn template_piece(
    blocks: &Blocks,
    placing: &mut Placing<'_, '_, '_>,
    random: &mut WorldgenRandom,
    piece: &TemplatePiece,
    pos: [i32; 3],
    marker: impl Fn(&str) -> Option<&'static str>,
    out: &mut Vec<StructureContainer>,
) {
    let template: &Template = &piece.template;
    let Some(palette) = template.palette_at(pos) else {
        return;
    };
    if template.size.contains(&0) {
        return;
    }
    let chunk = writable(placing.region);
    let mut placed = Vec::new();
    for block in template.nbt_blocks(palette) {
        let world = piece.world(pos, block.pos);
        if chunk.contains(world) && blocks.loot_container(&block.block) == Some(true) {
            random.next_long();
            placed.push((block.pos, &block.block));
        }
    }
    for m in template.markers(palette) {
        let Some(table) = marker(&m.name) else {
            continue;
        };
        if !chunk.contains(piece.world(pos, m.pos)) {
            continue;
        }
        let below = [m.pos[0], m.pos[1] - 1, m.pos[2]];
        let Some((_, block)) = placed.iter().find(|(p, _)| *p == below) else {
            continue;
        };
        let seed = random.next_long();
        out.push(placing.container(piece.world(pos, below), block, table, seed));
    }
}

/// `ShipwreckPiece.postProcess`: the wreck at the mean ocean floor under its unturned
/// footprint, or a beached one below the lowest surface under it by half its height and
/// `nextInt(3)`; then its template, whose chests the `map_chest`, `supply_chest` and
/// `treasure_chest` markers above them fill. The chunk that places it first settles the
/// height, and on its own that's this one.
fn shipwreck(
    start: &Start<'_>,
    placing: &mut Placing<'_, '_, '_>,
    seed: i64,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let Ok([piece]) =
        <[TemplatePiece; 1]>::try_from(template_pieces(start.info, seed, start.chunk))
    else {
        return;
    };
    let size = piece.template.size;
    // The piece's box, the turned template's, meets the chunk.
    let corners = [
        piece.world(piece.pos, [0, 0, 0]),
        piece.world(piece.pos, [size[0] - 1, 0, size[2] - 1]),
    ];
    let chunk = writable(placing.region);
    let reaches = |i: usize| {
        corners[0][i].min(corners[1][i]) <= chunk.max[i]
            && chunk.min[i] <= corners[0][i].max(corners[1][i])
    };
    // isTooBigToFitInWorldGenRegion settles the height at layout instead; no vanilla wreck is.
    if !reaches(0) || !reaches(2) || size[0] > 32 || size[1] > 32 {
        return;
    }
    let heightmap = if start.info.beached {
        Heightmap::WorldSurfaceWg
    } else {
        Heightmap::OceanFloorWg
    };
    let [x0, _, z0] = piece.pos;
    let mut lowest = placing.region.max_y() + 1;
    let mut sum = 0i32;
    for x in x0..x0 + size[0] {
        for z in z0..z0 + size[2] {
            let h = placing.terrain_height(heightmap, x, z);
            sum += h;
            lowest = lowest.min(h);
        }
    }
    let y = if start.info.beached {
        lowest - size[1] / 2 - random.next_int_bounded(3)
    } else {
        sum / (size[0] * size[2])
    };
    let marker = |name: &str| match name {
        "map_chest" => Some("minecraft:chests/shipwreck_map"),
        "supply_chest" => Some("minecraft:chests/shipwreck_supply"),
        "treasure_chest" => Some("minecraft:chests/shipwreck_treasure"),
        _ => None,
    };
    template_piece(
        start.blocks,
        placing,
        random,
        &piece,
        [x0, y, z0],
        marker,
        out,
    );
}

/// `IglooPiece.postProcess` for each piece: its template moved to the surface at a point by
/// the igloo's door, then placed; the laboratory's `chest` marker fills the chest below it.
fn igloo(
    start: &Start<'_>,
    placing: &mut Placing<'_, '_, '_>,
    seed: i64,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    for piece in template_pieces(start.info, seed, start.chunk) {
        let [ox, _, oz] = piece.offset;
        let probe = piece.world(piece.pos, [3 - ox, 0, -oz]);
        let h = placing.terrain_height(Heightmap::WorldSurfaceWg, probe[0], probe[2]);
        let pos = [piece.pos[0], piece.pos[1] + h - 90 - 1, piece.pos[2]];
        let marker = |name: &str| (name == "chest").then_some("minecraft:chests/igloo_chest");
        template_piece(start.blocks, placing, random, &piece, pos, marker, out);
    }
}

/// `EndCityPiece.postProcess` for each of an end city's pieces, in order: its template, whose
/// chests the `Chest` markers above them fill. Sentries and the ship's elytra don't draw.
pub(crate) fn end_city(
    pieces: &[CityPiece],
    blocks: &Blocks,
    structure: usize,
    region: &mut Region<'_>,
    random: &mut WorldgenRandom,
    reads: &Reads<'_>,
    out: &mut Vec<StructureContainer>,
) {
    let mut placing = Placing {
        region,
        reads,
        structure,
        doubt: None,
    };
    let marker = |name: &str| {
        name.starts_with("Chest")
            .then_some("minecraft:chests/end_city_treasure")
    };
    for p in pieces {
        let piece = TemplatePiece {
            template: p.template.clone(),
            pos: p.pos,
            rotation: p.rotation,
            pivot: [0; 3],
            offset: [0; 3],
        };
        template_piece(blocks, &mut placing, random, &piece, p.pos, marker, out);
    }
}

/// The chests of a fortress's small corridor turns that decided on one as they were laid out,
/// in piece order: at (1, 2, 3) in a right turn's own coordinates, (3, 2, 3) in a left turn's,
/// where inside the chunk. Nothing else in a fortress draws from the structure random.
pub(crate) fn fortress(
    pieces: &[FortressPiece],
    structure: usize,
    region: &mut Region<'_>,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let chunk = writable(region);
    for piece in pieces.iter().filter(|p| p.chest) {
        let local = match piece.kind {
            FortressKind::CastleSmallCorridorRightTurn => [1, 2, 3],
            FortressKind::CastleSmallCorridorLeftTurn => [3, 2, 3],
            _ => continue,
        };
        let pos = world_pos(&piece.bounds, piece.facing, local);
        if !chunk.contains(pos) {
            continue;
        }
        let seed = random.next_long();
        out.push(StructureContainer {
            structure,
            pos,
            block: "minecraft:chest".into(),
            table: "minecraft:chests/nether_bridge".into(),
            seed: Some(seed),
            doubt: None,
        });
    }
}

/// The chests of a stronghold's pieces, in piece order. Every piece but the portal room first
/// fills its box with stone bricks from the smooth-stone selector, skipping air, which draws a
/// `nextFloat()` for each edge block inside the chunk that isn't air: so the draws before a chest
/// depend on the blocks there before the stronghold, and where those are in doubt, its seed isn't
/// known. The rest of each piece's draws are fixed.
pub(crate) fn stronghold(
    pieces: &[StrongholdPiece],
    structure: usize,
    region: &mut Region<'_>,
    random: &mut WorldgenRandom,
    reads: &Reads<'_>,
    out: &mut Vec<StructureContainer>,
) {
    let chunk = writable(region);
    // Why the draws so far might not be the game's.
    let mut unknown: Option<ContainerDoubt> = None;
    for piece in pieces.iter().filter(|p| p.bounds.intersects(&chunk)) {
        // Its size across, up and along, in its own coordinates.
        let span = |i: usize| piece.bounds.max[i] - piece.bounds.min[i] + 1;
        let size = match piece.facing {
            Facing::North | Facing::South => [span(0), span(1), span(2)],
            Facing::East | Facing::West => [span(2), span(1), span(0)],
        };
        // The shell, then the draws that don't read the world, then the chests at their own
        // coordinates, with their tables.
        let (shell, fixed, chests): (bool, i32, &[([i32; 3], &str)]) = match piece.kind {
            StrongholdKind::Straight { .. } => (true, 4, &[]),
            StrongholdKind::PrisonHall => (true, 12, &[]),
            StrongholdKind::FiveCrossing { .. } => (true, 109, &[]),
            StrongholdKind::ChestCorridor => (
                true,
                0,
                &[([3, 2, 3], "minecraft:chests/stronghold_corridor")],
            ),
            StrongholdKind::RoomCrossing { room: 2 } => (
                true,
                0,
                &[([3, 4, 8], "minecraft:chests/stronghold_crossing")],
            ),
            StrongholdKind::Library { tall: false } => (
                true,
                520,
                &[([3, 3, 5], "minecraft:chests/stronghold_library")],
            ),
            StrongholdKind::Library { tall: true } => (
                true,
                520,
                &[
                    ([3, 3, 5], "minecraft:chests/stronghold_library"),
                    ([12, 8, 1], "minecraft:chests/stronghold_library"),
                ],
            ),
            StrongholdKind::PortalRoom => (false, 772, &[]),
            StrongholdKind::FillerCorridor => (false, 0, &[]),
            _ => (true, 0, &[]),
        };
        if shell {
            // Only the box inside the chunk reads or draws.
            let inside = BoundingBox {
                min: [0, 1, 2].map(|i| piece.bounds.min[i].max(chunk.min[i])),
                max: [0, 1, 2].map(|i| piece.bounds.max[i].min(chunk.max[i])),
            };
            if inside.min.iter().zip(&inside.max).all(|(lo, hi)| lo <= hi) {
                if unknown.is_none()
                    && let Some(why) = reads(inside.min, inside.max, structure)
                {
                    unknown = Some(ContainerDoubt::Read(why));
                }
                region.take_doubt();
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        for z in 0..size[2] {
                            let edge = y == 0
                                || y == size[1] - 1
                                || x == 0
                                || x == size[0] - 1
                                || z == 0
                                || z == size[2] - 1;
                            let pos = world_pos(&piece.bounds, piece.facing, [x, y, z]);
                            if !edge || !chunk.contains(pos) {
                                continue;
                            }
                            let here = region.get(pos);
                            if !region.blocks().is_air(here) {
                                random.next_float();
                            }
                        }
                    }
                }
                if let Some(why) = region.take_doubt() {
                    unknown.get_or_insert(ContainerDoubt::Read(why));
                }
            }
        }
        for _ in 0..fixed {
            random.next_float();
        }
        for &(local, table) in chests {
            let pos = world_pos(&piece.bounds, piece.facing, local);
            if !chunk.contains(pos) {
                continue;
            }
            let seed = random.next_long();
            out.push(StructureContainer {
                structure,
                pos,
                block: "minecraft:chest".into(),
                table: table.into(),
                seed: unknown.is_none().then_some(seed),
                doubt: unknown.clone(),
            });
        }
    }
}

/// `WoodlandMansionPiece.postProcess` for each of a mansion's pieces, in order: its template's
/// loot containers inside the chunk draw, then each `Chest` marker inside it places a chest of
/// `chests/woodland_mansion` there, at the marker itself, drawing its seed. The mobs' markers
/// draw from the region's random, not the structure's.
pub(crate) fn mansion(
    pieces: &[MansionPiece],
    blocks: &Blocks,
    structure: usize,
    region: &mut Region<'_>,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let chunk = writable(region);
    for piece in pieces.iter().filter(|p| p.bounds().intersects(&chunk)) {
        let template = &piece.template;
        let Some(palette) = template.palette_at(piece.pos) else {
            continue;
        };
        for block in template.nbt_blocks(palette) {
            if chunk.contains(piece.world(block.pos))
                && blocks.loot_container(&block.block) == Some(true)
            {
                random.next_long();
            }
        }
        for m in template.markers(palette) {
            let pos = piece.world(m.pos);
            if !m.name.starts_with("Chest") || !chunk.contains(pos) {
                continue;
            }
            let seed = random.next_long();
            out.push(StructureContainer {
                structure,
                pos,
                block: "minecraft:chest".into(),
                table: "minecraft:chests/woodland_mansion".into(),
                seed: Some(seed),
                doubt: None,
            });
        }
    }
}

/// What one of an ocean ruin's pieces wrote at a block, as a later piece's height scan reads
/// it.
#[derive(Clone, Copy)]
struct Written {
    /// Whether the scan passes it: air, water or ice.
    passes: bool,
    /// Whether it holds water, which waterlogs a block placed there after.
    water: bool,
    /// The piece that wrote it.
    piece: usize,
}

/// Whether the block at `pos` holds water, with what a ruin's pieces wrote.
fn water_at(
    written: &HashMap<[i32; 3], Written>,
    placing: &mut Placing<'_, '_, '_>,
    pos: [i32; 3],
) -> bool {
    match written.get(&pos) {
        Some(w) => w.water,
        None => {
            let s = placing.get(pos);
            placing.region.blocks().fluid_is(s, FluidKind::Water)
        }
    }
}

/// `OceanRuinPiece.postProcess` for each piece meeting the chunk, in order: its height settled
/// on the ocean floor there (`getHeight`), its template's blocks, then each `chest` marker
/// inside the chunk places a chest there, at the marker, drawing its seed. The seeds don't
/// depend on the world; a chest's height does. A later piece's scan reads what earlier pieces
/// of the start put in the chunk, so their blocks are followed: each kept by its integrity
/// (`BlockRotProcessor`, a random from its position), air and structure blocks dropped,
/// waterlogged where it lands in water. A chest placed over another replaces it.
fn ocean_ruin(
    start: &Start<'_>,
    placing: &mut Placing<'_, '_, '_>,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let Some(pieces) = start.starts.ocean_ruin_pieces(start.structure, start.chunk) else {
        return;
    };
    let chunk = writable(placing.region);
    let first = out.len();
    let mut written: HashMap<[i32; 3], Written> = HashMap::new();
    // Each piece's doubt about its height, which what it wrote inherits.
    let mut doubts: Vec<Option<ContainerDoubt>> = vec![None; pieces.len()];
    for (index, piece) in pieces.iter().enumerate() {
        if !piece.bounds().intersects(&chunk) {
            continue;
        }
        placing.doubt = None;
        let size = piece.template.size;
        let [x0, _, z0] = piece.pos;
        let h = placing.terrain_height(Heightmap::OceanFloorWg, x0, z0);
        let far = piece.rotation.transform([size[0] - 1, 0, size[2] - 1]);
        let corner = [x0 + far[0], h, z0 + far[2]];
        // getHeight: down each column of the footprint through air, water and ice.
        let min_y = placing.region.min_y();
        let top = h - 1;
        let (mut lowest, mut deep) = (512, 0);
        for x in x0.min(corner[0])..=x0.max(corner[0]) {
            for z in z0.min(corner[2])..=z0.max(corner[2]) {
                let mut y = h - 1;
                loop {
                    let passes = match written.get(&[x, y, z]) {
                        Some(w) => {
                            if let Some(why) = &doubts[w.piece] {
                                placing.doubt.get_or_insert(why.clone());
                            }
                            w.passes
                        }
                        None => {
                            let s = placing.get([x, y, z]);
                            let blocks = placing.region.blocks();
                            blocks.is_air(s)
                                || blocks.fluid_is(s, FluidKind::Water)
                                || blocks.is(s, start.ice)
                        }
                    };
                    if !passes || y <= min_y + 1 {
                        break;
                    }
                    y -= 1;
                }
                lowest = lowest.min(y);
                if y < top - 2 {
                    deep += 1;
                }
            }
        }
        let span = (x0 - corner[0]).abs();
        let y = if top - lowest > 2 && deep > span - 2 {
            lowest + 1
        } else {
            h
        };
        doubts[index] = placing.doubt.clone();
        let at = |local: [i32; 3]| {
            let p = piece.rotation.transform(local);
            [x0 + p[0], y + p[1], z0 + p[2]]
        };
        if let Some(blocks) = start
            .ruins
            .get(&piece.template.id)
            .and_then(|t| t.palettes.first())
        {
            for block in blocks {
                let pos = at(block.pos);
                if !chunk.contains(pos) {
                    continue;
                }
                let state = block.states[0];
                let b = placing.region.blocks();
                if b.is_air(state) || b.palette().block_id(state) == "minecraft:structure_block" {
                    continue;
                }
                let mut integrity = LegacyRandomSource::new(block_seed(pos[0], pos[1], pos[2]));
                if integrity.next_float() > piece.integrity {
                    continue;
                }
                let water = b.fluid_is(state, FluidKind::Water)
                    || (b.waterloggable(state) && water_at(&written, placing, pos));
                let b = placing.region.blocks();
                let passes = water || b.is_air(state) || b.is(state, start.ice);
                written.insert(
                    pos,
                    Written {
                        passes,
                        water,
                        piece: index,
                    },
                );
                // Overwritten, a chest an earlier piece placed here is gone.
                let mut i = first;
                while i < out.len() {
                    if out[i].pos == pos {
                        out.remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
        }
        let Some(palette) = piece.template.palette_at(piece.pos) else {
            continue;
        };
        let table = if piece.large {
            "minecraft:chests/underwater_ruin_big"
        } else {
            "minecraft:chests/underwater_ruin_small"
        };
        for m in piece.template.markers(palette) {
            let pos = at(m.pos);
            if !chunk.contains(pos) {
                continue;
            }
            match m.name.as_str() {
                "chest" => {
                    let water = water_at(&written, placing, pos);
                    written.insert(
                        pos,
                        Written {
                            passes: water,
                            water,
                            piece: index,
                        },
                    );
                    let seed = random.next_long();
                    out.retain(|c| c.pos != pos || c.structure != start.structure);
                    out.push(StructureContainer {
                        structure: start.structure,
                        pos,
                        block: "minecraft:chest".into(),
                        table: table.into(),
                        seed: Some(seed),
                        doubt: doubts[index].clone(),
                    });
                }
                // Water below the sea, air above: the scan passes either.
                "drowned" => {
                    written.insert(
                        pos,
                        Written {
                            passes: true,
                            water: true,
                            piece: index,
                        },
                    );
                }
                _ => {}
            }
        }
    }
}

/// `RuinedPortalPiece.postProcess`: only the chunk holding the template's centre places it,
/// and whole. Its chest draws its seed unless the block already there is one it can't replace
/// (`protected_blocks`) or lava, which takes a block that isn't full
/// (`LavaSubmergedBlockProcessor`).
fn ruined_portal(
    start: &Start<'_>,
    placing: &mut Placing<'_, '_, '_>,
    random: &mut WorldgenRandom,
    out: &mut Vec<StructureContainer>,
) {
    let Some(layout) = start.starts.ruined_portal(start.structure, start.chunk) else {
        return;
    };
    let BoundingBox { min, max } = layout.bounds;
    let centre = [0, 1, 2].map(|i| min[i] + (max[i] - min[i] + 1) / 2);
    if !writable(placing.region).contains(centre) {
        return;
    }
    let Some(palette) = layout.template.palette_at(layout.pos) else {
        return;
    };
    for block in layout.template.nbt_blocks(palette) {
        if start.blocks.loot_container(&block.block) != Some(true) {
            continue;
        }
        let p = layout
            .rotation
            .transform_about(block.pos, layout.front_back, layout.pivot);
        let world = [0, 1, 2].map(|i| layout.pos[i].wrapping_add(p[i]));
        let here = placing.get(world);
        let blocks = placing.region.blocks();
        if blocks.is(here, start.cannot_replace)
            || blocks.palette().block_id(here) == "minecraft:lava"
        {
            continue;
        }
        let seed = random.next_long();
        if let Some(table) = &block.loot_table {
            out.push(placing.container(world, &block.block, table, seed));
        }
    }
}
