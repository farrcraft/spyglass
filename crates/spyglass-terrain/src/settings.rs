//! The parts of a dimension's noise settings (`NoiseGeneratorSettings`) that terrain needs, and
//! their binding to a seed. 26.2's settings (the 1.21 family, ADR 0026) load as a whole
//! [`v1_21::NoiseRouter`].

use std::collections::HashMap;

use serde_json::Value;
use spyglass_data::anchor::AnchorHeights;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, full_id, int, object};
use spyglass_density::{Binder, Df, Loader, NOISE_SETTINGS, Resources, Sampler, v1_21};
use spyglass_noise::NormalNoiseParameters;
use spyglass_rng::{
    PositionalRandomFactory, RandomSource, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use crate::aquifer::{AquiferSamplers, Block, FluidPicker};
use crate::tags::block_tag;

/// `DimensionType.MIN_Y` and `MAX_Y`: the range every noise setting's y must stay in.
const DIMENSION_MIN_Y: i32 = -2032;
const DIMENSION_MAX_Y: i32 = 2031;

/// Default blocks whose `blocksMotion()` is true, for versions with the
/// `heightmap_blocks_motion` switch: the ones vanilla noise settings name.
const BLOCKS_MOTION: [&str; 3] = [
    "minecraft:stone",
    "minecraft:netherrack",
    "minecraft:end_stone",
];

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
    /// `default_block`, a block id (`minecraft:stone` in the Overworld). Absent from 26.4 on,
    /// where a column leaves solid blocks to the material rule (`docs/specs/terrain.md`).
    pub default_block: Option<String>,
    /// `default_fluid`.
    pub default_fluid: Block,
    /// Whether a solid block counts for `OCEAN_FLOOR_WG`: whether `default_block` is in
    /// `#minecraft:blocks_motion_in_heightmap`, or always with the `solid_only_heights` switch.
    pub default_block_blocks_motion: bool,
    /// The `solid_only_heights` switch (ADR 0026): an empty column's height is the column's
    /// bottom, not the level's.
    pub solid_only_heights: bool,
    /// The dimension type's `min_y`: the level's bottom, which queries are clamped to.
    pub level_min_y: i32,
    /// The dimension type's `height`.
    pub level_height: i32,
    pub(crate) family: Family,
}

/// The density functions terrain evaluates, by algorithm family (ADR 0026).
#[derive(Debug, Clone)]
pub(crate) enum Family {
    /// 26.3 on: `final_density` and the `aquifers` functions, evaluated over volumes.
    Volume {
        final_density: Df,
        /// `aquifers`, absent where the dimension has none (the aquifer is then disabled).
        aquifers: Option<[Df; 6]>,
        noises: HashMap<String, NormalNoiseParameters>,
        /// `legacy_random_source`.
        legacy: bool,
    },
    /// 26.2: the whole router, evaluated through `NoiseChunk`s.
    V1_21(Box<v1_21::NoiseRouter>),
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
    /// block and fluid, the router's `final_density` and the `aquifers` functions; and from
    /// `dimension_type` (e.g. `minecraft:overworld`), the level's range. With
    /// `legacy_random_source: true`, settings with aquifers aren't supported: no vanilla
    /// dimension has both, and the aquifer's random source isn't ported for legacy.
    ///
    /// 26.2's noise settings (`noise.size_horizontal`, [`v1_21::is_v1_21`]) load their whole
    /// router, with `aquifers_enabled` and `ore_veins_enabled`; legacy randomness with either
    /// isn't supported, for the same reason.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
        dimension_type: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let root = At::root(&id);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| root.error(e.into()))?;
        let settings = object(&json, &root)?;
        let router = if v1_21::is_v1_21(&json) {
            let router = v1_21::NoiseRouter::from_json(resources, &id, &json)?;
            for (enabled, name) in [
                (router.aquifers_enabled(), "aquifers_enabled"),
                (router.ore_veins_enabled(), "ore_veins_enabled"),
            ] {
                if enabled && router.legacy_random_source() {
                    return Err(root.child(name).error(LoadErrorKind::Unsupported {
                        what: "aquifers or ore veins with",
                        name: "legacy_random_source".to_string(),
                    }));
                }
            }
            Some(router)
        } else {
            None
        };
        let legacy = match settings.get("legacy_random_source") {
            None => false,
            Some(v) => v.as_bool().ok_or_else(|| {
                root.child("legacy_random_source")
                    .error(LoadErrorKind::Shape("true or false"))
            })?,
        };
        if legacy && router.is_none() && settings.contains_key("aquifers") {
            return Err(root.child("aquifers").error(LoadErrorKind::Unsupported {
                what: "aquifers with",
                name: "legacy_random_source".to_string(),
            }));
        }
        let (min_y, height) = match &router {
            Some(router) => {
                let noise = router.noise_settings();
                (noise.min_y, noise.height)
            }
            None => noise_range(field(settings, "noise", &root)?, &root.child("noise"))?,
        };
        let sea_level = int(
            field(settings, "sea_level", &root)?,
            &root.child("sea_level"),
        )?;
        let default_block = match settings.get("default_block") {
            Some(json) => Some(block_id(json, &root.child("default_block"))?),
            None => None,
        };
        let solid_only_heights = resources.behavior().solid_only_heights;
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
        let behavior = resources.behavior();
        let default_block_blocks_motion = match &default_block {
            _ if solid_only_heights => true,
            // `blocksMotion()` is a block property spyglass doesn't have, so only the default
            // blocks vanilla settings name, which all block motion, are supported.
            Some(block) if behavior.heightmap_blocks_motion => {
                if !BLOCKS_MOTION.contains(&block.as_str()) {
                    return Err(root
                        .child("default_block")
                        .error(LoadErrorKind::Unsupported {
                            what: "default block, for blocksMotion(),",
                            name: block.clone(),
                        }));
                }
                true
            }
            Some(block) => {
                block_tag(resources, "minecraft:blocks_motion_in_heightmap")?.contains(block)
            }
            None => return Err(root.error(LoadErrorKind::Missing("default_block"))),
        };
        let (level_min_y, level_height) = level_range(resources, dimension_type)?;
        if let Some(router) = router {
            return Ok(TerrainSettings {
                min_y,
                height,
                sea_level,
                default_block,
                default_fluid,
                default_block_blocks_motion,
                solid_only_heights,
                level_min_y,
                level_height,
                family: Family::V1_21(Box::new(router)),
            });
        }
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
            default_block_blocks_motion,
            solid_only_heights,
            level_min_y,
            level_height,
            family: Family::Volume {
                final_density,
                aquifers,
                noises: loader.noises().clone(),
                legacy,
            },
        })
    }

    /// What vertical anchors resolve against in this dimension (`VerticalAnchor.Context.from`):
    /// within both the noise settings' heights and the level's.
    pub fn anchors(&self) -> AnchorHeights {
        AnchorHeights::within_level(
            AnchorHeights {
                min_y: self.min_y,
                height: self.height,
            },
            AnchorHeights {
                min_y: self.level_min_y,
                height: self.level_height,
            },
        )
    }

    /// `final_density`, unbound; `None` for 26.2's settings, whose functions are another
    /// family's ([`router_v1_21`](Self::router_v1_21)).
    pub fn final_density(&self) -> Option<&Df> {
        match &self.family {
            Family::Volume { final_density, .. } => Some(final_density),
            Family::V1_21(_) => None,
        }
    }

    /// 26.2's noise router, if these are 26.2's settings.
    pub fn router_v1_21(&self) -> Option<&v1_21::NoiseRouter> {
        match &self.family {
            Family::Volume { .. } => None,
            Family::V1_21(router) => Some(router),
        }
    }

    /// The fluid picker for these settings (`NoiseBasedChunkGenerator.createFluidPicker`).
    pub fn fluid_picker(&self) -> FluidPicker {
        FluidPicker {
            sea_level: self.sea_level,
            default_fluid: self.default_fluid,
        }
    }

    /// Binds terrain's functions for `seed` with one [`Binder`], so they share their common
    /// nodes and noises as `RandomState` shares them. 26.2's settings don't bind this way:
    /// [`Terrain::new`](crate::Terrain::new) binds them.
    pub fn bind(&self, seed: i64) -> Result<BoundTerrain, LoadError> {
        let Family::Volume {
            final_density,
            aquifers,
            noises,
            legacy,
        } = &self.family
        else {
            return Err(LoadError::new(
                "",
                "",
                LoadErrorKind::Unsupported {
                    what: "binding as volumes for",
                    name: "26.2's noise settings".to_string(),
                },
            ));
        };
        let mut binder =
            Binder::with_legacy_random(noises, seed, *legacy).with_anchors(self.anchors());
        let final_density = binder.bind(final_density)?;
        let aquifer = match aquifers {
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

/// A dimension type's `min_y` and `height`.
fn level_range<R: Resources + ?Sized>(resources: &R, id: &str) -> Result<(i32, i32), LoadError> {
    let id = full_id(id);
    let root = At::root(&id);
    let json = resources
        .resource("dimension_type", &id)
        .map_err(|e| root.error(e.into()))?;
    let dimension = object(&json, &root)?;
    let min_y = int(field(dimension, "min_y", &root)?, &root.child("min_y"))?;
    let height = int(field(dimension, "height", &root)?, &root.child("height"))?;
    Ok((min_y, height))
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
