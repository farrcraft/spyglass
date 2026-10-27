//! The noise router's climate entries (`NoiseRouter`) and the climate sampler built from them
//! (`NoiseRouter.createClimateSampler`, `Climate.Sampler`), for either algorithm family: the
//! noise settings' shape says which (ADR 0026).

use std::collections::HashMap;

use spyglass_noise::NormalNoiseParameters;

use crate::function::Df;
use crate::load::{Loader, Resources};
use crate::sampler::{Binder, Sampler};
use crate::v1_21;
use spyglass_data::anchor::AnchorHeights;
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

/// The climate half of a dimension's noise router, loaded from its noise settings: 26.3's six
/// entries, or 26.2's whole router (the 1.21 family, [`v1_21`]).
#[derive(Debug, Clone)]
pub struct ClimateRouter(Family);

#[derive(Debug, Clone)]
enum Family {
    V26_3 {
        functions: [Df; 6],
        noises: HashMap<String, NormalNoiseParameters>,
        /// `legacy_random_source`.
        legacy: bool,
        /// What vertical anchors resolve against: `noise.min_y` and `noise.height`, narrowed by
        /// the level if [`within_level`](ClimateRouter::within_level) was called.
        anchors: Option<AnchorHeights>,
    },
    V1_21(Box<v1_21::NoiseRouter>),
}

impl ClimateRouter {
    /// Loads `noise_settings` (e.g. `minecraft:overworld`) and the six climate entries of its
    /// `noise_router`, and which random source binding uses (`legacy_random_source`). Noise
    /// settings whose `noise` has `size_horizontal` are 26.2's, and load as its whole router
    /// ([`v1_21::NoiseRouter`]).
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let error = |path: &str, kind| LoadError::new(&id, path, kind);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| error("", e.into()))?;
        if v1_21::is_v1_21(&json) {
            let router = v1_21::NoiseRouter::from_json(resources, &id, &json)?;
            return Ok(ClimateRouter(Family::V1_21(Box::new(router))));
        }
        let legacy = match json.get("legacy_random_source") {
            None => false,
            Some(v) => v.as_bool().ok_or_else(|| {
                error(
                    "legacy_random_source",
                    LoadErrorKind::Invalid("expected true or false"),
                )
            })?,
        };
        let anchors = AnchorHeights::of_noise_settings(&json, &id)?;
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
        Ok(ClimateRouter(Family::V26_3 {
            functions: [t?, h?, c?, e?, d?, w?],
            noises: loader.noises().clone(),
            legacy,
            anchors,
        }))
    }

    /// The router as a level of `level`'s heights binds it (`RandomState.createForGenerator`):
    /// anchors resolve within both the level and the noise settings
    /// ([`AnchorHeights::within_level`]). Without it, they resolve against the noise settings'
    /// heights alone.
    pub fn within_level(mut self, level: AnchorHeights) -> Self {
        if let Family::V26_3 {
            anchors: Some(anchors),
            ..
        } = &mut self.0
        {
            *anchors = AnchorHeights::within_level(*anchors, level);
        }
        self
    }

    /// The six climate functions, in [`CLIMATE_ENTRIES`] order, if they're 26.3's family.
    pub fn functions(&self) -> Option<&[Df; 6]> {
        match &self.0 {
            Family::V26_3 { functions, .. } => Some(functions),
            Family::V1_21(_) => None,
        }
    }

    /// The 26.2 router, if the noise settings are the 1.21 family's.
    pub fn v1_21(&self) -> Option<&v1_21::NoiseRouter> {
        match &self.0 {
            Family::V26_3 { .. } => None,
            Family::V1_21(router) => Some(router),
        }
    }

    /// The noises the functions use, by full id.
    pub fn noises(&self) -> &HashMap<String, NormalNoiseParameters> {
        match &self.0 {
            Family::V26_3 { noises, .. } => noises,
            Family::V1_21(router) => router.noises(),
        }
    }

    /// Whether binding uses the legacy random source (`legacy_random_source`).
    pub fn legacy_random(&self) -> bool {
        match &self.0 {
            Family::V26_3 { legacy, .. } => *legacy,
            Family::V1_21(router) => router.legacy_random_source(),
        }
    }

    /// The climate sampler for `seed`.
    pub fn sampler(&self, seed: i64) -> Result<ClimateSampler, LoadError> {
        match &self.0 {
            Family::V26_3 {
                functions,
                noises,
                legacy,
                anchors,
            } => {
                let mut binder = Binder::with_legacy_random(noises, seed, *legacy);
                if let Some(anchors) = anchors {
                    binder = binder.with_anchors(*anchors);
                }
                let [t, h, c, e, d, w] = functions.each_ref().map(|f| binder.bind(f));
                Ok(ClimateSampler(Sampler6::V26_3([t?, h?, c?, e?, d?, w?])))
            }
            Family::V1_21(router) => Ok(ClimateSampler(Sampler6::V1_21(
                router.climate_sampler(seed)?,
            ))),
        }
    }
}

/// `Climate.Sampler` for one seed.
#[derive(Debug, Clone)]
pub struct ClimateSampler(Sampler6);

#[derive(Debug, Clone)]
enum Sampler6 {
    V26_3([Sampler; 6]),
    V1_21(v1_21::ClimateSampler),
}

impl ClimateSampler {
    /// The six climate values at quart `(qx, qy, qz)`, sampled at block `4 * q` (`QuartPos.toBlock`),
    /// in temperature, humidity, continentalness, erosion, depth, weirdness order. In 26.3's
    /// family the six share their common subtrees, and sampling a column in sequence computes
    /// each y-independent subtree once ([`Sampler::sample`]). In 26.2's each is computed in
    /// `f64` and narrowed ([`v1_21::ClimateSampler::sample`]).
    pub fn sample(&self, qx: i32, qy: i32, qz: i32) -> [f32; 6] {
        match &self.0 {
            Sampler6::V26_3(samplers) => {
                let (x, y, z) = (qx.wrapping_mul(4), qy.wrapping_mul(4), qz.wrapping_mul(4));
                std::array::from_fn(|i| samplers[i].sample(x, y, z))
            }
            Sampler6::V1_21(sampler) => sampler.sample(qx, qy, qz),
        }
    }

    /// The 26.2 sampler, if the router is the 1.21 family's: its `f64` values before narrowing.
    pub fn v1_21(&self) -> Option<&v1_21::ClimateSampler> {
        match &self.0 {
            Sampler6::V26_3(_) => None,
            Sampler6::V1_21(sampler) => Some(sampler),
        }
    }
}
