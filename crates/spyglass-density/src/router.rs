//! The noise router's climate entries (`NoiseRouter`) and the climate sampler built from them
//! (`NoiseRouter.createClimateSampler`, `Climate.Sampler`).

use std::collections::HashMap;

use spyglass_noise::NormalNoiseParameters;

use crate::error::{LoadError, LoadErrorKind};
use crate::function::Df;
use crate::load::{Loader, Resources, full_id};
use crate::sampler::{Binder, Sampler};

/// The registry path of noise settings.
const NOISE_SETTINGS: &str = "worldgen/noise_settings";

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
}

impl ClimateRouter {
    /// Loads `noise_settings` (e.g. `minecraft:overworld`) and the six climate entries of its
    /// `noise_router`. Settings with `legacy_random_source: true` aren't supported yet.
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let error = |path: &str, kind| LoadError {
            resource: id.clone(),
            path: path.to_string(),
            kind: Box::new(kind),
        };
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| error("", e.into()))?;
        if json.get("legacy_random_source").and_then(|v| v.as_bool()) == Some(true) {
            return Err(error(
                "legacy_random_source",
                LoadErrorKind::Unsupported {
                    what: "random source",
                    name: "legacy".to_string(),
                },
            ));
        }
        let router = json
            .get("noise_router")
            .ok_or_else(|| error("", LoadErrorKind::Missing("noise_router")))?;
        let mut loader = Loader::new(resources);
        let mut functions = Vec::with_capacity(6);
        for entry in CLIMATE_ENTRIES {
            let path = format!("noise_router.{entry}");
            let value = router
                .get(entry)
                .ok_or_else(|| error("noise_router", LoadErrorKind::Missing(entry)))?;
            // Router entries are references in the vanilla data; inline functions are named
            // after the settings for error messages.
            let f = match value.as_str() {
                Some(name) => loader.function(name)?,
                None => {
                    return Err(error(
                        &path,
                        LoadErrorKind::Shape("a density function reference"),
                    ));
                }
            };
            functions.push(f);
        }
        Ok(ClimateRouter {
            functions: functions.try_into().expect("six entries"),
            noises: loader.noises().clone(),
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

    /// The climate sampler for `seed`.
    pub fn sampler(&self, seed: i64) -> Result<ClimateSampler, LoadError> {
        let mut binder = Binder::new(&self.noises, seed);
        let mut bound = Vec::with_capacity(6);
        for f in &self.functions {
            bound.push(binder.bind(f)?);
        }
        Ok(ClimateSampler(bound.try_into().expect("six samplers")))
    }
}

/// `Climate.Sampler` for one seed.
#[derive(Debug, Clone)]
pub struct ClimateSampler([Sampler; 6]);

impl ClimateSampler {
    /// The six climate values at quart `(qx, qy, qz)`, sampled at block `4 * q` (`QuartPos.toBlock`),
    /// in temperature, humidity, continentalness, erosion, depth, weirdness order.
    pub fn sample(&self, qx: i32, qy: i32, qz: i32) -> [f32; 6] {
        let (x, y, z) = (qx.wrapping_mul(4), qy.wrapping_mul(4), qz.wrapping_mul(4));
        std::array::from_fn(|i| self.0[i].sample(x, y, z))
    }

    /// This sampler for the column at quart `(qx, qz)`, with every y-independent subtree
    /// evaluated once ([`Sampler::at_column`]). It gives the same values as this sampler, but
    /// only at this `qx` and `qz`.
    pub fn at_column(&self, qx: i32, qz: i32) -> ClimateSampler {
        let (x, z) = (qx.wrapping_mul(4), qz.wrapping_mul(4));
        ClimateSampler(std::array::from_fn(|i| self.0[i].at_column(x, z)))
    }

    /// One climate function, by [`CLIMATE_ENTRIES`] index.
    pub fn function(&self, i: usize) -> &Sampler {
        &self.0[i]
    }
}
