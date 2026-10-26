//! The parts of a dimension's noise settings (`NoiseGeneratorSettings`) that terrain needs, and
//! their binding to a seed.

use std::collections::HashMap;

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_density::{Binder, Df, Loader, NOISE_SETTINGS, Resources, Sampler};
use spyglass_noise::NormalNoiseParameters;
use spyglass_rng::{
    PositionalRandomFactory, RandomSource, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use crate::aquifer::{AquiferSamplers, Block, FluidPicker};

/// `DimensionType.MIN_Y` and `MAX_Y`: the range every noise setting's y must stay in.
const DIMENSION_MIN_Y: i32 = -2032;
const DIMENSION_MAX_Y: i32 = 2031;

/// The `aquifers` entries, in the order of [`AquiferSamplers`]' fields.
const AQUIFER_ENTRIES: [&str; 6] = [
    "barrier",
    "fluid_level_floodedness",
    "fluid_level_spread",
    "lava",
    "exclusion",
    "surface_level",
];

/// A dimension's terrain settings, loaded and seed-independent.
#[derive(Debug, Clone)]
pub struct TerrainSettings {
    /// `noise.min_y`.
    pub min_y: i32,
    /// `noise.height`.
    pub height: i32,
    /// `sea_level`.
    pub sea_level: i32,
    /// `default_block`, a block id (`minecraft:stone` in the Overworld).
    pub default_block: String,
    /// `default_fluid`.
    pub default_fluid: Block,
    final_density: Df,
    /// `aquifers`, absent where the dimension has none (the aquifer is then disabled).
    aquifers: Option<[Df; 6]>,
    noises: HashMap<String, NormalNoiseParameters>,
}

/// Terrain's functions bound to one seed.
#[derive(Debug, Clone)]
pub struct BoundTerrain {
    /// The router's `final_density`: positive is solid.
    pub final_density: Sampler,
    /// The `aquifers` functions, if the settings have them.
    pub aquifer: Option<AquiferSamplers>,
    /// `RandomState.getOrCreateRandomFactory("minecraft:aquifer")`.
    pub aquifer_random: XoroshiroPositionalRandomFactory,
    /// The settings' fluid picker.
    pub fluid_picker: FluidPicker,
}

impl TerrainSettings {
    /// Loads `noise_settings` (e.g. `minecraft:overworld`): the noise range, sea level, default
    /// block and fluid, the router's `final_density` and the `aquifers` functions. Settings with
    /// `legacy_random_source: true` aren't supported yet.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let root = At::root(&id);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| root.error(e.into()))?;
        let settings = object(&json, &root)?;
        if settings
            .get("legacy_random_source")
            .and_then(Value::as_bool)
            == Some(true)
        {
            return Err(root
                .child("legacy_random_source")
                .error(LoadErrorKind::Unsupported {
                    what: "random source",
                    name: "legacy".to_string(),
                }));
        }
        let (min_y, height) = noise_range(field(settings, "noise", &root)?, &root.child("noise"))?;
        let sea_level = int(
            field(settings, "sea_level", &root)?,
            &root.child("sea_level"),
        )?;
        let default_block = block_id(
            field(settings, "default_block", &root)?,
            &root.child("default_block"),
        )?;
        let fluid_at = root.child("default_fluid");
        let default_fluid =
            match block_id(field(settings, "default_fluid", &root)?, &fluid_at)?.as_str() {
                "minecraft:water" => Block::Water,
                "minecraft:lava" => Block::Lava,
                "minecraft:air" => Block::Air,
                other => {
                    return Err(fluid_at.error(LoadErrorKind::Unsupported {
                        what: "default fluid",
                        name: other.to_string(),
                    }));
                }
            };
        let mut loader = Loader::new(resources);
        let router = field(settings, "noise_router", &root)?;
        let router = object(router, &root.child("noise_router"))?;
        let final_density = loader.inline(
            &id,
            "noise_router.final_density",
            field(router, "final_density", &root.child("noise_router"))?,
        )?;
        let aquifers = match settings.get("aquifers") {
            None => None,
            Some(json) => {
                let at = root.child("aquifers");
                let entries = object(json, &at)?;
                let [a, b, c, d, e, f] = AQUIFER_ENTRIES.map(|entry| {
                    let value = field(entries, entry, &at)?;
                    loader.inline(&id, &format!("aquifers.{entry}"), value)
                });
                Some([a?, b?, c?, d?, e?, f?])
            }
        };
        Ok(TerrainSettings {
            min_y,
            height,
            sea_level,
            default_block,
            default_fluid,
            final_density,
            aquifers,
            noises: loader.noises().clone(),
        })
    }

    /// `final_density`, unbound.
    pub fn final_density(&self) -> &Df {
        &self.final_density
    }

    /// The fluid picker for these settings (`NoiseBasedChunkGenerator.createFluidPicker`).
    pub fn fluid_picker(&self) -> FluidPicker {
        FluidPicker {
            sea_level: self.sea_level,
            default_fluid: self.default_fluid,
        }
    }

    /// Binds terrain's functions for `seed` with one [`Binder`], so they share their common
    /// nodes and noises as `RandomState` shares them.
    pub fn bind(&self, seed: i64) -> Result<BoundTerrain, LoadError> {
        let mut binder = Binder::new(&self.noises, seed);
        let final_density = binder.bind(&self.final_density)?;
        let aquifer = match &self.aquifers {
            None => None,
            Some(functions) => {
                let [a, b, c, d, e, f] = functions.each_ref().map(|f| binder.bind(f));
                Some(AquiferSamplers {
                    barrier: a?,
                    fluid_level_floodedness: b?,
                    fluid_level_spread: c?,
                    lava: d?,
                    exclusion: e?,
                    surface_level: f?,
                })
            }
        };
        // RandomState: the seed's positional factory, then fromHashOf(id).forkPositional().
        let aquifer_random = XoroshiroRandomSource::new(seed)
            .fork_positional()
            .from_hash_of("minecraft:aquifer")
            .fork_positional();
        Ok(BoundTerrain {
            final_density,
            aquifer,
            aquifer_random,
            fluid_picker: self.fluid_picker(),
        })
    }
}

/// `NoiseSettings.CODEC`: `min_y` and `height` in the dimension range, both multiples of 16,
/// and not above the dimension's top (`guardY`).
fn noise_range(json: &Value, at: &At<'_>) -> Result<(i32, i32), LoadError> {
    let noise = object(json, at)?;
    let min_y = int(field(noise, "min_y", at)?, &at.child("min_y"))?;
    let height = int(field(noise, "height", at)?, &at.child("height"))?;
    let invalid = |reason| at.error(LoadErrorKind::Invalid(reason));
    if !(DIMENSION_MIN_Y..=DIMENSION_MAX_Y).contains(&min_y) {
        return Err(invalid("min_y is outside the dimension range"));
    }
    if !(0..=DIMENSION_MAX_Y - DIMENSION_MIN_Y + 1).contains(&height) {
        return Err(invalid("height is outside the dimension range"));
    }
    if min_y + height > DIMENSION_MAX_Y + 1 {
        return Err(invalid(
            "min_y + height can't be higher than the dimension's top",
        ));
    }
    if min_y % 16 != 0 {
        return Err(invalid("min_y has to be a multiple of 16"));
    }
    if height % 16 != 0 {
        return Err(invalid("height has to be a multiple of 16"));
    }
    Ok((min_y, height))
}

/// A block state's block id, with its namespace: a plain id, or `{"Name": id, ...}`, with
/// properties ignored.
fn block_id(json: &Value, at: &At<'_>) -> Result<String, LoadError> {
    let id = match json {
        Value::String(s) => s.as_str(),
        Value::Object(o) => o
            .get("Name")
            .and_then(Value::as_str)
            .ok_or_else(|| at.error(LoadErrorKind::Missing("Name")))?,
        _ => return Err(at.error(LoadErrorKind::Shape("a block state"))),
    };
    Ok(full_id(id))
}
