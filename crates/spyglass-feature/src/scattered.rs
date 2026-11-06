//! The loot containers of the single-piece structures spyglass places
//! (`docs/specs/loot.md#containers`). What each draws from the structure's random before its
//! containers doesn't depend on the world: the treasure's chest is the first draw, a pyramid's
//! four chests the second to fifth, a temple's dispensers, chests and stone selector a fixed
//! count, a template piece one draw per container it places and then one per marker that fills
//! one. Mostly only where they are does: the heights below them, the blocks a treasure's scan
//! reads; and for a ruined portal, whether its chest is placed at all.

use spyglass_rng::{RandomSource, WorldgenRandom};
use spyglass_structure::{
    BoundingBox, ScatteredPiece, Starts, Structure, StructureType, Template, TemplatePiece,
    template_pieces,
};

use crate::blocks::{Blocks, Heightmap, Tag};
use crate::containers::{ContainerDoubt, Reads, StructureContainer};
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
    )
}

/// How many chunks from its start a start of `kind` can place a container: the scattered
/// pieces and buried treasure only in their start chunk, template pieces turned about their
/// pivot up to two away.
pub(crate) fn reach(kind: StructureType) -> i32 {
    match kind {
        StructureType::Igloo | StructureType::Shipwreck | StructureType::RuinedPortal => 2,
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
    start: &Start<'_>,
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
        if chunk.contains(world) && start.blocks.loot_container(&block.block) == Some(true) {
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
    template_piece(start, placing, random, &piece, [x0, y, z0], marker, out);
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
        template_piece(start, placing, random, &piece, pos, marker, out);
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
