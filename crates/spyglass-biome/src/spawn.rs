//! The world spawn's climate search (`NoiseSpawnFinder`) over a dimension's spawn targets
//! (`NoiseGeneratorSettings.spawnTarget`): the chunk the game's spawn search starts from. See
//! `docs/specs/spawn.md`.

use std::collections::{HashMap, HashSet};

use spyglass_data::codec::{LoadError, LoadErrorKind, full_id};
use spyglass_density::{Binder, Df, Loader, NOISE_SETTINGS, Resources, Sampler};
use spyglass_noise::NormalNoiseParameters;

use crate::parameters::{Parameter, quantize};

/// `Mth.square(2048L)`: how much one unit of climate fitness outweighs squared distance.
const FITNESS_WEIGHT: i64 = 2048 * 2048;

/// A dimension's spawn targets, independent of the seed. Each is a `SpawnTargetPoint`: density
/// functions, each with the range its value should be in.
#[derive(Debug, Clone)]
pub struct SpawnTargets {
    points: Vec<Vec<(Df, Parameter)>>,
    noises: HashMap<String, NormalNoiseParameters>,
}

impl SpawnTargets {
    /// The `spawn_target` list of `noise_settings` (e.g. `minecraft:overworld`): maps from
    /// density-function ids to parameters (`SpawnTargetPoint.CODEC`), read with
    /// [`Parameter::from_codec`].
    pub fn load<R: Resources + ?Sized>(
        resources: &R,
        noise_settings: &str,
    ) -> Result<Self, LoadError> {
        let id = full_id(noise_settings);
        let error = |path: String, kind| LoadError::new(&id, path, kind);
        let json = resources
            .resource(NOISE_SETTINGS, &id)
            .map_err(|e| error(String::new(), e.into()))?;
        let list = json
            .get("spawn_target")
            .ok_or_else(|| error(String::new(), LoadErrorKind::Missing("spawn_target")))?
            .as_array()
            .ok_or_else(|| {
                error(
                    "spawn_target".into(),
                    LoadErrorKind::Shape("a list of spawn targets"),
                )
            })?;
        let mut loader = Loader::new(resources);
        let mut points = Vec::with_capacity(list.len());
        for (i, point) in list.iter().enumerate() {
            let at = format!("spawn_target[{i}]");
            let map = point.as_object().ok_or_else(|| {
                error(
                    at.clone(),
                    LoadErrorKind::Shape("a map from density functions to parameters"),
                )
            })?;
            // The keys are registry holders, so two spellings of one id are a duplicate key.
            let mut seen = HashSet::new();
            let mut parameters = Vec::with_capacity(map.len());
            for (function, range) in map {
                let path = format!("{at}.{function}");
                if !seen.insert(full_id(function)) {
                    return Err(error(path, LoadErrorKind::Invalid("duplicate key")));
                }
                let parameter = Parameter::from_codec(range).map_err(|kind| error(path, kind))?;
                parameters.push((loader.function(function)?, parameter));
            }
            points.push(parameters);
        }
        Ok(SpawnTargets {
            points,
            noises: loader.noises().clone(),
        })
    }

    /// Whether there are no targets, so the search doesn't run.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The search for `seed`.
    pub fn with_seed(&self, seed: i64) -> Result<SpawnSearch, LoadError> {
        let mut binder = Binder::new(&self.noises, seed);
        let points = self
            .points
            .iter()
            .map(|point| {
                point
                    .iter()
                    .map(|(f, range)| Ok((binder.bind(f)?, *range)))
                    .collect()
            })
            .collect::<Result<_, LoadError>>()?;
        Ok(SpawnSearch { points })
    }
}

/// `NoiseSpawnFinder.Result`: a block position, at y 0, and its fitness; lower is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnPoint {
    /// Block x and z.
    pub position: [i32; 2],
    /// Climate fitness times 2048², plus the squared distance from (0, 0).
    pub fitness: i64,
}

/// Where the game's world-spawn search starts, for one seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnOrigin {
    /// `ChunkGenerator.getOrigin`: the chunk containing [`found`](Self::found), or (0, 0)
    /// without spawn targets.
    pub chunk: [i32; 2],
    /// The climate search's result.
    pub found: Option<SpawnPoint>,
    /// Whether the game's answer may differ by platform, because a sample position in the
    /// search is within a few ulps of a `Math.sin` or `Math.cos` rounding boundary and could
    /// have changed the result (ADR 0012).
    pub platform_dependent: bool,
}

/// The spawn search for one seed.
#[derive(Debug, Clone)]
pub struct SpawnSearch {
    points: Vec<Vec<(Sampler, Parameter)>>,
}

impl SpawnSearch {
    /// `NoiseSpawnFinder.getSpawnPositionAndFitness`: the climate fitness at the quart
    /// containing `(x, 0, z)` (the minimum of each target's `SpawnTargetPoint.sampleFitness`,
    /// the sum of its squared parameter distances), times 2048², plus `x² + z²`, all in wrapping
    /// `long` arithmetic. The position is `(x, z)` itself, not the quart's.
    pub fn fitness_at(&self, x: i32, z: i32) -> SpawnPoint {
        // QuartPos.toBlock(QuartPos.fromBlock(x)).
        let (qx, qz) = ((x >> 2) << 2, (z >> 2) << 2);
        let square = |v: i64| v.wrapping_mul(v);
        let climate = self
            .points
            .iter()
            .map(|point| {
                point.iter().fold(0_i64, |sum, (f, range)| {
                    let value = quantize(f.sample(qx, 0, qz));
                    sum.wrapping_add(square(range.distance(value)))
                })
            })
            .fold(i64::MAX, i64::min);
        let distance = square(i64::from(x)).wrapping_add(square(i64::from(z)));
        SpawnPoint {
            position: [x, z],
            fitness: climate.wrapping_mul(FITNESS_WEIGHT).wrapping_add(distance),
        }
    }

    /// `NoiseSpawnFinder.radialSearch`: rings of radius `step`, `2 * step`, ... up to
    /// `max_radius` around `best`, each sampled from angle 0 in steps of `step / radius`
    /// radians (`f32`) while the angle is at most 2π. A sample at
    /// `(int) (Math.sin(angle) * radius)`, `(int) (Math.cos(angle) * radius)` from the centre
    /// replaces the best if its fitness is strictly lower.
    ///
    /// Also returns whether the result may differ by platform: whether a sample whose position
    /// is within 4 ulps of a trig rounding boundary replaced the best, or would have at its other
    /// position. Otherwise every platform's search passes through the same bests. Angle 0 is
    /// exact everywhere (sine 0, cosine 1).
    pub fn radial_search(
        &self,
        best: SpawnPoint,
        max_radius: f32,
        step: f32,
    ) -> (SpawnPoint, bool) {
        let [cx, cz] = best.position;
        let mut best = best;
        let mut platform_dependent = false;
        let mut angle = 0.0_f32;
        let mut radius = step;
        while radius <= max_radius {
            let (dx, other_x) = trig_offset(angle, f64::sin, radius);
            let (dz, other_z) = trig_offset(angle, f64::cos, radius);
            let sample = self.fitness_at(cx.wrapping_add(dx), cz.wrapping_add(dz));
            let replaces = sample.fitness < best.fitness;
            if other_x.is_some() || other_z.is_some() {
                let xs = [Some(dx), other_x];
                let zs = [Some(dz), other_z];
                platform_dependent |= replaces
                    || xs.iter().flatten().any(|&x| {
                        zs.iter().flatten().any(|&z| {
                            self.fitness_at(cx.wrapping_add(x), cz.wrapping_add(z))
                                .fitness
                                < best.fitness
                        })
                    });
            }
            if replaces {
                best = sample;
            }
            angle += step / radius;
            if f64::from(angle) > std::f64::consts::TAU {
                angle = 0.0;
                radius += step;
            }
        }
        (best, platform_dependent)
    }

    /// `NoiseBasedChunkGenerator.getOrigin`: without spawn targets, chunk (0, 0); otherwise
    /// `NoiseSpawnFinder.findSpawnPosition`, the fitness at (0, 0) improved by a radial search
    /// with radius 2048 and step 512 and then one with 512 and 32, and the chunk containing it.
    pub fn origin(&self) -> SpawnOrigin {
        if self.points.is_empty() {
            return SpawnOrigin {
                chunk: [0, 0],
                found: None,
                platform_dependent: false,
            };
        }
        let (coarse, coarse_dependent) = self.radial_search(self.fitness_at(0, 0), 2048.0, 512.0);
        let (found, fine_dependent) = self.radial_search(coarse, 512.0, 32.0);
        SpawnOrigin {
            // ChunkPos.containing.
            chunk: [found.position[0] >> 4, found.position[1] >> 4],
            found: Some(found),
            platform_dependent: coarse_dependent || fine_dependent,
        }
    }
}

/// `(int) (trig(angle) * radius)`, with the angle and radius widened to `f64`, and the other
/// value it takes if `trig`'s result moves by up to 4 ulps either way (ADR 0012). At angle 0,
/// sine and cosine are exact on every platform.
fn trig_offset(angle: f32, trig: fn(f64) -> f64, radius: f32) -> (i32, Option<i32>) {
    let radius = f64::from(radius);
    let value = trig(f64::from(angle));
    // `as` truncates toward zero and saturates, as Java's d2i does.
    let answer = (value * radius) as i32;
    if angle == 0.0 {
        return (answer, None);
    }
    let (mut up, mut down) = (value, value);
    let mut other = None;
    for _ in 0..4 {
        up = up.next_up();
        down = down.next_down();
        for v in [up, down] {
            let t = (v * radius) as i32;
            if t != answer {
                other = Some(t);
            }
        }
    }
    (answer, other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    /// Noise settings `minecraft:test` with `spawn_target`, a constant `minecraft:c` (0.3) and
    /// `minecraft:gx`, which rises from 0 at x = 0 to 1 at x = 4096.
    fn resources(spawn_target: Value) -> HashMap<String, Value> {
        HashMap::from([
            (
                "worldgen/noise_settings/minecraft:test".to_string(),
                json!({"spawn_target": spawn_target}),
            ),
            (
                "worldgen/density_function/minecraft:c".to_string(),
                json!(0.3),
            ),
            (
                "worldgen/density_function/minecraft:gx".to_string(),
                json!({"type": "gradient", "axis": "x", "from_coordinate": 0, "from_value": 0,
                       "to_coordinate": 4096, "to_value": 1}),
            ),
        ])
    }

    fn search(spawn_target: Value) -> SpawnSearch {
        SpawnTargets::load(&resources(spawn_target), "test")
            .unwrap()
            .with_seed(0)
            .unwrap()
    }

    #[test]
    fn fitness_is_climate_then_distance() {
        // 0.3 is 2000 below [0.5, 1].
        let s = search(json!([{"c": [0.5, 1.0]}]));
        let climate = 2000 * 2000 * FITNESS_WEIGHT;
        assert_eq!(s.fitness_at(0, 0).fitness, climate);
        assert_eq!(s.fitness_at(-3, 4).fitness, climate + 25);
        assert_eq!(s.fitness_at(-3, 4).position, [-3, 4]);
        // The best target counts.
        let s = search(json!([{"c": [0.5, 1.0]}, {"c": 0.31}]));
        assert_eq!(s.fitness_at(1, 0).fitness, 100 * 100 * FITNESS_WEIGHT + 1);
        // Squared distances wrap as longs.
        let s = search(json!([{"c": 0.3}]));
        assert_eq!(s.fitness_at(i32::MIN, i32::MIN).fitness, i64::MIN);
    }

    #[test]
    fn fitness_samples_the_quart() {
        let s = search(json!([{"gx": 1.0}]));
        // x = 4 and x = 7 are in the same quart, sampled at x = 4; x = 3 is in the one before.
        let climate = |x: i32| s.fitness_at(x, 0).fitness - i64::from(x * x);
        assert_eq!(climate(7), climate(4));
        assert_ne!(climate(3), climate(4));
    }

    #[test]
    fn the_search_follows_the_climate() {
        // Everywhere equal: nothing beats (0, 0).
        let origin = search(json!([{"c": 0.0}])).origin();
        assert_eq!(origin.found.unwrap().position, [0, 0]);
        assert_eq!(origin.chunk, [0, 0]);
        // Wanting x >= 1024 moves the spawn there, as close to the origin as the rings allow.
        let origin = search(json!([{"gx": [0.25, 1.0]}])).origin();
        let found = origin.found.unwrap();
        assert!(found.position[0] >= 1024, "{found:?}");
        assert_eq!(found.fitness % FITNESS_WEIGHT, {
            let [x, z] = found.position.map(i64::from);
            x * x + z * z
        });
        assert_eq!(origin.chunk, found.position.map(|b| b >> 4));
        assert!(!origin.platform_dependent);
    }

    #[test]
    fn no_targets_means_chunk_zero() {
        let targets = SpawnTargets::load(&resources(json!([])), "test").unwrap();
        assert!(targets.is_empty());
        let origin = targets.with_seed(1).unwrap().origin();
        assert_eq!(origin.chunk, [0, 0]);
        assert_eq!(origin.found, None);
    }

    #[test]
    fn load_errors_name_the_path() {
        let error = |target: Value| SpawnTargets::load(&resources(target), "test").unwrap_err();
        let e = error(json!([{"c": 0.0, "minecraft:c": 0.0}]));
        assert!(matches!(*e.kind, LoadErrorKind::Invalid(_)), "{e}");
        let e = error(json!([{"c": 0.0}, {"gx": [1.0, 0.0]}]));
        assert_eq!(e.path, "spawn_target[1].gx");
        let e = error(json!([{"missing": 0.0}]));
        assert_eq!(e.resource, "minecraft:missing");
        let e = error(json!({"c": 0.0}));
        assert_eq!(e.path, "spawn_target");
        let e = SpawnTargets::load(
            &HashMap::from([(
                "worldgen/noise_settings/minecraft:test".to_string(),
                json!({}),
            )]),
            "test",
        )
        .unwrap_err();
        assert!(matches!(*e.kind, LoadErrorKind::Missing("spawn_target")));
    }

    #[test]
    fn trig_offsets_flag_boundaries() {
        // Exact at angle 0, where cos(0) * r is an integer.
        assert_eq!(trig_offset(0.0, f64::cos, 512.0), (512, None));
        assert_eq!(trig_offset(0.0, f64::sin, 512.0), (0, None));
        // 0.5 * 6 is exactly 3, so a slightly smaller sine truncates to 2.
        let sin = |_: f64| 0.5;
        assert_eq!(trig_offset(1.0, sin, 6.0), (3, Some(2)));
        let sin = |_: f64| 0.55;
        assert_eq!(trig_offset(1.0, sin, 6.0), (3, None));
        // Truncation toward zero: a tiny value either side of 0 is 0.
        let sin = |_: f64| 1e-300;
        assert_eq!(trig_offset(1.0, sin, 6.0), (0, None));
        let sin = |_: f64| -0.5;
        assert_eq!(trig_offset(1.0, sin, 6.0), (-3, Some(-2)));
    }
}
