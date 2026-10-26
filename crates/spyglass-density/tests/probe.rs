//! Density functions and the climate sampler against the oracle's probe files, bit for bit, and
//! every density function in a real bundle loaded.
//!
//! Needs `cargo xtask oracle probe` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-density -- --ignored`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use spyglass_data::bundle::Bundle;
use spyglass_data::codec::LoadErrorKind;
use spyglass_density::{Binder, ClimateRouter, Loader, Sampler, Volume};
use spyglass_oracle::probe::{ProbeFile, probe_files};

fn bundle_for(version: &str) -> Bundle {
    let dir: PathBuf = spyglass_oracle::default_dir()
        .join("../bundles")
        .join(version);
    Bundle::open(&dir).unwrap_or_else(|e| panic!("{e}; run spyglass-extract {version}"))
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

#[test]
#[ignore = "needs the oracle's probe files and a bundle: cargo xtask oracle probe"]
fn every_density_value_matches_the_game() {
    let files = probe_files(&spyglass_oracle::default_dir(), "density.json");
    assert!(
        !files.is_empty(),
        "no probe files; run cargo xtask oracle probe"
    );
    for path in files {
        let file = ProbeFile::read(&path).unwrap();
        let bundle = bundle_for(&file.version);
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
                        match Binder::with_legacy_random(loader.noises(), seed, legacy).bind(&f) {
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
                    (case.i64s() != [digest])
                        .then(|| format!("{}: chunk digest differs", case.label()))
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
}

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn every_bundled_density_function_loads() {
    let bundle = bundle_for("26.3");
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
