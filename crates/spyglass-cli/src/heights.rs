//! `spyglass heights`: terrain heights for one column or a region (`docs/specs/terrain.md`).

use anyhow::{Result, bail};

use crate::Out;
use crate::args::Heights;
use crate::json;

/// The most columns one request may ask for, checked before loading anything.
const MAX_COLUMNS: i64 = spyglass::MAX_HEIGHT_COLUMNS;

pub fn run(args: &Heights) -> Result<()> {
    let (min, max) = match (args.at, args.min, args.max) {
        (Some(at), _, _) => (at, at),
        (None, Some(min), Some(max)) => (min, max),
        _ => bail!("give --at, or --min and --max"),
    };
    let size = [0, 1].map(|d| i64::from(max[d]) - i64::from(min[d]) + 1);
    if size.iter().any(|&s| s <= 0) {
        bail!("--max must be at least --min on both axes");
    }
    if size[0].saturating_mul(size[1]) > MAX_COLUMNS {
        bail!(
            "{} × {} columns is more than {MAX_COLUMNS}; ask for a smaller region",
            size[0],
            size[1]
        );
    }
    let (world, seed, report) = crate::load(&args.common, false)?;
    let session = world.session(seed)?;
    let out = Out {
        session: &session,
        json: args.common.json,
        report: report.as_ref(),
    };
    let heightmap = args.heightmap.heightmap();
    let heights = session.heights_in(min, max, heightmap)?;
    let rows: Vec<&[i32]> = heights.chunks(size[0] as usize).collect();
    if out.json {
        return out.print(
            "heights",
            json::HeightsAnswer {
                heightmap: heightmap.key(),
                min,
                max,
                heights: rows,
            },
        );
    }
    if min == max {
        out!("Height at {} {}{}", min[0], min[1], out.context(None));
        out!("  {}: {}", heightmap.key(), heights[0]);
    } else {
        out!(
            "Heights from {} {} to {} {}, x across and z down{}",
            min[0],
            min[1],
            max[0],
            max[1],
            out.context(None)
        );
        out!("  {}", heightmap.key());
        for (z, row) in (min[1]..).zip(rows) {
            let row: Vec<String> = row.iter().map(|h| format!("{h:>4}")).collect();
            out!("  z {z:>7} {}", row.join(""));
        }
    }
    out!(
        "Heights are one above the highest block, before structures, carvers, features and \
         surface blocks change the column."
    );
    out.warn();
    Ok(())
}
