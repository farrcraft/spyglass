//! 26.2's noise router (`NoiseRouter`, 15 entries) and noise settings, loaded from a
//! dimension's noise settings, and bound to a seed as `RandomState` binds them. See
//! `docs/specs/density-functions.md#262-router`.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, LoadResult, full_id, int_field};
use spyglass_noise::NormalNoiseParameters;

use super::bind::{Binder, Function, Wiring};
use super::function::Df;
use super::load::Loader;
use crate::load::Resources;
use crate::router::NOISE_SETTINGS;

/// A 26.2 noise router entry, in `NoiseRouter`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouterEntry {
    /// `barrier`.
    Barrier,
    /// `fluid_level_floodedness`.
    FluidLevelFloodedness,
    /// `fluid_level_spread`.
    FluidLevelSpread,
    /// `lava`.
    Lava,
    /// `temperature`.
    Temperature,
    /// `vegetation`: humidity.
    Vegetation,
    /// `continents`.
    Continents,
    /// `erosion`.
    Erosion,
    /// `depth`.
    Depth,
    /// `ridges`: weirdness.
    Ridges,
    /// `preliminary_surface_level`.
    PreliminarySurfaceLevel,
    /// `final_density`.
    FinalDensity,
    /// `vein_toggle`.
    VeinToggle,
    /// `vein_ridged`.
    VeinRidged,
    /// `vein_gap`.
    VeinGap,
}

impl RouterEntry {
    /// Every entry, in `NoiseRouter`'s order.
    pub const ALL: [RouterEntry; 15] = [
        RouterEntry::Barrier,
        RouterEntry::FluidLevelFloodedness,
        RouterEntry::FluidLevelSpread,
        RouterEntry::Lava,
        RouterEntry::Temperature,
        RouterEntry::Vegetation,
        RouterEntry::Continents,
        RouterEntry::Erosion,
        RouterEntry::Depth,
        RouterEntry::Ridges,
        RouterEntry::PreliminarySurfaceLevel,
        RouterEntry::FinalDensity,
        RouterEntry::VeinToggle,
        RouterEntry::VeinRidged,
        RouterEntry::VeinGap,
    ];

    /// The entry's field name in the noise settings' `noise_router`.
    pub fn name(self) -> &'static str {
        match self {
            RouterEntry::Barrier => "barrier",
            RouterEntry::FluidLevelFloodedness => "fluid_level_floodedness",
            RouterEntry::FluidLevelSpread => "fluid_level_spread",
            RouterEntry::Lava => "lava",
            RouterEntry::Temperature => "temperature",
            RouterEntry::Vegetation => "vegetation",
            RouterEntry::Continents => "continents",
            RouterEntry::Erosion => "erosion",
            RouterEntry::Depth => "depth",
            RouterEntry::Ridges => "ridges",
            RouterEntry::PreliminarySurfaceLevel => "preliminary_surface_level",
            RouterEntry::FinalDensity => "final_density",
            RouterEntry::VeinToggle => "vein_toggle",
            RouterEntry::VeinRidged => "vein_ridged",
            RouterEntry::VeinGap => "vein_gap",
        }
    }

    /// The entry named `name`, if any.
    pub fn of(name: &str) -> Option<RouterEntry> {
        RouterEntry::ALL.into_iter().find(|e| e.name() == name)
    }
}

/// The six climate entries, in `Climate.Sampler`'s order.
pub(crate) const CLIMATE: [RouterEntry; 6] = [
    RouterEntry::Temperature,
    RouterEntry::Vegetation,
    RouterEntry::Continents,
    RouterEntry::Erosion,
    RouterEntry::Depth,
    RouterEntry::Ridges,
];

/// 26.2's `NoiseSettings`: the y range and the cell sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseSettings {
    /// `min_y`.
    pub min_y: i32,
    /// `height`.
    pub height: i32,
    /// `size_horizontal`, 1 to 4.
    pub size_horizontal: i32,
    /// `size_vertical`, 1 to 4.
    pub size_vertical: i32,
}

impl NoiseSettings {
    /// `getCellWidth()`: `QuartPos.toBlock(size_horizontal)`.
    pub fn cell_width(&self) -> i32 {
        self.size_horizontal << 2
    }

    /// `getCellHeight()`: `QuartPos.toBlock(size_vertical)`.
    pub fn cell_height(&self) -> i32 {
        self.size_vertical << 2
    }

    /// `clampToHeightAccessor`: the y range cut to a level from `level_min_y`, `level_height`
    /// blocks high (`getMaxY() + 1` is its top).
    pub fn clamped(&self, level_min_y: i32, level_height: i32) -> NoiseSettings {
        let min_y = self.min_y.max(level_min_y);
        let top = self
            .min_y
            .wrapping_add(self.height)
            .min(level_min_y.wrapping_add(level_height));
        NoiseSettings {
            min_y,
            height: top.wrapping_sub(min_y),
            ..*self
        }
    }

    /// `NoiseSettings.CODEC`, with `guardY`'s checks.
    fn parse(json: &Value, at: &At<'_>) -> LoadResult<NoiseSettings> {
        let obj = json
            .as_object()
            .ok_or_else(|| at.error(LoadErrorKind::Shape("a noise settings object")))?;
        // DimensionType.MIN_Y, MAX_Y and Y_SIZE.
        let settings = NoiseSettings {
            min_y: int_field(obj, "min_y", (-2032, 2031), None, at)?,
            height: int_field(obj, "height", (0, 4064), None, at)?,
            size_horizontal: int_field(obj, "size_horizontal", (1, 4), None, at)?,
            size_vertical: int_field(obj, "size_vertical", (1, 4), None, at)?,
        };
        if settings.min_y.wrapping_add(settings.height) > 2032 {
            return Err(at.error(LoadErrorKind::Invalid(
                "min_y + height cannot be higher than 2032",
            )));
        }
        if settings.height % 16 != 0 {
            return Err(at.error(LoadErrorKind::Invalid("height has to be a multiple of 16")));
        }
        if settings.min_y % 16 != 0 {
            return Err(at.error(LoadErrorKind::Invalid("min_y has to be a multiple of 16")));
        }
        Ok(settings)
    }
}

/// Whether noise settings are the 1.21 family's (ADR 0026): their `noise` has
/// `size_horizontal`.
pub fn is_v1_21(noise_settings: &Value) -> bool {
    noise_settings
        .get("noise")
        .is_some_and(|noise| noise.get("size_horizontal").is_some())
}

/// A dimension's 26.2 noise router and the settings binding and the `NoiseChunk` need,
/// seed-independent.
#[derive(Debug, Clone)]
pub struct NoiseRouter {
    entries: [Df; 15],
    noise: NoiseSettings,
    legacy_random_source: bool,
    aquifers_enabled: bool,
    ore_veins_enabled: bool,
    noises: HashMap<String, NormalNoiseParameters>,
}

impl NoiseRouter {
    /// Loads `noise_settings` (e.g. `minecraft:overworld`): its `noise`, its router's fifteen
    /// entries, and the flags binding and generation read.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| LoadError::new(&id, "", e.into()))?;
        Self::from_json(resources, &id, &json)
    }

    /// The router of noise settings `json`, the resource `id`, with functions and noises from
    /// `resources`.
    pub fn from_json<R: Resources + ?Sized>(
        resources: &R,
        id: &str,
        json: &Value,
    ) -> Result<Self, LoadError> {
        let at = At::root(id);
        let flag = |name: &'static str| -> LoadResult<bool> {
            let v = json
                .get(name)
                .ok_or_else(|| at.error(LoadErrorKind::Missing(name)))?;
            v.as_bool()
                .ok_or_else(|| at.child(name).error(LoadErrorKind::Shape("true or false")))
        };
        let noise = NoiseSettings::parse(
            json.get("noise")
                .ok_or_else(|| at.error(LoadErrorKind::Missing("noise")))?,
            &at.child("noise"),
        )?;
        let router = json
            .get("noise_router")
            .ok_or_else(|| at.error(LoadErrorKind::Missing("noise_router")))?;
        let mut loader = Loader::new(resources);
        let mut entries = Vec::with_capacity(15);
        for entry in RouterEntry::ALL {
            let value = router.get(entry.name()).ok_or_else(|| {
                at.child("noise_router")
                    .error(LoadErrorKind::Missing(entry.name()))
            })?;
            entries.push(loader.inline(id, &format!("noise_router.{}", entry.name()), value)?);
        }
        Ok(NoiseRouter {
            entries: entries.try_into().expect("fifteen entries"),
            noise,
            legacy_random_source: flag("legacy_random_source")?,
            aquifers_enabled: flag("aquifers_enabled")?,
            ore_veins_enabled: flag("ore_veins_enabled")?,
            noises: loader.noises().clone(),
        })
    }

    /// Router entry `entry`.
    pub fn entry(&self, entry: RouterEntry) -> &Df {
        &self.entries[entry as usize]
    }

    /// The noise settings' `noise`.
    pub fn noise_settings(&self) -> NoiseSettings {
        self.noise
    }

    /// `legacy_random_source`.
    pub fn legacy_random_source(&self) -> bool {
        self.legacy_random_source
    }

    /// `aquifers_enabled`.
    pub fn aquifers_enabled(&self) -> bool {
        self.aquifers_enabled
    }

    /// `ore_veins_enabled`.
    pub fn ore_veins_enabled(&self) -> bool {
        self.ore_veins_enabled
    }

    /// The noises the entries use, by full id.
    pub fn noises(&self) -> &HashMap<String, NormalNoiseParameters> {
        &self.noises
    }

    /// `RandomState.create(settings, noises, seed)`: the router wired to `seed`, as a point
    /// evaluates it, as a `NoiseChunk` wraps it, and as the climate sampler strips it.
    pub fn bind(&self, seed: i64) -> Result<RandomState, LoadError> {
        let mut binder = Binder::new(&self.noises, seed, self.legacy_random_source);
        let bind_all = |binder: &mut Binder<'_>, wiring| -> Result<[Function; 15], LoadError> {
            let v = self
                .entries
                .iter()
                .map(|f| binder.bind_with(f, wiring))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(v.try_into().expect("fifteen entries"))
        };
        let plain = bind_all(&mut binder, Wiring::Plain)?;
        let chunk = bind_all(&mut binder, Wiring::Chunk)?;
        let climate = CLIMATE.map(|e| binder.bind_with(&self.entries[e as usize], Wiring::Climate));
        let [t, h, c, e, d, w] = climate;
        // cacheAllInCell(add(finalDensity, beardifier)): an Ap2 add of the beardifier's 0.
        let density = Function::add_zero(&chunk[RouterEntry::FinalDensity as usize]);
        Ok(RandomState {
            plain,
            climate: ClimateSampler([t?, h?, c?, e?, d?, w?]),
            chunk: Arc::new(ChunkGraph {
                binder: binder.id(),
                slots: binder.slots(),
                entries: chunk,
                density,
            }),
            noise: self.noise,
        })
    }
}

impl NoiseRouter {
    /// `RandomState.sampler()` for `seed` alone, without binding the rest of the router.
    pub fn climate_sampler(&self, seed: i64) -> Result<ClimateSampler, LoadError> {
        let mut binder = Binder::new(&self.noises, seed, self.legacy_random_source);
        let [t, h, c, e, d, w] =
            CLIMATE.map(|e| binder.bind_with(&self.entries[e as usize], Wiring::Climate));
        Ok(ClimateSampler([t?, h?, c?, e?, d?, w?]))
    }
}

/// What a `NoiseChunk` evaluates: the router as its `wrap` maps it.
#[derive(Debug)]
pub(crate) struct ChunkGraph {
    /// The binder that numbered the markers' slots.
    pub(crate) binder: u64,
    /// How many slots it numbered.
    pub(crate) slots: u32,
    pub(crate) entries: [Function; 15],
    /// `fullNoiseDensity`.
    pub(crate) density: Function,
}

/// 26.2's `RandomState` for one seed: the router wired to the seed.
#[derive(Debug, Clone)]
pub struct RandomState {
    plain: [Function; 15],
    climate: ClimateSampler,
    chunk: Arc<ChunkGraph>,
    noise: NoiseSettings,
}

impl RandomState {
    /// `router().<entry>`: what [`Function::compute`] evaluates at a point outside any
    /// `NoiseChunk`.
    pub fn entry(&self, entry: RouterEntry) -> &Function {
        &self.plain[entry as usize]
    }

    /// `sampler()`: the climate sampler, its six functions stripped of references and markers.
    pub fn sampler(&self) -> &ClimateSampler {
        &self.climate
    }

    /// The noise settings' `noise`.
    pub fn noise_settings(&self) -> NoiseSettings {
        self.noise
    }

    pub(crate) fn chunk_graph(&self) -> &Arc<ChunkGraph> {
        &self.chunk
    }
}

/// 26.2's `Climate.Sampler` from `RandomState.sampler()`: six functions evaluated in `f64` at
/// `SinglePointContext(4·qx, 4·qy, 4·qz)`.
#[derive(Debug, Clone)]
pub struct ClimateSampler([Function; 6]);

impl ClimateSampler {
    /// The six values at quart `(qx, qy, qz)`, in `f64`, before `Climate.Sampler.sample`
    /// narrows them.
    pub fn sample_f64(&self, qx: i32, qy: i32, qz: i32) -> [f64; 6] {
        let (x, y, z) = (qx.wrapping_mul(4), qy.wrapping_mul(4), qz.wrapping_mul(4));
        std::array::from_fn(|i| self.0[i].compute(x, y, z))
    }

    /// `Climate.Sampler.sample`'s values, each `(float)` of its `f64`.
    pub fn sample(&self, qx: i32, qy: i32, qz: i32) -> [f32; 6] {
        self.sample_f64(qx, qy, qz).map(|v| v as f32)
    }

    /// The six functions, in temperature, humidity, continentalness, erosion, depth, weirdness
    /// order.
    pub fn functions(&self) -> &[Function; 6] {
        &self.0
    }
}
