//! Density functions and the climate sampler against the oracle's probe files, bit for bit, and
//! every density function in a real bundle loaded.
//!
//! Needs `cargo xtask oracle probe` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-density -- --ignored`.

use std::collections::HashMap;
use std::path::PathBuf;

use spyglass_data::bundle::Bundle;
use spyglass_density::{Binder, ClimateRouter, LoadErrorKind, Loader, Sampler};
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
        let router = ClimateRouter::load(&bundle, "minecraft:overworld").unwrap();
        let mut samplers: HashMap<(i64, String), Sampler> = HashMap::new();
        let mut climates = HashMap::new();
        let mut failures = Vec::new();
        for case in &file.cases {
            let seed = case.seed.expect("density cases have a seed");
            let [x, y, z] = case.pos.expect("a position");
            let failure = match case.test.as_str() {
                "density" => {
                    let id = case.name.clone().expect("a function id");
                    let sampler = match samplers.get(&(seed, id.clone())) {
                        Some(s) => s.clone(),
                        None => {
                            let f = loader.function(&id).unwrap();
                            let s = Binder::new(loader.noises(), seed).bind(&f).unwrap();
                            samplers.insert((seed, id.clone()), s.clone());
                            s
                        }
                    };
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
                        .entry(seed)
                        .or_insert_with(|| router.sampler(seed).unwrap());
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
                        .entry(seed)
                        .or_insert_with(|| router.sampler(seed).unwrap());
                    // The chunk at (x, z), full overworld height (quart y -16..80), hashed as
                    // the harness does: h = h * 1000003 + bits, y outermost, then z, then x.
                    let (qx0, qz0) = (x * 4, z * 4);
                    let mut digest: i64 = 1;
                    for qy in -16..80 {
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
        println!("{}: all {} cases match", path.display(), file.cases.len());
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract 26.3"]
fn every_bundled_density_function_loads_or_is_unsupported() {
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
    let (mut loaded, mut unsupported) = (0, Vec::new());
    for id in &ids {
        match Loader::new(&bundle).function(id) {
            Ok(_) => loaded += 1,
            Err(e) if matches!(*e.kind, LoadErrorKind::Unsupported { .. }) => {
                unsupported.push(e.to_string())
            }
            Err(e) => panic!("{e}"),
        }
    }
    println!("{loaded} loaded, {} unsupported:", unsupported.len());
    for u in &unsupported {
        println!("  {u}");
    }
    ClimateRouter::load(&bundle, "minecraft:overworld").unwrap();
}
