//! spyglass: a local seed explorer for Minecraft: Java Edition.
//!
//! This is the public API that every front end uses (ADR 0014). Load one dimension of a version
//! bundle once as a [`World`], then bind it to seeds as [`Session`]s and ask them questions:
//!
//! ```no_run
//! let world = spyglass::World::open("bundles/26.3")?;
//! let session = world.session(spyglass::parse_seed("spyglass")?)?;
//! let villages = session.locate_structure("#minecraft:village", [0, 64, 0], spyglass::LOCATE_RADIUS)?;
//! if let Some(hit) = villages.hits.first() {
//!     println!("{:?} certain: {}", hit.start.position, hit.start.certain);
//! }
//! # Ok::<(), spyglass::Error>(())
//! ```
//!
//! Answers say how sure they are. A biome cell can be tied, a structure start can need terrain
//! spyglass doesn't compute yet, and some results depend on the platform the game runs on;
//! each result type says which applies. See `docs/specs/locate.md`.

use std::path::PathBuf;

use spyglass_biome::{BiomeId, BiomeSource, Biomes, SpawnSearch, SpawnTargets};
use spyglass_data::bundle::{Bundle, BundleError};
use spyglass_density::LoadError;
use spyglass_structure::{Placements, Starts, StructureSets};

pub use spyglass_structure::LOCATE_RADIUS;

/// `/locate biome`'s search radius in blocks (`LocateCommand`).
pub const LOCATE_BIOME_RADIUS: i32 = 6400;
/// `/locate biome`'s horizontal step in blocks.
const BIOME_HORIZONTAL_STEP: i32 = 32;
/// `/locate biome`'s vertical step in blocks.
const BIOME_VERTICAL_STEP: i32 = 64;

/// Why a request failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The bundle couldn't be read.
    #[error(transparent)]
    Bundle(#[from] BundleError),
    /// The bundle's data couldn't be loaded.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// A structure, structure tag or biome the bundle doesn't have.
    #[error("unknown {what} {id:?}")]
    Unknown {
        /// What was looked up: `structure`, `structure tag` or `biome`.
        what: &'static str,
        /// The id as given.
        id: String,
    },
    /// Seed text the game would treat differently from spyglass, or none.
    #[error("{0}")]
    Seed(&'static str),
}

/// A result, or an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// The seed for `text`, as the world-creation screen reads it (`WorldOptions.parseSeed`):
/// trimmed of characters up to U+0020, then a decimal `long` (an optional sign, then digits),
/// else `String.hashCode` of the trimmed text, widened.
///
/// Empty text is an error (the game picks a random seed). So is text that `Long.parseLong`
/// might read with non-ASCII digits, which spyglass doesn't implement.
pub fn parse_seed(text: &str) -> Result<i64> {
    let trimmed = text.trim_matches(|c: char| c <= ' ');
    if trimmed.is_empty() {
        return Err(Error::Seed("an empty seed means a random one"));
    }
    if let Ok(seed) = trimmed.parse::<i64>() {
        return Ok(seed);
    }
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if !digits.is_empty() && digits.chars().all(char::is_numeric) && !digits.is_ascii() {
        return Err(Error::Seed(
            "seed text with non-ASCII digits isn't supported",
        ));
    }
    // String.hashCode: s[0]·31^(n-1) + ... over UTF-16 code units, wrapping.
    let hash = trimmed
        .encode_utf16()
        .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
    Ok(i64::from(hash))
}

/// A bundle found by [`bundles_in`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInfo {
    /// The game version, e.g. `26.3`.
    pub version: String,
    /// The game's data version, which orders versions.
    pub data_version: u32,
    /// The bundle's directory.
    pub dir: PathBuf,
}

/// The bundles in `dir`'s subdirectories (e.g. `bundles/`), newest first by data version.
/// Subdirectories that aren't bundles are skipped.
pub fn bundles_in(dir: impl AsRef<std::path::Path>) -> Result<Vec<BundleInfo>> {
    let dir = dir.as_ref();
    let entries = std::fs::read_dir(dir).map_err(|source| BundleError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut found: Vec<BundleInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let manifest = spyglass_data::bundle::read_manifest(&entry.path()).ok()?;
            Some(BundleInfo {
                version: manifest.version,
                data_version: manifest.data_version,
                dir: entry.path(),
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.data_version
            .cmp(&a.data_version)
            .then(a.version.cmp(&b.version))
    });
    Ok(found)
}

/// One dimension of a version bundle, loaded once: everything that doesn't depend on the seed.
pub struct World {
    bundle: Bundle,
    dimension: String,
    source: BiomeSource,
    sets: StructureSets,
    spawn: SpawnTargets,
}

impl World {
    /// The Overworld of the Default world preset, from the bundle in `dir` (e.g.
    /// `bundles/26.3`).
    pub fn open(dir: impl Into<PathBuf>) -> Result<World> {
        let bundle = Bundle::open(dir)?;
        let source = BiomeSource::overworld(&bundle)?;
        let sets = StructureSets::overworld(&bundle, &source)?;
        let spawn = SpawnTargets::load(&bundle, source.noise_settings())?;
        Ok(World {
            bundle,
            dimension: "minecraft:overworld".to_string(),
            source,
            sets,
            spawn,
        })
    }

    /// The game version, e.g. `26.3`.
    pub fn version(&self) -> &str {
        &self.bundle.manifest().version
    }

    /// The dimension, e.g. `minecraft:overworld`.
    pub fn dimension(&self) -> &str {
        &self.dimension
    }

    /// Every biome the dimension can have.
    pub fn biomes(&self) -> &[String] {
        self.source.parameters().names()
    }

    /// Every structure the dimension's structure sets name, possible or not.
    pub fn structures(&self) -> impl Iterator<Item = &str> {
        self.sets.structures().iter().map(|s| s.id.as_str())
    }

    /// This world for `seed`.
    pub fn session(&self, seed: i64) -> Result<Session<'_>> {
        let biomes = self.source.with_seed(seed)?;
        let placements = self.sets.with_seed(seed, &biomes);
        Ok(Session {
            world: self,
            seed,
            spawn: self.spawn.with_seed(seed)?,
            biomes,
            placements,
        })
    }

    fn exists(&self, registry: &str, id: &str) -> bool {
        let id = if id.contains(':') {
            id.to_string()
        } else {
            format!("minecraft:{id}")
        };
        Bundle::resource_path(registry, &id)
            .is_ok_and(|path| self.bundle.manifest().files.contains_key(&path))
    }

    /// The structures a `/locate structure` argument names.
    fn targets(&self, argument: &str) -> Result<Vec<usize>> {
        let (registry, what, id) = match argument.strip_prefix('#') {
            Some(tag) => ("tags/worldgen/structure", "structure tag", tag),
            None => ("worldgen/structure", "structure", argument),
        };
        if !self.exists(registry, id) {
            return Err(Error::Unknown {
                what,
                id: argument.to_string(),
            });
        }
        Ok(self.sets.resolve(&self.bundle, argument)?)
    }
}

/// A [`World`] bound to a seed. Cheap to create; per-seed data that takes a while (stronghold
/// rings) is computed on first use and kept.
pub struct Session<'w> {
    world: &'w World,
    seed: i64,
    biomes: Biomes<'w>,
    placements: Placements<'w>,
    spawn: SpawnSearch,
}

/// The biome at a position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeAt {
    /// The biome, e.g. `minecraft:plains`. Where the cell is tied, the first of the tie.
    pub biome: String,
    /// Every biome the game may give here when the cell is tied (its answer depends on lookup
    /// history), [`biome`](Self::biome) first; empty when it's unique.
    pub tied: Vec<String>,
}

/// A structure start, or a chunk where one may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureStart {
    /// The structure set, e.g. `minecraft:villages`.
    pub set: String,
    /// The chunk.
    pub chunk: [i32; 2],
    /// The position `/locate` reports: the chunk's minimum block plus the set's locate offset.
    pub position: [i32; 3],
    /// The structures that may start here, in the order the game tries them.
    pub structures: Vec<String>,
    /// Whether the game certainly starts one of them here. Otherwise it's a candidate: the
    /// start depends on terrain, a tied biome cell, or a platform-dependent ring position.
    pub certain: bool,
}

/// A hit in [`Session::locate_structure`]'s answer.
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedStructure {
    /// Where.
    pub start: StructureStart,
    /// `BlockPos.distSqr` from the search's origin.
    pub distance_sqr: f64,
    /// The distance `/locate` prints: horizontal, rounded down.
    pub blocks_away: i32,
}

/// The answer to [`Session::locate_structure`].
#[derive(Debug, Clone, PartialEq)]
pub struct StructureLocate {
    /// Every place the game's answer may be, in its search order; each search path ends at
    /// its first certain hit.
    pub hits: Vec<LocatedStructure>,
    /// Whether the search ended where the game's does. If so, the game's answer is one of
    /// the hits, or nothing if every hit is a candidate the game rejects. If not, spyglass
    /// gave up after too many candidates.
    pub complete: bool,
    /// The game's answer if every candidate fails, as an index into
    /// [`hits`](Self::hits).
    pub certain_answer: Option<usize>,
}

/// A hit in [`Session::locate_biome`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedBiome {
    /// The block position the game would report.
    pub position: [i32; 3],
    /// The biome there.
    pub biome: BiomeAt,
    /// Whether the game certainly stops here; otherwise the cell is tied, and some of its
    /// biomes don't match.
    pub certain: bool,
    /// The distance `/locate` prints: in 3D, rounded down.
    pub blocks_away: i32,
}

/// The answer to [`Session::locate_biome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeLocate {
    /// Every place the game's answer may be, in its search order, ending at the first
    /// certain one. Empty if nothing matches within the radius.
    pub hits: Vec<LocatedBiome>,
}

/// The world spawn's starting point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnOrigin {
    /// The chunk the game's spawn search starts from (`ChunkGenerator.getOrigin`). The final
    /// spawn is in the 11 × 11 chunks around it, decided by terrain spyglass doesn't compute
    /// yet.
    pub chunk: [i32; 2],
    /// The climate search's best block (y is 0), if the dimension has spawn targets.
    pub position: Option<[i32; 3]>,
    /// Whether the game's answer may differ by platform (ADR 0012).
    pub platform_dependent: bool,
}

/// `Mth.floor(Mth.sqrt(f))`: the square root in `float`, rounded down.
fn floor_sqrt(f: f32) -> i32 {
    (f64::from(f).sqrt() as f32).floor() as i32
}

impl<'w> Session<'w> {
    /// The world.
    pub fn world(&self) -> &'w World {
        self.world
    }

    /// The seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    fn biome_names(&self, lookup: &spyglass_biome::Lookup) -> BiomeAt {
        let name = |b: &BiomeId| self.world.source.name(*b).to_string();
        BiomeAt {
            biome: name(&lookup.biome()),
            tied: if lookup.is_tied() {
                lookup.candidates().iter().map(name).collect()
            } else {
                Vec::new()
            },
        }
    }

    /// The biome at block `(x, y, z)`: its quart's, as the game stores it.
    pub fn biome_at(&self, [x, y, z]: [i32; 3]) -> BiomeAt {
        self.biome_names(&self.biomes.biome_at(x >> 2, y >> 2, z >> 2))
    }

    fn start(
        &self,
        set: usize,
        chunk: [i32; 2],
        position: [i32; 3],
        structures: &[usize],
        certain: bool,
    ) -> StructureStart {
        let sets = &self.world.sets;
        StructureStart {
            set: sets.sets()[set].id.clone(),
            chunk,
            position,
            structures: structures
                .iter()
                .map(|&s| sets.structures()[s].id.clone())
                .collect(),
            certain,
        }
    }

    /// `/locate structure <argument>` run at `from`: `argument` is a structure id or a
    /// `#tag`, and `radius` is how many rings of placement cells to search
    /// ([`LOCATE_RADIUS`] in the game). Follows the game's search order and distance; see
    /// `docs/specs/locate.md`.
    pub fn locate_structure(
        &self,
        argument: &str,
        from: [i32; 3],
        radius: i32,
    ) -> Result<StructureLocate> {
        let targets = self.world.targets(argument)?;
        let starts = Starts::new(&self.placements, &self.biomes);
        let search = starts.locate(&targets, from, radius);
        let hits = search
            .hits
            .iter()
            .map(|h| {
                let [dx, dz] = [
                    h.position[0].wrapping_sub(from[0]),
                    h.position[2].wrapping_sub(from[2]),
                ];
                // LocateCommand.dist: int arithmetic, then a float square root.
                let horizontal = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as f32;
                LocatedStructure {
                    start: self.start(h.set, h.chunk, h.position, &h.structures, h.certain),
                    distance_sqr: h.distance_sqr,
                    blocks_away: floor_sqrt(horizontal),
                }
            })
            .collect();
        Ok(StructureLocate {
            hits,
            complete: search.complete,
            certain_answer: search.certain_answer,
        })
    }

    /// Every chunk in `min..=max` (chunk coordinates, inclusive) where a structure that
    /// `argument` names (an id or `#tag`) may start, by set, then in each set's chunk order.
    pub fn structures_in(
        &self,
        argument: &str,
        min: [i32; 2],
        max: [i32; 2],
    ) -> Result<Vec<StructureStart>> {
        let targets = self.world.targets(argument)?;
        let starts = Starts::new(&self.placements, &self.biomes);
        let sets = &self.world.sets;
        let mut out = Vec::new();
        for (i, set) in sets.sets().iter().enumerate() {
            if !set.possible || !set.entries.iter().any(|e| targets.contains(&e.structure)) {
                continue;
            }
            for chunk in self.placements.structure_chunks_in(i, min, max) {
                if let Some((structures, certain)) = starts.targets_at(i, chunk, &targets) {
                    let offset = set.placement.locate_offset;
                    let position = [
                        chunk[0].wrapping_shl(4).wrapping_add(offset[0]),
                        offset[1],
                        chunk[1].wrapping_shl(4).wrapping_add(offset[2]),
                    ];
                    out.push(self.start(i, chunk, position, &structures, certain));
                }
            }
        }
        Ok(out)
    }

    /// `/locate biome <biome>` run at `from`, searching `radius` blocks
    /// ([`LOCATE_BIOME_RADIUS`] in the game) in the game's order: a spiral of columns 32 blocks
    /// apart, and heights 64 apart from `from`'s outwards. A biome that exists but that this
    /// dimension can't have finds nothing.
    pub fn locate_biome(&self, biome: &str, from: [i32; 3], radius: i32) -> Result<BiomeLocate> {
        if !self.world.exists("worldgen/biome", biome) {
            return Err(Error::Unknown {
                what: "biome",
                id: biome.to_string(),
            });
        }
        let full = if biome.contains(':') {
            biome.to_string()
        } else {
            format!("minecraft:{biome}")
        };
        let target = self.world.source.parameters().id(&full);
        let heights = self.world.sets.heights();
        let min_y = heights.level_min_y;
        let max_y = min_y.wrapping_add(heights.level_height).wrapping_sub(1);
        let search = self.biomes.find_closest_biome_3d(
            from,
            radius,
            BIOME_HORIZONTAL_STEP,
            BIOME_VERTICAL_STEP,
            min_y,
            max_y,
            |b| Some(b) == target,
        );
        let hits = search
            .hits
            .iter()
            .map(|h| {
                let d = |i: usize| f64::from(h.position[i]) - f64::from(from[i]);
                let distance_sqr = d(0) * d(0) + d(1) * d(1) + d(2) * d(2);
                LocatedBiome {
                    position: h.position,
                    biome: self.biome_names(&h.biome),
                    certain: h.certain,
                    blocks_away: floor_sqrt(distance_sqr as f32),
                }
            })
            .collect();
        Ok(BiomeLocate { hits })
    }

    /// Whether chunk `(x, z)` is a slime chunk.
    pub fn is_slime_chunk(&self, [x, z]: [i32; 2]) -> bool {
        spyglass_structure::is_slime_chunk(self.seed, x, z)
    }

    /// Every slime chunk in `min..=max` (chunk coordinates, inclusive), x fastest.
    pub fn slime_chunks(&self, min: [i32; 2], max: [i32; 2]) -> Vec<[i32; 2]> {
        (min[1]..=max[1])
            .flat_map(|z| (min[0]..=max[0]).map(move |x| [x, z]))
            .filter(|&c| self.is_slime_chunk(c))
            .collect()
    }

    /// Where the world spawn search starts.
    pub fn spawn_origin(&self) -> SpawnOrigin {
        let origin = self.spawn.origin();
        SpawnOrigin {
            chunk: origin.chunk,
            position: origin.found.map(|p| [p.position[0], 0, p.position[1]]),
            platform_dependent: origin.platform_dependent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_parse_like_the_game() {
        assert_eq!(parse_seed("12345").unwrap(), 12345);
        assert_eq!(parse_seed("  -7 \t").unwrap(), -7);
        assert_eq!(parse_seed("+7").unwrap(), 7);
        assert_eq!(parse_seed("-9223372036854775808").unwrap(), i64::MIN);
        // Out of range, or not a number: String.hashCode.
        assert_eq!(parse_seed("9223372036854775808").unwrap(), {
            let h = "9223372036854775808"
                .bytes()
                .fold(0_i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)));
            i64::from(h)
        });
        // "spyglass".hashCode() is -1556512960 (the golden set's text seed).
        assert_eq!(parse_seed("spyglass").unwrap(), -1_556_512_960);
        assert_eq!(parse_seed("-").unwrap(), 45);
        // Characters above U+0020 aren't trimmed; supplementary characters hash as two units.
        assert_ne!(parse_seed("\u{a0}1").unwrap(), 1);
        assert_eq!(
            parse_seed("😀").unwrap(),
            i64::from(0xD83D_i32 * 31 + 0xDE00)
        );
        assert!(parse_seed("   ").is_err());
        assert!(parse_seed("１２３").is_err());
    }

    #[test]
    fn floor_sqrt_rounds_down_in_float() {
        assert_eq!(floor_sqrt(0.0), 0);
        assert_eq!(floor_sqrt(965_120.0), 982); // 272² + 944²
        assert_eq!(floor_sqrt(16.0), 4);
        assert_eq!(floor_sqrt(15.999), 3);
    }
}
