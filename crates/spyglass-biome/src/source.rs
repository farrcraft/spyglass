//! A dimension's biome source, loaded from a world preset: multi-noise (`MultiNoiseBiomeSource`,
//! the Overworld's and the Nether's), the End's (`TheEndBiomeSource`) or a checkerboard
//! (`CheckerboardColumnBiomeSource`, which no vanilla preset uses); and its lookups for one seed.

use std::collections::BTreeMap;

use serde_json::Value;
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, full_id};
use spyglass_density::{ClimateRouter, ClimateSampler, Resources};
use spyglass_rng::RandomSource;

use crate::parameters::{BiomeId, BiomeNames, ParameterList, TargetPoint};
use crate::search::{Lookup, SearchTree};

const WORLD_PRESET: &str = "worldgen/world_preset";
const DIMENSION_TYPE: &str = "dimension_type";
const PARAMETER_LIST: &str = "worldgen/multi_noise_biome_source_parameter_list";
const BIOME: &str = "worldgen/biome";
const BIOME_TAG: &str = "tags/worldgen/biome";

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
    /// `CheckerboardColumnBiomeSource`: diagonal stripes of `biomes`, in order, each
    /// `2^shift` quarts wide.
    Checkerboard {
        /// The holder set's biomes in order, repeats kept.
        biomes: Vec<BiomeId>,
        /// `bitShift`: `scale + 2`.
        shift: u32,
    },
}

impl BiomeSource {
    /// The overworld of the default world preset (`minecraft:normal`).
    pub fn overworld(bundle: &Bundle) -> Result<Self, LoadError> {
        Self::load(bundle, "minecraft:normal", "minecraft:overworld")
    }

    /// The biome source of `dimension` in world preset `preset`. The dimension's generator must
    /// be `minecraft:noise`, with a `minecraft:multi_noise` biome source that names a parameter
    /// list preset, a `minecraft:the_end` or a `minecraft:checkerboard` biome source; its noise
    /// settings must be a reference.
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
        // Anchors resolve within the level as well as the noise settings.
        let type_at = format!("dimensions.{dimension}.type");
        let dimension_type = json
            .get("dimensions")
            .and_then(|d| d.get(dimension))
            .and_then(|d| d.get("type"))
            .and_then(Value::as_str)
            .ok_or_else(|| error(preset, &type_at, LoadErrorKind::Shape("an id")))?;
        let level = bundle
            .resource(DIMENSION_TYPE, dimension_type)
            .map_err(|e| error(dimension_type, "", e.into()))?;
        let level = AnchorHeights::of_dimension_type(&level, &At::root(dimension_type))?;
        let router = || -> Result<ClimateRouter, LoadError> {
            Ok(ClimateRouter::load(bundle, settings)?.within_level(level))
        };
        let source_type = id("biome_source.type")?;
        match full_id(source_type).as_str() {
            "minecraft:multi_noise" => {}
            "minecraft:the_end" => {
                return Ok(BiomeSource {
                    router: router()?,
                    kind: Kind::TheEnd,
                    names: BiomeNames::new(END_BIOMES.map(str::to_string).to_vec()),
                    noise_settings: full_id(settings),
                });
            }
            // CheckerboardColumnBiomeSource.CODEC: `biomes` (Biome.LIST_CODEC) and `scale`, an
            // int in 0..=62, default 2.
            "minecraft:checkerboard" => {
                let source_at = format!("{at}.biome_source");
                let source = &generator["biome_source"];
                let listed = checkerboard_biomes(
                    bundle,
                    source.get("biomes").ok_or_else(|| {
                        error(preset, &source_at, LoadErrorKind::Missing("biomes"))
                    })?,
                    preset,
                    &format!("{source_at}.biomes"),
                )?;
                if listed.is_empty() {
                    // getNoiseBiome's floorMod would divide by zero.
                    return Err(error(
                        preset,
                        &format!("{source_at}.biomes"),
                        LoadErrorKind::Invalid("an empty biome list"),
                    ));
                }
                let scale = match source.get("scale") {
                    None => 2,
                    Some(v) => v.as_i64().filter(|s| (0..=62).contains(s)).ok_or_else(|| {
                        error(
                            preset,
                            &format!("{source_at}.scale"),
                            LoadErrorKind::OutOfRange {
                                value: v.to_string(),
                                min: "0".into(),
                                max: "62".into(),
                            },
                        )
                    })?,
                };
                let mut names: Vec<String> = Vec::new();
                let biomes = listed
                    .into_iter()
                    .map(|name| {
                        let i = names.iter().position(|n| *n == name).unwrap_or_else(|| {
                            names.push(name);
                            names.len() - 1
                        });
                        BiomeId(i as u16)
                    })
                    .collect();
                return Ok(BiomeSource {
                    router: router()?,
                    kind: Kind::Checkerboard {
                        biomes,
                        shift: scale as u32 + 2,
                    },
                    names: BiomeNames::new(names),
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
        let mut parameters = ParameterList::from_report(&report, &report_path)?;
        if bundle.behavior().climate_half_open {
            parameters = parameters.half_open();
        }

        Ok(BiomeSource {
            router: router()?,
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

    /// The parameter list of a multi-noise source; `None` for the others.
    pub fn parameters(&self) -> Option<&ParameterList> {
        match &self.kind {
            Kind::MultiNoise { parameters, .. } => Some(parameters),
            Kind::TheEnd | Kind::Checkerboard { .. } => None,
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
            Kind::TheEnd | Kind::Checkerboard { .. } => None,
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
            Kind::Checkerboard { biomes, shift } => {
                Lookup::Unique(checkerboard_biome(biomes, *shift, qx, qz))
            }
        }
    }

    /// `TheEndBiomeSource.getNoiseBiome`: `the_end` within 64 sections of the origin, else by the
    /// router's erosion at the section's centre block, `((s·2 + 1)·8, qy << 2, …)`. That block
    /// is quart `4s + 2`, so the climate sampler's quart sample gives it. In 26.3's family the
    /// value is the sampler's `f32` widened; in 26.2's it is the `f64` evaluation itself.
    fn end_biome(&self, qx: i32, qy: i32, qz: i32) -> BiomeId {
        // QuartPos.toBlock, then SectionPos.blockToSectionCoord.
        let sx = qx.wrapping_shl(2) >> 4;
        let sz = qz.wrapping_shl(2) >> 4;
        let (lx, lz) = (i64::from(sx), i64::from(sz));
        if lx * lx + lz * lz <= 4096 {
            return BiomeId(0);
        }
        let quart = |s: i32| s.wrapping_mul(4).wrapping_add(2);
        let erosion = match self.climate.v1_21() {
            // 26.2: erosion().compute(new SinglePointContext(...)), never narrowed.
            Some(sampler) => sampler.functions()[3].compute(
                quart(sx).wrapping_mul(4),
                qy.wrapping_mul(4),
                quart(sz).wrapping_mul(4),
            ),
            None => f64::from(self.climate.sample(quart(sx), qy, quart(sz))[3]),
        };
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

/// `CheckerboardColumnBiomeSource.getNoiseBiome`:
/// `biomes.get(floorMod((qx >> shift) + (qz >> shift), size))`, independent of y. Java's `ishr`
/// uses the low five bits of the shift, as `wrapping_shr` does, so a scale of 30 to 62 shifts
/// by `scale + 2 - 32`.
fn checkerboard_biome(biomes: &[BiomeId], shift: u32, qx: i32, qz: i32) -> BiomeId {
    let stripe = qx.wrapping_shr(shift).wrapping_add(qz.wrapping_shr(shift));
    biomes[stripe.rem_euclid(biomes.len() as i32) as usize]
}

/// `Biome.LIST_CODEC` (`RegistryCodecs.homogeneousList`), in the order the game holds it: a
/// list of ids as written, repeats included (`HolderSet.direct`); a single id; or `"#tag"`,
/// whose entries are expanded in order with nested tags in place and each biome kept at its
/// first appearance (`TagLoader`'s `LinkedHashSet`). Every biome must exist; an optional tag
/// entry (`"required": false`) that doesn't is skipped.
fn checkerboard_biomes<R: Resources + ?Sized>(
    resources: &R,
    json: &Value,
    resource: &str,
    path: &str,
) -> Result<Vec<String>, LoadError> {
    let biome = |id: &str, path: &str| -> Result<String, LoadError> {
        let id = full_id(id);
        resources
            .resource(BIOME, &id)
            .map_err(|_| {
                LoadError::new(
                    resource,
                    path,
                    LoadErrorKind::Unsupported {
                        what: "biome",
                        name: id.clone(),
                    },
                )
            })
            .map(|_| id)
    };
    match json {
        Value::String(s) => match s.strip_prefix('#') {
            Some(tag) => {
                let mut out = Vec::new();
                tag_biomes(resources, tag, &mut Vec::new(), &mut out)?;
                Ok(out)
            }
            None => Ok(vec![biome(s, path)?]),
        },
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let path = format!("{path}[{i}]");
                let id = item.as_str().ok_or_else(|| {
                    LoadError::new(resource, &path, LoadErrorKind::Shape("a biome id"))
                })?;
                biome(id, &path)
            })
            .collect(),
        _ => Err(LoadError::new(
            resource,
            path,
            LoadErrorKind::Shape("a biome tag, id or list of ids"),
        )),
    }
}

/// Appends biome tag `tag`'s biomes to `out` in the game's order, skipping those already there.
/// `loading` holds the tags being expanded, to report a cycle.
fn tag_biomes<R: Resources + ?Sized>(
    resources: &R,
    tag: &str,
    loading: &mut Vec<String>,
    out: &mut Vec<String>,
) -> Result<(), LoadError> {
    let tag = full_id(tag);
    let resource = format!("#{tag}");
    if loading.contains(&tag) {
        let mut cycle = loading.clone();
        cycle.push(tag);
        return Err(LoadError::new(&resource, "", LoadErrorKind::Cycle(cycle)));
    }
    let json = resources
        .resource(BIOME_TAG, &tag)
        .map_err(|e| LoadError::new(&resource, "", e.into()))?;
    let values = json
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| LoadError::new(&resource, "", LoadErrorKind::Missing("values")))?;
    loading.push(tag.clone());
    for (i, entry) in values.iter().enumerate() {
        let path = format!("values[{i}]");
        let error = |kind| LoadError::new(&resource, &path, kind);
        // TagEntry.CODEC: an id or `#tag`, or {"id": ..., "required": bool}.
        let (id, required) = match entry {
            Value::String(s) => (s.as_str(), true),
            Value::Object(obj) => (
                obj.get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| error(LoadErrorKind::Missing("id")))?,
                match obj.get("required") {
                    None => true,
                    Some(v) => v
                        .as_bool()
                        .ok_or_else(|| error(LoadErrorKind::Shape("a boolean")))?,
                },
            ),
            _ => return Err(error(LoadErrorKind::Shape("a tag entry"))),
        };
        match id.strip_prefix('#') {
            Some(nested) => {
                if !required && resources.resource(BIOME_TAG, &full_id(nested)).is_err() {
                    continue;
                }
                tag_biomes(resources, nested, loading, out)?;
            }
            None => {
                let id = full_id(id);
                if resources.resource(BIOME, &id).is_err() {
                    if required {
                        return Err(error(LoadErrorKind::Unsupported {
                            what: "biome",
                            name: id,
                        }));
                    }
                    continue;
                }
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    loading.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::{Value, json};

    use super::*;

    /// The End's source over noise settings `minecraft:end` whose erosion is `erosion`, in
    /// 26.2's family if `v1_21`, else 26.3's.
    fn end(erosion: f64, v1_21: bool) -> BiomeSource {
        let settings = if v1_21 {
            let mut router = serde_json::Map::new();
            for entry in spyglass_density::v1_21::RouterEntry::ALL {
                router.insert(entry.name().to_string(), json!(0));
            }
            router.insert("erosion".to_string(), json!(erosion));
            json!({
                "noise": {"min_y": 0, "height": 128, "size_horizontal": 2, "size_vertical": 1},
                "noise_router": router,
                "legacy_random_source": true,
                "aquifers_enabled": false,
                "ore_veins_enabled": false,
            })
        } else {
            let mut router: serde_json::Map<String, Value> = spyglass_density::CLIMATE_ENTRIES
                .iter()
                .map(|e| (e.to_string(), json!(0)))
                .collect();
            router.insert("erosion".to_string(), json!(erosion));
            json!({"noise_router": router, "legacy_random_source": true})
        };
        let resources = HashMap::from([(
            "worldgen/noise_settings/minecraft:end".to_string(),
            settings,
        )]);
        BiomeSource {
            router: ClimateRouter::load(&resources, "end").unwrap(),
            kind: Kind::TheEnd,
            names: BiomeNames::new(END_BIOMES.map(str::to_string).to_vec()),
            noise_settings: "minecraft:end".to_string(),
        }
    }

    #[test]
    fn checkerboard_stripes_follow_the_bytecode() {
        let b = [BiomeId(0), BiomeId(1), BiomeId(2)];
        // Scale 2: bitShift 4, so stripes 16 quarts wide, (qx >> 4) + (qz >> 4) mod 3.
        assert_eq!(checkerboard_biome(&b, 4, 0, 0), BiomeId(0));
        assert_eq!(checkerboard_biome(&b, 4, 15, 15), BiomeId(0));
        assert_eq!(checkerboard_biome(&b, 4, 16, 0), BiomeId(1));
        assert_eq!(checkerboard_biome(&b, 4, 16, 16), BiomeId(2));
        assert_eq!(checkerboard_biome(&b, 4, 32, 16), BiomeId(0));
        // floorMod, not %: -1 >> 4 is -1, and floorMod(-1, 3) is 2.
        assert_eq!(checkerboard_biome(&b, 4, -1, 0), BiomeId(2));
        assert_eq!(checkerboard_biome(&b, 4, -17, 0), BiomeId(1));
        assert_eq!(checkerboard_biome(&b, 4, -17, -1), BiomeId(0));
        // Scale 62: bitShift 64, and ishr shifts by 64 & 31 = 0, so every quart is a stripe.
        assert_eq!(checkerboard_biome(&b, 64, 1, 0), BiomeId(1));
        assert_eq!(checkerboard_biome(&b, 64, 1, 1), BiomeId(2));
        // Scale 29: bitShift 31, the last that shifts as written: two stripes, by sign.
        assert_eq!(checkerboard_biome(&b, 31, i32::MAX, 0), BiomeId(0));
        assert_eq!(checkerboard_biome(&b, 31, i32::MIN, 0), BiomeId(2));
        // The sum wraps: (MAX >> 0) + (MAX >> 0) is -2, and floorMod(-2, 3) is 1.
        assert_eq!(checkerboard_biome(&b, 32, i32::MAX, i32::MAX), BiomeId(1));
    }

    #[test]
    fn checkerboard_biomes_keep_the_games_order() {
        let biome = |id: &str| (format!("worldgen/biome/minecraft:{id}"), json!({}));
        let tag = |id: &str, values: Value| {
            (
                format!("tags/worldgen/biome/minecraft:{id}"),
                json!({ "values": values }),
            )
        };
        let resources: HashMap<String, Value> = HashMap::from([
            biome("plains"),
            biome("desert"),
            biome("forest"),
            tag("warm", json!(["minecraft:desert", "minecraft:plains"])),
            tag(
                "mixed",
                json!([
                    "minecraft:forest",
                    "#minecraft:warm",
                    "minecraft:plains",
                    {"id": "minecraft:not_there", "required": false},
                    {"id": "#minecraft:no_tag", "required": false},
                ]),
            ),
            tag("loop", json!(["#minecraft:loop"])),
        ]);
        let read = |v: Value| checkerboard_biomes(&resources, &v, "test", "biomes");
        let names = |v: Value| read(v).unwrap();
        // A tag: nested tags in place, each biome at its first appearance.
        assert_eq!(
            names(json!("#minecraft:mixed")),
            ["minecraft:forest", "minecraft:desert", "minecraft:plains"]
        );
        // A direct list keeps repeats; a single id is a list of one.
        assert_eq!(
            names(json!(["plains", "minecraft:plains", "desert"])),
            ["minecraft:plains", "minecraft:plains", "minecraft:desert"]
        );
        assert_eq!(names(json!("forest")), ["minecraft:forest"]);
        assert!(read(json!(["minecraft:not_there"])).is_err());
        assert!(read(json!("#minecraft:loop")).is_err());
        assert!(read(json!(3)).is_err());
    }

    #[test]
    fn the_ends_erosion_is_f64_in_26_2() {
        // Section (100, 0), off the main island. Just above 0.25 in f64 is highlands; narrowed
        // to f32 it is 0.25 itself, which is midlands.
        let above = 0.25_f64.next_up();
        assert_eq!(above as f32, 0.25);
        let biome = |source: &BiomeSource| source.with_seed(0).unwrap().biome_at(400, 0, 0);
        assert_eq!(biome(&end(above, true)), Lookup::Unique(BiomeId(1)));
        assert_eq!(biome(&end(above, false)), Lookup::Unique(BiomeId(2)));
        // The main island ignores erosion.
        let island = end(above, true).with_seed(0).unwrap().biome_at(0, 0, 0);
        assert_eq!(island, Lookup::Unique(BiomeId(0)));
    }
}
