//! A dimension's biome source, loaded from a world preset: multi-noise (`MultiNoiseBiomeSource`,
//! the Overworld's and the Nether's) or the End's (`TheEndBiomeSource`); and its lookups for one
//! seed.

use std::collections::BTreeMap;

use serde_json::Value;
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::{LoadError, LoadErrorKind, full_id};
use spyglass_density::{ClimateRouter, ClimateSampler};
use spyglass_rng::RandomSource;

use crate::parameters::{BiomeId, BiomeNames, ParameterList, TargetPoint};
use crate::search::{Lookup, SearchTree};

const WORLD_PRESET: &str = "worldgen/world_preset";
const PARAMETER_LIST: &str = "worldgen/multi_noise_biome_source_parameter_list";

/// The most tied cells [`Biomes::find_biome_horizontal`] replays every combination of; beyond
/// it, a result is reported as tie-dependent without checking.
pub const MAX_REPLAYED_TIES: usize = 12;

/// `TheEndBiomeSource`'s biomes, in `collectPossibleBiomes` order: [`BiomeId`]s 0 to 4.
const END_BIOMES: [&str; 5] = [
    "minecraft:the_end",
    "minecraft:end_highlands",
    "minecraft:end_midlands",
    "minecraft:small_end_islands",
    "minecraft:end_barrens",
];

/// A biome source: a dimension's climate router and how it picks biomes, independent of the
/// seed.
#[derive(Debug, Clone)]
pub struct BiomeSource {
    router: ClimateRouter,
    kind: Kind,
    names: BiomeNames,
    noise_settings: String,
}

#[derive(Debug, Clone)]
enum Kind {
    /// `MultiNoiseBiomeSource`: the nearest parameter point to the climate.
    MultiNoise {
        parameters: ParameterList,
        tree: SearchTree,
    },
    /// `TheEndBiomeSource`: the main island, then thresholds on erosion.
    TheEnd,
}

impl BiomeSource {
    /// The overworld of the default world preset (`minecraft:normal`).
    pub fn overworld(bundle: &Bundle) -> Result<Self, LoadError> {
        Self::load(bundle, "minecraft:normal", "minecraft:overworld")
    }

    /// The biome source of `dimension` in world preset `preset`. The dimension's generator must
    /// be `minecraft:noise`, with a `minecraft:multi_noise` biome source that names a parameter
    /// list preset or a `minecraft:the_end` biome source; its noise settings must be a
    /// reference.
    pub fn load(bundle: &Bundle, preset: &str, dimension: &str) -> Result<Self, LoadError> {
        let error = |resource: &str, path: &str, kind| LoadError::new(resource, path, kind);
        let json = bundle
            .resource(WORLD_PRESET, preset)
            .map_err(|e| error(preset, "", e.into()))?;
        let at = format!("dimensions.{dimension}.generator");
        let generator = json
            .get("dimensions")
            .and_then(|d| d.get(dimension))
            .and_then(|d| d.get("generator"))
            .ok_or_else(|| error(preset, &at, LoadErrorKind::Missing("generator")))?;
        // An id at a dotted path in the generator, or an error naming it.
        let id = |path: &str| -> Result<&str, LoadError> {
            path.split('.')
                .try_fold(generator, |v, k| v.get(k))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    error(
                        preset,
                        &format!("{at}.{path}"),
                        LoadErrorKind::Shape("an id"),
                    )
                })
        };
        let unsupported = |path: &str, found: &str| {
            error(
                preset,
                &format!("{at}.{path}"),
                LoadErrorKind::Unsupported {
                    what: "type",
                    name: found.to_string(),
                },
            )
        };
        let generator_type = id("type")?;
        if full_id(generator_type) != "minecraft:noise" {
            return Err(unsupported("type", generator_type));
        }
        let settings = id("settings")?;
        let source_type = id("biome_source.type")?;
        match full_id(source_type).as_str() {
            "minecraft:multi_noise" => {}
            "minecraft:the_end" => {
                return Ok(BiomeSource {
                    router: ClimateRouter::load(bundle, settings)?,
                    kind: Kind::TheEnd,
                    names: BiomeNames::new(END_BIOMES.map(str::to_string).to_vec()),
                    noise_settings: full_id(settings),
                });
            }
            _ => return Err(unsupported("biome_source.type", source_type)),
        }
        let list_preset = id("biome_source.preset")?;

        // MultiNoiseBiomeSourceParameterList: {"preset": id}, which names the report.
        let list_json = bundle
            .resource(PARAMETER_LIST, list_preset)
            .map_err(|e| error(list_preset, "", e.into()))?;
        let report_id = list_json
            .get("preset")
            .and_then(Value::as_str)
            .ok_or_else(|| error(list_preset, "preset", LoadErrorKind::Shape("a preset id")))?;
        let (namespace, path) = report_id
            .split_once(':')
            .unwrap_or(("minecraft", report_id));
        let report_path = format!("reports/biome_parameters/{namespace}/{path}.json");
        let report = bundle
            .report(&report_path)
            .map_err(|e| error(&report_path, "", e.into()))?;
        let parameters = ParameterList::from_report(&report, &report_path)?;

        Ok(BiomeSource {
            router: ClimateRouter::load(bundle, settings)?,
            names: parameters.biome_names().clone(),
            kind: Kind::MultiNoise {
                tree: SearchTree::new(&parameters),
                parameters,
            },
            noise_settings: full_id(settings),
        })
    }

    /// The id of the dimension's noise settings, e.g. `minecraft:overworld`, which also hold
    /// its spawn targets ([`SpawnTargets::load`](crate::SpawnTargets::load)).
    pub fn noise_settings(&self) -> &str {
        &self.noise_settings
    }

    /// Every biome the source can give, by [`BiomeId`].
    pub fn biome_names(&self) -> &BiomeNames {
        &self.names
    }

    /// The parameter list of a multi-noise source; `None` for the End's.
    pub fn parameters(&self) -> Option<&ParameterList> {
        match &self.kind {
            Kind::MultiNoise { parameters, .. } => Some(parameters),
            Kind::TheEnd => None,
        }
    }

    /// A biome's id, e.g. `minecraft:plains`.
    pub fn name(&self, biome: BiomeId) -> &str {
        self.names.name(biome)
    }

    /// The search tree of a multi-noise source, for tests that compare it with
    /// [`SearchTree::brute_force`].
    #[doc(hidden)]
    pub fn tree(&self) -> Option<&SearchTree> {
        match &self.kind {
            Kind::MultiNoise { tree, .. } => Some(tree),
            Kind::TheEnd => None,
        }
    }

    /// Lookups for `seed`.
    pub fn with_seed(&self, seed: i64) -> Result<Biomes<'_>, LoadError> {
        Ok(Biomes {
            source: self,
            climate: self.router.sampler(seed)?,
        })
    }
}

/// A biome source bound to a seed.
#[derive(Debug, Clone)]
pub struct Biomes<'s> {
    source: &'s BiomeSource,
    climate: ClimateSampler,
}

impl Biomes<'_> {
    /// The biome source.
    pub fn source(&self) -> &BiomeSource {
        self.source
    }

    /// `Climate.Sampler.sample`: the quantized climate at quart `(qx, qy, qz)`.
    pub fn climate_at(&self, qx: i32, qy: i32, qz: i32) -> TargetPoint {
        TargetPoint::quantize(self.climate.sample(qx, qy, qz))
    }

    /// The biome at quart `(qx, qy, qz)`, with ties.
    pub fn biome_at(&self, qx: i32, qy: i32, qz: i32) -> Lookup {
        self.lookup_at(qx, qy, qz, &mut Vec::new())
    }

    /// [`biome_at`](Self::biome_at), with a buffer the multi-noise search reuses.
    pub(crate) fn lookup_at(&self, qx: i32, qy: i32, qz: i32, hits: &mut Vec<u32>) -> Lookup {
        match &self.source.kind {
            Kind::MultiNoise { tree, .. } => tree.lookup_with(&self.climate_at(qx, qy, qz), hits),
            Kind::TheEnd => Lookup::Unique(self.end_biome(qx, qy, qz)),
        }
    }

    /// `TheEndBiomeSource.getNoiseBiome`: `the_end` within 64 sections of the origin, else by the
    /// router's erosion at the section's centre block, `((s·2 + 1)·8, qy << 2, …)`. That block
    /// is quart `4s + 2`, so the climate sampler's quart sample gives it.
    fn end_biome(&self, qx: i32, qy: i32, qz: i32) -> BiomeId {
        // QuartPos.toBlock, then SectionPos.blockToSectionCoord.
        let sx = qx.wrapping_shl(2) >> 4;
        let sz = qz.wrapping_shl(2) >> 4;
        let (lx, lz) = (i64::from(sx), i64::from(sz));
        if lx * lx + lz * lz <= 4096 {
            return BiomeId(0);
        }
        let quart = |s: i32| s.wrapping_mul(4).wrapping_add(2);
        let erosion = f64::from(self.climate.sample(quart(sx), qy, quart(sz))[3]);
        // dcmpl/ifle, dcmpl/iflt, dcmpg/ifge: NaN fails every test and gives barrens.
        if erosion > 0.25 {
            BiomeId(1)
        } else if erosion >= -0.0625 {
            BiomeId(2)
        } else if erosion < -0.21875 {
            BiomeId(3)
        } else {
            BiomeId(4)
        }
    }

    /// `BiomeSource.findBiomeHorizontal(x, y, z, radius, increment, predicate, random,
    /// findClosest)`, in block coordinates: a biome matching `matches` among the quarts of the
    /// square of `area.radius` blocks around `area.center` at its height.
    ///
    /// With `find_closest`, rings are scanned outwards from the centre and the first match is
    /// returned. Otherwise the whole square is scanned, z outer and x inner, and one match is
    /// chosen by reservoir sampling: the first match is kept, and the `n`-th replaces it when
    /// `random.next_int_bounded(n) == 0`.
    ///
    /// `matches` sees each cell's answer, a tied cell's first candidate. A tied cell whose
    /// candidates disagree on `matches` could go either way in the game. The search is replayed
    /// for every combination of those cells' outcomes, from the same random state, and every
    /// other outcome is listed in [`HorizontalSearch::alternatives`]. With more than
    /// [`MAX_REPLAYED_TIES`] such cells, nothing is replayed and the alternatives are unknown.
    ///
    /// `area.increment` must be positive: the game's loops never end otherwise.
    pub fn find_biome_horizontal<R: RandomSource + Clone>(
        &self,
        area: HorizontalArea,
        matches: impl Fn(BiomeId) -> bool,
        random: &mut R,
    ) -> HorizontalSearch {
        assert!(area.increment > 0, "increment must be positive");
        let cells = self.scan_cells(&area, &matches);
        let y = area.center[1];
        let position = |i: Option<usize>| i.map(|i| [cells[i].x << 2, y, cells[i].z << 2]);
        let start = random.clone();
        let answer = choose(
            &cells,
            area.find_closest,
            &|_, c| matches(c.lookup.biome()),
            random,
        );
        let tied = cells.iter().filter(|c| c.tied).count();
        let alternatives_known = tied <= MAX_REPLAYED_TIES;
        let alternatives = if tied > 0 && alternatives_known {
            replay_ties(&cells, area.find_closest, &matches, &start)
                .into_iter()
                .map(position)
                .filter(|p| *p != position(answer))
                .fold(Vec::new(), |mut out, p| {
                    if !out.contains(&p) {
                        out.push(p);
                    }
                    out
                })
        } else {
            Vec::new()
        };
        HorizontalSearch {
            found: answer.map(|i| HorizontalMatch {
                position: position(Some(i)).expect("a match"),
                biome: cells[i].lookup.clone(),
            }),
            alternatives,
            alternatives_known,
        }
    }

    /// The cells that can matter, in the game's scan order: every hit, and every tied cell that
    /// could be one. With `find_closest`, the scan stops at the first certain hit, since nothing
    /// after it changes the closest one.
    fn scan_cells(&self, area: &HorizontalArea, matches: &impl Fn(BiomeId) -> bool) -> Vec<Cell> {
        let [x, y, z] = area.center;
        let (qx, qy, qz, qr) = (x >> 2, y >> 2, z >> 2, area.radius >> 2);
        let step = area.increment;
        let mut cells = Vec::new();
        let mut r = if area.find_closest { 0 } else { qr };
        while r <= qr {
            let mut dz = -r;
            while dz <= r {
                let z_edge = dz.wrapping_abs() == r;
                let mut dx = -r;
                while dx <= r {
                    if area.find_closest && dx.wrapping_abs() != r && !z_edge {
                        dx = dx.wrapping_add(step);
                        continue;
                    }
                    let (cx, cz) = (qx.wrapping_add(dx), qz.wrapping_add(dz));
                    let lookup = self.biome_at(cx, qy, cz);
                    let hit = matches(lookup.biome());
                    let tied = lookup.candidates().iter().any(|&b| matches(b) != hit);
                    if hit || tied {
                        cells.push(Cell {
                            x: cx,
                            z: cz,
                            lookup,
                            tied,
                        });
                        if area.find_closest && hit && !tied {
                            return cells;
                        }
                    }
                    dx = dx.wrapping_add(step);
                }
                dz = dz.wrapping_add(step);
            }
            r = r.wrapping_add(step);
        }
        cells
    }

    /// Every biome in the box of `size` quarts from `min`, column by column, so each column's
    /// y-independent climate is evaluated once (`spyglass_density::Sampler::sample`). The answers
    /// are [`biome_at`](Self::biome_at)'s on every cell.
    pub fn biomes_in(&self, min: [i32; 3], size: [u32; 3]) -> BiomeRegion {
        let [sx, sy, sz] = size.map(|s| s as usize);
        let count = sx
            .checked_mul(sy)
            .and_then(|n| n.checked_mul(sz))
            .expect("a region small enough to count its cells");
        let mut cells = vec![BiomeId(0); count];
        let mut ties = BTreeMap::new();
        let mut hits = Vec::new();
        for z in 0..sz {
            let qz = min[2].wrapping_add(z as i32);
            for x in 0..sx {
                let qx = min[0].wrapping_add(x as i32);
                for y in 0..sy {
                    let qy = min[1].wrapping_add(y as i32);
                    let lookup = self.lookup_at(qx, qy, qz, &mut hits);
                    let index = (y * sz + z) * sx + x;
                    cells[index] = lookup.biome();
                    if let Lookup::Tied(set) = lookup {
                        ties.insert(index, set);
                    }
                }
            }
        }
        BiomeRegion {
            min,
            size,
            cells,
            ties,
        }
    }

    /// The biomes at quart height `qy` on a horizontal grid: `size` cells from quart
    /// `origin` (x, z), `step` quarts apart, x fastest. Each cell is
    /// [`biome_at`](Self::biome_at)'s answer; positions wrap like the game's `int`s.
    pub fn biome_grid(&self, origin: [i32; 2], qy: i32, step: i32, size: [u32; 2]) -> Vec<Lookup> {
        let [sx, sz] = size;
        let mut hits = Vec::new();
        let mut cells = Vec::with_capacity(sx as usize * sz as usize);
        for z in 0..sz {
            let qz = origin[1].wrapping_add(step.wrapping_mul(z as i32));
            for x in 0..sx {
                let qx = origin[0].wrapping_add(step.wrapping_mul(x as i32));
                cells.push(self.lookup_at(qx, qy, qz, &mut hits));
            }
        }
        cells
    }
}

/// Where [`Biomes::find_biome_horizontal`] looks: `BiomeSource.findBiomeHorizontal`'s
/// position and search arguments, in blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizontalArea {
    /// The centre; its y is the height searched.
    pub center: [i32; 3],
    /// Half the square's side.
    pub radius: i32,
    /// The step between cells, in quarts. Must be positive.
    pub increment: i32,
    /// Scan rings outwards and stop at the first match, rather than choosing at random among
    /// every match in the square.
    pub find_closest: bool,
}

/// A cell [`Biomes::find_biome_horizontal`] kept: a hit, or a tie that could be one.
struct Cell {
    x: i32,
    z: i32,
    lookup: Lookup,
    /// Whether the cell's candidates disagree on `matches`.
    tied: bool,
}

/// The game's draws over `cells`, given which match: the first, with `find_closest`, else
/// reservoir sampling with `random`.
fn choose<R: RandomSource>(
    cells: &[Cell],
    find_closest: bool,
    is_match: &dyn Fn(usize, &Cell) -> bool,
    random: &mut R,
) -> Option<usize> {
    let mut chosen = None;
    let mut count = 0_i32;
    for (i, cell) in cells.iter().enumerate() {
        if !is_match(i, cell) {
            continue;
        }
        if chosen.is_none() || random.next_int_bounded(count + 1) == 0 {
            chosen = Some(i);
            if find_closest {
                return chosen;
            }
        }
        count += 1;
    }
    chosen
}

/// Every answer the game could give if the tied cells went another way: the search replayed
/// from `start` for each combination of their outcomes but the one `matches` gives.
fn replay_ties<R: RandomSource + Clone>(
    cells: &[Cell],
    find_closest: bool,
    matches: &impl Fn(BiomeId) -> bool,
    start: &R,
) -> Vec<Option<usize>> {
    let tied: Vec<usize> = (0..cells.len()).filter(|&i| cells[i].tied).collect();
    (1_u32..1 << tied.len())
        .map(|mask| {
            let flipped = |i: usize, cell: &Cell| {
                let hit = matches(cell.lookup.biome());
                match tied.iter().position(|&t| t == i) {
                    Some(bit) if mask >> bit & 1 == 1 => !hit,
                    _ => hit,
                }
            };
            choose(cells, find_closest, &flipped, &mut start.clone())
        })
        .collect()
}

/// A match from [`Biomes::find_biome_horizontal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HorizontalMatch {
    /// The block position: the matching quart's minimum x and z, and the `y` searched at.
    pub position: [i32; 3],
    /// The lookup there.
    pub biome: Lookup,
}

/// The answer to [`Biomes::find_biome_horizontal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HorizontalSearch {
    /// spyglass's answer, with each tied cell giving its first candidate; `None` if nothing
    /// matches.
    pub found: Option<HorizontalMatch>,
    /// Every other position the game may answer, `None` meaning no match, from other outcomes
    /// of tied cells (in the whole square when scanning all of it, or up to the match with
    /// `find_closest`). Empty when the result doesn't depend on ties.
    pub alternatives: Vec<Option<[i32; 3]>>,
    /// Whether [`alternatives`](Self::alternatives) is complete. False when more than
    /// [`MAX_REPLAYED_TIES`] tied cells were found, so the result depends on ties in ways
    /// spyglass didn't enumerate.
    pub alternatives_known: bool,
}

impl HorizontalSearch {
    /// Whether the game's answer may differ between runs, because of tied cells.
    pub fn tie_dependent(&self) -> bool {
        !self.alternatives.is_empty() || !self.alternatives_known
    }
}

/// Biomes over a box of quarts, from [`Biomes::biomes_in`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeRegion {
    min: [i32; 3],
    size: [u32; 3],
    /// Each cell's answer, x fastest, then z, then y.
    cells: Vec<BiomeId>,
    /// Tied cells' sets, by cell index.
    ties: BTreeMap<usize, Box<[BiomeId]>>,
}

impl BiomeRegion {
    /// The minimum quart corner.
    pub fn min(&self) -> [i32; 3] {
        self.min
    }

    /// The size in quarts.
    pub fn size(&self) -> [u32; 3] {
        self.size
    }

    fn index(&self, qx: i32, qy: i32, qz: i32) -> Option<usize> {
        let offset = |q: i32, d: usize| {
            let o = i64::from(q) - i64::from(self.min[d]);
            (0..i64::from(self.size[d]))
                .contains(&o)
                .then_some(o as usize)
        };
        let (x, y, z) = (offset(qx, 0)?, offset(qy, 1)?, offset(qz, 2)?);
        let [sx, _, sz] = self.size.map(|s| s as usize);
        Some((y * sz + z) * sx + x)
    }

    /// The lookup at a quart position, or `None` outside the box.
    pub fn get(&self, qx: i32, qy: i32, qz: i32) -> Option<Lookup> {
        let i = self.index(qx, qy, qz)?;
        Some(match self.ties.get(&i) {
            Some(set) => Lookup::Tied(set.clone()),
            None => Lookup::Unique(self.cells[i]),
        })
    }

    /// Each cell's answer (a tied cell's first candidate), x fastest, then z, then y.
    pub fn answers(&self) -> &[BiomeId] {
        &self.cells
    }

    /// Number of tied cells.
    pub fn tie_count(&self) -> usize {
        self.ties.len()
    }
}
