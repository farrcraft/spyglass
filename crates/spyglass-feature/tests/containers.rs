//! Structures' containers, against the oracle's container golden files
//! (`docs/specs/oracle.md#container-golden-files`): every chunk the golden starts reach, on its
//! 3 × 3 carved chunks, with the containers the jigsaw structures' pieces place compared with the
//! game's. Every certain container must be the game's, seed included, and every container the
//! game's jigsaw pieces placed must be an answer, certain or not.
//!
//! Needs `cargo xtask oracle containers` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-feature --test containers -- --ignored`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rayon::prelude::*;
use spyglass_biome::BiomeSource;
use spyglass_blocks::BlockSettings;
use spyglass_data::bundle::Bundle;
use spyglass_feature::containers::{ContainerDoubt, StructureContainers};
use spyglass_feature::{
    Blocks, Decorator, Doubt, FeatureSettings, NeighbourBounds, Region, structures,
};
use spyglass_oracle::containers::{ContainerFile, Holder};
use spyglass_structure::{JigsawPools, Starts, StructureSets, StructureType};
use spyglass_terrain::Terrain;

/// One file's comparison.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// The game's containers placed by jigsaw structures.
    game: usize,
    /// spyglass's certain answers, all the game's.
    certain: usize,
    /// spyglass's candidates, by reason.
    candidates: BTreeMap<String, usize>,
    /// By structure: the game's containers, and spyglass's certain ones.
    by_structure: BTreeMap<String, (usize, usize)>,
}

fn check(path: &Path) -> Report {
    let file = ContainerFile::read(path).unwrap();
    let mut report = Report::default();
    if file.chunks.is_empty() {
        return report;
    }
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", file.version));
    let v1_21 = bundle
        .resource("worldgen/configured_feature", "minecraft:monster_room")
        .is_ok();
    let biomes = BiomeSource::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let sets = StructureSets::load(&bundle, "minecraft:normal", &file.dimension, &biomes).unwrap();
    let jigsaw = |id: &str| {
        sets.structures()
            .iter()
            .any(|s| s.id == id && s.kind == StructureType::Jigsaw)
    };
    // Without jigsaw starts of the game's (the End has none) there's nothing to compare, and
    // the End's features can't be loaded yet.
    if !file.starts.iter().any(|s| jigsaw(&s.structure)) {
        return report;
    }
    let features = FeatureSettings::load(&bundle, biomes.biome_names(), v1_21).unwrap();
    let settings = BlockSettings::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let generator = settings.generator(file.seed).unwrap();
    let mut blocks = Blocks::new(&bundle, settings.palette()).unwrap();
    let pools = JigsawPools::load(&bundle, &sets).unwrap();
    let containers = StructureContainers::load(&bundle, &mut blocks, &pools).unwrap();
    let seeded = biomes.with_seed(file.seed).unwrap();
    let placements = sets.with_seed(file.seed, &seeded);
    let terrain = settings.terrain();
    let terrain_of_seed = Terrain::new(terrain, file.seed).unwrap();
    let starts = Starts::with_terrain(&placements, &seeded, &terrain_of_seed).with_pools(&pools);
    // What the chunk's own features could write, for what protected blocks read.
    let all: Vec<usize> = features.order().iter().flatten().copied().collect();
    let everything = Decorator::new(
        &features,
        &generator,
        file.seed,
        terrain.anchors(),
        terrain.level_min_y,
        &all,
    )
    .unwrap();
    let bounds = NeighbourBounds::load(&bundle, &features).unwrap();
    let mut carved = HashMap::new();
    let mut stored = HashMap::new();
    for chunk in &file.chunks {
        let [cx, cz] = chunk.chunk;
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let key = [cx + dx, cz + dz];
                let c = carved
                    .entry(key)
                    .or_insert_with(|| generator.carved_chunk(key[0], key[1]));
                chunks.push(c.clone());
            }
        }
        let mut region = Region::new(&blocks, chunk.chunk, chunks);
        let mut biomes = |c: [i32; 2]| {
            stored
                .entry(c)
                .or_insert_with(|| generator.chunk_biomes(c[0], c[1]))
                .clone()
        };
        let own = bounds.own_writes(&everything, &features, &region, &mut biomes);
        let pieces = structures::pieces(
            &starts,
            &sets,
            chunk.chunk,
            spyglass_structure::STEPS.len() - 1,
            region.min_y(),
            region.max_y(),
        );
        // A feature of an earlier step, or another structure's piece placed before, could have
        // written what a processor reads.
        let reads = |pos: [i32; 3], structure: usize| {
            let info = &sets.structures()[structure];
            let holds =
                |min: [i32; 3], max: [i32; 3]| (0..3).all(|i| min[i] <= pos[i] && pos[i] <= max[i]);
            own.iter()
                .find(|f| f.order.0 < info.step && holds(f.min, f.max))
                .map(|f| Doubt::OtherFeature(f.placed))
                .or_else(|| {
                    pieces
                        .iter()
                        .find(|p| {
                            p.structure != structure
                                && (p.step, sets.structures()[p.structure].step_index)
                                    < (info.step, info.step_index)
                                && holds(p.min, p.max)
                        })
                        .map(|p| Doubt::Structure(p.structure))
                })
        };
        let ours = structures::containers(
            &starts,
            &sets,
            &containers,
            &blocks,
            &mut region,
            file.seed,
            &reads,
        );
        let theirs: Vec<_> = chunk
            .containers
            .iter()
            .filter(|c| jigsaw(&c.by) && matches!(c.what, Holder::Block(_)))
            .collect();
        report.game += theirs.len();
        for c in &theirs {
            report.by_structure.entry(c.by.clone()).or_default().0 += 1;
        }
        for c in &ours {
            let id = &sets.structures()[c.structure].id;
            let found = theirs
                .iter()
                .find(|t| t.by == *id && t.pos == c.pos && t.table == c.table);
            match (&c.doubt, c.seed, found) {
                (None, Some(seed), Some(t)) if t.seed == seed => {
                    report.certain += 1;
                    report.by_structure.entry(id.clone()).or_default().1 += 1;
                }
                (None, _, found) => report.failures.push(format!(
                    "seed {} chunk {:?}: a certain {} container at {:?} ({}, seed {:?}); the \
                     game's: {:?}",
                    file.seed,
                    chunk.chunk,
                    id,
                    c.pos,
                    c.table,
                    c.seed,
                    found.map(|t| t.seed)
                )),
                (Some(why), _, _) => {
                    let why = match why {
                        ContainerDoubt::FeatureElement(f) => {
                            format!("after feature element {}", f.as_deref().unwrap_or("inline"))
                        }
                        ContainerDoubt::Read(Doubt::Structure(s)) => {
                            format!("reads {}'s pieces", sets.structures()[*s].id)
                        }
                        ContainerDoubt::Read(Doubt::OtherFeature(r)) => format!(
                            "reads {}'s writes",
                            features.placed(*r).id.clone().unwrap_or_default()
                        ),
                        why => format!("{why:?}"),
                    };
                    *report.candidates.entry(format!("{id}: {why}")).or_default() += 1;
                }
            }
        }
        for t in &theirs {
            if !ours.iter().any(|c| {
                sets.structures()[c.structure].id == t.by && c.pos == t.pos && c.table == t.table
            }) {
                report.failures.push(format!(
                    "seed {} chunk {:?}: the game's {} container at {:?} ({}) isn't an answer",
                    file.seed, chunk.chunk, t.by, t.pos, t.table
                ));
            }
        }
    }
    report
}

#[test]
#[ignore = "needs the oracle's container golden files and a bundle: cargo xtask oracle containers"]
fn jigsaw_containers_match_the_game() {
    let files = spyglass_oracle::containers::files();
    assert!(
        !files.is_empty(),
        "no container files for {}; run cargo xtask oracle containers",
        spyglass_oracle::version()
    );
    let reports: Vec<Report> = files.par_iter().map(|p| check(p)).collect();
    let mut by_structure: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut candidates: BTreeMap<String, usize> = BTreeMap::new();
    for r in &reports {
        for (k, (g, c)) in &r.by_structure {
            let e = by_structure.entry(k.clone()).or_default();
            e.0 += g;
            e.1 += c;
        }
        for (k, v) in &r.candidates {
            *candidates.entry(k.clone()).or_default() += v;
        }
    }
    for (k, (g, c)) in &by_structure {
        println!("  {k}: {g} in the game, {c} certain");
    }
    for (k, v) in &candidates {
        println!("  candidate, {k}: {v}");
    }
    let failures: Vec<&String> = reports.iter().flat_map(|r| &r.failures).collect();
    println!(
        "{} files: {} jigsaw containers in the game, {} certain answers, {} candidates, {} \
         differences",
        files.len(),
        reports.iter().map(|r| r.game).sum::<usize>(),
        reports.iter().map(|r| r.certain).sum::<usize>(),
        candidates.values().sum::<usize>(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
