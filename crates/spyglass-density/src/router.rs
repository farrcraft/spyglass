//! The noise router's climate entries (`NoiseRouter`) and the climate sampler built from them
//! (`NoiseRouter.createClimateSampler`, `Climate.Sampler`).

use std::collections::HashMap;

use spyglass_noise::NormalNoiseParameters;

use crate::function::Df;
use crate::load::{Loader, Resources};
use crate::sampler::{Binder, Sampler};
use spyglass_data::codec::{LoadError, LoadErrorKind, full_id};

/// The registry path of noise settings.
pub const NOISE_SETTINGS: &str = "worldgen/noise_settings";

/// The router entries a climate sampler uses, in `Climate.Sampler`'s order, with the parameter
/// each feeds: `vegetation` is humidity and `ridges` is weirdness.
pub const CLIMATE_ENTRIES: [&str; 6] = [
    "temperature",
    "vegetation",
    "continents",
    "erosion",
    "depth",
    "ridges",
];

/// The climate half of a dimension's noise router, loaded from its noise settings.
#[derive(Debug, Clone)]
pub struct ClimateRouter {
    functions: [Df; 6],
    noises: HashMap<String, NormalNoiseParameters>,
    /// `legacy_random_source`.
    legacy: bool,
}

impl ClimateRouter {
    /// Loads `noise_settings` (e.g. `minecraft:overworld`) and the six climate entries of its
    /// `noise_router`, and which random source binding uses (`legacy_random_source`).
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let error = |path: &str, kind| LoadError::new(&id, path, kind);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| error("", e.into()))?;
        let legacy = match json.get("legacy_random_source") {
            None => false,
            Some(v) => v.as_bool().ok_or_else(|| {
                error(
                    "legacy_random_source",
                    LoadErrorKind::Invalid("expected true or false"),
                )
            })?,
        };
        let router = json
            .get("noise_router")
            .ok_or_else(|| error("", LoadErrorKind::Missing("noise_router")))?;
        let mut loader = Loader::new(resources);
        let [t, h, c, e, d, w] = CLIMATE_ENTRIES.map(|entry| {
            let value = router
                .get(entry)
                .ok_or_else(|| error("noise_router", LoadErrorKind::Missing(entry)))?;
            // References in the vanilla data; data packs may write functions inline.
            loader.inline(&id, &format!("noise_router.{entry}"), value)
        });
        Ok(ClimateRouter {
            functions: [t?, h?, c?, e?, d?, w?],
            noises: loader.noises().clone(),
            legacy,
        })
    }

    /// The six climate functions, in [`CLIMATE_ENTRIES`] order.
    pub fn functions(&self) -> &[Df; 6] {
        &self.functions
    }

    /// The noises the functions use, by full id.
    pub fn noises(&self) -> &HashMap<String, NormalNoiseParameters> {
        &self.noises
    }

    /// Whether binding uses the legacy random source (`legacy_random_source`).
    pub fn legacy_random(&self) -> bool {
        self.legacy
    }

    /// The climate sampler for `seed`.
    pub fn sampler(&self, seed: i64) -> Result<ClimateSampler, LoadError> {
        let mut binder = Binder::with_legacy_random(&self.noises, seed, self.legacy);
        let [t, h, c, e, d, w] = self.functions.each_ref().map(|f| binder.bind(f));
        Ok(ClimateSampler([t?, h?, c?, e?, d?, w?]))
    }
}

/// `Climate.Sampler` for one seed.
#[derive(Debug, Clone)]
pub struct ClimateSampler([Sampler; 6]);

impl ClimateSampler {
    /// The six climate values at quart `(qx, qy, qz)`, sampled at block `4 * q` (`QuartPos.toBlock`),
    /// in temperature, humidity, continentalness, erosion, depth, weirdness order. The six share
    /// their common subtrees, and sampling a column in sequence computes each y-independent
    /// subtree once ([`Sampler::sample`]).
    pub fn sample(&self, qx: i32, qy: i32, qz: i32) -> [f32; 6] {
        let (x, y, z) = (qx.wrapping_mul(4), qy.wrapping_mul(4), qz.wrapping_mul(4));
        std::array::from_fn(|i| self.0[i].sample(x, y, z))
    }
}
