//! The biome check (`Structure.findValidGenerationPoint`) and the choice among a set's
//! structures (`ChunkGenerator.createStructures`), as far as they can be decided without terrain.
//! See `docs/specs/structure-placement.md`.

use spyglass_biome::{Biomes, Lookup};
use spyglass_rng::{RandomSource, worldgen};

use crate::biomes::BiomeSet;
use crate::jigsaw::JigsawCheck;
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

/// Structure starts for one seed: [`Placements`] plus the biome source for the same seed.
pub struct Starts<'a> {
    placements: &'a Placements<'a>,
    biomes: &'a Biomes<'a>,
}

impl<'a> Starts<'a> {
    /// Starts for the placements' seed; `biomes` must be for the same seed.
    pub fn new(placements: &'a Placements<'a>, biomes: &'a Biomes<'a>) -> Self {
        Starts { placements, biomes }
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
        let sets = self.placements.sets();
        let s: &Structure = &sets.structures()[structure];
        let heights = sets.heights();
        let (min_x, min_z) = (chunk[0].wrapping_shl(4), chunk[1].wrapping_shl(4));
        let level_min_y = heights.level_min_y;
        let level_max_y = level_min_y
            .wrapping_add(heights.level_height)
            .wrapping_sub(1);
        // couldValidBiomeExistInTerrainColumn: one block below the level, to its top.
        let terrain_column =
            |x: i32, z: i32| self.column(&s.biomes, x, z, level_min_y.wrapping_sub(1), level_max_y);
        let on_top_of_chunk_center = || precondition(terrain_column(min_x + 8, min_z + 8));
        match s.kind {
            StructureType::Stronghold => self.point(&s.biomes, [min_x, 0, min_z]),
            StructureType::Fortress => self.point(&s.biomes, [min_x, 64, min_z]),
            StructureType::BuriedTreasure
            | StructureType::DesertPyramid
            | StructureType::JungleTemple
            | StructureType::SwampHut
            | StructureType::Igloo
            | StructureType::OceanRuin
            | StructureType::Shipwreck => on_top_of_chunk_center(),
            StructureType::OceanMonument => {
                match self.area(
                    sets.monument_surroundings(),
                    [min_x + 9, sets.sea_level(), min_z + 9],
                    29,
                ) {
                    Check::Rejected => Check::Rejected,
                    _ => on_top_of_chunk_center(),
                }
            }
            StructureType::WoodlandMansion | StructureType::EndCity => {
                precondition(terrain_column(min_x + 7, min_z + 7))
            }
            // The check is at (midX, 50 + dy, minZ), with dy from the piece layout; y is in the
            // level, so the column there is a sound test.
            StructureType::Mineshaft => {
                precondition(self.column(&s.biomes, min_x + 8, min_z, level_min_y, level_max_y))
            }
            StructureType::Jigsaw => {
                let jigsaw = s
                    .jigsaw
                    .as_ref()
                    .expect("jigsaw structures load their settings");
                let mut random =
                    worldgen::large_feature(self.placements.seed(), chunk[0], chunk[1]);
                match jigsaw.check_position(chunk, heights, &mut random) {
                    JigsawCheck::NoStart => Check::Rejected,
                    JigsawCheck::At(pos) => self.point(&s.biomes, pos),
                    JigsawCheck::Projected { x, z, .. } => {
                        precondition(self.column(&s.biomes, x, z, level_min_y, level_max_y))
                    }
                }
            }
            // Position from the template and terrain; not filtered yet.
            StructureType::RuinedPortal | StructureType::NetherFossil => Check::Candidate,
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

    /// `BiomeResolver.getBiomesWithin(x, y, z, radius)` all in `set`: every quart from
    /// `(p - radius) >> 2` to `(p + radius) >> 2` on each axis.
    fn area(&self, set: &BiomeSet, [x, y, z]: [i32; 3], radius: i32) -> Check {
        let range = |p: i32| ((p - radius) >> 2)..=((p + radius) >> 2);
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

/// A climate-only precondition, before a check whose position needs terrain: the structure is
/// rejected if it fails, and a candidate otherwise.
fn precondition(check: Check) -> Check {
    match check {
        Check::Rejected => Check::Rejected,
        Check::Passes | Check::Candidate => Check::Candidate,
    }
}
