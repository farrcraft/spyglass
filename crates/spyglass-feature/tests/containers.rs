//! Structures' containers, against the oracle's container golden files
//! (`docs/specs/oracle.md#container-golden-files`): every chunk the golden starts reach, on its
//! 3 × 3 carved chunks, with the containers of the structures spyglass places compared with the
//! game's. Every certain container must be the game's, seed included, and every container the
//! game placed for those structures must be an answer, certain or not.
//!
//! Then contents: every container the game placed, filled by spyglass from its table and seed at
//! its block, with spyglass's slots and biomes, must hold what the game's own fill put in it. With
//! the first comparison, a certain container's contents are the game's. Exploration maps find
//! nothing in either fill; their targets are compared by the locate oracle's map cases
//! (`docs/specs/loot.md#maps`).
//!
//! Needs `cargo xtask oracle containers` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-feature --test containers -- --ignored`.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rayon::prelude::*;
use spyglass_biome::{BiomeSource, Biomes, Lookup};
use spyglass_blocks::BlockSettings;
use spyglass_blocks::zoom::BiomeZoom;
use spyglass_data::bundle::Bundle;
use spyglass_feature::containers::{ContainerDoubt, StructureContainers, container_slots};
use spyglass_feature::{
    Blocks, Decorator, Doubt, FeatureSettings, NeighbourBounds, Placer, Reading, Region, structures,
};
use spyglass_loot::{Loot, MapRequest, World};
use spyglass_oracle::containers::{ContainerFile, Holder};
use spyglass_oracle::stacks;
use spyglass_structure::{Assembly, BoundingBox, JigsawPools, Starts, StructureSets};
use spyglass_terrain::Terrain;

/// One file's comparison.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    /// The game's containers placed by the structures spyglass places.
    game: usize,
    /// spyglass's certain answers, all the game's.
    certain: usize,
    /// spyglass's candidates, by reason.
    candidates: BTreeMap<String, usize>,
    /// Candidates with a seed found among the game's, but those of several starts' orders: how
    /// many have the game's seed, and how many don't.
    candidate_seeds: (usize, usize),
    /// By structure: the game's containers, and spyglass's certain ones.
    by_structure: BTreeMap<String, (usize, usize)>,
    /// The golden starts spyglass lays out, all with the game's pieces and bounds.
    layouts: usize,
}

/// Whether the game's container is the block or entity `name`.
fn holds(t: &spyglass_oracle::containers::GoldenContainer, name: &str) -> bool {
    match &t.what {
        Holder::Block(b) | Holder::Entity(b) => b == name,
    }
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
    let placed = |id: &str| {
        sets.structures()
            .iter()
            .any(|s| s.id == id && structures::places_containers(s.kind))
    };
    // Without starts of the game's that spyglass places (the End has none) there's nothing to
    // compare, and the End's features can't be loaded yet.
    if !file.starts.iter().any(|s| placed(&s.structure)) {
        return report;
    }
    let features = FeatureSettings::load(&bundle, biomes.biome_names(), v1_21).unwrap();
    let settings = BlockSettings::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let generator = settings.generator(file.seed).unwrap();
    let mut blocks = Blocks::new(&bundle, settings.palette()).unwrap();
    let pools = JigsawPools::load(&bundle, &sets).unwrap();
    let containers = StructureContainers::load(&bundle, &mut blocks, &pools, &sets).unwrap();
    let seeded = biomes.with_seed(file.seed).unwrap();
    let placements = sets.with_seed(file.seed, &seeded);
    let terrain = settings.terrain();
    let terrain_of_seed = Terrain::new(terrain, file.seed).unwrap();
    let starts = Starts::with_terrain(&placements, &seeded, &terrain_of_seed).with_pools(&pools);
    // The starts' pieces, where spyglass lays them out.
    for start in &file.starts {
        let Some(structure) = sets.structure_index(&start.structure) else {
            continue;
        };
        let Some(Assembly::Built(layout)) = starts.assemble(structure, start.chunk) else {
            continue;
        };
        let boxes = |b: &BoundingBox| [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]];
        let ours: Vec<[i32; 6]> = layout.pieces.iter().map(boxes).collect();
        if ours == start.pieces && boxes(&layout.bounds) == start.bounds {
            report.layouts += 1;
        } else {
            let first = ours.iter().zip(&start.pieces).position(|(a, b)| a != b);
            report.failures.push(format!(
                "{} {:?}: {} pieces, game {}; first difference at {first:?}",
                start.structure,
                start.chunk,
                ours.len(),
                start.pieces.len()
            ));
        }
    }
    // For each step with structures spyglass places, the features of the steps before it,
    // placed where spyglass places them.
    let mut steps: Vec<usize> = sets
        .structures()
        .iter()
        .filter(|s| structures::places_containers(s.kind))
        .map(|s| s.step)
        .collect();
    steps.sort_unstable();
    steps.dedup();
    let mut before = Vec::new();
    for &step in &steps {
        // A feature spyglass can't load yet isn't placed, and its writes aren't known.
        let mut wanted = Vec::new();
        let mut skipped = Vec::new();
        for &r in features.order()[..step].iter().flatten() {
            let mut reading = Reading {
                resources: &bundle,
                blocks: &mut blocks,
            };
            let loads = Decorator::with_reading(
                &features,
                &generator,
                file.seed,
                terrain.anchors(),
                terrain.level_min_y,
                &[r],
                Some(&mut reading),
            )
            .is_ok()
                && Placer::load(&bundle, &features, &mut blocks, &[r]).is_ok();
            if loads {
                wanted.push(r);
            } else {
                skipped.push(r);
            }
        }
        let placer = Placer::load(&bundle, &features, &mut blocks, &wanted).unwrap();
        let mut reading = Reading {
            resources: &bundle,
            blocks: &mut blocks,
        };
        let decorator = Decorator::with_reading(
            &features,
            &generator,
            file.seed,
            terrain.anchors(),
            terrain.level_min_y,
            &wanted,
            Some(&mut reading),
        )
        .unwrap();
        before.push((step, placer, decorator, skipped));
    }
    // What the chunk's own features, and its neighbours', could write.
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
        let region = Region::new(&blocks, chunk.chunk, chunks);
        let mut biomes = |c: [i32; 2]| {
            stored
                .entry(c)
                .or_insert_with(|| generator.chunk_biomes(c[0], c[1]))
                .clone()
        };
        let foreign = bounds.writes(&everything, &features, &region, &mut biomes);
        let own = bounds.own_writes(&everything, &features, &region, &mut biomes);
        let pieces = structures::pieces(
            &starts,
            &sets,
            chunk.chunk,
            spyglass_structure::STEPS.len() - 1,
            region.min_y(),
            region.max_y(),
        );
        let mut ours = Vec::new();
        for (step, placer, decorator, skipped) in &before {
            let mut region = region.clone();
            let mut unsure = Vec::new();
            placer.decorate_noting(
                decorator,
                &mut region,
                file.seed,
                &foreign,
                &own,
                &pieces,
                &mut unsure,
            );
            // A feature of an earlier step whose writes aren't known, or another structure's
            // piece placed before, could have written in the box a structure reads.
            let reads = |min: [i32; 3], max: [i32; 3], structure: usize| {
                let info = &sets.structures()[structure];
                let meets =
                    |lo: [i32; 3], hi: [i32; 3]| (0..3).all(|i| lo[i] <= max[i] && min[i] <= hi[i]);
                own.iter()
                    .filter(|f| f.order.0 < info.step && meets(f.min, f.max))
                    .find_map(|f| {
                        (skipped.contains(&f.placed) || unsure.iter().any(|(r, _)| *r == f.placed))
                            .then_some(Doubt::OtherFeature(f.placed))
                    })
                    .or_else(|| {
                        pieces
                            .iter()
                            .find(|p| {
                                p.structure != structure
                                    && (p.step, sets.structures()[p.structure].step_index)
                                        < (info.step, info.step_index)
                                    && meets(p.min, p.max)
                            })
                            .map(|p| Doubt::Structure(p.structure))
                    })
            };
            ours.extend(structures::containers(
                &starts,
                &sets,
                &containers,
                &blocks,
                &generator,
                &mut region,
                file.seed,
                *step,
                &reads,
            ));
        }
        let theirs: Vec<_> = chunk.containers.iter().filter(|c| placed(&c.by)).collect();
        report.game += theirs.len();
        for c in &theirs {
            report.by_structure.entry(c.by.clone()).or_default().0 += 1;
        }
        for c in &ours {
            let id = &sets.structures()[c.structure].id;
            let found = theirs.iter().find(|t| {
                t.by == *id && t.pos == c.pos && t.table == c.table && holds(t, &c.block)
            });
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
                (Some(why), seed, found) => {
                    // Each order of several starts gives its own seeds; one of them is the game's.
                    if let (Some(seed), Some(t), false) =
                        (seed, found, *why == ContainerDoubt::StartOrder)
                    {
                        if t.seed == seed {
                            report.candidate_seeds.0 += 1;
                        } else {
                            report.candidate_seeds.1 += 1;
                        }
                    }
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
            // A candidate whose height was read in doubt (an ocean ruin's chest) answers for
            // the game's at its x and z with its seed.
            if !ours.iter().any(|c| {
                let height_in_doubt = c.doubt.is_some()
                    && c.seed == Some(t.seed)
                    && [c.pos[0], c.pos[2]] == [t.pos[0], t.pos[2]];
                sets.structures()[c.structure].id == t.by
                    && (c.pos == t.pos || height_in_doubt)
                    && c.table == t.table
                    && holds(t, &c.block)
            }) {
                let near: Vec<_> = ours
                    .iter()
                    .filter(|c| sets.structures()[c.structure].id == t.by)
                    .map(|c| (c.pos, c.seed, &c.doubt))
                    .collect();
                report.failures.push(format!(
                    "seed {} chunk {:?}: the game's {} container at {:?} ({}, seed {}) isn't an \
                     answer; ours there: {near:?}",
                    file.seed, chunk.chunk, t.by, t.pos, t.table, t.seed
                ));
            }
        }
    }
    report
}

/// The world a container is filled in: biomes as spyglass looks them up at a block, noting
/// whether one was a tie, and no structure for any map, as the harness's fills find none.
struct FillWorld<'a> {
    source: &'a BiomeSource,
    biomes: &'a Biomes<'a>,
    zoom: BiomeZoom,
    /// Whether a biome the fill asked for was a tie, so spyglass's might not be the game's.
    tied: Cell<bool>,
}

impl World for FillWorld<'_> {
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let [qx, qy, qz] = self.zoom.quart([x, y, z]);
        let lookup = self.biomes.biome_at(qx, qy, qz);
        if matches!(lookup, Lookup::Tied(_)) {
            self.tied.set(true);
        }
        Some(self.source.name(lookup.biome()).to_string())
    }

    fn map_target(&self, _request: &MapRequest) -> Option<[i32; 2]> {
        None
    }
}

#[test]
#[ignore = "needs the oracle's container golden files and a bundle: cargo xtask oracle containers"]
fn container_contents_match_the_game() {
    let files = spyglass_oracle::containers::files();
    assert!(
        !files.is_empty(),
        "no container files for {}; run cargo xtask oracle containers",
        spyglass_oracle::version()
    );
    let bundle = Bundle::open(spyglass_oracle::bundle_dir()).unwrap();
    let mut loot = Loot::new();
    let mut dimensions = HashMap::new();
    let (mut filled, mut stacks_seen, mut unfilled, mut tied) = (0, 0, 0, 0);
    let mut by_table: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for path in &files {
        let file = ContainerFile::read(path).unwrap();
        let (source, blocks) = dimensions.entry(file.dimension.clone()).or_insert_with(|| {
            let source = BiomeSource::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
            let settings =
                BlockSettings::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
            let blocks = Blocks::new(&bundle, settings.palette()).unwrap();
            (source, blocks)
        });
        let biomes = source.with_seed(file.seed).unwrap();
        let world = FillWorld {
            source,
            biomes: &biomes,
            zoom: BiomeZoom::new(file.seed),
            tied: Cell::new(false),
        };
        for chunk in &file.chunks {
            for c in &chunk.containers {
                let at = format!(
                    "seed {} {} at {:?} ({} by {})",
                    file.seed, file.dimension, c.pos, c.table, c.by
                );
                let (Holder::Block(holder) | Holder::Entity(holder)) = &c.what;
                let slots = container_slots(blocks, holder);
                if slots != Some(c.slots) {
                    failures.push(format!(
                        "{at}: {holder} has {} slots, spyglass says {slots:?}",
                        c.slots
                    ));
                    continue;
                }
                let Some(items) = &c.items else {
                    unfilled += 1;
                    continue;
                };
                if !loot.has(&c.table) {
                    loot.load(&bundle, &c.table)
                        .unwrap_or_else(|e| panic!("{}: {e}", c.table));
                }
                world.tied.set(false);
                let ours = match loot.fill(&c.table, c.seed, c.pos, c.slots, &world) {
                    Ok(ours) => ours,
                    Err(e) => {
                        failures.push(format!("{at}: {e}"));
                        continue;
                    }
                };
                let mut theirs: BTreeMap<usize, &serde_json::Value> = BTreeMap::new();
                for stack in items {
                    theirs.insert(stack["slot"].as_u64().unwrap() as usize, stack);
                }
                let wrong: Vec<String> = ours
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, ours)| {
                        let want = theirs.get(&slot).map(|v| stacks::game(v));
                        let got = ours.as_ref().map(|s| stacks::ours(s, &bundle));
                        (want != got)
                            .then(|| format!("slot {slot}: game {want:?}, spyglass {got:?}"))
                    })
                    .collect();
                if wrong.is_empty() {
                    filled += 1;
                    stacks_seen += theirs.len();
                    *by_table.entry(c.table.clone()).or_default() += 1;
                } else if world.tied.get() {
                    // A biome tie spyglass answers one way and the game the other.
                    tied += 1;
                } else {
                    failures.push(format!("{at}: {}", wrong.join("; ")));
                }
            }
        }
    }
    for (table, n) in &by_table {
        println!("  {table}: {n}");
    }
    println!(
        "{} files: {filled} containers filled as the game fills them ({stacks_seen} stacks), \
         {tied} after a biome tie, {unfilled} the harness couldn't fill; {} differences",
        files.len(),
        failures.len()
    );
    assert!(
        filled > 0,
        "no filled containers; run cargo xtask oracle containers"
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

#[test]
#[ignore = "needs the oracle's container golden files and a bundle: cargo xtask oracle containers"]
fn structure_containers_match_the_game() {
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
        "candidates with a seed: {} the game's, {} not",
        reports.iter().map(|r| r.candidate_seeds.0).sum::<usize>(),
        reports.iter().map(|r| r.candidate_seeds.1).sum::<usize>()
    );
    println!(
        "{} files: {} starts laid out as the game's; {} structure containers in the game, {} \
         certain answers, {} candidates, {} differences",
        files.len(),
        reports.iter().map(|r| r.layouts).sum::<usize>(),
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
