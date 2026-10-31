//! A biome's temperature at a block, as `Biome` computes it in 26.3 and in 26.2: the climate's base
//! temperature, the `frozen` modifier's patches, and cooling above the snow line
//! (`docs/specs/material.md#temperature`).

use serde_json::Value;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, field, float, full_id, object};
use spyglass_density::Resources;
use spyglass_noise::v1_21::PerlinSimplexNoise;
use spyglass_noise::{Layer, NoiseStack, SimplexNoise};
use spyglass_rng::LegacyRandomSource;

/// A biome's `ClimateSettings`, as far as temperature goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Climate {
    /// `temperature`.
    pub temperature: f32,
    /// `temperature_modifier` is `frozen` (otherwise `none`).
    pub frozen: bool,
}

impl Climate {
    /// The climate of biome `id`, from its `worldgen/biome` entry.
    pub fn load<R: Resources + ?Sized>(resources: &R, id: &str) -> Result<Self, LoadError> {
        let id = full_id(id);
        let root = At::root(&id);
        let json = resources
            .resource("worldgen/biome", &id)
            .map_err(|e| root.error(e.into()))?;
        let biome = object(&json, &root)?;
        let temperature = float(
            field(biome, "temperature", &root)?,
            &root.child("temperature"),
        )?;
        let at = root.child("temperature_modifier");
        let frozen = match biome.get("temperature_modifier") {
            None => false,
            Some(Value::String(s)) if s == "none" => false,
            Some(Value::String(s)) if s == "frozen" => true,
            Some(_) => return Err(at.error(LoadErrorKind::Shape("none or frozen"))),
        };
        Ok(Climate {
            temperature,
            frozen,
        })
    }
}

/// `Biome`'s three temperature noises, which don't depend on the world seed.
#[derive(Debug, Clone)]
pub struct Temperature(Noises);

/// The noises, by family (ADR 0026).
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "one per seed, never moved in a loop"
)]
enum Noises {
    /// 26.3 on: simplex noise in `f32`.
    Simplex {
        /// `TEMPERATURE_NOISE`: simplex, legacy random 1234.
        noise: SimplexNoise,
        /// `FROZEN_TEMPERATURE_NOISE`: three simplex layers from legacy random 3456.
        frozen: NoiseStack<SimplexNoise>,
        /// `BIOME_INFO_NOISE`: simplex, legacy random 2345.
        info: SimplexNoise,
    },
    /// 26.2: `PerlinSimplexNoise` in `f64`.
    V1_21 {
        /// `TEMPERATURE_NOISE`: octave 0, legacy random 1234.
        noise: PerlinSimplexNoise,
        /// `FROZEN_TEMPERATURE_NOISE`: octaves −2 to 0, legacy random 3456.
        frozen: PerlinSimplexNoise,
        /// `BIOME_INFO_NOISE`: octave 0, legacy random 2345.
        info: PerlinSimplexNoise,
    },
}

impl Default for Temperature {
    fn default() -> Self {
        Self::new()
    }
}

impl Temperature {
    /// 26.3's noises, as `Biome`'s static initializer makes them.
    pub fn new() -> Self {
        let simplex = |seed| SimplexNoise::new(&mut LegacyRandomSource::new(seed), true);
        let mut random = LegacyRandomSource::new(3456);
        let layers = [(1.0, 0.142_857_15), (0.5, 0.285_714_3), (0.25, 0.571_428_6)].map(
            |(frequency, amplitude)| Layer {
                noise: SimplexNoise::new(&mut random, true),
                frequency,
                amplitude,
            },
        );
        Temperature(Noises::Simplex {
            noise: simplex(1234),
            frozen: NoiseStack::simplex(layers.into()),
            info: simplex(2345),
        })
    }

    /// 26.2's noises, as its `Biome`'s static initializer makes them.
    pub fn v1_21() -> Self {
        let noise = |seed, octaves: &[i32]| {
            PerlinSimplexNoise::new(&mut LegacyRandomSource::new(seed), octaves)
        };
        Temperature(Noises::V1_21 {
            noise: noise(1234, &[0]),
            frozen: noise(3456, &[-2, -1, 0]),
            info: noise(2345, &[0]),
        })
    }

    /// `Biome.BIOME_INFO_NOISE` at `(x, z)`, as the noise-count placement modifiers sample it:
    /// `getValue(x, z, false)` in `f64` in 26.2, `get(x, z)` in `f32` from 26.3, widened.
    pub fn biome_info(&self, x: f64, z: f64) -> f64 {
        match &self.0 {
            Noises::Simplex { info, .. } => f64::from(info.get(x, z)),
            Noises::V1_21 { info, .. } => info.get_value(x, z),
        }
    }

    /// `TemperatureModifier.modifyTemperature`: `frozen` gives 0.2 in patches where the frozen
    /// and info noises are low.
    fn modified(&self, climate: Climate, x: i32, z: i32) -> f32 {
        if !climate.frozen {
            return climate.temperature;
        }
        let (x, z) = (f64::from(x), f64::from(z));
        // Each family's frozen and info noise, and the info noise for the patch test, which is
        // sampled only if the first test holds.
        let (freeze, info) = match &self.0 {
            Noises::Simplex { frozen, info, .. } => (
                f64::from(frozen.get_2d(x * 0.05, z * 0.05) * 7.0),
                f64::from(info.get(x * 0.2, z * 0.2)),
            ),
            Noises::V1_21 { frozen, info, .. } => (
                frozen.get_value(x * 0.05, z * 0.05) * 7.0,
                info.get_value(x * 0.2, z * 0.2),
            ),
        };
        let patch = || match &self.0 {
            Noises::Simplex { info, .. } => f64::from(info.get(x * 0.09, z * 0.09)),
            Noises::V1_21 { info, .. } => info.get_value(x * 0.09, z * 0.09),
        };
        // dcmpg/ifge: NaN keeps the base temperature.
        if freeze + info < 0.3 && patch() < 0.8 {
            return 0.2;
        }
        climate.temperature
    }

    /// `getHeightAdjustedTemperature` (`getTemperature` without its cache): above
    /// `sea_level + 17`, cooler by `(v + y − snow line) · 0.05 / 40`, with `v` the temperature
    /// noise times 8 (in `f32` from 26.3, narrowed from `f64` in 26.2).
    pub fn at(&self, climate: Climate, [x, y, z]: [i32; 3], sea_level: i32) -> f32 {
        let t = self.modified(climate, x, z);
        let snow_line = sea_level.wrapping_add(17);
        if y <= snow_line {
            return t;
        }
        let (nx, nz) = (f64::from(x as f32 / 8.0), f64::from(z as f32 / 8.0));
        let v = match &self.0 {
            Noises::Simplex { noise, .. } => noise.get(nx, nz) * 8.0,
            Noises::V1_21 { noise, .. } => (noise.get_value(nx, nz) * 8.0) as f32,
        };
        t - (v + y as f32 - snow_line as f32) * 0.05 / 40.0
    }

    /// `coldEnoughToSnow`: not `warmEnoughToRain` (at least 0.15). NaN is cold.
    pub fn cold_enough_to_snow(&self, climate: Climate, pos: [i32; 3], sea_level: i32) -> bool {
        // fcmpl/iflt: NaN isn't warm enough to rain.
        let t = self.at(climate, pos, sea_level);
        t.is_nan() || t < 0.15
    }

    /// `shouldMeltFrozenOceanIcebergSlightly`: warmer than 0.1.
    pub fn melts_icebergs(&self, climate: Climate, pos: [i32; 3], sea_level: i32) -> bool {
        self.at(climate, pos, sea_level) > 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_patches_and_the_snow_line() {
        for t in [Temperature::new(), Temperature::v1_21()] {
            let plain = Climate {
                temperature: 0.8,
                frozen: false,
            };
            // No modifier, at or below the snow line: the base temperature.
            assert_eq!(t.at(plain, [100, 80, -5], 63), 0.8);
            // Above it, cooler with height.
            assert!(t.at(plain, [100, 200, -5], 63) < t.at(plain, [100, 120, -5], 63));
            // The frozen modifier gives 0.2 or the base temperature, nothing else.
            let frozen = Climate {
                temperature: 0.5,
                frozen: true,
            };
            let mut seen = [false; 2];
            for x in (-4000..4000).step_by(37) {
                let v = t.at(frozen, [x, 63, x / 3], 63);
                assert!(v == 0.2 || v == 0.5, "{v}");
                seen[usize::from(v == 0.2)] = true;
            }
            assert_eq!(seen, [true, true]);
        }
    }
}
