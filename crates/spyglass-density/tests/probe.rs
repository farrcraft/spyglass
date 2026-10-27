//! Density functions and the climate sampler against the oracle's probe files, bit for bit, and
//! every density function in a real bundle loaded.
//!
//! Needs `cargo xtask oracle probe` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-density -- --ignored`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use spyglass_data::anchor::AnchorHeights;
use spyglass_data::bundle::Bundle;
use spyglass_data::codec::LoadErrorKind;
use spyglass_density::v1_21::{self, NoiseChunk, RandomState, RouterEntry};
use spyglass_density::{Binder, ClimateRouter, Loader, Sampler, Volume};
use spyglass_oracle::probe::{ProbeFile, probe_file};

/// The bundle for the version under test (`spyglass_oracle::version`).
fn bundle() -> Bundle {
    let version = spyglass_oracle::version();
    Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
}

/// `Climate.quantizeCoord`: `(long)(f * 10000.0f)`.
fn quantize(f: f32) -> i64 {
    (f * 10000.0) as i64
}

/// A dimension of the Default preset's noise settings.
fn noise_settings(dimension: &str) -> &'static str {
    match dimension {
        "minecraft:overworld" => "minecraft:overworld",
        "minecraft:the_nether" => "minecraft:nether",
        "minecraft:the_end" => "minecraft:end",
        other => panic!("unknown dimension {other}"),
    }
}

/// What a dimension's cases bind with: its climate router, and its random source.
struct Dimension {
    router: ClimateRouter,
    settings: serde_json::Value,
    id: &'static str,
}

impl Dimension {
    fn load(bundle: &Bundle, dimension: &str) -> Dimension {
        let id = noise_settings(dimension);
        Dimension {
            router: ClimateRouter::load(bundle, id).unwrap(),
            settings: bundle
                .resource(
                    "worldgen/noise_settings",
                    id.trim_start_matches("minecraft:"),
                )
                .unwrap(),
            id,
        }
    }

    /// Function `name`: a named function, or `noise_router.<entry>` from the noise settings.
    fn function(&self, loader: &mut Loader<'_, Bundle>, name: &str) -> spyglass_density::Df {
        match name.strip_prefix("noise_router.") {
            Some(entry) => loader
                .inline(self.id, name, &self.settings["noise_router"][entry])
                .unwrap(),
            None => loader.function(name).unwrap(),
        }
    }
}

/// Whether the bundle's routers are the 1.21 family's (26.2), by the noise settings' shape.
fn is_v1_21(bundle: &Bundle) -> bool {
    v1_21::is_v1_21(
        &bundle
            .resource("worldgen/noise_settings", "overworld")
            .unwrap(),
    )
}

#[test]
#[ignore = "needs the oracle's probe files and a bundle: cargo xtask oracle probe"]
fn every_density_value_matches_the_game() {
    let path = probe_file("density.json");
    let file = ProbeFile::read(&path).unwrap();
    let bundle = bundle();
    if is_v1_21(&bundle) {
        return replay_v1_21(&path, &file, &bundle);
    }
    let mut loader = Loader::new(&bundle);
    let mut dimensions: HashMap<String, Dimension> = HashMap::new();
    // None for functions spyglass can't bind yet, which are skipped and listed.
    let mut samplers: HashMap<(String, i64, String), Option<Sampler>> = HashMap::new();
    let mut unsupported = BTreeSet::new();
    let mut climates = HashMap::new();
    let mut failures = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for case in &file.cases {
        let seed = case.seed.expect("density cases have a seed");
        let [x, y, z] = case.pos.expect("a position");
        let dimension = case
            .dimension
            .clone()
            .unwrap_or_else(|| "minecraft:overworld".to_string());
        *counts.entry(dimension.clone()).or_default() += 1;
        let dim = dimensions
            .entry(dimension.clone())
            .or_insert_with(|| Dimension::load(&bundle, &dimension));
        let mut sampler_for = |id: &str| {
            samplers
                .entry((dimension.clone(), seed, id.to_string()))
                .or_insert_with(|| {
                    let f = dim.function(&mut loader, id);
                    let legacy = dim.router.legacy_random();
                    // The harness's RandomState.createForTesting: anchors from the noise
                    // settings' own heights.
                    let anchors = AnchorHeights::of_noise_settings(&dim.settings, dim.id)
                        .unwrap()
                        .expect("noise settings have heights");
                    let mut binder = Binder::with_legacy_random(loader.noises(), seed, legacy)
                        .with_anchors(anchors);
                    match binder.bind(&f) {
                        Ok(s) => Some(s),
                        Err(e) if matches!(*e.kind, LoadErrorKind::Unsupported { .. }) => {
                            unsupported.insert(format!("{id}: {e}"));
                            None
                        }
                        Err(e) => panic!("{id}: {e}"),
                    }
                })
                .clone()
        };
        let failure = match case.test.as_str() {
            "density" => {
                let id = case.name.clone().expect("a function id");
                let sampler = sampler_for(&id);
                let Some(sampler) = sampler else { continue };
                let (want, got) = (case.f32_bits()[0], sampler.sample(x, y, z).to_bits());
                (want != got).then(|| {
                    format!(
                        "{}\n    game:     {}\n    spyglass: {}",
                        case.label(),
                        f32::from_bits(want),
                        f32::from_bits(got)
                    )
                })
            }
            "climate" => {
                let climate = climates
                    .entry((dimension.clone(), seed))
                    .or_insert_with(|| dim.router.sampler(seed).unwrap());
                let floats = climate.sample(x, y, z);
                match case.method.as_deref() {
                    Some("floats") => {
                        let got: Vec<u32> = floats.iter().map(|f| f.to_bits()).collect();
                        (case.f32_bits() != got).then(|| {
                            format!(
                                "{}\n    game:     {:?}\n    spyglass: {floats:?}",
                                case.label(),
                                case.f32_bits()
                                    .iter()
                                    .map(|b| f32::from_bits(*b))
                                    .collect::<Vec<_>>()
                            )
                        })
                    }
                    Some("target") => {
                        let got: Vec<i64> = floats.iter().map(|f| quantize(*f)).collect();
                        (case.i64s() != got).then(|| {
                            format!(
                                "{}\n    game:     {:?}\n    spyglass: {got:?}",
                                case.label(),
                                case.i64s()
                            )
                        })
                    }
                    other => panic!("{}: unknown method {other:?}", case.label()),
                }
            }
            "climate_digest" => {
                let climate = climates
                    .entry((dimension.clone(), seed))
                    .or_insert_with(|| dim.router.sampler(seed).unwrap());
                // The chunk at (x, z), the dimension's full height in quarts (from y and
                // the height where the case gives them, else the overworld's -16..80),
                // hashed as the harness does: h = h * 1000003 + bits, y outermost, then z,
                // then x.
                let (qx0, qz0) = (x * 4, z * 4);
                let quarts = match case.height {
                    Some(height) => y..y + height,
                    None => -16..80,
                };
                let mut digest: i64 = 1;
                for qy in quarts {
                    for qz in qz0..qz0 + 4 {
                        for qx in qx0..qx0 + 4 {
                            for v in climate.sample(qx, qy, qz) {
                                digest = digest
                                    .wrapping_mul(1_000_003)
                                    .wrapping_add(i64::from(v.to_bits()));
                            }
                        }
                    }
                }
                (case.i64s() != [digest]).then(|| format!("{}: chunk digest differs", case.label()))
            }
            // The volume path, as the generator's columns and chunks use it: every value of
            // the volume, hashed in its index order as the harness does.
            "density_column" | "density_column_values" | "density_chunk" => {
                let id = case.name.clone().expect("a function id");
                let sampler = sampler_for(&id);
                let Some(sampler) = sampler else { continue };
                let height = case.height.expect("a height");
                let width = if case.test == "density_chunk" { 16 } else { 1 };
                let volume = Volume::blocks([width, height, width], [x, y, z]);
                let values = sampler.sample_volume(&volume);
                if case.test == "density_column_values" {
                    let want = case.f32_bits();
                    let got: Vec<u32> = values.iter().map(|v| v.to_bits()).collect();
                    want.iter().zip(&got).position(|(w, g)| w != g).map(|i| {
                        format!(
                            "{} at y {}\n    game:     {}\n    spyglass: {}",
                            case.label(),
                            y + i as i32,
                            f32::from_bits(want[i]),
                            f32::from_bits(got[i])
                        )
                    })
                } else {
                    let digest = values.iter().fold(1_i64, |h, v| {
                        h.wrapping_mul(1_000_003)
                            .wrapping_add(i64::from(v.to_bits()))
                    });
                    (case.i64s() != [digest])
                        .then(|| format!("{}: volume digest differs", case.label()))
                }
            }
            // A fact about the game, not spyglass: if its volume path ever stops matching its
            // point path, porting only the point path is no longer enough.
            "climate_volume_vs_point" => (case.i64s() != [0]).then(|| {
                format!(
                    "{}: the game's volume and point paths differ in {} cells",
                    case.label(),
                    case.i64s()[0]
                )
            }),
            other => panic!("{}: unknown density test {other}", case.label()),
        };
        failures.extend(failure);
    }
    assert!(
        failures.is_empty(),
        "{}: {} of {} cases differ:\n{}",
        path.display(),
        failures.len(),
        file.cases.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    println!(
        "{}: all {} cases match ({counts:?}), but for {} functions that can't be bound yet:",
        path.display(),
        file.cases.len(),
        unsupported.len()
    );
    for u in &unsupported {
        println!("  {u}");
    }
}

/// The `h = h·1000003 + bits` digest the harness computes, over `f64` bits.
fn digest_f64(values: impl IntoIterator<Item = f64>) -> i64 {
    values.into_iter().fold(1_i64, |h, v| {
        h.wrapping_mul(1_000_003).wrapping_add(v.to_bits() as i64)
    })
}

/// A dimension type's `min_y` and `height`: the level a dimension's `NoiseChunk`s are clamped to.
fn level(bundle: &Bundle, dimension: &str) -> (i32, i32) {
    let json = bundle
        .resource("dimension_type", dimension.trim_start_matches("minecraft:"))
        .unwrap();
    let int = |name: &str| json[name].as_i64().unwrap() as i32;
    (int("min_y"), int("height"))
}

/// 26.2's cases (`docs/specs/oracle.md`, "26.2's probe cases"): `f64` point values of every
/// named function and router entry, the climate sampler before and after narrowing, and what a
/// `NoiseChunk` computes: its cached climate, and final density interpolated per column and per
/// chunk.
fn replay_v1_21(path: &std::path::Path, file: &ProbeFile, bundle: &Bundle) {
    let mut loader = v1_21::Loader::new(bundle);
    let mut routers: HashMap<String, v1_21::NoiseRouter> = HashMap::new();
    let mut states: HashMap<(String, i64), RandomState> = HashMap::new();
    let mut functions: HashMap<(String, i64, String), v1_21::Function> = HashMap::new();
    let mut failures = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for case in &file.cases {
        let seed = case.seed.expect("density cases have a seed");
        let [x, y, z] = case.pos.expect("a position");
        let dimension = case
            .dimension
            .clone()
            .unwrap_or_else(|| "minecraft:overworld".to_string());
        *counts.entry(dimension.clone()).or_default() += 1;
        *kinds.entry(case.test.clone()).or_default() += 1;
        let router = routers.entry(dimension.clone()).or_insert_with(|| {
            v1_21::NoiseRouter::load(bundle, noise_settings(&dimension)).unwrap()
        });
        let legacy = router.legacy_random_source();
        let state = states
            .entry((dimension.clone(), seed))
            .or_insert_with(|| router.bind(seed).unwrap());
        let (level_min_y, level_height) = level(bundle, &dimension);
        let differ = |want: u64, got: f64| {
            (want != got.to_bits()).then(|| {
                format!(
                    "{}\n    game:     {:e}\n    spyglass: {got:e}",
                    case.label(),
                    f64::from_bits(want)
                )
            })
        };
        let failure = match case.test.as_str() {
            "density" => {
                let name = case.name.clone().expect("a function id");
                let got = match name.strip_prefix("noise_router.") {
                    Some(entry) => {
                        let entry = RouterEntry::of(entry).expect("a 26.2 router entry");
                        state.entry(entry).compute(x, y, z)
                    }
                    None => functions
                        .entry((dimension.clone(), seed, name.clone()))
                        .or_insert_with(|| {
                            let f = loader.function(&name).unwrap();
                            v1_21::Binder::new(loader.noises(), seed, legacy)
                                .bind(&f)
                                .unwrap_or_else(|e| panic!("{name}: {e}"))
                        })
                        .compute(x, y, z),
                };
                differ(case.f64_bits()[0], got)
            }
            "climate" => {
                let values = state.sampler().sample_f64(x, y, z);
                match case.method.as_deref() {
                    Some("doubles") => {
                        let want = case.f64_bits();
                        (0..6).find_map(|i| differ(want[i], values[i]))
                    }
                    Some("target") => {
                        let got: Vec<i64> = values.iter().map(|v| quantize(*v as f32)).collect();
                        (case.i64s() != got).then(|| {
                            format!(
                                "{}\n    game:     {:?}\n    spyglass: {got:?}",
                                case.label(),
                                case.i64s()
                            )
                        })
                    }
                    other => panic!("{}: unknown method {other:?}", case.label()),
                }
            }
            // pos is [chunk x, quart min y, chunk z], height in quarts.
            "noise_chunk_climate_digest" | "noise_chunk_climate_vs_point" => {
                let chunk = NoiseChunk::for_chunk(state, x, z, level_min_y, level_height);
                let height = case.height.expect("a height");
                let (mut values, mut differing) = (Vec::new(), 0_i64);
                for qy in y..y + height {
                    for qz in z * 4..z * 4 + 4 {
                        for qx in x * 4..x * 4 + 4 {
                            let cached = chunk.climate(qx, qy, qz);
                            let point = state.sampler().sample_f64(qx, qy, qz);
                            differing += (0..6)
                                .filter(|&i| cached[i].to_bits() != point[i].to_bits())
                                .count() as i64;
                            values.extend(cached);
                        }
                    }
                }
                let got = if case.test == "noise_chunk_climate_digest" {
                    digest_f64(values)
                } else {
                    differing
                };
                (case.i64s() != [got]).then(|| format!("{}: {got} differs", case.label()))
            }
            "noise_chunk_density_column" => {
                let chunk = NoiseChunk::for_column(state, x, z, level_min_y, level_height);
                let want = case.f64_bits();
                assert_eq!(chunk.min_y(), y, "{}", case.label());
                assert_eq!(chunk.height(), case.height.unwrap(), "{}", case.label());
                (0..want.len()).find_map(|i| {
                    let yy = y + i as i32;
                    differ(want[i], chunk.interpolated_density(x, yy, z))
                        .map(|f| format!("{f}\n    at y {yy}"))
                })
            }
            "noise_chunk_density_chunk" => {
                let chunk = NoiseChunk::for_chunk(state, x >> 4, z >> 4, level_min_y, level_height);
                let height = case.height.expect("a height");
                assert_eq!(
                    (chunk.min_y(), chunk.height()),
                    (y, height),
                    "{}",
                    case.label()
                );
                let mut values = Vec::with_capacity(16 * 16 * height as usize);
                for bz in z..z + 16 {
                    for bx in x..x + 16 {
                        for by in y..y + height {
                            values.push(chunk.interpolated_density(bx, by, bz));
                        }
                    }
                }
                let got = digest_f64(values);
                (case.i64s() != [got]).then(|| format!("{}: chunk digest differs", case.label()))
            }
            other => panic!("{}: unknown 26.2 density test {other}", case.label()),
        };
        failures.extend(failure);
    }
    assert!(
        failures.is_empty(),
        "{}: {} of {} cases differ:\n{}",
        path.display(),
        failures.len(),
        file.cases.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    println!(
        "{}: all {} cases match ({counts:?}; {kinds:?})",
        path.display(),
        file.cases.len()
    );
}

#[test]
#[ignore = "needs bundles/<version> (SPYGLASS_ORACLE_VERSION, else the newest release)"]
fn every_bundled_density_function_loads() {
    let bundle = bundle();
    if is_v1_21(&bundle) {
        return every_v1_21_function_loads(&bundle);
    }
    let prefix = "data/minecraft/worldgen/density_function/";
    let ids: Vec<String> = bundle
        .manifest()
        .files
        .keys()
        .filter_map(|p| p.strip_prefix(prefix)?.strip_suffix(".json"))
        .map(|p| format!("minecraft:{p}"))
        .collect();
    assert!(ids.len() > 50, "only {} density functions", ids.len());
    for id in &ids {
        if let Err(e) = Loader::new(&bundle).function(id) {
            panic!("{e}");
        }
    }
    println!("all {} density functions load", ids.len());
    ClimateRouter::load(&bundle, "minecraft:overworld").unwrap();
}

/// 26.2: every density function loads as the 1.21 family's and binds, and each dimension's
/// router loads and binds.
fn every_v1_21_function_loads(bundle: &Bundle) {
    let prefix = "data/minecraft/worldgen/density_function/";
    let ids: Vec<String> = bundle
        .manifest()
        .files
        .keys()
        .filter_map(|p| p.strip_prefix(prefix)?.strip_suffix(".json"))
        .map(|p| format!("minecraft:{p}"))
        .collect();
    assert!(ids.len() > 30, "only {} density functions", ids.len());
    let mut loader = v1_21::Loader::new(bundle);
    for id in &ids {
        let f = loader.function(id).unwrap_or_else(|e| panic!("{e}"));
        v1_21::Binder::new(loader.noises(), 0, false)
            .bind(&f)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    for settings in ["minecraft:overworld", "minecraft:nether", "minecraft:end"] {
        v1_21::NoiseRouter::load(bundle, settings)
            .unwrap()
            .bind(0)
            .unwrap();
    }
    println!("all {} density functions load", ids.len());
}
