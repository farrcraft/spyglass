//! Chunks after the noise fill and the material rule against the oracle's block golden files
//! without carvers (`docs/specs/oracle.md#block-golden-files`): every block of every chunk.
//!
//! Needs `cargo xtask oracle blocks` and a bundle for the same version. Run with
//! `cargo test --release -p spyglass-blocks --test material -- --ignored`.

use std::collections::BTreeMap;
use std::path::Path;

use rayon::prelude::*;
use spyglass_blocks::BlockSettings;
use spyglass_data::bundle::Bundle;
use spyglass_oracle::blocks::BlockFile;

/// One file's comparison.
#[derive(Default)]
struct Report {
    /// Each chunk's first difference where spyglass's block is certain.
    failures: Vec<String>,
    /// Every difference, by what the game has and what spyglass has.
    tally: BTreeMap<(String, String), usize>,
    checked: usize,
    /// Blocks that depend on a tied biome.
    uncertain: usize,
}

/// Compares one file.
fn check(path: &Path) -> Report {
    let file = BlockFile::read(path).unwrap();
    let bundle = Bundle::open(spyglass_oracle::bundle_dir())
        .unwrap_or_else(|e| panic!("{e}; run spyglass-extract {}", file.version));
    let settings = BlockSettings::load(&bundle, "minecraft:normal", &file.dimension).unwrap();
    let generator = settings.generator(file.seed).unwrap();
    let palette = settings.palette();
    let Report {
        mut failures,
        mut tally,
        mut checked,
        mut uncertain,
    } = Report::default();
    for golden in &file.chunks {
        let [cx, cz] = golden.chunk;
        let chunk = generator.chunk(cx, cz);
        // Where a block depends on a tied biome, any answer is the game's for some history.
        if let Some(diff) = golden.first_difference(|p| {
            if chunk.is_certain(p) {
                palette.name(chunk.get(p))
            } else {
                golden.block(p)
            }
        }) {
            let lookup = generator.block_biome(diff.pos);
            let names: Vec<&str> = lookup
                .candidates()
                .iter()
                .map(|&b| settings.biome_name(b))
                .collect();
            failures.push(format!(
                "seed {} {} chunk {cx}, {cz} {diff} (biome {})",
                file.seed,
                file.dimension,
                names.join(" or ")
            ));
        }
        for (pos, want) in golden.blocks() {
            if !chunk.is_certain(pos) {
                uncertain += 1;
            }
            let got = palette.name(chunk.get(pos));
            if got != want {
                let certain = if chunk.is_certain(pos) {
                    ""
                } else {
                    " (tied biome)"
                };
                *tally
                    .entry((want.to_string(), format!("{got}{certain}")))
                    .or_default() += 1;
            }
        }
        checked += 256 * file.height as usize;
    }
    Report {
        failures,
        tally,
        checked,
        uncertain,
    }
}

#[test]
#[ignore = "needs the oracle's block golden files and a bundle: cargo xtask oracle blocks"]
fn chunks_without_carvers_match_the_game() {
    let files = spyglass_oracle::blocks::files(false);
    assert!(
        !files.is_empty(),
        "no block files for {}; run cargo xtask oracle blocks",
        spyglass_oracle::version()
    );
    let results: Vec<_> = files.par_iter().map(|p| check(p)).collect();
    let checked: usize = results.iter().map(|r| r.checked).sum();
    let uncertain: usize = results.iter().map(|r| r.uncertain).sum();
    let failures: Vec<&String> = results.iter().flat_map(|r| &r.failures).collect();
    let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in &results {
        for (k, n) in &r.tally {
            *tally.entry(k.clone()).or_default() += n;
        }
    }
    let mut kinds: Vec<_> = tally.into_iter().collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
    let differing: usize = kinds.iter().map(|(_, n)| n).sum();
    println!(
        "{} files, {checked} blocks, {uncertain} on tied biomes, {differing} differ; {} chunks differ where certain",
        files.len(),
        failures.len()
    );
    for ((want, got), n) in kinds.iter().take(25) {
        println!("  {n}: game {want}, spyglass {got}");
    }
    assert!(
        failures.is_empty(),
        "{} chunks differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
