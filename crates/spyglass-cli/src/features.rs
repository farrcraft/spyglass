//! `spyglass features`: the features in a region (`docs/specs/locate.md#features`).

use anyhow::{Result, bail};

use crate::args::Features;
use crate::locate::{feature_line, feature_notes};
use crate::{Out, json};

/// The most chunks one request may ask for: each takes a while to place.
const MAX_CHUNKS: i64 = 4096;

pub fn run(args: &Features) -> Result<()> {
    let (min, max) = (args.min, args.max);
    let chunks = [min[0] >> 4, min[1] >> 4, max[0] >> 4, max[1] >> 4];
    let size = [chunks[2] - chunks[0] + 1, chunks[3] - chunks[1] + 1].map(i64::from);
    if size.iter().any(|&s| s <= 0) {
        bail!("--max must be at least --min on both axes");
    }
    if size[0].saturating_mul(size[1]) > MAX_CHUNKS {
        bail!(
            "{} × {} chunks is more than {MAX_CHUNKS}; ask for a smaller region",
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
    let query = world.feature_query(&args.target)?;
    let options = spyglass::FeatureOptions {
        neighbours: args.neighbours,
    };
    let found = session.features_in(
        &query,
        [chunks[0], chunks[1]],
        [chunks[2], chunks[3]],
        options,
    )?;
    let hits: Vec<&spyglass::FoundFeature> = found
        .iter()
        .filter(|f| {
            let [x, _, z] = f.position;
            (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z)
        })
        .filter(|f| args.all || f.certain || f.likely)
        .collect();
    if out.json {
        return out.print(
            "features",
            json::FeaturesInAnswer {
                target: &args.target,
                min,
                max,
                features: hits
                    .iter()
                    .map(|f| json::FeatureHit::new(f, None))
                    .collect(),
            },
        );
    }
    out!(
        "{} from {} {} to {} {}{}",
        args.target,
        min[0],
        min[1],
        max[0],
        max[1],
        out.context(None)
    );
    if hits.is_empty() {
        out!("  none");
    }
    for f in &hits {
        feature_line(f, None);
    }
    feature_notes(&hits, args.all);
    out.warn();
    Ok(())
}
