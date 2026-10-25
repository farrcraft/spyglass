//! A dimension's multi-noise biome source (`MultiNoiseBiomeSource`), loaded from a world preset,
//! and its lookups for one seed.

use std::collections::BTreeMap;

use serde_json::Value;
use spyglass_data::bundle::Bundle;
use spyglass_density::{ClimateRouter, ClimateSampler, LoadError, LoadErrorKind};

use crate::parameters::{BiomeId, ParameterList, TargetPoint};
use crate::search::{Lookup, SearchTree};

const WORLD_PRESET: &str = "worldgen/world_preset";
const PARAMETER_LIST: &str = "worldgen/multi_noise_biome_source_parameter_list";

/// An id with the default namespace made explicit.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// A multi-noise biome source: a dimension's climate router and parameter list, independent of
/// the seed.
#[derive(Debug, Clone)]
pub struct BiomeSource {
    router: ClimateRouter,
    parameters: ParameterList,
    tree: SearchTree,
}

impl BiomeSource {
    /// The overworld of the default world preset (`minecraft:normal`).
    pub fn overworld(bundle: &Bundle) -> Result<Self, LoadError> {
        Self::load(bundle, "minecraft:normal", "minecraft:overworld")
    }

    /// The biome source of `dimension` in world preset `preset`. The dimension's generator must
    /// be `minecraft:noise` with a `minecraft:multi_noise` biome source that names a parameter
    /// list preset; its noise settings must be a reference.
    pub fn load(bundle: &Bundle, preset: &str, dimension: &str) -> Result<Self, LoadError> {
        let error = |resource: &str, path: &str, kind| LoadError {
            resource: resource.to_string(),
            path: path.to_string(),
            kind: Box::new(kind),
        };
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
        for (path, want) in [
            ("type", "minecraft:noise"),
            ("biome_source.type", "minecraft:multi_noise"),
        ] {
            let found = id(path)?;
            if full_id(found) != want {
                return Err(error(
                    preset,
                    &format!("{at}.{path}"),
                    LoadErrorKind::Unsupported {
                        what: "type",
                        name: found.to_string(),
                    },
                ));
            }
        }
        let settings = id("settings")?;
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
            tree: SearchTree::new(&parameters),
            parameters,
        })
    }

    /// The parameter list, for biome names and ids.
    pub fn parameters(&self) -> &ParameterList {
        &self.parameters
    }

    /// A biome's id, e.g. `minecraft:plains`.
    pub fn name(&self, biome: BiomeId) -> &str {
        self.parameters.name(biome)
    }

    /// The biome nearest to a climate target.
    pub fn lookup(&self, target: &TargetPoint) -> Lookup {
        self.tree.lookup(target)
    }

    /// The search tree.
    pub fn tree(&self) -> &SearchTree {
        &self.tree
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
        self.source.lookup(&self.climate_at(qx, qy, qz))
    }

    /// Every biome in the box of `size` quarts from `min`. Each column's y-independent climate
    /// is evaluated once ([`ClimateSampler::at_column`]), which gives the same answers as
    /// [`biome_at`](Self::biome_at) on every cell.
    pub fn biomes_in(&self, min: [i32; 3], size: [u32; 3]) -> BiomeRegion {
        let [sx, sy, sz] = size.map(|s| s as usize);
        let mut cells = vec![BiomeId(0); sx * sy * sz];
        let mut ties = BTreeMap::new();
        let mut hits = Vec::new();
        for z in 0..sz {
            let qz = min[2].wrapping_add(z as i32);
            for x in 0..sx {
                let qx = min[0].wrapping_add(x as i32);
                let column = self.climate.at_column(qx, qz);
                for y in 0..sy {
                    let qy = min[1].wrapping_add(y as i32);
                    let target = TargetPoint::quantize(column.sample(qx, qy, qz));
                    let lookup = self.source.tree.lookup_with(&target, &mut hits);
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
