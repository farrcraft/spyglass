//! What the map's underground layers sample, against whole chunks on the real bundle
//! (`docs/specs/render.md#underground`): the noise fill's probes, ore veins at a position and
//! carvings agree with the chunks the generator makes.
//!
//! Needs a bundle for the oracle's version. Run with
//! `cargo test --release -p spyglass-blocks --test layers -- --ignored`.

use rayon::prelude::*;
use spyglass_blocks::BlockSettings;
use spyglass_blocks::material::VeinPart;
use spyglass_data::bundle::Bundle;
use spyglass_terrain::Block;

/// The chunks checked, far enough apart to see different terrain.
const CHUNKS: [[i32; 2]; 4] = [[0, 0], [7, -3], [-40, 25], [1250, -900]];
const SEED: i64 = 1;

fn settings(dimension: &str) -> BlockSettings {
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", spyglass_oracle::version()));
    BlockSettings::load(&bundle, "minecraft:normal", dimension).unwrap()
}

/// The vein block `part` of the vein named by its ore.
fn vein_block(ore: &str, part: VeinPart) -> &'static str {
    match (ore, part) {
        ("minecraft:copper_ore", VeinPart::Ore) => "minecraft:copper_ore",
        ("minecraft:copper_ore", VeinPart::RawOre) => "minecraft:raw_copper_block",
        ("minecraft:copper_ore", VeinPart::Filler) => "minecraft:granite",
        ("minecraft:deepslate_iron_ore", VeinPart::Ore) => "minecraft:deepslate_iron_ore",
        ("minecraft:deepslate_iron_ore", VeinPart::RawOre) => "minecraft:raw_iron_block",
        ("minecraft:deepslate_iron_ore", VeinPart::Filler) => "minecraft:tuff",
        _ => panic!("an unknown vein {ore}"),
    }
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn probes_and_veins_are_the_chunks() {
    let settings = settings("minecraft:overworld");
    assert!(settings.has_veins());
    let generator = settings.generator(SEED).unwrap();
    let palette = settings.palette();
    let ores = [
        "minecraft:copper_ore",
        "minecraft:raw_copper_block",
        "minecraft:deepslate_iron_ore",
        "minecraft:raw_iron_block",
    ];
    let veins: usize = CHUNKS
        .par_iter()
        .map(|&[cx, cz]| {
            let chunk = generator.chunk(cx, cz);
            let xs: Vec<i32> = (0..16).map(|i| cx * 16 + i).collect();
            let zs: Vec<i32> = (0..16).map(|i| cz * 16 + i).collect();
            let mut veins = 0;
            for y in (-64..=40).step_by(3) {
                let probes = generator.probes_on(&xs, &zs, y);
                for (i, probe) in probes.iter().enumerate() {
                    let pos = [xs[i % 16], y, zs[i / 16]];
                    let block = palette.name(chunk.get(pos));
                    let open = matches!(probe.block, Block::Air | Block::Water | Block::Lava);
                    // Below the surface, the rule leaves the fill's air and fluids as they are.
                    let fluid = matches!(
                        block,
                        "minecraft:air" | "minecraft:water" | "minecraft:lava"
                    );
                    assert_eq!(open, fluid, "{pos:?}: fill {probe:?}, chunk {block}");
                    match generator.vein_at(pos, probe.block) {
                        Some(vein) => {
                            veins += 1;
                            let ore = palette.name(vein.ore);
                            assert_eq!(block, vein_block(ore, vein.part), "{pos:?}");
                        }
                        None => assert!(!ores.contains(&block), "{pos:?}: {block}"),
                    }
                }
            }
            veins
        })
        .sum();
    assert!(veins > 0, "no veins sampled");
}

#[test]
#[ignore = "needs a bundle: spyglass-extract"]
fn carvings_hold_every_carved_block() {
    for dimension in ["minecraft:overworld", "minecraft:the_nether"] {
        let settings = settings(dimension);
        let generator = settings.generator(SEED).unwrap();
        let carved: usize = CHUNKS
            .par_iter()
            .map(|&[cx, cz]| {
                let (before, after) = (generator.chunk(cx, cz), generator.carved_chunk(cx, cz));
                let carvings = generator.carvings(cx, cz);
                let mut carved = 0;
                for x in 0..16 {
                    for z in 0..16 {
                        let runs = carvings.column_runs(x, z);
                        for y in before.min_y..before.min_y + before.height {
                            let pos = [cx * 16 + x, y, cz * 16 + z];
                            let kind = carvings.kind_at(x, y, z);
                            assert_eq!(
                                kind,
                                runs.iter().find(|r| (r.0..=r.1).contains(&y)).map(|r| r.2),
                                "{dimension} {pos:?}"
                            );
                            if kind.is_some() {
                                carved += 1;
                            }
                            // A carved block, or dirt under a carved grass block made the
                            // surface again.
                            if before.get(pos) != after.get(pos) && after.is_certain(pos) {
                                assert!(
                                    kind.is_some() || carvings.kind_at(x, y + 1, z).is_some(),
                                    "{dimension} {pos:?} changed but isn't carved"
                                );
                            }
                        }
                    }
                }
                carved
            })
            .sum();
        assert!(carved > 0, "{dimension}: nothing carved");
    }
}
