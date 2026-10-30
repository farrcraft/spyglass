//! `spyglass blocks`: a chunk's blocks before features, at one block, a column or a whole chunk
//! (`docs/specs/material.md`).

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use spyglass::{BlockColumn, Carvers, ChunkBlocks};

use crate::Out;
use crate::args::Blocks;
use crate::json;

pub fn run(args: &Blocks) -> Result<()> {
    if !args.no_carvers {
        bail!("blocks after carvers aren't computed yet; pass --no-carvers for the blocks before");
    }
    let carvers = Carvers::Off;
    let (world, seed, report) = crate::load(&args.common, false)?;
    let session = world.session(seed)?;
    let out = Out {
        session: &session,
        json: args.common.json,
        report: report.as_ref(),
    };
    if let Some(pos @ [x, y, z]) = args.at {
        let block = session.block_at(pos, carvers)?;
        if out.json {
            return out.print(
                "blocks",
                json::BlockAnswer {
                    carvers: false,
                    position: pos,
                    block: &block.block,
                    certain: block.certain,
                },
            );
        }
        out!("Block at {x} {y} {z}{}", out.context(None));
        out!("  {}{}", block.block, tie_note(block.certain));
    } else if let Some(column @ [x, z]) = args.column {
        let blocks = session.block_column(column, carvers)?;
        if out.json {
            return out.print(
                "blocks",
                json::BlocksAnswer {
                    carvers: false,
                    min_y: blocks.min_y,
                    columns: vec![json::BlockColumn::new([x, z], &blocks)],
                },
            );
        }
        out!("Blocks at {x} {z}, top down{}", out.context(None));
        print_runs(&blocks);
    } else if let Some([cx, cz]) = args.chunk {
        let chunk = session.chunk_blocks([cx, cz], carvers)?;
        if out.json {
            let columns = columns(&chunk)
                .map(|(xz, column)| json::BlockColumn::new(xz, &column))
                .collect();
            return out.print(
                "blocks",
                json::BlocksAnswer {
                    carvers: false,
                    min_y: chunk.min_y(),
                    columns,
                },
            );
        }
        out!("Blocks in chunk {cx} {cz}, by count{}", out.context(None));
        print_counts(&chunk);
    }
    out!("Blocks are the game's before carvers, structures and features.");
    out.warn();
    Ok(())
}

/// What a block that depends on a tied biome is marked with.
fn tie_note(certain: bool) -> &'static str {
    if certain {
        ""
    } else {
        " (depends on a tied biome: the game may have another)"
    }
}

/// Each column of `chunk`, z outer and x inner, with its block position.
fn columns<'c>(chunk: &'c ChunkBlocks<'_>) -> impl Iterator<Item = ([i32; 2], BlockColumn)> + 'c {
    let [cx, cz] = chunk.chunk();
    (0..16).flat_map(move |dz| {
        (0..16).map(move |dx| {
            let xz = [cx * 16 + dx, cz * 16 + dz];
            (xz, chunk.column(xz))
        })
    })
}

/// A column's runs of one block, top down.
fn print_runs(column: &BlockColumn) {
    let mut runs: Vec<(i32, i32, &str, bool)> = Vec::new();
    for (y, b) in (column.min_y..).zip(&column.blocks) {
        match runs.last_mut() {
            Some((_, top, block, certain)) if *block == b.block && *certain == b.certain => {
                *top = y;
            }
            _ => runs.push((y, y, &b.block, b.certain)),
        }
    }
    for (bottom, top, block, certain) in runs.into_iter().rev() {
        let ys = if bottom == top {
            format!("{top}")
        } else {
            format!("{top} to {bottom}")
        };
        out!("  {ys:>12}  {block}{}", tie_note(certain));
    }
}

/// How many of each block a chunk has, most first.
fn print_counts(chunk: &ChunkBlocks<'_>) {
    let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let [cx, cz] = chunk.chunk();
    for dz in 0..16 {
        for dx in 0..16 {
            for y in chunk.min_y()..chunk.min_y() + chunk.height() {
                let pos = [cx * 16 + dx, y, cz * 16 + dz];
                let count = counts.entry(chunk.block(pos)).or_default();
                count.0 += 1;
                if !chunk.is_certain(pos) {
                    count.1 += 1;
                }
            }
        }
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by_key(|&(block, (n, _))| (std::cmp::Reverse(n), block));
    for (block, (n, uncertain)) in counts {
        let note = if uncertain == 0 {
            String::new()
        } else {
            format!(" ({uncertain} depend on a tied biome)")
        };
        out!("  {n:>7}  {block}{note}");
    }
    out!("--column x,z shows a column; --json gives every block.");
}
