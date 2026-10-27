//! The biome check (`Structure.findValidGenerationPoint`) and the choice among a set's
//! structures (`ChunkGenerator.createStructures`): from the climate alone, or with the
//! generator's terrain, which gives most types their real check position. See
//! `docs/specs/structure-placement.md`.

use spyglass_biome::{BiomeId, Biomes, Lookup};
use spyglass_rng::{RandomSource, worldgen};
use spyglass_terrain::{Block, Heightmap, Terrain};

use crate::assembly::{Assembly, JigsawPools};
use crate::biomes::BiomeSet;
use crate::jigsaw::{Jigsaw, JigsawCheck};
use crate::load::{Structure, StructureType};
use crate::placement::Placements;

/// What spyglass can say about one structure's biome check in one chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The game rejects it here, whatever the terrain.
    Rejected,
    /// The game accepts it here: the check's position doesn't depend on terrain, and its biome
    /// is in the structure's set.
    Passes,
    /// Undecided: the check's position needs terrain (or templates spyglass doesn't read yet),
    /// or a tied biome cell could go either way. Every climate-only condition the game tests
    /// first has passed.
    Candidate,
}

/// The outcome of the game's choice among a set's structures in one placement chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Structures the game may start here, in the order it tries them, with each one's check.
    /// The game starts the first that passes. Every entry but the last is a
    /// [`Check::Candidate`]; the last may be [`Check::Passes`], in which case a start is
    /// certain. Empty: no structure of the set starts here.
    pub tries: Vec<(usize, Check)>,
}

impl Selection {
    /// The structure that certainly starts here, if spyglass can tell.
    pub fn definite(&self) -> Option<usize> {
        match self.tries.as_slice() {
            [(s, Check::Passes)] => Some(*s),
            _ => None,
        }
    }
}

/// Structure starts for one seed: [`Placements`] plus the biome source for the same seed, and
/// optionally its terrain.
pub struct Starts<'a> {
    placements: &'a Placements<'a>,
    biomes: &'a Biomes<'a>,
    terrain: Option<&'a Terrain>,
    pools: Option<&'a JigsawPools>,
}

/// What a check found before its biome test: the conditions' verdict so far, and the test's
/// position if spyglass can tell.
struct Found {
    /// `Rejected` if a condition fails (or terrain rejects the start); `Candidate` if one hangs
    /// on a tie; `Passes` if every condition so far certainly holds.
    before: Check,
    /// The check's position (the game's stub).
    at: Option<[i32; 3]>,
}

impl Found {
    const REJECTED: Found = Found {
        before: Check::Rejected,
        at: None,
    };

    /// Conditions so far, with the position still unknown.
    fn pending(before: Check) -> Found {
        Found { before, at: None }
    }
}

impl<'a> Starts<'a> {
    /// Starts for the placements' seed, from the climate alone; `biomes` must be for the same
    /// seed. Types whose check position needs terrain are candidates.
    pub fn new(placements: &'a Placements<'a>, biomes: &'a Biomes<'a>) -> Self {
        Starts {
            placements,
            biomes,
            terrain: None,
            pools: None,
        }
    }

    /// Starts with the seed's Overworld terrain as well, so that most types are checked at
    /// their real position, and rejected where terrain rejects them.
    pub fn with_terrain(
        placements: &'a Placements<'a>,
        biomes: &'a Biomes<'a>,
        terrain: &'a Terrain,
    ) -> Self {
        Starts {
            placements,
            biomes,
            terrain: Some(terrain),
            pools: None,
        }
    }

    /// The same starts, able to [`assemble`](Self::assemble) jigsaw structures with `pools`,
    /// loaded for the same structure sets.
    pub fn with_pools(self, pools: &'a JigsawPools) -> Self {
        Starts {
            pools: Some(pools),
            ..self
        }
    }

    /// A jigsaw structure's pieces and bounds if it starts in `chunk`
    /// (`JigsawPlacement.addPieces`), whether or not its biome check passes. Pieces that
    /// follow the terrain need it: without, the answer is [`Assembly::NeedsTerrain`]. `None` if
    /// the structure isn't a jigsaw or there are no pools.
    pub fn assemble(&self, structure: usize, chunk: [i32; 2]) -> Option<Assembly> {
        let sets = self.placements.sets();
        let jigsaw = sets.structures()[structure].jigsaw.as_ref()?;
        let terrain = self.terrain;
        let mut first_free = |x: i32, z: i32, heightmap: Heightmap| {
            terrain.map(|t| t.first_free_height(x, z, heightmap))
        };
        Some(jigsaw.assemble(
            self.placements.seed(),
            chunk,
            sets.heights(),
            self.pools?,
            &mut first_free,
        ))
    }

    /// The placements.
    pub fn placements(&self) -> &Placements<'a> {
        self.placements
    }

    /// Every placement chunk of set `set` in `min..=max` (chunks, inclusive), with the
    /// structure the game would choose there.
    pub fn starts_in(
        &self,
        set: usize,
        min: [i32; 2],
        max: [i32; 2],
    ) -> Vec<([i32; 2], Selection)> {
        self.placements
            .structure_chunks_in(set, min, max)
            .into_iter()
            .map(|chunk| (chunk, self.select(set, chunk)))
            .filter(|(_, s)| !s.tries.is_empty())
            .collect()
    }

    /// The game's choice among set `set`'s structures in placement chunk `chunk`: a single entry
    /// directly; otherwise a weighted pick from `setLargeFeatureSeed(seed, x, z)`, removing each
    /// structure that fails and picking again from the same random.
    pub fn select(&self, set: usize, chunk: [i32; 2]) -> Selection {
        let sets = self.placements.sets();
        let entries = &sets.sets()[set].entries;
        let mut tries = Vec::new();
        if let [only] = entries.as_slice() {
            let check = self.check(only.structure, chunk);
            if check != Check::Rejected {
                tries.push((only.structure, check));
            }
            return Selection { tries };
        }
        let mut list = entries.clone();
        let mut total: i32 = list.iter().map(|e| e.weight).sum();
        let mut random = worldgen::large_feature(self.placements.seed(), chunk[0], chunk[1]);
        while !list.is_empty() {
            let mut n = random.next_int_bounded(total);
            let mut i = 0;
            for entry in &list {
                n -= entry.weight;
                if n < 0 {
                    break;
                }
                i += 1;
            }
            let entry = list[i];
            match self.check(entry.structure, chunk) {
                Check::Passes => {
                    tries.push((entry.structure, Check::Passes));
                    break;
                }
                Check::Candidate => tries.push((entry.structure, Check::Candidate)),
                Check::Rejected => {}
            }
            list.remove(i);
            total -= entry.weight;
        }
        Selection { tries }
    }

    /// One structure's biome check in `chunk`, by its type's rules.
    pub fn check(&self, structure: usize, chunk: [i32; 2]) -> Check {
        let found = self.find(structure, chunk);
        let s = &self.placements.sets().structures()[structure];
        let (before, at) = match (found.before, found.at) {
            (Check::Rejected, _) => return Check::Rejected,
            // A mineshaft whose whole column is in its set, or a ruined portal whose set is every
            // biome, passes wherever its check is.
            (Check::Passes, None)
                if matches!(
                    s.kind,
                    StructureType::Mineshaft | StructureType::RuinedPortal
                ) =>
            {
                return Check::Passes;
            }
            (before, None) => return precondition(before),
            (before, Some(at)) => (before, at),
        };
        and(before, self.point(&s.biomes, at))
    }

    /// Where `structure`'s biome check in `chunk` is (`findGenerationPoint`'s stub), if
    /// spyglass can tell: always for types at a fixed position, and with terrain for most
    /// others. `None` if a condition fails before the check, or the position needs what
    /// spyglass doesn't compute.
    pub fn stub(&self, structure: usize, chunk: [i32; 2]) -> Option<[i32; 3]> {
        self.find(structure, chunk).at
    }

    /// The conditions before the biome check, and its position, by the type's rules.
    fn find(&self, structure: usize, chunk: [i32; 2]) -> Found {
        let sets = self.placements.sets();
        let s: &Structure = &sets.structures()[structure];
        let heights = sets.heights();
        let (min_x, min_z) = (chunk[0].wrapping_shl(4), chunk[1].wrapping_shl(4));
        let (mid_x, mid_z) = (min_x.wrapping_add(8), min_z.wrapping_add(8));
        let level_min_y = heights.level_min_y;
        let level_max_y = level_min_y
            .wrapping_add(heights.level_height)
            .wrapping_sub(1);
        let terrain = self.terrain;
        // couldValidBiomeExistInTerrainColumn: one block below the level, to its top.
        let terrain_column =
            |x: i32, z: i32| self.column(&s.biomes, x, z, level_min_y.wrapping_sub(1), level_max_y);
        // onTopOfChunkCenter: the chunk-centre pre-check, then the check at the centre's first
        // occupied height.
        let on_top_of_chunk_center = |heightmap: Heightmap| {
            let before = terrain_column(mid_x, mid_z);
            if before == Check::Rejected {
                return Found::REJECTED;
            }
            let at = terrain.map(|t| {
                let y = t.first_occupied_height(mid_x, mid_z, heightmap);
                [mid_x, y, mid_z]
            });
            Found { before, at }
        };
        let fixed = |at: [i32; 3]| Found {
            before: Check::Passes,
            at: Some(at),
        };
        match s.kind {
            StructureType::Stronghold => fixed([min_x, 0, min_z]),
            StructureType::Fortress => fixed([min_x, 64, min_z]),
            StructureType::BuriedTreasure | StructureType::OceanRuin => {
                on_top_of_chunk_center(Heightmap::OceanFloorWg)
            }
            StructureType::Shipwreck if s.beached => {
                on_top_of_chunk_center(Heightmap::WorldSurfaceWg)
            }
            StructureType::Shipwreck => on_top_of_chunk_center(Heightmap::OceanFloorWg),
            StructureType::Igloo | StructureType::SwampHut => {
                on_top_of_chunk_center(Heightmap::WorldSurfaceWg)
            }
            // SinglePieceStructure: the pre-check, then no start if the footprint's lowest
            // corner is below the sea level.
            StructureType::DesertPyramid | StructureType::JungleTemple => {
                let before = terrain_column(mid_x, mid_z);
                if before == Check::Rejected {
                    return Found::REJECTED;
                }
                let Some(t) = terrain else {
                    return Found::pending(before);
                };
                let (dx, dz) = if s.kind == StructureType::DesertPyramid {
                    (21, 21)
                } else {
                    (12, 15)
                };
                if lowest_corner(t, min_x, min_z, dx, dz) < sets.sea_level() {
                    return Found::REJECTED;
                }
                on_top_of_chunk_center(Heightmap::WorldSurfaceWg)
            }
            StructureType::OceanMonument => {
                let area = self.area(
                    sets.monument_surroundings(),
                    [
                        min_x.wrapping_add(9),
                        sets.sea_level(),
                        min_z.wrapping_add(9),
                    ],
                    29,
                );
                if area == Check::Rejected {
                    return Found::REJECTED;
                }
                let found = on_top_of_chunk_center(Heightmap::OceanFloorWg);
                Found {
                    before: and(area, found.before),
                    ..found
                }
            }
            // WoodlandMansionStructure and EndCityStructure: the rotation from the context
            // random, then the lowest corner of a 5 × 5 box rotated with it; below 60, no start.
            StructureType::WoodlandMansion | StructureType::EndCity => {
                let (x, z) = (min_x.wrapping_add(7), min_z.wrapping_add(7));
                let before = terrain_column(x, z);
                if before == Check::Rejected {
                    return Found::REJECTED;
                }
                let Some(t) = terrain else {
                    return Found::pending(before);
                };
                let mut random =
                    worldgen::large_feature(self.placements.seed(), chunk[0], chunk[1]);
                // Rotation.getRandom: none, clockwise 90, 180, counterclockwise 90.
                let (dx, dz) = match random.next_int_bounded(4) {
                    0 => (5, 5),
                    1 => (-5, 5),
                    2 => (-5, -5),
                    _ => (5, -5),
                };
                let y = lowest_corner(t, x, z, dx, dz);
                if y < 60 {
                    return Found::REJECTED;
                }
                Found {
                    before,
                    at: Some([x, y, z]),
                }
            }
            // The check is at (midX, 50 + dy, minZ), with dy from the piece layout, which isn't
            // ported. y is in the level, so the level's column there decides it when its quarts
            // are all in the set (it passes wherever the layout puts it) or none is.
            StructureType::Mineshaft => {
                let some = self.column(&s.biomes, mid_x, min_z, level_min_y, level_max_y);
                if some != Check::Rejected
                    && self.whole_column(&s.biomes, mid_x, min_z, level_min_y, level_max_y)
                {
                    return Found {
                        before: Check::Passes,
                        at: None,
                    };
                }
                Found::pending(precondition(some))
            }
            StructureType::Jigsaw => {
                let jigsaw = s
                    .jigsaw
                    .as_ref()
                    .expect("jigsaw structures load their settings");
                // With size 0 the start has no pieces, so it isn't valid.
                if jigsaw.size == 0 {
                    return Found::REJECTED;
                }
                let mut random =
                    worldgen::large_feature(self.placements.seed(), chunk[0], chunk[1]);
                match jigsaw.check_position(chunk, heights, &mut random) {
                    JigsawCheck::NoStart => Found::REJECTED,
                    JigsawCheck::At(_) if !self.start_corners_pass(jigsaw, chunk, None) => {
                        Found::REJECTED
                    }
                    JigsawCheck::At(pos) => fixed(pos),
                    JigsawCheck::Projected {
                        x,
                        z,
                        start_y,
                        offset_y,
                        box_y,
                        heightmap,
                    } => {
                        let before = self.column(&s.biomes, x, z, level_min_y, level_max_y);
                        if before == Check::Rejected {
                            return Found::REJECTED;
                        }
                        let (Some(t), Some(heightmap)) = (terrain, heightmap_named(&heightmap))
                        else {
                            return Found::pending(before);
                        };
                        let y = start_y.wrapping_add(t.first_free_height(x, z, heightmap));
                        if !jigsaw.fits_padding(y, box_y, heights)
                            || !self.start_corners_pass(jigsaw, chunk, Some(y))
                        {
                            return Found::REJECTED;
                        }
                        Found {
                            before,
                            at: Some([x, y.wrapping_add(offset_y), z]),
                        }
                    }
                }
            }
            // RuinedPortalStructure: no condition before the check, whose height needs terrain.
            // It always finds a position, so where its biomes are every biome the source can
            // give (the Nether's portal, in the Nether) it passes without one, and where they're
            // none (the Overworld's portals there) it fails.
            StructureType::RuinedPortal => {
                let Some(t) = terrain else {
                    let every = self.biomes.source().biome_names().names().len();
                    let everywhere = (0..every).all(|b| s.biomes.contains(BiomeId(b as u16)));
                    return if s.biomes.is_empty() {
                        Found::REJECTED
                    } else if everywhere {
                        Found {
                            before: Check::Passes,
                            at: None,
                        }
                    } else {
                        Found::pending(Check::Candidate)
                    };
                };
                let portal = s
                    .ruined_portal
                    .as_ref()
                    .expect("ruined portals load their setups");
                let mut random =
                    worldgen::large_feature(self.placements.seed(), chunk[0], chunk[1]);
                Found {
                    before: Check::Passes,
                    at: Some(portal.check_position(chunk, t, level_min_y, &mut random)),
                }
            }
            // NetherFossilStructure: x and z in the chunk, the height provider, then down the base
            // column while above the sea level, to a floor under air; none, no start. The check
            // is at the floor. Without terrain, the column there decides what it can.
            StructureType::NetherFossil => {
                let mut random =
                    worldgen::large_feature(self.placements.seed(), chunk[0], chunk[1]);
                let x = min_x.wrapping_add(random.next_int_bounded(16));
                let z = min_z.wrapping_add(random.next_int_bounded(16));
                let Some(t) = terrain else {
                    let column = self.column(&s.biomes, x, z, level_min_y, level_max_y);
                    return Found::pending(precondition(column));
                };
                let height = s.height.expect("nether fossils load their height");
                let mut y = height.sample(&mut random, heights);
                let column = t.base_column(x, z);
                let sea_level = sets.sea_level();
                while y > sea_level {
                    let top = column.at(y);
                    y = y.wrapping_sub(1);
                    // Soul sand never occurs in a base column; the default block is a full
                    // block, so its top face is sturdy, and fluids' and air's aren't.
                    if top == Block::Air && column.at(y) == Block::Default {
                        break;
                    }
                }
                if y <= sea_level {
                    return Found::REJECTED;
                }
                Found {
                    before: Check::Passes,
                    at: Some([x, y, z]),
                }
            }
        }
    }

    /// `GenerationContext.isValidBiome` for `structure` at a known check position, such as one
    /// terrain supplies: `Passes`, `Rejected`, or `Candidate` if a tied cell could go either way.
    pub fn check_at(&self, structure: usize, pos: [i32; 3]) -> Check {
        self.point(&self.placements.sets().structures()[structure].biomes, pos)
    }

    /// The biome check at a block position: `isValidBiome`.
    fn point(&self, set: &BiomeSet, [x, y, z]: [i32; 3]) -> Check {
        classify(set, &self.biomes.biome_at(x >> 2, y >> 2, z >> 2))
    }

    /// `couldStructureExistInColumn(x, z, minY, maxY)`: whether some quart from `min_y` to
    /// `max_y` at `(x, z)` has a biome in the set. `Passes` if one certainly does, `Rejected` if
    /// none can, `Candidate` if it hangs on a tie.
    /// Whether the start piece in `chunk` passes its element's corner constraint, if it has one,
    /// moved to `y`: the projected height, or with `None` its own start height
    /// (`JigsawPlacement.addPieces`). Only constrained starts draw again from the random.
    fn start_corners_pass(&self, jigsaw: &Jigsaw, chunk: [i32; 2], y: Option<i32>) -> bool {
        let seed = self.placements.seed();
        let sets = self.placements.sets();
        let mut random = worldgen::large_feature(seed, chunk[0], chunk[1]);
        let Some(start) = jigsaw.start_piece(chunk, sets.heights(), &mut random) else {
            return true;
        };
        let Some(corner) = start.element.corner_constraint() else {
            return true;
        };
        let y = y.unwrap_or(start.pos[1]);
        // The piece moves so its box's minimum y plus the ground level delta (1) is at y.
        let dy = y.wrapping_sub(start.bbox.min[1].wrapping_add(1));
        corner.test(
            &start.bbox.moved([0, dy, 0]),
            &mut sets.corner_samplers(seed),
        )
    }

    fn column(&self, set: &BiomeSet, x: i32, z: i32, min_y: i32, max_y: i32) -> Check {
        let (qx, qz) = (x >> 2, z >> 2);
        let mut result = Check::Rejected;
        for qy in (min_y >> 2)..=(max_y >> 2) {
            match classify(set, &self.biomes.biome_at(qx, qy, qz)) {
                Check::Passes => return Check::Passes,
                Check::Candidate => result = Check::Candidate,
                Check::Rejected => {}
            }
        }
        result
    }

    /// Whether every quart from `min_y` to `max_y` at `(x, z)` is certainly in the set.
    fn whole_column(&self, set: &BiomeSet, x: i32, z: i32, min_y: i32, max_y: i32) -> bool {
        let (qx, qz) = (x >> 2, z >> 2);
        ((min_y >> 2)..=(max_y >> 2))
            .all(|qy| classify(set, &self.biomes.biome_at(qx, qy, qz)) == Check::Passes)
    }

    /// `BiomeResolver.getBiomesWithin(x, y, z, radius)` all in `set`: every quart from
    /// `(p - radius) >> 2` to `(p + radius) >> 2` on each axis.
    fn area(&self, set: &BiomeSet, [x, y, z]: [i32; 3], radius: i32) -> Check {
        let range = |p: i32| (p.wrapping_sub(radius) >> 2)..=(p.wrapping_add(radius) >> 2);
        let mut result = Check::Passes;
        for qz in range(z) {
            for qx in range(x) {
                for qy in range(y) {
                    match classify(set, &self.biomes.biome_at(qx, qy, qz)) {
                        Check::Rejected => return Check::Rejected,
                        Check::Candidate => result = Check::Candidate,
                        Check::Passes => {}
                    }
                }
            }
        }
        result
    }
}

/// A lookup against a set: `Passes` if every candidate is in it, `Rejected` if none is.
fn classify(set: &BiomeSet, lookup: &Lookup) -> Check {
    let inside = lookup
        .candidates()
        .iter()
        .filter(|&&b| set.contains(b))
        .count();
    if inside == lookup.candidates().len() {
        Check::Passes
    } else if inside == 0 {
        Check::Rejected
    } else {
        Check::Candidate
    }
}

/// Both conditions: `Rejected` if either is, `Passes` if both are, else `Candidate`.
fn and(a: Check, b: Check) -> Check {
    match (a, b) {
        (Check::Rejected, _) | (_, Check::Rejected) => Check::Rejected,
        (Check::Passes, Check::Passes) => Check::Passes,
        _ => Check::Candidate,
    }
}

/// `Structure.getLowestY(context, x, z, dx, dz)`: the lowest first occupied
/// `WORLD_SURFACE_WG` height of the corners `(x, z)`, `(x, z + dz)`, `(x + dx, z)` and
/// `(x + dx, z + dz)` (`getCornerHeights`).
fn lowest_corner(terrain: &Terrain, x: i32, z: i32, dx: i32, dz: i32) -> i32 {
    let h = |x, z| terrain.first_occupied_height(x, z, Heightmap::WorldSurfaceWg);
    let corners = [
        h(x, z),
        h(x, z.wrapping_add(dz)),
        h(x.wrapping_add(dx), z),
        h(x.wrapping_add(dx), z.wrapping_add(dz)),
    ];
    corners[0].min(corners[1]).min(corners[2].min(corners[3]))
}

/// The heightmaps the generator answers for, by their serialization key.
fn heightmap_named(name: &str) -> Option<Heightmap> {
    Heightmap::ALL.into_iter().find(|h| h.key() == name)
}

/// A climate-only precondition, before a check whose position needs terrain: the structure is
/// rejected if it fails, and a candidate otherwise.
fn precondition(check: Check) -> Check {
    match check {
        Check::Rejected => Check::Rejected,
        Check::Passes | Check::Candidate => Check::Candidate,
    }
}
